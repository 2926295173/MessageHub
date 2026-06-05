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
import im.zyx.phonebridge.core.protocol.DeviceType
import im.zyx.phonebridge.core.protocol.Envelope
import im.zyx.phonebridge.core.protocol.MessageType
import im.zyx.phonebridge.core.protocol.json
import im.zyx.phonebridge.data.PrefsRepository
import im.zyx.phonebridge.pairing.PairingMachine
import im.zyx.phonebridge.ui.MainActivity
import java.time.Instant
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
 *  2. Sends `device.hello` right after the TLS-WS upgrade so the
 *     daemon inserts a Responder session keyed by our device id.
 *  3. Dispatches every incoming envelope: pairing messages go to
 *     [PairingMachine]; the rest are logged.
 *  4. Posts a low-priority persistent notification so the user can
 *     see the bridge is up.
 */
@AndroidEntryPoint
class BridgeService : LifecycleService() {

    @Inject lateinit var client: BridgeClient
    @Inject lateinit var prefs: PrefsRepository
    @Inject lateinit var pairing: PairingMachine
    @Inject lateinit var nsd: NsdRegistrar

    override fun onCreate() {
        super.onCreate()
        pairing.ensureIdentity("phonebridge-android")
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
                        is BridgeStatus.Connected -> {
                            // Send hello on first Connected transition.
                            sendHello()
                            "Connected to ${s.host}:${s.port}"
                        }
                        is BridgeStatus.Error -> "Error: ${s.message}"
                    }
                    refreshNotification(text)
                }
        }
        lifecycleScope.launch {
            pairing.state.collectLatest { s -> Log.d(TAG, "pairing: $s") }
        }
        lifecycleScope.launch {
            client.incoming.collect { env -> handleIncoming(env) }
        }
    }

    private fun handleIncoming(env: Envelope) {
        when (env.type) {
            MessageType.DEVICE_PAIR_REQUEST -> {
                val reply = pairing.onRequest(env) ?: return
                client.send(reply)
            }
            MessageType.DEVICE_PAIR_ACCEPT -> {
                val reply = pairing.onAccept(env, ourDeviceId = pairing.ourDeviceId())
                client.send(reply)
            }
            MessageType.DEVICE_PAIR_REJECT -> {
                pairing.onReject(env)
            }
            MessageType.DEVICE_PAIR_COMPLETE -> {
                val reply = pairing.onComplete(env, ourDeviceId = pairing.ourDeviceId()) ?: return
                client.send(reply)
            }
            else -> Log.d(TAG, "incoming ${env.type} (id=${env.id})")
        }
    }

    private fun kickoff() {
        // Persist the device id the first time we run.
        lifecycleScope.launch {
            val saved = prefs.deviceId.first()
            if (saved == null) prefs.setDeviceId(pairing.ourDeviceId())
        }
        // Wait until the user has stored a desktop host/port in prefs
        // (PairingScreen writes it after a successful mDNS resolve).
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
            }
        }
    }

    private fun sendHello() {
        val ourId = pairing.ourDeviceId()
        val pubB64 = try {
            pairing.longTermPublicBase64
        } catch (t: Throwable) {
            Log.w(TAG, "no long-term key: $t")
            "stub"
        }
        val hello = Envelope(
            v = 1,
            id = java.util.UUID.randomUUID().toString(),
            ts = Instant.now().toEpochMilli(),
            type = MessageType.DEVICE_HELLO,
            device_id = ourId,
            payload = json.encodeToJsonElement(
                DeviceHelloPayload.serializer(),
                DeviceHelloPayload(
                    name = android.os.Build.MODEL ?: "Android",
                    device_type = DeviceType.Android,
                    protocol_version = 1,
                    pubkey = pubB64,
                    port = null,
                    manufacturer = android.os.Build.MANUFACTURER,
                    model = android.os.Build.MODEL
                )
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
