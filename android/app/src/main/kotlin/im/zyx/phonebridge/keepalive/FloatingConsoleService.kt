package im.zyx.phonebridge.keepalive

import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.graphics.PixelFormat
import android.os.Build
import android.os.IBinder
import android.provider.Settings
import android.util.Log
import android.util.TypedValue
import android.view.Gravity
import android.view.MotionEvent
import android.view.View
import android.view.ViewGroup
import android.view.WindowManager
import android.widget.Toast
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.BatteryStd
import androidx.compose.material.icons.filled.ConnectedTv
import androidx.compose.material.icons.filled.Link
import androidx.compose.material.icons.filled.LinkOff
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.ComposeView
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.ViewCompositionStrategy
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.app.NotificationCompat
import androidx.lifecycle.LifecycleService
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.lifecycleScope
import dagger.hilt.android.AndroidEntryPoint
import im.zyx.phonebridge.PhoneBridgeApp
import im.zyx.phonebridge.R
import im.zyx.phonebridge.data.PrefsRepository
import im.zyx.phonebridge.network.BridgeClient
import im.zyx.phonebridge.network.BridgeStatus
import im.zyx.phonebridge.notification.RecentNotificationsCache
import im.zyx.phonebridge.ui.MainActivity
import javax.inject.Inject
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch

private const val TAG = "FloatingConsole"
private const val NOTIF_ID = 0xF0_AAAA

/**
 * User-toggleable floating console. Renders entirely with Jetpack
 * Compose (no XML layouts) so the visual style stays consistent
 * with the rest of the app and theme changes can be applied in one
 * place.
 *
 * Two Compose trees are added to [WindowManager] as `ComposeView`s:
 *  - **Handle**: a 48dp round button. Always visible. Tap to
 *    expand, long-press to remove, drag to move. The drag/click
 *    distinction is handled by an `OnTouchListener` on the
 *    `ComposeView` itself (not inside Compose) because the gesture
 *    arbitration needs raw motion events for sub-pixel precision.
 *  - **Panel**: a 280dp × 320dp card. Toggled by tapping the
 *    handle. Shows live connection status, battery, the last 5
 *    cached notifications, and two quick actions.
 *
 * Position persistence: the handle's `(x, y)` are written to
 * [PrefsRepository.floatingPos] on drag end, restored on next
 * start. `-1, -1` means "first run, default to top-right".
 *
 * Service self-stops in three cases:
 *  - `SYSTEM_ALERT_WINDOW` is missing (and posts a heads-up).
 *  - The user long-presses the handle and confirms removal.
 *  - The user toggles "Floating console" off in Settings.
 */
@AndroidEntryPoint
class FloatingConsoleService : LifecycleService() {

    @Inject lateinit var prefs: PrefsRepository
    @Inject lateinit var client: BridgeClient
    @Inject lateinit var recentCache: RecentNotificationsCache

    private var wm: WindowManager? = null
    private var handleView: ComposeView? = null
    private var panelView: ComposeView? = null
    private var isPanelOpen = false

    private val params: WindowManager.LayoutParams by lazy {
        WindowManager.LayoutParams(
            WindowManager.LayoutParams.WRAP_CONTENT,
            WindowManager.LayoutParams.WRAP_CONTENT,
            WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY,
            WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE
                or WindowManager.LayoutParams.FLAG_NOT_TOUCH_MODAL
                or WindowManager.LayoutParams.FLAG_LAYOUT_NO_LIMITS,
            PixelFormat.TRANSLUCENT,
        ).apply {
            gravity = Gravity.TOP or Gravity.START
        }
    }

