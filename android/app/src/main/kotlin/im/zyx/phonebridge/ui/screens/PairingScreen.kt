package im.zyx.phonebridge.ui.screens

import android.content.Intent
import android.util.Log
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.Menu
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Wifi
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.material3.pulltorefresh.rememberPullToRefreshState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.foundation.layout.size
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import im.zyx.phonebridge.R
import androidx.hilt.navigation.compose.hiltViewModel
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import im.zyx.phonebridge.network.BridgeClient
import im.zyx.phonebridge.network.BridgeService
import im.zyx.phonebridge.network.BridgeStatus
import im.zyx.phonebridge.network.DiscoveredDesktop
import im.zyx.phonebridge.network.NsdRegistrar
import im.zyx.phonebridge.pairing.PairingMachine
import im.zyx.phonebridge.data.PrefsRepository
import javax.inject.Inject
import kotlinx.coroutines.launch

@HiltViewModel
class PairingViewModel @Inject constructor(
    private val nsd: NsdRegistrar,
    private val prefs: PrefsRepository,
    val pairing: PairingMachine,
    val client: BridgeClient
) : ViewModel() {

    val pairingState = pairing.state
    val bridgeStatus = client.status

    /** Live stream of desktops currently visible on the LAN. */
    val discovered = nsd.desktops

    /** True while a pull-to-refresh discovery is in flight. */
    var isRefreshing = MutableStateFlow(false)
        private set

    private var refreshJob: Job? = null

    companion object {
        /** Minimum time the spinner stays visible after a refresh
         *  is triggered. Below this, the mDNS callback often hasn't
         *  fired yet and the spinner flickers. */
        private const val MIN_SPINNER_MS = 1500L
    }

    init {
        // Discover the first desktop on the LAN. When we get a host:port,
        // persist it to prefs so the foreground service can connect.
        viewModelScope.launch {
            nsd.discoverFirstDesktop().collect { (host, port) ->
                prefs.setDesktop(host, port)
            }
        }
        // Make sure the long-term identity is generated so device.hello
        // has a real pubkey.
        pairing.ensureIdentity("phonebridge-android")
        // Start a passive discovery session in the background so the
        // discovered list isn't empty the first time the user opens the
        // screen. A pull-to-refresh re-runs an active session.
        startDiscovery()
    }

    /**
     * Re-runs an mDNS browse for PhoneBridge message-centers on the LAN.
     * Called from the pull-to-refresh gesture on the pairing screen.
     * If a discovery is already running, this is a no-op.
     *
     * Spinner visibility: the mDNS discovery can stay open for a long
     * time (it's a passive listener), so we don't tie `isRefreshing`
     * to the job's lifetime. Instead we show the spinner for at least
     * [MIN_SPINNER_MS] so the user has a clear visual confirmation,
     * then hide it even if the discovery is still listening for more
     * late-arriving devices. The job itself keeps running until
     * [viewModelScope] is cleared.
     */
    fun refreshDesktops() {
        Log.i("Pairing", "refreshDesktops called; active=${refreshJob?.isActive}")
        if (refreshJob?.isActive == true) {
            // Job already running; just re-show the spinner for a
            // fresh window so the user gets feedback.
            isRefreshing.value = true
            viewModelScope.launch {
                kotlinx.coroutines.delay(MIN_SPINNER_MS)
                isRefreshing.value = false
            }
            return
        }
        isRefreshing.value = true
        refreshJob = viewModelScope.launch {
            try {
                nsd.discoverDesktops().collect { /* heartbeat */ }
            } finally {
                Log.i("Pairing", "refreshDesktops finished")
            }
        }
        // Hide the spinner after the minimum-visible window, even
        // though the discovery listener above is still open.
        viewModelScope.launch {
            kotlinx.coroutines.delay(MIN_SPINNER_MS)
            isRefreshing.value = false
        }
    }

    private fun startDiscovery() {
        viewModelScope.launch {
            nsd.discoverDesktops().collect { /* passive background scan */ }
        }
    }

    /**
     * Tap a discovered desktop: persist its host/port to prefs (so
     * the foreground service connects) and start a phone-initiated
     * pairing. The user then waits for the desktop's web console to
     * click Accept.
     */
    fun pairWithDiscovered(d: DiscoveredDesktop) {
        val host = d.host ?: return
        viewModelScope.launch {
            prefs.setDesktop(host, d.port)
            // Reuse the manual-flow path: it just sends pair.request.
            initiatePairing()
        }
    }

    fun reset() {
        pairing.reset()
    }

    fun saveManualDesktop(host: String, port: Int) {
        viewModelScope.launch { prefs.setDesktop(host, port) }
    }

    /**
     * The user clicked "Accept" on the phone. Send `device.pair.confirm(true)`
     * to the desktop directly. The phone is the trusted UI surface — the
     * user's click here is the canonical confirmation; we don't wait for
     * the desktop to send `device.pair.accept` (no code is typed anywhere).
     */
    fun acceptByUser() {
        val env = pairing.userAccepts() ?: return
        client.send(env)
    }

    /**
     * The user clicked "Reject" on the phone. Send `device.pair.confirm(false)`
     * and transition to Failed.
     */
    fun rejectByUser(reason: String = "用户在手机上拒绝") {
        val env = pairing.userRejects(reason) ?: return
        client.send(env)
    }

    /**
     * The user clicked "开始配对" next to the manual host/port entry.
     * Sends `device.pair.request` to the desktop — the phone is the
     * initiator, the desktop's user will see an incoming-pairing
     * banner and click Accept/Reject. No verification code is
     * generated for this direction (per the project threat model:
     * the phone is the trusted UI surface).
     */
    fun initiatePairing() {
        val env = pairing.initiate() ?: return
        client.send(env)
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PairingScreen(
    onOpenAddByIp: () -> Unit = {},
    onOpenSettings: () -> Unit = {},
    onOpenDrawer: () -> Unit = {},
    vm: PairingViewModel = hiltViewModel()
) {
    val ctx = LocalContext.current
    val status by vm.bridgeStatus.collectAsState()
    val discovered by vm.discovered.collectAsState()
    val isRefreshing by vm.isRefreshing.collectAsState()
    val pullState = rememberPullToRefreshState()
    var overflowOpen by remember { mutableStateOf(false) }

    LaunchedEffect(Unit) {
        // Start the foreground service so it picks up the host/port
        // from prefs once NSD finds (or the user types) the desktop.
        val i = Intent(ctx, BridgeService::class.java)
        try { ctx.startForegroundService(i) } catch (_: Throwable) {}
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.pairing_title)) },
                navigationIcon = {
                    IconButton(onClick = onOpenDrawer) {
                        Icon(
                            imageVector = androidx.compose.material.icons.Icons.Filled.Menu,
                            contentDescription = stringResource(R.string.menu_open),
                        )
                    }
                },
                actions = {
                    // Overflow menu (right side). Houses the
                    // manual / trust controls that used to live
                    // as inline cards on this screen, plus a
                    // pull-down-refresh shortcut.
                    IconButton(onClick = { overflowOpen = true }) {
                        Icon(
                            imageVector = Icons.Filled.MoreVert,
                            contentDescription = stringResource(R.string.pairing_menu_more),
                        )
                    }
                    androidx.compose.material3.DropdownMenu(
                        expanded = overflowOpen,
                        onDismissRequest = { overflowOpen = false },
                    ) {
                        androidx.compose.material3.DropdownMenuItem(
                            text = { Text(stringResource(R.string.pairing_menu_refresh)) },
                            leadingIcon = {
                                Icon(
                                    imageVector = Icons.Filled.Refresh,
                                    contentDescription = null,
                                )
                            },
                            onClick = {
                                overflowOpen = false
                                vm.refreshDesktops()
                            },
                        )
                        androidx.compose.material3.DropdownMenuItem(
                            text = { Text(stringResource(R.string.pairing_menu_add_by_ip)) },
                            leadingIcon = {
                                Icon(
                                    imageVector = Icons.Filled.Add,
                                    contentDescription = null,
                                )
                            },
                            onClick = {
                                overflowOpen = false
                                onOpenAddByIp()
                            },
                        )
                        androidx.compose.material3.DropdownMenuItem(
                            text = { Text(stringResource(R.string.pairing_menu_trusted_networks)) },
                            leadingIcon = {
                                Icon(
                                    imageVector = Icons.Filled.Wifi,
                                    contentDescription = null,
                                )
                            },
                            onClick = {
                                overflowOpen = false
                                onOpenSettings()
                            },
                        )
                    }
                },
            )
        }
    ) { pad ->
        PullToRefreshBox(
            isRefreshing = isRefreshing,
            onRefresh = { vm.refreshDesktops() },
            state = pullState,
            modifier = Modifier
                .fillMaxSize()
                .padding(pad),
        ) {
            Column(
                modifier = Modifier
                    .fillMaxSize()
                    .verticalScroll(rememberScrollState())
                    .padding(20.dp),
                verticalArrangement = Arrangement.spacedBy(16.dp),
                horizontalAlignment = Alignment.CenterHorizontally
            ) {
                Text("Bridge: ${statusLabel(status)}", style = MaterialTheme.typography.bodyMedium)

                // Card: discovered desktops (mDNS browse)
                Card(
                    modifier = Modifier.fillMaxWidth(),
                    colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainer)
                ) {
                    Column(modifier = Modifier.padding(16.dp)) {
                        Text(
                            stringResource(R.string.pairing_discovered_title),
                            style = MaterialTheme.typography.titleMedium
                        )
                        Spacer(Modifier.height(4.dp))
                        Text(
                            stringResource(R.string.pairing_discovered_hint),
                            style = MaterialTheme.typography.bodySmall,
                            color = MaterialTheme.colorScheme.onSurfaceVariant
                        )
                        Spacer(Modifier.height(8.dp))
                        if (discovered.isEmpty()) {
                            Text(
                                stringResource(R.string.pairing_discovered_empty),
                                style = MaterialTheme.typography.bodySmall,
                                color = MaterialTheme.colorScheme.onSurfaceVariant
                            )
                        } else {
                            Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                                discovered.values
                                    .sortedBy { it.name.lowercase() }
                                    .forEach { d ->
                                        DiscoveredDesktopRow(
                                            desktop = d,
                                            onPair = { vm.pairWithDiscovered(d) }
                                        )
                                    }
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun DiscoveredDesktopRow(
    desktop: DiscoveredDesktop,
    onPair: () -> Unit,
) {
    Row(
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(8.dp),
        modifier = Modifier.fillMaxWidth()
    ) {
        Column(modifier = Modifier.weight(1f)) {
            Text(
                desktop.name,
                style = MaterialTheme.typography.bodyLarge,
            )
            Text(
                if (desktop.host != null) "${desktop.host}:${desktop.port}"
                else "resolving…",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        Button(
            onClick = onPair,
            enabled = desktop.host != null && !desktop.isResolving,
        ) { Text(stringResource(R.string.pairing_pair_btn)) }
    }
}

@Composable
internal fun BigCode(code: String) {
    Text(
        text = code,
        fontSize = 44.sp,
        fontWeight = FontWeight.Black,
        fontFamily = FontFamily.Monospace,
        modifier = Modifier.padding(vertical = 8.dp)
    )
}

private fun statusLabel(s: BridgeStatus): String = when (s) {
    is BridgeStatus.Disconnected -> "已断开"
    is BridgeStatus.Connecting -> "正在连接 ${s.host}:${s.port}"
    is BridgeStatus.Connected -> "已连接 ${s.host}:${s.port}"
    is BridgeStatus.Error -> "错误：${s.message}"
}
