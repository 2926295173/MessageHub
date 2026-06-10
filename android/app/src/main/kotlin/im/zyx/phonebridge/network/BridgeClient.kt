package im.zyx.phonebridge.network

import android.util.Log
import im.zyx.phonebridge.core.protocol.DeviceHelloPayload
import im.zyx.phonebridge.core.protocol.DeviceType
import im.zyx.phonebridge.core.protocol.Envelope
import im.zyx.phonebridge.core.protocol.MessageType
import im.zyx.phonebridge.core.protocol.json
import io.ktor.client.HttpClient
import io.ktor.client.engine.okhttp.OkHttp
import io.ktor.client.plugins.websocket.WebSockets
import io.ktor.client.plugins.websocket.webSocketSession
import io.ktor.client.request.url
import io.ktor.websocket.DefaultWebSocketSession
import io.ktor.websocket.Frame
import io.ktor.websocket.readText
import io.ktor.websocket.send
import okhttp3.OkHttpClient
import java.security.MessageDigest
import java.security.cert.X509Certificate
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicReference
import javax.inject.Inject
import javax.inject.Singleton
import javax.net.ssl.SSLContext
import javax.net.ssl.TrustManager
import javax.net.ssl.X509TrustManager
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.decodeFromJsonElement

private const val TAG = "BridgeClient"

/**
 * High-level state of the bridge client.
 */
sealed interface BridgeStatus {
    data object Disconnected : BridgeStatus
    data class Connecting(val host: String, val port: Int) : BridgeStatus
    data class Connected(val host: String, val port: Int, val fingerprint: String) : BridgeStatus
    data class Error(val message: String) : BridgeStatus
}

/**
 * Persistent Ktor-OkHttp WebSocket client to the desktop message-center.
 *
 * Lifecycle:
 * - [start] launches a long-running coroutine that tries to (re)connect
 *   with exponential backoff while [running] is true.
 * - Incoming envelopes are pushed to [incoming] (a hot SharedFlow).
 * - Outgoing envelopes are queued via [send] (a Channel).
 * - Pairing is driven from outside; this class is a transport.
 *
 * Liveness:
 * - The OkHttp engine is configured with `pingInterval(20s)` so
 *   middlebox / NAT idle timers don't silently drop the socket. A
 *   missed pong is reported as a connection failure, which kicks
 *   the reconnect loop immediately.
 * - [heartbeatEchoes] is a hot SharedFlow of (envelopeId → rttMs)
 *   pairs that [im.zyx.phonebridge.keepalive.HeartbeatController]
 *   uses to detect a stuck session (message-center responsive to TCP but
 *   not echoing our app-level heartbeats).
 * - [forceReconnect] is called by the heartbeat controller after
 *   `MISSED_HEARTBEATS_BEFORE_RECONNECT` consecutive misses.
 *
 * Transport:
 * - Defaults to **plain `ws://`** (matches the message-center's `--no-tls`
 *   mode). To use TLS, pass [start] a non-null [pinnedFingerprint]
 *   — the engine will then build a `wss://` client with the
 *   existing PinnedTrustManager TOFU logic. The same URL host:port
 *   is used; only the scheme flips.
 */
@Singleton
open class BridgeClient @Inject constructor() {

    private val _status = MutableStateFlow<BridgeStatus>(BridgeStatus.Disconnected)
    open val status: StateFlow<BridgeStatus> = _status.asStateFlow()

    /**
     * Display name of the desktop we're currently connected to,
     * captured from its `device.hello` envelope. Null when
     * disconnected, connecting, or the hello hasn't arrived yet.
     *
     * The desktop sends `device.hello` on every accepted WebSocket
     * (per protocol v1, the same envelope shape used by the phone),
     * so this is the canonical name for the bridge end on the LAN.
     * It's exposed as a hot StateFlow so UI layers can react
     * immediately to name changes (e.g., user renames the daemon
     * host and restarts it).
     */
    private val _desktopName = MutableStateFlow<String?>(null)
    open val desktopName: StateFlow<String?> = _desktopName.asStateFlow()

    private val _incoming = MutableSharedFlow<Envelope>(extraBufferCapacity = 64)
    open val incoming: SharedFlow<Envelope> = _incoming.asSharedFlow()

    /**
     * Emits the id of every `device.heartbeat` we receive back from
     * the message-center. Consumed by [HeartbeatController] to await an
     * echo for a given send and measure the round-trip.
     */
    private val _heartbeatEchoes = MutableSharedFlow<String>(extraBufferCapacity = 16)
    open val heartbeatEchoes: SharedFlow<String> = _heartbeatEchoes.asSharedFlow()