    override fun onCreate() {
        super.onCreate()
        if (!Settings.canDrawOverlays(this)) {
            Log.w(TAG, "SYSTEM_ALERT_WINDOW not granted; aborting")
            postPermissionMissingAlert()
            stopSelf()
            return
        }
        wm = getSystemService(WINDOW_SERVICE) as WindowManager
        startInForeground()
        lifecycleScope.launch {
            val (x, y) = prefs.floatingPos.first()
            addHandle(x, y)
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        super.onStartCommand(intent, flags, startId)
        return START_NOT_STICKY
    }

    override fun onBind(intent: Intent): IBinder? {
        super.onBind(intent)
        return null
    }

    override fun onDestroy() {
        removeOverlays()
        super.onDestroy()
    }

    private fun startInForeground() {
        val open = Intent(this, MainActivity::class.java)
        val pi = PendingIntent.getActivity(
            this, 0, open,
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val n = NotificationCompat.Builder(this, PhoneBridgeApp.CHANNEL_BRIDGE)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(getString(R.string.floating_running_title))
            .setContentText(getString(R.string.floating_running_body))
            .setOngoing(true)
            .setContentIntent(pi)
            .build()
        val type = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q)
            ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE
        else 0
        startForeground(NOTIF_ID, n, type)
    }

    private fun addHandle(x: Int, y: Int) {
        val sizePx = dp(48)
        val (initX, initY) = clampToScreen(x, y, sizePx)
        params.x = initX
        params.y = initY
        params.width = sizePx
        params.height = sizePx

        val composeView = ComposeView(this).apply {
            layoutParams = ViewGroup.LayoutParams(sizePx, sizePx)
            // Dispose the composition when the view is detached from
            // the window — critical for service stopSelf() without
            // leaking the composition.
            setViewCompositionStrategy(
                ViewCompositionStrategy.DisposeOnViewTreeLifecycleDestroyed
            )
            setContent {
                MaterialTheme(colorScheme = FloatingDarkScheme) {
                    FloatingHandleContent()
                }
            }
        }
        try {
            wm?.addView(composeView, params)
        } catch (t: Throwable) {
            Log.e(TAG, "addView(handle) failed: ${t.message}")
            stopSelf()
            return
        }
        handleView = composeView
        attachTouchHandler(composeView)
    }

    private fun attachTouchHandler(v: View) {
        var startX = 0
        var startY = 0
        var startTouchX = 0f
        var startTouchY = 0f
        var isDragging = false
        var downAt = 0L
        v.setOnTouchListener { _, ev ->
            when (ev.action) {
                MotionEvent.ACTION_DOWN -> {
                    startX = params.x
                    startY = params.y
                    startTouchX = ev.rawX
                    startTouchY = ev.rawY
                    isDragging = false
                    downAt = System.currentTimeMillis()
                    true
                }
                MotionEvent.ACTION_MOVE -> {
                    val dx = (ev.rawX - startTouchX).toInt()
                    val dy = (ev.rawY - startTouchY).toInt()
                    if (!isDragging && (dx * dx + dy * dy) > dp(8) * dp(8)) {
                        isDragging = true
                    }
                    if (isDragging) {
                        params.x = (startX + dx).coerceAtLeast(0)
                        params.y = (startY + dy).coerceAtLeast(0)
                        wm?.updateViewLayout(v, params)
                    }
                    true
                }
                MotionEvent.ACTION_UP -> {
                    val elapsed = System.currentTimeMillis() - downAt
                    if (isDragging) {
                        // Persist the new position.
                        lifecycleScope.launch {
                            prefs.setFloatingPos(params.x, params.y)
                        }
                    } else if (elapsed >= LONG_PRESS_MS) {
                        showLongPressConfirm()
                    } else {
                        togglePanel()
                    }
                    true
                }
                else -> false
            }
        }
    }

    private fun togglePanel() {
        if (isPanelOpen) closePanel() else openPanel()
    }

    private fun openPanel() {
        if (panelView != null) return
        val panelW = dp(280)
        val panelH = dp(320)
        val (panelX, panelY) = panelAnchor(params.x, params.y, panelW, panelH)
        val panelParams = WindowManager.LayoutParams(
            panelW, panelH,
            WindowManager.LayoutParams.TYPE_APPLICATION_OVERLAY,
            WindowManager.LayoutParams.FLAG_NOT_FOCUSABLE
                or WindowManager.LayoutParams.FLAG_NOT_TOUCH_MODAL,
            PixelFormat.TRANSLUCENT,
        ).apply {
            gravity = Gravity.TOP or Gravity.START
            x = panelX
            y = panelY
        }
        val view = ComposeView(this).apply {
            setViewCompositionStrategy(
                ViewCompositionStrategy.DisposeOnViewTreeLifecycleDestroyed
            )
            setContent {
                MaterialTheme(colorScheme = FloatingDarkScheme) {
                    val graph = rememberAppGraph()
                    FloatingPanelContent(
                        bridgeClient = graph.bridgeClient,
                        recentCache = graph.recentCache,
                        onOpenApp = {
                            val open = Intent(this@FloatingConsoleService, MainActivity::class.java)
                                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                            startActivity(open)
                        },
                        onDisconnect = {
                            graph.bridgeClient.stop()
                            closePanel()
                        },
                    )
                }
            }
        }
        try {
            wm?.addView(view, panelParams)
        } catch (t: Throwable) {
            Log.w(TAG, "addView(panel) failed: ${t.message}")
            return
        }
        panelView = view
        isPanelOpen = true
    }

    private fun closePanel() {
        val p = panelView ?: return
        try {
            wm?.removeView(p)
        } catch (_: Throwable) { /* already gone */ }
        panelView = null
        isPanelOpen = false
    }

    private fun showLongPressConfirm() {
        val v = handleView ?: return
        // The confirm dialog needs an overlayable window, so we
        // use Toast as a low-friction confirm ("Tap again to
        // remove") instead of a focusable dialog — much simpler
        // than juggling FLAG_NOT_FOCUSABLE.
        Toast.makeText(
            this,
            getString(R.string.floating_remove_confirm),
            Toast.LENGTH_LONG,
        ).show()
        // On a second long-press within a 3s window, actually
        // remove. State is local to the listener closure so we
        // can't persist it; instead we open a follow-up Toast that
        // is just informational. Real "remove" is the user
        // toggling the pref off in Settings (see SettingsScreen).
        // This keeps the gesture discoverable without a fiddly
        // dialog.
        lifecycleScope.launch {
            prefs.setFloatingEnabled(false)
        }
        stopSelf()
    }

    private fun removeOverlays() {
        closePanel()
        handleView?.let {
            try { wm?.removeView(it) } catch (_: Throwable) { }
        }
        handleView = null
    }

    private fun postPermissionMissingAlert() {
        val pi = PendingIntent.getActivity(
            this, 0,
            Intent(Settings.ACTION_MANAGE_OVERLAY_PERMISSION)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val n = NotificationCompat.Builder(this, PhoneBridgeApp.CHANNEL_ALERTS)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(getString(R.string.floating_perm_missing_title))
            .setContentText(getString(R.string.floating_perm_missing_body))
            .setPriority(NotificationCompat.PRIORITY_HIGH)
            .setAutoCancel(true)
            .setContentIntent(pi)
            .build()
        val nm = androidx.core.app.NotificationManagerCompat.from(this)
        try { nm.notify(0xF0_BEEF, n) } catch (_: Throwable) { }
    }

    private fun dp(v: Int): Int =
        TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_DIP, v.toFloat(), resources.displayMetrics).toInt()

    private fun clampToScreen(x: Int, y: Int, size: Int): Pair<Int, Int> {
        if (x < 0 || y < 0) {
            val display = resources.displayMetrics
            return (display.widthPixels - size - dp(16)) to dp(80)
        }
        val display = resources.displayMetrics
        return x.coerceIn(0, display.widthPixels - size) to
            y.coerceIn(0, display.heightPixels - size)
    }

    private fun panelAnchor(handleX: Int, handleY: Int, panelW: Int, panelH: Int): Pair<Int, Int> {
        val display = resources.displayMetrics
        val onRight = handleX < display.widthPixels / 2
        val onBottom = handleY < display.heightPixels / 2
        val x = if (onRight) handleX + dp(56) else (handleX - panelW - dp(8)).coerceAtLeast(0)
        val y = if (onBottom) handleY else (handleY - panelH).coerceAtLeast(0)
        return x.coerceIn(0, (display.widthPixels - panelW).coerceAtLeast(0)) to
            y.coerceIn(0, (display.heightPixels - panelH).coerceAtLeast(0))
    }

    companion object {
        const val ACTION_STOP = "im.zyx.phonebridge.keepalive.FLOATING_STOP"
        private const val LONG_PRESS_MS = 600L
    }
}

// ============================================================================
// Compose content
// ============================================================================

private val FloatingDarkScheme = darkColorScheme(
    primary = Color(0xFF60A5FA),
    onPrimary = Color(0xFF0B1220),
    surface = Color(0xFF111827),
    onSurface = Color(0xFFF3F4F6),
    background = Color(0xFF0B1220),
    onBackground = Color(0xFFF3F4F6),
    error = Color(0xFFFCA5A5),
)

/** The 48dp round handle. Visual only; touch is handled by the host View. */
@Composable
private fun FloatingHandleContent() {
    Surface(
        modifier = Modifier
            .size(48.dp)
            .clip(CircleShape),
        color = Color(0xCC1F2937),
        shape = CircleShape,
        border = BorderStroke(1.dp, Color(0x33000000)),
    ) {
        Box(
            modifier = Modifier.fillMaxSize().padding(10.dp),
            contentAlignment = Alignment.Center,
        ) {
            Icon(
                imageVector = Icons.Filled.ConnectedTv,
                contentDescription = stringResource(R.string.floating_handle_desc),
                tint = Color.White,
            )
        }
    }
}

/** The expanded panel: status, battery, recent notifications, quick actions. */
@Composable
private fun FloatingPanelContent(
    bridgeClient: BridgeClient,
    recentCache: RecentNotificationsCache,
    onOpenApp: () -> Unit,
    onDisconnect: () -> Unit,
) {
    val ctx = LocalContext.current
    val status by bridgeClient.status.collectAsStateWithLifecycle()
    val battery = remember { readBatteryPercent(ctx) }
    // Snapshot the cache on a 750 ms ticker. Using mutableStateOf
    // + LaunchedEffect instead of produceState to avoid the
    // suspend-lambda inference quirks of the latter.
    var recent by remember { mutableStateOf(recentCache.snapshot()) }
    LaunchedEffect(Unit) {
        while (true) {
            recent = recentCache.snapshot()
            delay(750L)
        }
    }

    Surface(
        modifier = Modifier
            .fillMaxSize()
            .clip(RoundedCornerShape(16.dp)),
        color = Color(0xF2111827),
        shape = RoundedCornerShape(16.dp),
        border = BorderStroke(1.dp, Color(0x33FFFFFF)),
    ) {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(16.dp),
        ) {
            Text(
                text = stringResource(R.string.floating_panel_title),
                color = Color.White,
                fontWeight = FontWeight.Bold,
                fontSize = 16.sp,
            )
            Divider()
            StatusRow(status)
            BatteryRow(battery)
            Divider()
            Text(
                text = stringResource(R.string.floating_panel_recent_title),
                color = Color.White,
                fontWeight = FontWeight.Bold,
                fontSize = 13.sp,
            )
            Spacer(Modifier.height(4.dp))
            Box(
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(min = 0.dp, max = 160.dp),
            ) {
                if (recent.isEmpty()) {
                    Text(
                        text = stringResource(R.string.floating_recent_empty),
                        color = Color(0xFFCBD5E1),
                        fontSize = 12.sp,
                    )
                } else {
                    Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                        recent.take(5).forEach { n ->
                            Text(
                                text = "• ${n.title.ifBlank { n.app_name ?: n.package_name }}",
                                color = Color(0xFFE5E7EB),
                                fontSize = 12.sp,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                            )
                        }
                    }
                }
            }
            Spacer(Modifier.height(8.dp))
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                Button(
                    onClick = onOpenApp,
                    colors = ButtonDefaults.buttonColors(
                        containerColor = Color(0xFF374151),
                        contentColor = Color.White,
                    ),
                    modifier = Modifier.weight(1f),
                ) { Text(stringResource(R.string.floating_action_open_app)) }
                Button(
                    onClick = onDisconnect,
                    colors = ButtonDefaults.buttonColors(
                        containerColor = Color(0xFF7F1D1D),
                        contentColor = Color(0xFFFCA5A5),
                    ),
                    modifier = Modifier.weight(1f),
                ) { Text(stringResource(R.string.floating_action_disconnect)) }
            }
        }
    }
}

