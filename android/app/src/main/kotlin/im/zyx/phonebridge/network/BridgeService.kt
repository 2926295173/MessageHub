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
import java.security.MessageDigest
import java.time.Instant
import javax.inject.Inject
import javax.net.ssl.HostnameVerifier
import javax.net.ssl.HttpsURLConnection
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withContext
import java.net.URL

private const val TAG = "BridgeService"

/**
 * Long-lived foreground service that owns the [BridgeClient] connection.
 *
 * On start it:
 *  1. Loads the persisted device id and long-term keypair (via
 *     [PairingMachine] / [PrefsRepository]).
 *  2. Reads the last known desktop host/port from prefs. If present,
 *     and we don't yet have a daemon fingerprint pinned, fetches
 *     `https://host:port/api/v1/cert` and stores the SHA-256
 *     fingerprint in prefs (TOFU pinning).
 *  3. Starts the [BridgeClient], which now pins the cert by
 *     fingerprint and refuses to connect if the daemon rotates its
 *     self-signed cert.
 *  4. On Connected transition, sends `device.hello` with the
 *     device's stable identity.
 *  5. Dispatches every incoming envelope: pairing messages go to
 *     [PairingMachine]; the rest are logged.
 *  6. Posts a low-priority persistent notification.
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
                            sendHello()
                            // TOFU persistence: if we connected without a
                            // pinned fingerprint, persist the captured one
                            // so future reconnects enforce pinning. The
                            // combine() in kickoff() will see the new value
                            // and re-emit, but client.start is idempotent
                            // (early-returns if already running).
                            val s2 = s as BridgeStatus.Connected
                            if (s2.fingerprint.isNotEmpty()) {
                                val cur = runBlocking { prefs.fingerprint.first() }
                                if (cur.isNullOrBlank()) {
                                    Log.i(TAG, "TOFU: persisting fp=${s2.fingerprint}")
                                    runBlocking { prefs.setFingerprint(s2.fingerprint) }
                                }
                            }
                            "Connected to ${s2.host}:${s2.port}"
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

    private suspend fun handleIncoming(env: Envelope) {
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
        // Wait until the user has stored a desktop host/port in prefs
        // (PairingScreen writes it after a successful NSD resolve or
        // manual entry). When the fingerprint is null we let the
        // client TOFU-capture and persist on the first Connected
        // transition; the combine() re-emits and we then start with
        // the pinned fingerprint.
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