    private val outgoing = Channel<Envelope>(capacity = 64)
    private val running = AtomicBoolean(false)
    private val supervisor = SupervisorJob()
    private val scope = CoroutineScope(Dispatchers.IO + supervisor)
    private var loopJob: Job? = null

    open fun start(initialHost: String, initialPort: Int, pinnedFingerprint: String?) {
        if (!running.compareAndSet(false, true)) return
        loopJob?.cancel()
        loopJob = scope.launch { runLoop(initialHost, initialPort, pinnedFingerprint) }
    }

    open fun stop() {
        running.set(false)
        loopJob?.cancel()
        loopJob = null
        _status.value = BridgeStatus.Disconnected
    }

    /**
     * Bypass the backoff and reconnect on the next scheduler tick.
     * Called by [HeartbeatController] when it has decided the
     * connection is stale.
     */
    open fun forceReconnect() {
        val current = loopJob
        if (current != null && current.isActive) {
            current.cancel()
        }
        // The next call to `start` will see running==true and the
        // previous loopJob is already cancelled, so a fresh loop is
        // launched immediately. We must re-arm the flag though:
        running.set(false)
    }

    open fun send(envelope: Envelope): Boolean {
        val ok = outgoing.trySend(envelope).isSuccess
        if (!ok) Log.w(TAG, "outgoing channel full, dropping ${envelope.type}")
        return ok
    }

    private suspend fun runLoop(initialHost: String, initialPort: Int, pinnedFingerprint: String?) {
        var attempt = 0
        var host = initialHost
        var port = initialPort
        // Reset the desktop-name slot on every (re)connect. The new
        // session will repopulate it as soon as the daemon's hello
        // arrives; until then the UI shows the fallback host:port.
        _desktopName.value = null
        while (running.get()) {
            _status.value = BridgeStatus.Connecting(host, port)
            try {
                runOnce(host, port, pinnedFingerprint)
                attempt = 0
            } catch (t: Throwable) {
                if (!running.get()) break
                Log.w(TAG, "connection failed: ${t.message}")
                _status.value = BridgeStatus.Error(t.message ?: t::class.simpleName ?: "error")
            }
            if (!running.get()) break
            attempt = (attempt + 1).coerceAtMost(8)
            val delayMs = 500L * (1L shl attempt) // 1, 2, 4, 8, 16, 32, 64, 128, 256 s
            delay(delayMs)
        }
        _status.value = BridgeStatus.Disconnected
    }

    private suspend fun runOnce(host: String, port: Int, pinnedFingerprint: String?) {
        // Use TLS only when a fingerprint is pinned. The pair is
        // stored together in prefs; flipping the pin to null
        // therefore cleanly drops back to plain ws:// without
        // touching any other code path.
        val useTls = pinnedFingerprint != null
        val scheme = if (useTls) "wss" else "ws"

        val capturer = if (useTls) PinnedTrustManager(pinnedFingerprint) else null

        val ok = if (useTls && capturer != null) {
            val trustManagers = arrayOf<TrustManager>(capturer)
            val sslCtx = SSLContext.getInstance("TLS").apply { init(null, trustManagers, null) }
            OkHttpClient.Builder()
                .sslSocketFactory(sslCtx.socketFactory, capturer)
                .hostnameVerifier { _, _ -> true } // self-signed: hostname irrelevant; pin is the cert
                .connectTimeout(10, TimeUnit.SECONDS)
                .readTimeout(0, TimeUnit.SECONDS)
                .pingInterval(20, TimeUnit.SECONDS)
                .build()
        } else {
            OkHttpClient.Builder()
                .connectTimeout(10, TimeUnit.SECONDS)
                .readTimeout(0, TimeUnit.SECONDS)
                .pingInterval(20, TimeUnit.SECONDS)
                .build()
        }
        val client = HttpClient(OkHttp) {
            install(WebSockets)
            engine { preconfigured = ok }
        }
        try {
            val session = client.webSocketSession { url("$scheme://$host:$port/ws") }
            // For TLS, the cert chain is in capturer.lastChain (set
            // during checkServerTrusted). For plain ws:// there is
            // no chain; report an empty fingerprint.
            val actualFp = capturer?.lastChain?.get()?.firstOrNull()?.let { cert ->
                sha256ColonUpper(cert.encoded)
            } ?: ""
            if (useTls) {
                if (pinnedFingerprint != null) {
                    Log.i(TAG, "TLS pinned fp verified: $actualFp")
                } else {
                    Log.i(TAG, "TLS first-use: captured fp=$actualFp")
                }
            } else {
                Log.i(TAG, "plain ws:// connected (no TLS)")
            }
            _status.value = BridgeStatus.Connected(host, port, actualFp)
            pumpLoop(session)
        } finally {
            client.close()
        }
    }