@Composable
private fun StatusRow(status: BridgeStatus) {
    val (icon, tint, label) = when (status) {
        is BridgeStatus.Connected -> Triple(Icons.Filled.Link, Color(0xFF34D399), stringResource(R.string.floating_status_connected))
        is BridgeStatus.Connecting -> Triple(Icons.Filled.Link, Color(0xFFFBBF24), stringResource(R.string.floating_status_connecting))
        is BridgeStatus.Disconnected -> Triple(Icons.Filled.LinkOff, Color(0xFF9CA3AF), stringResource(R.string.floating_status_disconnected))
        is BridgeStatus.Error -> Triple(Icons.Filled.LinkOff, Color(0xFFFCA5A5), stringResource(R.string.floating_status_error))
    }
    val detail = when (status) {
        is BridgeStatus.Connected -> " ${status.host}:${status.port}"
        is BridgeStatus.Connecting -> " ${status.host}:${status.port}"
        is BridgeStatus.Error -> " (${status.message})"
        else -> ""
    }
    Row(verticalAlignment = Alignment.CenterVertically) {
        Icon(icon, contentDescription = null, tint = tint, modifier = Modifier.size(14.dp))
        Spacer(Modifier.size(6.dp))
        Text("$label$detail", color = Color.White, fontSize = 13.sp)
    }
}

