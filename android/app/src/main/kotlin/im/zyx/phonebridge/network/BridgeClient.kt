package im.zyx.phonebridge.network

import android.util.Log
import im.zyx.phonebridge.core.protocol.Envelope
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
 * Persistent Ktor-OkHttp WebSocket client to the desktop daemon.
 *
 * Lifecycle:
 * - [start] launches a long-running coroutine that tries to (re)connect
 *   with exponential backoff while [running] is true.
 * - Incoming envelopes are pushed to [incoming] (a hot SharedFlow).
 * - Outgoing envelopes are queued via [send] (a Channel).
 * - Pairing is driven from outside; this class is a transport.
 */
@Singleton
class BridgeClient @Inject constructor() {

    private val _status = MutableStateFlow<BridgeStatus>(BridgeStatus.Disconnected)
    val status: StateFlow<BridgeStatus> = _status.asStateFlow()

    private val _incoming = MutableSharedFlow<Envelope>(extraBufferCapacity = 64)
    val incoming: SharedFlow<Envelope> = _incoming.asSharedFlow()

    private val outgoing = Channel<Envelope>(capacity = 64)
    private val running = AtomicBoolean(false)
    private val supervisor = SupervisorJob()
    private val scope = CoroutineScope(Dispatchers.IO + supervisor)
    private var loopJob: Job? = null

    fun start(initialHost: String, initialPort: Int, pinnedFingerprint: String?) {
        if (!running.compareAndSet(false, true)) return
        loopJob?.cancel()
        loopJob = scope.launch { runLoop(initialHost, initialPort, pinnedFingerprint) }
    }

    fun stop() {
        running.set(false)
        loopJob?.cancel()
        loopJob = null
        _status.value = BridgeStatus.Disconnected
    }

    fun send(envelope: Envelope) {
        val ok = outgoing.trySend(envelope).isSuccess
        if (!ok) Log.w(TAG, "outgoing channel full, dropping ${envelope.type}")
    }

    private suspend fun runLoop(initialHost: String, initialPort: Int, pinnedFingerprint: String?) {
        var attempt = 0
        var host = initialHost
        var port = initialPort
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
            attempt = (attempt + 1).coerceAtMost(6)
            val delayMs = 500L * (1L shl attempt) // 1, 2, 4, 8, 16, 32, 64 s
            delay(delayMs)
        }
        _status.value = BridgeStatus.Disconnected
    }

    private suspend fun runOnce(host: String, port: Int, pinnedFingerprint: String?) {
        // M5+ fingerprint pinning with TOFU semantics:
        //   - First connect: pinnedFingerprint is null; we capture the
        //     peer's cert and report it via BridgeStatus.Connected so
        //     the caller (BridgeService) can persist it.
        //   - Subsequent connects: pinnedFingerprint is the value
        //     stored in PrefsRepository. We verify the captured cert's
        //     SHA-256 against it; mismatch -> SecurityException.
        val capturer = CapturingTrustManager()
        val trustManagers = arrayOf<TrustManager>(capturer)
        val sslCtx = SSLContext.getInstance("TLS").apply { init(null, trustManagers, null) }

        val ok = OkHttpClient.Builder()
            .sslSocketFactory(sslCtx.socketFactory, capturer)
            .hostnameVerifier { _, _ -> true } // pinning by fingerprint, not hostname
            .connectTimeout(10, TimeUnit.SECONDS)
            .readTimeout(0, TimeUnit.SECONDS) // WebSocket long-lived
            .build()
        val client = HttpClient(OkHttp) {
            install(WebSockets)
            engine { preconfigured = ok }
        }
        try {
            val session = client.webSocketSession { url("wss://$host:$port/ws") }
            // After TLS upgrade, OkHttp invokes checkServerTrusted; the
            // capturer now holds the chain. Verify against the pinned
            // fingerprint.
            val chain = capturer.lastChain.get()
            val actualFp = chain?.firstOrNull()?.let { cert ->
                sha256ColonUpper(cert.encoded)
            }
            if (pinnedFingerprint != null) {
                if (actualFp == null) {
                    throw SecurityException("no peer certificate captured")
                }
                if (actualFp != pinnedFingerprint) {
                    throw SecurityException(
                        "TLS fingerprint mismatch (expected $pinnedFingerprint, got $actualFp)"
                    )
                }
                Log.i(TAG, "TLS pinned fp verified: $actualFp")
            } else {
                Log.i(TAG, "TLS first-use: captured fp=${actualFp ?: "<none>"}")
            }
            _status.value = BridgeStatus.Connected(host, port, actualFp ?: "")
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
                _incoming.emit(env)
            }
        } finally {
            sender.cancel()
        }
    }

    fun shutdown() {
        stop()
        scope.cancel()
    }
}

/**
 * Captures the most recent server cert chain presented to
 * `checkServerTrusted`. Used by [BridgeClient] to enable
 * fingerprint-based TLS pinning: after the WS upgrade, we read
 * `lastChain` and compare the leaf cert's SHA-256 against the value
 * stored in `PrefsRepository.fingerprint`.
 */
private class CapturingTrustManager : X509TrustManager {
    val lastChain: AtomicReference<Array<X509Certificate>?> = AtomicReference(null)
    private val accepted = arrayOf<X509Certificate>()

    override fun checkClientTrusted(c: Array<out X509Certificate>, a: String) {
        lastChain.set(arrayOf(*c))
    }
    override fun checkServerTrusted(c: Array<out X509Certificate>, a: String) {
        lastChain.set(arrayOf(*c))
    }
    override fun getAcceptedIssuers(): Array<X509Certificate> = accepted
}
