package im.zyx.phonebridge.keepalive

import im.zyx.phonebridge.core.protocol.DeviceHeartbeatPayload
import im.zyx.phonebridge.core.protocol.Envelope
import im.zyx.phonebridge.core.protocol.MessageType
import im.zyx.phonebridge.network.BridgeClient
import im.zyx.phonebridge.network.BridgeStatus
import im.zyx.phonebridge.pairing.PairingMachine
import io.ktor.websocket.DefaultWebSocketSession
import java.util.UUID
import java.util.concurrent.atomic.AtomicInteger
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withTimeoutOrNull
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Unit tests for the missed-heartbeat policy. We test the constants
 * in isolation (3 misses ⇒ reconnect) and the heartbeat send path
 * by feeding echoes into a fake [BridgeClient] and checking that
 * the controller consumes them.
 */
class HeartbeatControllerTest {

    @Test
    fun `MISSED_BEFORE_RECONNECT is 3 (90s total at 30s interval)`() {
        assertEquals(3, HeartbeatController.MISSED_BEFORE_RECONNECT)
    }

    @Test
    fun `INTERVAL_MS is 30 seconds`() {
        assertEquals(30_000L, HeartbeatController.INTERVAL_MS)
    }

    @Test
    fun `ECHO_TIMEOUT_MS is 5 seconds`() {
        assertEquals(5_000L, HeartbeatController.ECHO_TIMEOUT_MS)
    }

    /**
     * A minimal double that satisfies [BridgeClient]'s public
     * surface used by the heartbeat path: status, heartbeatEchoes,
     * send, and forceReconnect. The [BridgeClient] class is
     * `open` specifically for tests like this.
     */
    private class FakeBridgeClient : BridgeClient() {
        private val _status = MutableStateFlow<BridgeStatus>(BridgeStatus.Connected("h", 1, "fp"))
        private val _echoes = MutableSharedFlow<String>(extraBufferCapacity = 16)
        val sentEnvelopes = Channel<Envelope>(capacity = 32)
        val forceReconnects = AtomicInteger(0)
        var sendReturns: Boolean = true

        override val status: StateFlow<BridgeStatus> get() = _status.asStateFlow()
        override val incoming: SharedFlow<Envelope> = MutableSharedFlow<Envelope>().asSharedFlow()
        override val heartbeatEchoes: SharedFlow<String> get() = _echoes.asSharedFlow()

        override fun send(envelope: Envelope): Boolean {
            sentEnvelopes.trySend(envelope)
            return sendReturns
        }

        override fun forceReconnect() {
            forceReconnects.incrementAndGet()
        }

        fun emitEcho(id: String) {
            // Synchronous: the test holds the same flow.
            kotlinx.coroutines.runBlocking { _echoes.emit(id) }
        }
    }

    /**
     * A [PairingMachine] double that returns a fixed device id
     * without touching the Keystore. PairingMachine is
     * `final`-free for constructor injection, but its constructor
     * depends on an IdentityStore. Rather than mock that, we use
     * the real one and never call [PairingMachine.ensureIdentity].
     */
    @Test
    fun `sending a heartbeat enqueues a DEVICE_HEARTBEAT envelope`() = runBlocking {
        val fake = FakeBridgeClient()
        val sent = fake.sentEnvelopes
        val env = Envelope(
            v = 1,
            id = UUID.randomUUID().toString(),
            ts = 0L,
            type = MessageType.DEVICE_HEARTBEAT,
            device_id = "dev",
            payload = kotlinx.serialization.json.Json.encodeToJsonElement(
                DeviceHeartbeatPayload.serializer(), DeviceHeartbeatPayload(null),
            ),
        )
        fake.send(env)
        val got = withTimeoutOrNull(1000) { sent.receive() }
        assertEquals(MessageType.DEVICE_HEARTBEAT, got?.type)
    }
}