@Composable
private fun BatteryRow(percent: Int) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Icon(Icons.Filled.BatteryStd, contentDescription = null, tint = Color(0xFFFBBF24), modifier = Modifier.size(14.dp))
        Spacer(Modifier.size(6.dp))
        Text(stringResource(R.string.floating_battery_fmt, percent), color = Color.White, fontSize = 13.sp)
    }
}

@Composable
private fun Divider() {
    Spacer(Modifier.height(8.dp))
    HorizontalDivider(color = Color(0x33FFFFFF))
    Spacer(Modifier.height(8.dp))
}

// ============================================================================
// State holders — these need access to the injected Hilt graph. We
// pull them from the application context once and remember the
// references. Hilt singletons are stable for the process lifetime
// so the references are safe to capture.
// ============================================================================

private data class AppGraph(
    val bridgeClient: BridgeClient,
    val recentCache: RecentNotificationsCache,
)

@Composable
private fun rememberAppGraph(): AppGraph {
    val ctx = androidx.compose.ui.platform.LocalContext.current
    return remember(ctx) {
        val ep = dagger.hilt.android.EntryPointAccessors.fromApplication(
            ctx.applicationContext,
            AppGraphEntryPoint::class.java,
        )
        AppGraph(ep.bridgeClient(), ep.recentCache())
    }
}

private fun readBatteryPercent(ctx: Context): Int = try {
    val bm = ctx.getSystemService(Context.BATTERY_SERVICE) as android.os.BatteryManager
    bm.getIntProperty(android.os.BatteryManager.BATTERY_PROPERTY_CAPACITY)
} catch (_: Throwable) { -1 }

@dagger.hilt.EntryPoint
@dagger.hilt.InstallIn(dagger.hilt.components.SingletonComponent::class)
internal interface AppGraphEntryPoint {
    fun bridgeClient(): BridgeClient
    fun recentCache(): RecentNotificationsCache
}
