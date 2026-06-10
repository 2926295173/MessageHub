package im.zyx.phonebridge.ui.screens

import android.content.Intent
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
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.filled.Menu
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.hilt.navigation.compose.hiltViewModel
import im.zyx.phonebridge.R
import im.zyx.phonebridge.network.BridgeService
import im.zyx.phonebridge.network.BridgeStatus
import im.zyx.phonebridge.pairing.PairingState

/**
 * "Add device by IP" screen — the user types the message-center's
 * host + port, optionally saves them, and triggers a pairing
 * request. The contents here used to live inline in
 * [PairingScreen] as the "Step 1" card; they were moved to
 * a dedicated route reachable from the pair-screen overflow
 * menu (and from the "通过 IP 添加设备" drawer item) so the
 * main pair screen is just the LAN-discovery surface.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun AddByIpScreen(
    onBack: () -> Unit,
    onOpenDrawer: () -> Unit = {},
    vm: PairingViewModel = hiltViewModel(),
) {
    val ctx = LocalContext.current
    val state by vm.pairingState.collectAsState()
    val status by vm.bridgeStatus.collectAsState()

    var manualHost by remember { mutableStateOf("") }
    var manualPort by remember { mutableStateOf("") }

    // Mirror PairingScreen: as soon as the user lands here, make
    // sure the foreground service is running so any host/port
    // we save is actually dialed.
    androidx.compose.runtime.LaunchedEffect(Unit) {
        val i = Intent(ctx, BridgeService::class.java)
        try { ctx.startForegroundService(i) } catch (_: Throwable) {}
    }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(stringResource(R.string.add_by_ip_title)) },
                navigationIcon = {
                    IconButton(onClick = onBack) {
                        Icon(
                            imageVector = Icons.AutoMirrored.Outlined.ArrowBack,
                            contentDescription = stringResource(R.string.settings_back),
                        )
                    }
                },
            )
        }
    ) { pad ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(pad)
                .verticalScroll(rememberScrollState())
                .padding(20.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            // ── Step 1: connection state (lifted from PairingScreen) ──
            Card(
                modifier = Modifier.fillMaxWidth(),
                shape = RoundedCornerShape(12.dp),
                colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainer)
            ) {
                Column(modifier = Modifier.padding(16.dp)) {
                    Text(
                        stringResource(R.string.pairing_step1_title),
                        style = MaterialTheme.typography.titleMedium
                    )
                    Spacer(Modifier.height(6.dp))
                    when (status) {
                        is BridgeStatus.Connecting -> {
                            Text(stringResource(R.string.pairing_connecting))
                            Spacer(Modifier.height(6.dp))
                            LinearProgressIndicator(modifier = Modifier.fillMaxWidth())
                        }
                        is BridgeStatus.Connected -> {
                            val s = status as BridgeStatus.Connected
                            Text(stringResource(R.string.pairing_connected_to, s.host, s.port))
                        }
                        is BridgeStatus.Error -> Text(
                            stringResource(R.string.pairing_error_prefix) + " ${(status as BridgeStatus.Error).message}"
                        )
                        is BridgeStatus.Disconnected -> {
                            Text(stringResource(R.string.pairing_manual_hint))
                        }
                    }
                }
            }

            // ── Step 1: host/port + use-this-desktop + start pairing ──
            Card(
                modifier = Modifier.fillMaxWidth(),
                shape = RoundedCornerShape(12.dp),
                colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainer)
            ) {
                Column(modifier = Modifier.padding(16.dp)) {
                    Row(
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(8.dp)
                    ) {
                        OutlinedTextField(
                            value = manualHost,
                            onValueChange = { manualHost = it.trim() },
                            label = { Text(stringResource(R.string.pairing_host)) },
                            singleLine = true,
                            modifier = Modifier.weight(2f)
                        )
                        OutlinedTextField(
                            value = manualPort,
                            onValueChange = { manualPort = it.trim() },
                            label = { Text(stringResource(R.string.pairing_port)) },
                            singleLine = true,
                            modifier = Modifier.weight(1f)
                        )
                    }
                    Spacer(Modifier.height(6.dp))
                    OutlinedButton(
                        onClick = {
                            val p = manualPort.toIntOrNull() ?: 8443
                            if (manualHost.isNotBlank()) vm.saveManualDesktop(manualHost, p)
                        },
                        modifier = Modifier.fillMaxWidth()
                    ) { Text(stringResource(R.string.pairing_use_desktop)) }
                    Spacer(Modifier.height(4.dp))
                    Button(
                        onClick = { vm.initiatePairing() },
                        enabled = manualHost.isNotBlank() && (state is PairingState.Idle),
                        modifier = Modifier.fillMaxWidth()
                    ) { Text(stringResource(R.string.pairing_start)) }
                }
            }

            // ── Step 2: code display (lifted from PairingScreen) ──
            Card(
                modifier = Modifier.fillMaxWidth(),
                shape = RoundedCornerShape(12.dp),
                colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainer)
            ) {
                Column(modifier = Modifier.padding(16.dp)) {
                    Text(
                        stringResource(R.string.pairing_step2_title),
                        style = MaterialTheme.typography.titleMedium
                    )
                    Spacer(Modifier.height(6.dp))
                    when (val s = state) {
                        is PairingState.Idle -> Text(stringResource(R.string.pairing_idle_hint))
                        is PairingState.Initiating -> {
                            Text(stringResource(R.string.pairing_initiating_hint))
                            Spacer(Modifier.height(4.dp))
                            OutlinedButton(onClick = { vm.reset() }) {
                                Text(stringResource(R.string.pairing_try_again))
                            }
                        }
                        is PairingState.ShowingCode -> {
                            BigCode(s.code)
                            Text(
                                stringResource(R.string.pairing_showing_hint),
                                style = MaterialTheme.typography.bodyMedium
                            )
                            Text(
                                stringResource(
                                    R.string.pairing_expires_in,
                                    ((s.expiresAtMs - System.currentTimeMillis()) / 1000).coerceAtLeast(0)
                                ),
                                style = MaterialTheme.typography.labelSmall
                            )
                            Spacer(Modifier.height(8.dp))
                            Row(
                                modifier = Modifier.fillMaxWidth(),
                                horizontalArrangement = Arrangement.spacedBy(8.dp)
                            ) {
                                OutlinedButton(
                                    onClick = { vm.rejectByUser() },
                                    modifier = Modifier.weight(1f)
                                ) { Text(stringResource(R.string.pairing_reject)) }
                                Button(
                                    onClick = { vm.acceptByUser() },
                                    modifier = Modifier.weight(1f)
                                ) { Text(stringResource(R.string.pairing_accept)) }
                            }
                        }
                        is PairingState.Confirming -> {
                            BigCode(s.code)
                            Text(stringResource(R.string.pairing_confirming))
                        }
                        is PairingState.Paired -> {
                            Text(
                                stringResource(R.string.pairing_paired),
                                color = MaterialTheme.colorScheme.primary
                            )
                            Text(
                                stringResource(R.string.pairing_peer_fp, s.peerFingerprint),
                                style = MaterialTheme.typography.labelSmall
                            )
                        }
                        is PairingState.Failed -> {
                            Text(
                                stringResource(R.string.pairing_failed_prefix) + " ${s.reason}",
                                color = MaterialTheme.colorScheme.error
                            )
                            Spacer(Modifier.height(6.dp))
                            OutlinedButton(onClick = { vm.reset() }) {
                                Text(stringResource(R.string.pairing_try_again))
                            }
                        }
                    }
                }
            }
        }
    }
}

// BigCode is defined in PairingScreen.kt (made `internal` so
// this file can re-use it without duplication).
