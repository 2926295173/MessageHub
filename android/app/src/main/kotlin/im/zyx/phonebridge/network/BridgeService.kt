package im.zyx.phonebridge.network

import android.app.Notification
import android.app.PendingIntent
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.lifecycle.LifecycleService
import androidx.lifecycle.lifecycleScope
import dagger.hilt.android.AndroidEntryPoint
import im.zyx.phonebridge.PhoneBridgeApp
import im.zyx.phonebridge.R
import im.zyx.phonebridge.core.protocol.DeviceHelloPayload
import im.zyx.phonebridge.core.protocol.DeviceInfo
import im.zyx.phonebridge.core.protocol.Envelope
import im.zyx.phonebridge.core.protocol.MessageType
import im.zyx.phonebridge.core.protocol.json
import im.zyx.phonebridge.data.PrefsRepository
import im.zyx.phonebridge.pairing.PairingMachine
import im.zyx.phonebridge.ui.MainActivity
import java.time.Instant
import java.util.UUID
import javax.inject.Inject
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch

private const val TAG = "BridgeService"

/**
 * Long-lived foreground service that owns the [BridgeClient] connection.
 *
 * On start it:
 *  1. Reads the last known desktop host/port + cert fingerprint from
 *     [PrefsRepository] and kicks the client off.
 *  2. Pumps incoming envelopes into the pairing state machine, dispatching
 *     pair.challenge / pair.result messages to [PairingMachine].
 *  3. Posts a low-priority persistent notification so the user can see
 *     the bridge is up.
 *
 * On stop, the foreground notification is removed and the client is
 * told to shut down.
 */
@AndroidEntryPoint
class BridgeService : LifecycleService() {

    @Inject lateinit var client: BridgeClient
    @Inject lateinit var prefs: PrefsRepository
    @Inject lateinit var pairing: PairingMachine
    @Inject lateinit var nsd: NsdRegistrar

    private val ourDeviceId: String by lazy { UUID.randomUUID().toString() }

    override fun onCreate() {
        super.onCreate()
        startInForeground("Connecting…")
        observe()
        kickoff()
    }

    private fun startInForeground(text: String) {
        val open = Intent(this, MainActivity::class.java)
        val pi = PendingIntent.getActivity(
            this, 0, open,
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        )
        val notif: Notification = NotificationCompat.Builder(this, PhoneBridgeApp.CHANNEL_BRIDGE)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(getString(R.string.app_name))
            .setContentText(text)
            .setOngoing(true)
            .setContentIntent(pi)
            .build()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            startForeground(NOTIF_ID, notif, ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE)
        } else {
            startForeground(NOTIF_ID, notif)
        }
    }

    private fun observe() {
        lifecycleScope.launch {
            client.status.stateIn(this, SharingStarted.Eagerly, BridgeStatus.Disconnected)
                .collectLatest { s ->
                    Log.d(TAG, "status: $s")
                    val text = when (s) {
                        is BridgeStatus.Disconnected -> "Idle"
                        is BridgeStatus.Connecting -> "Connecting to ${s.host}:${s.port}"
                        is BridgeStatus.Connected -> "Connected to ${s.host}:${s.port}"
                        is BridgeStatus.Error -> "Error: ${s.message}"
                    }
                    refreshNotification(text)
                }
        }
        lifecycleScope.launch {
            pairing.state.collectLatest { Log.d(TAG, "pairing: $it") }
        }
        lifecycleScope.launch {
            client.incoming.collect { env -> handleIncoming(env) }
        }
    }

    private suspend fun handleIncoming(env: Envelope) {
        when (env.type) {
            MessageType.DEVICE_PAIR_CHALLENGE -> {
                val confirm = pairing.onChallenge(env) ?: return
                client.send(confirm)
            }
            MessageType.DEVICE_PAIR_RESULT -> {
                pairing.onResult(env, ourDeviceId)
            }
            else -> Log.d(TAG, "incoming ${env.type} (id=${env.id})")
        }
    }

    private fun kickoff() {
        // Ensure we have a stable device id persisted.
        lifecycleScope.launch {
            val saved = prefs.deviceId.first()
            if (saved == null) prefs.setDeviceId(ourDeviceId)
            else Log.d(TAG, "our deviceId = $saved")
        }
        // Wait until the user has stored a desktop host/port in prefs.
        // (PairingScreen writes it after a successful mDNS resolve.)
        lifecycleScope.launch {
            combine(
                prefs.desktopHost,
                prefs.desktopPort,
                prefs.fingerprint
            ) { host, portStr, fp ->
                Triple(host, portStr, fp)
            }.collectLatest { (host, portStr, fp) ->
                if (host.isNullOrBlank() || portStr.isNullOrBlank()) return@collectLatest
                val port = portStr.toIntOrNull() ?: return@collectLatest
                Log.d(TAG, "starting client at $host:$port fp=$fp")
                client.start(host, port, fp)
                sendHello()
            }
        }
    }

    private fun sendHello() {
        val deviceId = ourDeviceId
        val info = DeviceInfo(
            deviceId = deviceId,
            name = android.os.Build.MODEL ?: "Android",
            model = (android.os.Build.MANUFACTURER ?: "?") + " " + (android.os.Build.MODEL ?: "?"),
            osVersion = "Android " + android.os.Build.VERSION.RELEASE,
            appVersion = "0.1.0"
        )
        val hello = Envelope(
            id = UUID.randomUUID().toString(),
            type = MessageType.DEVICE_HELLO,
            from = deviceId,
            to = "daemon",
            ts = Instant.now().toString(),
            payload = json.encodeToJsonElement(
                DeviceHelloPayload.serializer(),
                DeviceHelloPayload(device = info, certificate = "stub")
            )
        )
        client.send(hello)
    }

    private fun refreshNotification(text: String) {
        val open = Intent(this, MainActivity::class.java)
        val pi = PendingIntent.getActivity(
            this, 0, open,
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
        )
        val n = NotificationCompat.Builder(this, PhoneBridgeApp.CHANNEL_BRIDGE)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(getString(R.string.app_name))
            .setContentText(text)
            .setOngoing(true)
            .setContentIntent(pi)
            .build()
        val nm = NotificationManagerCompat.from(this)
        if (nm.areNotificationsEnabled()) nm.notify(NOTIF_ID, n)
    }

    override fun onDestroy() {
        client.shutdown()
        super.onDestroy()
    }

    companion object {
        const val NOTIF_ID = 0xB12D6E
    }
}
