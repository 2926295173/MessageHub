package im.zyx.phonebridge.ui.components

import android.Manifest
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.PowerManager
import android.provider.Settings
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalLifecycleOwner
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.LifecycleEventObserver
import im.zyx.phonebridge.R

/**
 * Permissions section — embedded in SettingsScreen.
 *
 * Aggregates the three permission classes the app needs:
 *  1. **Runtime permissions** — POST_NOTIFICATIONS (Android 13+),
 *     SMS (read / send / receive), READ_PHONE_STATE, ANSWER_PHONE_CALLS,
 *     READ_CALL_LOG. All granted together via the system multi-permission
 *     dialog.
 *  2. **Notification listener access** — system Settings → "Device & app
 *     notifications". Cannot be granted via `pm grant`; the user has to
 *     toggle the switch.
 *  3. **Battery-optimization exemption** — system Settings → "Ignore
 *     battery optimizations" dialog. Without this, the OS may kill the
 *     foreground service after a few minutes, breaking WS.
 *
 * Status is re-checked on every ON_RESUME (so a user who grants a
 * permission in a system dialog then returns to the app sees the
 * updated state immediately, without a hard refresh).
 */
@Composable
fun PermissionsSection(modifier: Modifier = Modifier) {
    val context = LocalContext.current
    val lifecycleOwner = LocalLifecycleOwner.current

    val reqPerms = remember {
        buildList {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                add(Manifest.permission.POST_NOTIFICATIONS)
            }
            add(Manifest.permission.RECEIVE_SMS)
            add(Manifest.permission.READ_SMS)
            add(Manifest.permission.SEND_SMS)
            add(Manifest.permission.READ_PHONE_STATE)
            add(Manifest.permission.ANSWER_PHONE_CALLS)
            add(Manifest.permission.READ_CALL_LOG)
        }
    }

    var runtimeGranted by remember {
        mutableStateOf(reqPerms.all { granted(context, it) })
    }
    var notifAccessGranted by remember {
        mutableStateOf(notificationListenerGranted(context))
    }
    var batteryOptGranted by remember {
        mutableStateOf(isBatteryOptimizationExempt(context))
    }

    DisposableEffect(lifecycleOwner) {
        val observer = LifecycleEventObserver { _, event ->
            if (event == Lifecycle.Event.ON_RESUME) {
                runtimeGranted = reqPerms.all { granted(context, it) }
                notifAccessGranted = notificationListenerGranted(context)
                batteryOptGranted = isBatteryOptimizationExempt(context)
            }
        }
        lifecycleOwner.lifecycle.addObserver(observer)
        onDispose { lifecycleOwner.lifecycle.removeObserver(observer) }
    }

    val multiPermLauncher = rememberLauncherForActivityResult(
        contract = ActivityResultContracts.RequestMultiplePermissions()
    ) { /* system dialog result; state is re-checked on ON_RESUME */ }

    Column(modifier = modifier, verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text(
            text = stringResource(R.string.permissions_section_title),
            style = MaterialTheme.typography.titleMedium,
            fontWeight = FontWeight.SemiBold,
            color = MaterialTheme.colorScheme.onSurface,
            modifier = Modifier.padding(horizontal = 4.dp, vertical = 4.dp),
        )

        PermissionCard(
            title = stringResource(R.string.permission_runtime_title),
            body = stringResource(R.string.permission_runtime_body),
            granted = runtimeGranted,
            actionLabel = stringResource(R.string.permission_grant_action),
            onAction = { multiPermLauncher.launch(reqPerms.toTypedArray()) },
        )

        PermissionCard(
            title = stringResource(R.string.permission_notif_title),
            body = stringResource(R.string.permission_notif_body),
            granted = notifAccessGranted,
            actionLabel = stringResource(R.string.permission_grant_action),
            onAction = { openNotificationAccessSettings(context) },
        )

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.M) {
            BatteryOptCard(
                granted = batteryOptGranted,
                onGrant = {
                    if (!batteryOptGranted) requestBatteryOptimizationExemption(context)
                },
            )
        }
    }
}

