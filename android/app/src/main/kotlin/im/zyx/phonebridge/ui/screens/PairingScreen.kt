package im.zyx.phonebridge.ui.screens

import android.content.Intent
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.ExperimentalMaterial3Api
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
import im.zyx.phonebridge.core.protocol.DeviceInfo
import im.zyx.phonebridge.network.BridgeClient
import im.zyx.phonebridge.network.BridgeStatus
import im.zyx.phonebridge.network.NsdRegistrar
import im.zyx.phonebridge.network.BridgeService
import im.zyx.phonebridge.pairing.PairingMachine
import im.zyx.phonebridge.pairing.PairingState
import im.zyx.phonebridge.data.PrefsRepository
import javax.inject.Inject
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch

@HiltViewModel
class PairingViewModel @Inject constructor(
    private val nsd: NsdRegistrar,
    private val prefs: PrefsRepository,
    private val pairing: PairingMachine,
    val client: BridgeClient
) : ViewModel() {

    private val _discovered = MutableStateFlow<Pair<String, Int>?>(null)
    val discovered = _discovered
    val pairingState = pairing.state
    val bridgeStatus = client.status

    init {
        viewModelScope.launch {
            nsd.discoverFirstDesktop().collect { (host, port) ->
                _discovered.value = host to port
            }
        }
    }

    fun pairNow() {
        val d = _discovered.value ?: return
        viewModelScope.launch {
            prefs.setDesktop(d.first, d.second)
            pairing.reset()
            pairing.begin(
                ourDeviceId = "android",
                desktopDeviceId = "daemon",
                code = PairingMachine.generateCode(),
                desktopInfo = DeviceInfo("daemon", "Desktop", "?", "?", "0.1.0")
            )
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PairingScreen(
    onOpenSettings: () -> Unit,
    vm: PairingViewModel = hiltViewModel()
) {
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    val discovered by vm.discovered.collectAsState()
    val state by vm.pairingState.collectAsState()
    val status by vm.bridgeStatus.collectAsState()

    LaunchedEffect(Unit) {
        // start the foreground service so it picks up the host/port
        // from prefs once we set it
        val i = Intent(ctx, BridgeService::class.java)
        try { ctx.startForegroundService(i) } catch (_: Throwable) {}
    }

    Scaffold(
        topBar = {
            TopAppBar(title = { Text("Pair with desktop") })
        }
    ) { pad ->
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(pad)
                .padding(20.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
            horizontalAlignment = Alignment.CenterHorizontally
        ) {
            Text("Bridge status: ${statusLabel(status)}", style = MaterialTheme.typography.bodyMedium)

            Card(
                modifier = Modifier.fillMaxWidth(),
                colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surface)
            ) {
                Column(modifier = Modifier.padding(16.dp)) {
                    Text("Step 1: discover desktop", style = MaterialTheme.typography.titleMedium)
                    Spacer(Modifier.height(6.dp))
                    if (discovered == null) {
                        Text("Searching the LAN for PhoneBridge daemons…")
                        Spacer(Modifier.height(6.dp))
                        LinearProgressIndicator(modifier = Modifier.fillMaxWidth())
                    } else {
                        val (h, p) = discovered!!
                        Text("Found: $h:$p", fontWeight = FontWeight.Bold)
                    }
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
                        is PairingState.Idle ->
                            Button(
                                onClick = { vm.pairNow() },
                                enabled = discovered != null,
                                modifier = Modifier.fillMaxWidth()
                            ) { Text("Generate code") }
                        is PairingState.AwaitingDesktop -> {
                            BigCode(s.code)
                            Text("Type this code on your desktop, then accept the prompt.")
                        }
                        is PairingState.ChallengeReceived -> {
                            BigCode(s.code)
                            Text("Confirm on desktop…")
                        }
                        is PairingState.Paired -> {
                            Text("Paired. Bridge is live.", color = MaterialTheme.colorScheme.primary)
                        }
                        is PairingState.Failed -> {
                            Text("Failed: ${s.reason}", color = MaterialTheme.colorScheme.error)
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
        modifier = Modifier
            .padding(vertical = 8.dp)
    )
}

private fun statusLabel(s: BridgeStatus): String = when (s) {
    is BridgeStatus.Disconnected -> "Disconnected"
    is BridgeStatus.Connecting -> "Connecting to ${s.host}:${s.port}"
    is BridgeStatus.Connected -> "Connected to ${s.host}:${s.port}"
    is BridgeStatus.Error -> "Error: ${s.message}"
}
