package im.zyx.phonebridge.ui

import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.viewModels
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.LocalIndication
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.outlined.AddCircle
import androidx.compose.material.icons.outlined.Info
import androidx.compose.material.icons.outlined.Menu
import androidx.compose.material.icons.outlined.Settings
import androidx.compose.material.icons.outlined.Smartphone
import androidx.compose.material3.DrawerValue
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalDrawerSheet
import androidx.compose.material3.ModalNavigationDrawer
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.rememberDrawerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import androidx.navigation.NavHostController
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import dagger.hilt.android.AndroidEntryPoint
import dagger.hilt.android.lifecycle.HiltViewModel
import im.zyx.phonebridge.BuildConfig
import im.zyx.phonebridge.R
import im.zyx.phonebridge.data.PrefsRepository
import im.zyx.phonebridge.network.BridgeClient
import im.zyx.phonebridge.network.BridgeStatus
import im.zyx.phonebridge.ui.screens.AboutScreen
import im.zyx.phonebridge.ui.screens.PairingScreen
import im.zyx.phonebridge.ui.screens.SettingsScreen
import im.zyx.phonebridge.ui.theme.PhoneBridgeTheme
import im.zyx.phonebridge.ui.theme.ThemeMode
import javax.inject.Inject
import kotlinx.coroutines.flow.SharingStarted.Companion.Eagerly
import kotlinx.coroutines.flow.SharingStarted.Companion.Lazily
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch

/**
 * Display info for the currently-paired desktop, shown in the
 * drawer. Null when not connected.
 *
 * - [name] is the desktop's own `device.hello` name field (e.g.
 *   "rk3588", "office-ubuntu"). If the hello hasn't arrived yet
 *   (network jitter, brand-new session) this falls back to the
 *   host:port we dialed.
 * - [host] / [port] are always the dialed endpoint, so the row
 *   can render a small subtitle even before the hello lands.
 */
data class PairedDesktop(val name: String, val host: String, val port: Int)

@HiltViewModel
class MainViewModel @Inject constructor(
    private val prefs: PrefsRepository,
    private val client: BridgeClient,
) : ViewModel() {
    val themeMode = prefs.themeMode
        .stateIn(viewModelScope, Eagerly, null)
    val deviceName = prefs.deviceName
        .stateIn(viewModelScope, Eagerly, null)

    /**
     * Combine bridge status with the desktop's reported name. We
     * only emit a non-null value while status is `Connected`, so
     * the drawer row vanishes on disconnect/error without any
     * extra gating.
     */
    val pairedDesktop = combine(client.status, client.desktopName) { status, reportedName ->
        when (status) {
            is BridgeStatus.Connected -> PairedDesktop(
                name = reportedName?.takeIf { it.isNotBlank() }
                    ?: "${status.host}:${status.port}",
                host = status.host,
                port = status.port,
            )
            else -> null
        }
    }.stateIn(viewModelScope, Lazily, null)
}

@AndroidEntryPoint
class MainActivity : ComponentActivity() {
    private val vm: MainViewModel by viewModels()
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            val modePref by vm.themeMode.collectAsState()
            PhoneBridgeTheme(mode = ThemeMode.fromPersisted(modePref)) {
                Surface(
                    modifier = Modifier.fillMaxSize(),
                    color = MaterialTheme.colorScheme.background
                ) {
                    AppNav(vm = vm)
                }
            }
        }
    }
}

private sealed class DrawerEntry(
    val key: String,
    val labelRes: Int,
    val icon: ImageVector,
    val iconSizeDp: Int,
) {
    object PairNew : DrawerEntry("pairing", R.string.drawer_pair_new, Icons.Outlined.AddCircle, 24)
    object Settings : DrawerEntry("settings", R.string.menu_settings, Icons.Outlined.Settings, 22)
    object About    : DrawerEntry("about",    R.string.menu_about,    Icons.Outlined.Info, 24)
}

