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
import im.zyx.phonebridge.keepalive.HeartbeatController
import im.zyx.phonebridge.pairing.PairingMachine
import im.zyx.phonebridge.ui.MainActivity
import java.security.MessageDigest
import java.time.Instant
import javax.inject.Inject
import javax.net.ssl.HostnameVerifier
import javax.net.ssl.HttpsURLConnection
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.first
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
 *     and we don't yet have a message-center fingerprint pinned, fetches
 *     `https://host:port/api/v1/cert` and stores the SHA-256
 *     fingerprint in prefs (TOFU pinning).
 *  3. Starts the [BridgeClient], which now pins the cert by
 *     fingerprint and refuses to connect if the message-center rotates its
 *     self-signed cert.
 *  4. On Connected transition, sends `device.hello` with the
 *     device's stable identity.
 *  5. Dispatches every incoming envelope: pairing messages go to
 *     [PairingMachine]; the rest are logged.
 *  6. Posts a low-priority persistent notification.
 *
 * Hardening (M6):
 *  - [onStartCommand] returns `START_STICKY` so a process kill by
 *    the OS (e.g. low-memory) re-creates the service.
 *  - [setStopWithTask] is called with `false` so swiping the app
 *    out of recents does not stop the service.
 *  - [HeartbeatController] is started when the service comes up
 *    and stopped on destroy.
 */
@AndroidEntryPoint
class BridgeService : LifecycleService() {

    @Inject lateinit var client: BridgeClient
    @Inject lateinit var prefs: PrefsRepository
    @Inject lateinit var pairing: PairingMachine
    @Inject lateinit var nsd: NsdRegistrar
    @Inject lateinit var heartbeat: HeartbeatController

    override fun onCreate() {
        super.onCreate()
        pairing.ensureIdentity("phonebridge-android")
        startInForeground(getString(R.string.notif_status_idle))
        observe()
        kickoff()
        heartbeat.start()
    }

    /**
     * Swiping the app out of recents must NOT stop the service. The
     * default `Service` implementation calls `stopSelf()`, which
     * would tear down the WS connection. We deliberately do nothing
     * here and rely on `START_STICKY` + the foreground-service
     * notification to keep the process alive.
     */
    override fun onTaskRemoved(rootIntent: Intent?) {
        // Intentionally no-op: keep the service running.
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        // START_STICKY: if the process is killed by the system
        // (Doze / low-memory), Android will redeliver a null intent
        // to re-create this service. onCreate() will then re-run
        // the kickoff() flow and reconnect.
        super.onStartCommand(intent, flags, startId)
        return START_STICKY
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
        // minSdk = 29, so Q-gated API is always available.
        startForeground(NOTIF_ID, notif, ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE)
    }


    private fun observe() {
        lifecycleScope.launch {
            // The status-bar line needs both the connection state and
            // the desktop's self-reported name (we get it from its
            // `device.hello`, captured by BridgeClient.desktopName).
            // Combine the two flows so the notification updates the
            // instant either changes — e.g. the WS connects, then ~10ms
            // later the hello lands and the line flips from
            // "已连接到 192.168.123.186:8080" to "已连接到 pve-1".
            combine(client.status, client.desktopName) { s, name -> s to name }
                .collectLatest { (s, desktopName) ->
                    Log.d(TAG, "status: $s, desktopName: $desktopName")
                    val text = when (s) {
                        is BridgeStatus.Disconnected -> getString(R.string.notif_status_idle)
                        is BridgeStatus.Connecting -> getString(
                            R.string.notif_status_connecting,
                            "${s.host}:${s.port}",
                        )
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
                            // Prefer the human-readable desktop name from
                            // its device.hello; fall back to host:port
                            // during the brief window between TCP-up
                            // and hello-arrived.
                            val label = desktopName?.takeIf { it.isNotBlank() }
                                ?: "${s2.host}:${s2.port}"
                            getString(R.string.notif_status_connected, label)
                        }
                        is BridgeStatus.Error -> getString(
                            R.string.notif_status_error,
                            s.message,
                        )
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
            MessageType.DEVICE_PAIR_CONFIRM -> {
                // Could be either: desktop acknowledging our pair.request
                // (Initiating flow), or some other state. Route to the
                // initiator-side handler which is permissive about
                // being in any state other than Initiating.
                val reply = pairing.onInitiatorConfirm(env, ourDeviceId = pairing.ourDeviceId()) ?: return
                client.send(reply)
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

    private suspend fun sendHello() {
        val ourId = pairing.ourDeviceId()
        val pubB64 = try {
            pairing.longTermPublicBase64
        } catch (t: Throwable) {
            Log.w(TAG, "no long-term key: $t")
            "stub"
        }
        // Honor the user-configured name (set in Settings); fall back
        // to Build.MODEL when nothing has been customized.
        val customName = prefs.deviceName.first().orEmpty().trim()
        val displayName = customName.ifEmpty { android.os.Build.MODEL ?: "Android" }
        val hello = Envelope(
            v = 1,
            id = java.util.UUID.randomUUID().toString(),
            ts = Instant.now().toEpochMilli(),
            type = MessageType.DEVICE_HELLO,
            device_id = ourId,
            payload = json.encodeToJsonElement(
                DeviceHelloPayload.serializer(),
                DeviceHelloPayload(
                    name = displayName,
                    device_type = DeviceType.Android,
                    protocol_version = 1,
                    pubkey = pubB64,
                    port = null,
                    manufacturer = android.os.Build.MANUFACTURER,
                    model = android.os.Build.MODEL,
                    hardware_id = readAndroidId(),
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
        heartbeat.stop()
        client.shutdown()
        super.onDestroy()
    }

    /**
     * Stable per-physical-device identifier sent in `device.hello`
     * so the message-center can dedupe reconnects that come with a freshly
     * minted `device_id` UUID (the UUID is wiped on `pm clear`
     * because the Keystore is wiped, but `ANDROID_ID` survives
     * `pm clear` — it is keyed by the app's signing key, not by
     * DataStore).
     *
     * `Settings.Secure.ANDROID_ID` has been the per-app stable id
     * since Android 8 (API 26); the device this app targets is
     * API 29+ so it is always available.
     *
     * Returns null on any failure (e.g. the system has revoked
     * the read-access) so the message-center can fall back to deduping on
     * `device_id` — never crash the bridge over a missing id.
     */
    private fun readAndroidId(): String? = try {
        @Suppress("HardwareIds")
        android.provider.Settings.Secure.getString(
            contentResolver,
            android.provider.Settings.Secure.ANDROID_ID,
        )
    } catch (t: Throwable) {
        Log.w(TAG, "readAndroidId failed: ${t.message}")
        null
    }

    companion object {
        const val NOTIF_ID = 0xB12D6E
    }
}
