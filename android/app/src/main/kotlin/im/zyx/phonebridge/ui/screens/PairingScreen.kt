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
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.hilt.navigation.compose.hiltViewModel
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import dagger.hilt.android.lifecycle.HiltViewModel
import im.zyx.phonebridge.network.BridgeClient
import im.zyx.phonebridge.network.BridgeService
import im.zyx.phonebridge.network.BridgeStatus
import im.zyx.phonebridge.network.NsdRegistrar
import im.zyx.phonebridge.pairing.PairingMachine
import im.zyx.phonebridge.pairing.PairingState
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
    }

    fun reset() {
        pairing.reset()
    }

    fun saveManualDesktop(host: String, port: Int) {
        viewModelScope.launch { prefs.setDesktop(host, port) }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PairingScreen(
    onOpenSettings: () -> Unit,
    vm: PairingViewModel = hiltViewModel()
) {
    val ctx = LocalContext.current
    val state by vm.pairingState.collectAsState()
    val status by vm.bridgeStatus.collectAsState()

    var manualHost by remember { mutableStateOf("") }
    var manualPort by remember { mutableStateOf("") }

    LaunchedEffect(Unit) {
        // Start the foreground service so it picks up the host/port
        // from prefs once NSD finds (or the user types) the desktop.
        val i = Intent(ctx, BridgeService::class.java)
        try { ctx.startForegroundService(i) } catch (_: Throwable) {}
    }

    Scaffold(
        topBar = { TopAppBar(title = { Text("Pair with desktop") }) }
    ) { pad ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(pad)
                .padding(20.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
            horizontalAlignment = Alignment.CenterHorizontally
        ) {
            Text("Bridge: ${statusLabel(status)}", style = MaterialTheme.typography.bodyMedium)

            Card(
                modifier = Modifier.fillMaxWidth(),
                colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface)
            ) {
                Column(modifier = Modifier.padding(16.dp)) {
                    Text("Step 1: desktop connection", style = MaterialTheme.typography.titleMedium)
                    Spacer(Modifier.height(6.dp))
                    when (status) {
                        is BridgeStatus.Connecting -> {
                            Text("Connecting…")
                            Spacer(Modifier.height(6.dp))
                            LinearProgressIndicator(modifier = Modifier.fillMaxWidth())
                        }
                        is BridgeStatus.Connected -> {
                            val s = status as BridgeStatus.Connected
                            Text("Connected to ${s.host}:${s.port}")
                        }
                        is BridgeStatus.Error -> Text("Error: ${(status as BridgeStatus.Error).message}")
                        is BridgeStatus.Disconnected -> {
                            Text("Auto-discover via mDNS is not always reliable on every LAN. " +
                                 "Type the desktop's IP and port below as a fallback.")
                        }
                    }
                    Spacer(Modifier.height(8.dp))
                    Row(
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(8.dp)
                    ) {
                        OutlinedTextField(
                            value = manualHost,
                            onValueChange = { manualHost = it.trim() },
                            label = { Text("Host") },
                            singleLine = true,
                            modifier = Modifier.weight(2f)
                        )
                        OutlinedTextField(
                            value = manualPort,
                            onValueChange = { manualPort = it.trim() },
                            label = { Text("Port") },
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
                    ) { Text("Use this desktop") }
                }
            }

            Card(
                modifier = Modifier.fillMaxWidth(),
                colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface)
            ) {
                Column(modifier = Modifier.padding(16.dp)) {
                    Text("Step 2: 6-digit code", style = MaterialTheme.typography.titleMedium)
                    Spacer(Modifier.height(6.dp))
                    when (val s = state) {
                        is PairingState.Idle -> Text(
                            "When the desktop clicks \"Pair\", a 6-digit code will appear here. " +
                                "Type it on the desktop to accept."
                        )
                        is PairingState.ShowingCode -> {
                            BigCode(s.code)
                            Text(
                                "Type this code on the desktop, then click Accept there.",
                                style = MaterialTheme.typography.bodyMedium
                            )
                            Text(
                                "Expires in ${((s.expiresAtMs - System.currentTimeMillis()) / 1000).coerceAtLeast(0)}s",
                                style = MaterialTheme.typography.labelSmall
                            )
                        }
                        is PairingState.Confirming -> {
                            BigCode(s.code)
                            Text("Confirming on desktop…")
                        }
                        is PairingState.Paired -> {
                            Text(
                                "Paired. Bridge is live.",
                                color = MaterialTheme.colorScheme.primary
                            )
                            Text("Peer fingerprint: ${s.peerFingerprint}",
                                style = MaterialTheme.typography.labelSmall)
                        }
                        is PairingState.Failed -> {
                            Text("Failed: ${s.reason}", color = MaterialTheme.colorScheme.error)
                            Spacer(Modifier.height(6.dp))
                            OutlinedButton(onClick = { vm.reset() }) { Text("Try again") }
                        }
                    }
                }
            }

            OutlinedButton(
                onClick = onOpenSettings,
                modifier = Modifier.fillMaxWidth()
            ) { Text("Open settings") }
        }
    }
}

@Composable
private fun BigCode(code: String) {
    Text(
        text = code,
        fontSize = 44.sp,
        fontWeight = FontWeight.Black,
        fontFamily = FontFamily.Monospace,
        modifier = Modifier.padding(vertical = 8.dp)
    )
}

private fun statusLabel(s: BridgeStatus): String = when (s) {
    is BridgeStatus.Disconnected -> "Disconnected"
    is BridgeStatus.Connecting -> "Connecting to ${s.host}:${s.port}"
    is BridgeStatus.Connected -> "Connected to ${s.host}:${s.port}"
    is BridgeStatus.Error -> "Error: ${s.message}"
}