private val DRAWER_DEVICES = listOf(DrawerEntry.PairNew)
private val DRAWER_APP     = listOf(DrawerEntry.Settings, DrawerEntry.About)

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun AppNav(vm: MainViewModel) {
    val nav: NavHostController = rememberNavController()
    val backStackEntry by nav.currentBackStackEntryAsState()
    val currentRoute = backStackEntry?.destination?.route

    val drawerState = rememberDrawerState(initialValue = DrawerValue.Closed)
    val scope = rememberCoroutineScope()

    val onOpenDrawer: () -> Unit = { scope.launch { drawerState.open() } }
    val onNavigate: (String) -> Unit = { route ->
        scope.launch { drawerState.close() }
        if (route != currentRoute) nav.navigate(route)
    }

    // Reference: drawer covers ~85% of screen width, capped so
    // tablets don't get a full-bleed sheet.
    val configuration = LocalConfiguration.current
    val drawerWidth = (configuration.screenWidthDp * 0.85f)
        .dp
        .coerceAtMost(360.dp)

    val deviceName by vm.deviceName.collectAsState()
    val hostName = remember(deviceName) { deviceName ?: defaultDeviceName() }
    val pairedDesktop by vm.pairedDesktop.collectAsState()

    ModalNavigationDrawer(
        drawerState = drawerState,
        drawerContent = {
            ModalDrawerSheet(
                modifier = Modifier.width(drawerWidth),
                // Reference signature: top corners are SHARP, the
                // bottom-end is heavily rounded (~80dp). Inverted
                // from the M3 default.
                drawerShape = RoundedCornerShape(
                    topStart = 0.dp,
                    topEnd = 0.dp,
                    bottomStart = 0.dp,
                    bottomEnd = 80.dp,
                ),
                drawerContainerColor = MaterialTheme.colorScheme.surface,
                drawerTonalElevation = 0.dp,
            ) {
                DrawerContent(
                    deviceName = hostName,
                    pairedDesktop = pairedDesktop,
                    currentRoute = currentRoute,
                    onNavigate = onNavigate,
                )
            }
        },
        scrimColor = MaterialTheme.colorScheme.scrim,
    ) {
        NavHost(navController = nav, startDestination = "pairing") {
            composable("pairing") {
                PairingScreen(
                    onOpenAddByIp = { nav.navigate("add_by_ip") },
                    onOpenSettings = { nav.navigate("settings") },
                    onOpenDrawer = onOpenDrawer,
                )
            }
            composable("add_by_ip") {
                im.zyx.phonebridge.ui.screens.AddByIpScreen(
                    onBack = { nav.popBackStack() },
                    onOpenDrawer = onOpenDrawer,
                )
            }
            composable("settings") {
                SettingsScreen(
                    onBack = { nav.popBackStack() },
                    onOpenDrawer = onOpenDrawer,
                )
            }
            composable("about") {
                AboutScreenWithChrome(onBack = { nav.popBackStack() })
            }
        }
    }
}