    private fun sha256ColonUpper(der: ByteArray): String {
        val d = MessageDigest.getInstance("SHA-256").digest(der)
        return d.joinToString(":") { "%02X".format(it.toInt() and 0xFF) }
    }

    private suspend fun pumpLoop(session: DefaultWebSocketSession) {
        val sender = scope.launch {
            for (msg in outgoing) {
                val text = json.encodeToString(Envelope.serializer(), msg)
                session.send(text)
            }
        }
        try {
            for (frame in session.incoming) {
                if (frame !is Frame.Text) continue
                val text = frame.readText()
                val env = runCatching { json.decodeFromString(Envelope.serializer(), text) }
                    .onFailure { Log.w(TAG, "bad envelope: $it; text=$text") }
                    .getOrNull() ?: continue
                if (env.type == MessageType.DEVICE_HEARTBEAT) {
                    _heartbeatEchoes.emit(env.id)
                } else if (env.type == MessageType.DEVICE_HELLO) {
                    // Capture the desktop's display name the moment
                    // its hello arrives. The hello envelope is
                    // bidirectional (the daemon sends one too), so
                    // this also tells us we're past the handshake
                    // and the peer is a real message-center. We
                    // filter on device_type=Desktop to ignore our
                    // own hello mirrored by the daemon in the rare
                    // debug-echo case.
                    runCatching {
                        val hello = json.decodeFromJsonElement(
                            DeviceHelloPayload.serializer(), env.payload
                        )
                        if (hello.device_type == DeviceType.Desktop) {
                            _desktopName.value = hello.name
                        }
                    }.onFailure { Log.w(TAG, "bad device.hello payload: $it") }
                }
                _incoming.emit(env)
            }
        } finally {
            sender.cancel()
        }
    }

    open fun shutdown() {
        stop()
        scope.cancel()
    }
}

/**
 * TLS trust manager for the phone-to-desktop bridge. Performs two jobs:
 *  1. Capture the most recent peer cert chain (for first-use TOFU
 *     reporting via [lastChain]).
 *  2. If a pinned fingerprint was supplied, verify the leaf cert's
 *     SHA-256 against it INSIDE `checkServerTrusted` — i.e. during
 *     the TLS handshake — and throw on mismatch. This aborts the
 *     handshake before any WebSocket frames are exchanged, closing
 *     the MITM hole that a post-handshake check would leave open.
 *
 * The message-center uses a self-signed cert that is not chained from any
 * public CA, so we intentionally do not delegate to the system trust
 * store; the pin is the source of truth. The `hostnameVerifier` in
 * the caller bypasses CN/SAN matching for the same reason.
 */
private class PinnedTrustManager(private val pinnedFingerprint: String?) :
    X509TrustManager {
    val lastChain: AtomicReference<Array<X509Certificate>?> = AtomicReference(null)
    private val accepted = arrayOf<X509Certificate>()

    override fun checkClientTrusted(c: Array<out X509Certificate>, a: String) {
        // Client mode is unused (we are always the TLS client).
    }

    override fun checkServerTrusted(c: Array<out X509Certificate>, a: String) {
        lastChain.set(arrayOf(*c))
        if (pinnedFingerprint != null) {
            val leaf = c.firstOrNull()
                ?: throw java.security.cert.CertificateException("empty server chain")
            val actual = sha256ColonUpperStatic(leaf.encoded)
            if (actual != pinnedFingerprint) {
                throw java.security.cert.CertificateException(
                    "TLS fingerprint mismatch (expected $pinnedFingerprint, got $actual)"
                )
            }
        }
    }

    override fun getAcceptedIssuers(): Array<X509Certificate> = accepted
}

/** Top-level helper to avoid capturing `this` of [BridgeClient]. */
private fun sha256ColonUpperStatic(der: ByteArray): String {
    val d = java.security.MessageDigest.getInstance("SHA-256").digest(der)
    return d.joinToString(":") { "%02X".format(it.toInt() and 0xFF) }
}