@Composable
private fun PermissionCard(
    title: String,
    body: String,
    granted: Boolean,
    actionLabel: String,
    onAction: () -> Unit,
) {
    Card(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(12.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainer),
    ) {
        Column(modifier = Modifier.padding(16.dp)) {
            Text(text = title, style = MaterialTheme.typography.titleSmall, fontWeight = FontWeight.SemiBold)
            Spacer(Modifier.height(4.dp))
            Text(
                text = body,
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(8.dp))
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = androidx.compose.ui.Alignment.CenterVertically,
            ) {
                StatusBadge(granted = granted)
                if (!granted) {
                    Button(onClick = onAction) {
                        Text(actionLabel)
                    }
                }
            }
        }
    }
}

@Composable
private fun BatteryOptCard(granted: Boolean, onGrant: () -> Unit) {
    Card(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(12.dp),
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainer),
    ) {
        Column(modifier = Modifier.padding(16.dp)) {
            Text(
                text = stringResource(R.string.battery_opt_title),
                style = MaterialTheme.typography.titleSmall,
                fontWeight = FontWeight.SemiBold,
            )
            Spacer(Modifier.height(4.dp))
            Text(
                text = stringResource(R.string.battery_opt_body),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(2.dp))
            Text(
                text = stringResource(R.string.battery_opt_oem_hint),
                style = MaterialTheme.typography.labelSmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Spacer(Modifier.height(8.dp))
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = androidx.compose.ui.Alignment.CenterVertically,
            ) {
                StatusBadge(granted = granted)
                if (!granted) {
                    Button(onClick = onGrant) {
                        Text(stringResource(R.string.battery_opt_button))
                    }
                }
            }
        }
    }
}

@Composable
private fun StatusBadge(granted: Boolean) {
    val color = if (granted) {
        MaterialTheme.colorScheme.primary
    } else {
        MaterialTheme.colorScheme.error
    }
    val label = if (granted) {
        stringResource(R.string.permission_granted)
    } else {
        stringResource(R.string.permission_pending)
    }
    Text(
        text = label,
        style = MaterialTheme.typography.labelMedium,
        color = color,
    )
}

// =============================================================================
//  Permission state helpers — public so they can be unit-tested or reused.
// =============================================================================

private fun granted(ctx: Context, perm: String): Boolean =
    ContextCompat.checkSelfPermission(ctx, perm) == PackageManager.PERMISSION_GRANTED

private fun notificationListenerGranted(ctx: Context): Boolean {
    val flat = Settings.Secure.getString(ctx.contentResolver, "enabled_notification_listeners")
        ?: return false
    return flat.contains(ctx.packageName)
}

private fun isBatteryOptimizationExempt(ctx: Context): Boolean {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.M) return true
    val pm = ctx.getSystemService(Context.POWER_SERVICE) as PowerManager
    return pm.isIgnoringBatteryOptimizations(ctx.packageName)
}

private fun requestBatteryOptimizationExemption(ctx: Context) {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.M) return
    val i = Intent(Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS).apply {
        data = Uri.parse("package:${ctx.packageName}")
        addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    }
    try {
        ctx.startActivity(i)
    } catch (_: Throwable) {
        // Fallback: send the user to the system battery list.
        val fallback = Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        try { ctx.startActivity(fallback) } catch (_: Throwable) { /* noop */ }
    }
}

private fun openNotificationAccessSettings(ctx: Context) {
    val i = Intent(Settings.ACTION_NOTIFICATION_LISTENER_SETTINGS)
    i.flags = Intent.FLAG_ACTIVITY_NEW_TASK
    try {
        ctx.startActivity(i)
    } catch (_: Throwable) { /* device without that settings page */ }
}