@Composable
private fun DrawerContent(
    deviceName: String,
    pairedDesktop: PairedDesktop?,
    currentRoute: String?,
    onNavigate: (String) -> Unit,
) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            // Reference: status-bar icons overlay the drawer, so we
            // start content below the system bar to keep the header
            // from sliding under the clock.
            .windowInsetsPadding(WindowInsets.statusBars)
            .background(MaterialTheme.colorScheme.surface),
    ) {
        // ── Header: smartphone glyph + app name + device subtitle ──
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(start = 24.dp, end = 24.dp, top = 16.dp, bottom = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(
                imageVector = Icons.Outlined.Smartphone,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.onSurface,
                modifier = Modifier.size(40.dp),
            )
            Spacer(Modifier.width(16.dp))
            Column(modifier = Modifier.weight(1f)) {
                Text(
                    text = stringResource(R.string.app_name),
                    fontSize = 24.sp,
                    fontWeight = FontWeight.SemiBold,
                    color = MaterialTheme.colorScheme.onSurface,
                )
                Text(
                    text = deviceName,
                    fontSize = 14.sp,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }

        // ── "设备" section label ──
        Spacer(Modifier.height(20.dp))
        Text(
            text = stringResource(R.string.drawer_section_devices),
            fontSize = 18.sp,
            fontWeight = FontWeight.SemiBold,
            color = MaterialTheme.colorScheme.onSurface,
            modifier = Modifier.padding(horizontal = 24.dp),
        )
        Spacer(Modifier.height(8.dp))

        // When paired, surface the live desktop between the section
        // label and the "pair new" entry — gives the user a quick
        // "this is the host you're bridged to" anchor. The row is a
        // status display, not a navigation target, so it's
        // intentionally non-clickable.
        if (pairedDesktop != null) {
            PairedDesktopCard(pairedDesktop)
            Spacer(Modifier.height(4.dp))
        }

        DRAWER_DEVICES.forEach { entry ->
            DrawerRow(
                entry = entry,
                selected = currentRoute == entry.key,
                onClick = { onNavigate(entry.key) },
            )
        }

        // Single thin divider per the reference, separating the
        // device section from the app section.
        HorizontalDivider(
            color = MaterialTheme.colorScheme.outlineVariant,
            thickness = 1.dp,
            modifier = Modifier.padding(horizontal = 24.dp, vertical = 12.dp),
        )

        // ── "应用" section ──
        DRAWER_APP.forEach { entry ->
            DrawerRow(
                entry = entry,
                selected = currentRoute == entry.key,
                onClick = { onNavigate(entry.key) },
            )
        }
    }
}

/**
 * Compact status card showing the currently-paired desktop.
 * Sits between the "设备" label and the "配对新设备" row. We use
 * a soft surfaceContainer tone (matches the screen cards) so it
 * reads as a status pill, not a navigation row, while still
 * sitting flat against the drawer's pinkish-white fill.
 */
@Composable
private fun PairedDesktopCard(desktop: PairedDesktop) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(horizontal = 24.dp, vertical = 6.dp)
            .background(
                color = MaterialTheme.colorScheme.surfaceContainer,
                shape = RoundedCornerShape(12.dp),
            )
            .padding(horizontal = 12.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        // Small green dot — a visual stand-in for "connected".
        // 8dp size, vertically centered with the title row.
        Box(
            modifier = Modifier
                .size(8.dp)
                .background(
                    color = androidx.compose.ui.graphics.Color(0xFF2EA043),
                    shape = androidx.compose.foundation.shape.CircleShape,
                ),
        )
        Spacer(Modifier.width(10.dp))
        Column(modifier = Modifier.weight(1f)) {
            Text(
                text = desktop.name,
                fontSize = 15.sp,
                fontWeight = FontWeight.SemiBold,
                color = MaterialTheme.colorScheme.onSurface,
            )
            Text(
                text = "${desktop.host}:${desktop.port}",
                fontSize = 12.sp,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

@Composable
private fun DrawerRow(
    entry: DrawerEntry,
    selected: Boolean,
    onClick: () -> Unit,
) {
    // Per the reference: icon at fixed slot, label to the right
    // with a 16dp gap; ~32dp center-to-center between rows. No
    // card/fill — flat icon + text, like the reference.
    val interactionSource = remember { MutableInteractionSource() }
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(
                interactionSource = interactionSource,
                indication = LocalIndication.current,
                onClick = onClick,
            )
            .padding(horizontal = 24.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(
            imageVector = entry.icon,
            contentDescription = null,
            tint = if (selected) MaterialTheme.colorScheme.primary
                   else MaterialTheme.colorScheme.onSurface,
            modifier = Modifier.size(entry.iconSizeDp.dp),
        )
        Spacer(Modifier.width(16.dp))
        Text(
            text = stringResource(entry.labelRes),
            fontSize = 18.sp,
            fontWeight = if (selected) FontWeight.SemiBold else FontWeight.Medium,
            color = MaterialTheme.colorScheme.onSurface,
        )
    }
}

private fun defaultDeviceName(): String {
    val manufacturer = Build.MANUFACTURER?.replaceFirstChar { it.uppercase() } ?: ""
    val model = Build.MODEL ?: ""
    return listOf(manufacturer, model).filter { it.isNotBlank() }.joinToString(" ")
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun ScreenTopBar(
    title: String,
    onOpenDrawer: () -> Unit,
    onBack: (() -> Unit)? = null,
) {
    androidx.compose.material3.TopAppBar(
        title = { Text(title) },
        navigationIcon = {
            if (onBack != null) {
                IconButton(onClick = onBack) {
                    Icon(
                        imageVector = Icons.AutoMirrored.Outlined.ArrowBack,
                        contentDescription = stringResource(R.string.settings_back),
                    )
                }
            } else {
                IconButton(onClick = onOpenDrawer) {
                    Icon(
                        imageVector = Icons.Outlined.Menu,
                        contentDescription = stringResource(R.string.menu_open),
                    )
                }
            }
        },
    )
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun AboutScreenWithChrome(onBack: () -> Unit) {
    androidx.compose.material3.Scaffold(
        topBar = {
            ScreenTopBar(
                title = stringResource(R.string.about_title),
                onOpenDrawer = {},
                onBack = onBack,
            )
        }
    ) { pad ->
        Box(
            modifier = Modifier
                .fillMaxSize()
                .padding(pad)
        ) {
            AboutScreen()
        }
    }
}
