package im.zyx.phonebridge.keepalive

import android.util.Log
import im.zyx.phonebridge.core.protocol.DeviceHeartbeatPayload
import im.zyx.phonebridge.core.protocol.Envelope
import im.zyx.phonebridge.core.protocol.MessageType
import im.zyx.phonebridge.core.protocol.json
import im.zyx.phonebridge.network.BridgeClient
import im.zyx.phonebridge.network.BridgeStatus
import im.zyx.phonebridge.pairing.PairingMachine
import java.time.Instant
import java.util.UUID
import java.util.concurrent.atomic.AtomicInteger
import javax.inject.Inject
import javax.inject.Singleton
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withTimeoutOrNull

private const val TAG = "Heartbeat"

/**
 * Application-level heartbeat on top of the TLS/WS layer's built-in
 * OkHttp pingInterval. Even when the TCP socket is alive (proxied
 * pings succeed), the message-center's business logic may have stopped
 * processing envelopes (e.g. wedged DB write, lost pairing session,
 * expired cert). The only reliable way to know is to ask the
 * message-center to echo a uniquely-identifiable message and time the
 * round-trip.
 *
 * Cadence and policy are taken straight from docs/protocol-v1.md:
 *   - 30 s between heartbeats
 *   - 3 consecutive misses ⇒ force reconnect
 *   - The message-center MUST respond to `device.heartbeat` with a
 *     `device.heartbeat` carrying the same envelope id (it's a
 *     pure echo on the server side, see
 *     `crates/phonebridge-net/src/ws_handler.rs`).
 */
@Singleton
class HeartbeatController @Inject constructor(
    private val client: BridgeClient,
    private val pairing: PairingMachine,
) {
    private val scope = CoroutineScope(Dispatchers.IO + SupervisorJob())
    private var job: Job? = null
    private val missed = AtomicInteger(0)
    private val sentAt: MutableMap<String, Long> = mutableMapOf()

    fun start() {
        if (job?.isActive == true) return
        job = scope.launch { runLoop() }
    }

    fun stop() {
        job?.cancel()
        job = null
        missed.set(0)
        synchronized(sentAt) { sentAt.clear() }
    }

    private suspend fun runLoop() {
        // Wait until we're actually connected before firing the
        // first heartbeat. This avoids piling up misses during the
        // initial handshake and during reconnects.
        client.status.first { it is BridgeStatus.Connected }
        while (scope.isActive) {
            val id = sendOne()
            if (id == null) {
                // send queue is full; treat as a miss.
                bumpMissed()
                delay(INTERVAL_MS)
                continue
            }
            val rtt = withTimeoutOrNull(ECHO_TIMEOUT_MS) {
                client.heartbeatEchoes.first { it == id }
                System.currentTimeMillis() - sentAtFor(id)
            }
            if (rtt == null) {
                val n = missed.incrementAndGet()
                Log.w(TAG, "heartbeat $id timed out; miss=$n")
                if (n >= MISSED_BEFORE_RECONNECT) {
                    Log.w(TAG, "$n consecutive misses; forcing reconnect")
                    client.forceReconnect()
                    missed.set(0)
                    // Wait until we're connected again before
                    // resuming the heartbeat loop.
                    client.status.first { it is BridgeStatus.Connected }
                }
            } else {
                Log.d(TAG, "heartbeat $id echo rtt=${rtt}ms")
                missed.set(0)
            }
            delay(INTERVAL_MS)
        }
    }

    @Synchronized
    private fun sentAtFor(id: String): Long = sentAt[id] ?: 0L

    private fun sendOne(): String? {
        val id = UUID.randomUUID().toString()
        val env = Envelope(
            v = 1,
            id = id,
            ts = Instant.now().toEpochMilli(),
            type = MessageType.DEVICE_HEARTBEAT,
            device_id = pairing.ourDeviceId(),
            payload = json.encodeToJsonElement(
                DeviceHeartbeatPayload.serializer(),
                DeviceHeartbeatPayload(rtt_ms = null),
            ),
        )
        val ok = client.send(env)
        if (ok) {
            synchronized(sentAt) { sentAt[id] = System.currentTimeMillis() }
        }
        return if (ok) id else null
    }

    private fun bumpMissed() {
        val n = missed.incrementAndGet()
        if (n >= MISSED_BEFORE_RECONNECT) {
            Log.w(TAG, "$n send-queue misses; forcing reconnect")
            client.forceReconnect()
            missed.set(0)
        }
    }

    companion object {
        const val INTERVAL_MS: Long = 30_000L
        const val ECHO_TIMEOUT_MS: Long = 5_000L
        const val MISSED_BEFORE_RECONNECT: Int = 3
    }
}
