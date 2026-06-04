package im.zyx.phonebridge.pairing

import im.zyx.phonebridge.core.protocol.DeviceInfo
import im.zyx.phonebridge.core.protocol.Envelope
import im.zyx.phonebridge.core.protocol.MessageType
import im.zyx.phonebridge.core.protocol.PairChallengePayload
import im.zyx.phonebridge.core.protocol.PairResultPayload
import im.zyx.phonebridge.core.protocol.json
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class PairingMachineTest {

    private val desktop = DeviceInfo("daemon-id", "Desktop", "PC", "Linux", "0.1.0")

    @Test
    fun `generateCode returns 6 digits`() {
        repeat(50) {
            val c = PairingMachine.generateCode()
            assertEquals(6, c.length)
            assertTrue(c.all { it.isDigit() })
        }
    }

    @Test
    fun `begin moves to AwaitingDesktop and produces a pair_request envelope`() = runTest {
        val m = PairingMachine()
        val env = m.begin(ourDeviceId = "and", desktopDeviceId = "d", code = "123456", desktopInfo = desktop)
        assertEquals(MessageType.DEVICE_PAIR_REQUEST, env.type)
        val s = m.state.first()
        assertTrue(s is PairingState.AwaitingDesktop)
        assertEquals("123456", (s as PairingState.AwaitingDesktop).code)
    }

    @Test
    fun `onChallenge with matching code produces a confirm and advances state`() = runTest {
        val m = PairingMachine()
        m.begin(ourDeviceId = "and", desktopDeviceId = "d", code = "123456", desktopInfo = desktop)
        val challenge = Envelope(
            id = "c1", type = MessageType.DEVICE_PAIR_CHALLENGE,
            from = "d", to = "and", ts = "ts",
            payload = json.encodeToJsonElement(
                PairChallengePayload.serializer(),
                PairChallengePayload(code = "123456")
            )
        )
        val confirm = m.onChallenge(challenge)
        assertNotNull(confirm)
        assertEquals(MessageType.DEVICE_PAIR_CONFIRM, confirm!!.type)
        val s = m.state.first()
        assertTrue(s is PairingState.ChallengeReceived)
    }

    @Test
    fun `onChallenge with mismatched code transitions to Failed`() = runTest {
        val m = PairingMachine()
        m.begin(ourDeviceId = "and", desktopDeviceId = "d", code = "123456", desktopInfo = desktop)
        val challenge = Envelope(
            id = "c1", type = MessageType.DEVICE_PAIR_CHALLENGE,
            from = "d", to = "and", ts = "ts",
            payload = json.encodeToJsonElement(
                PairChallengePayload.serializer(),
                PairChallengePayload(code = "999999")
            )
        )
        val confirm = m.onChallenge(challenge)
        assertNull(confirm)
        val s = m.state.first()
        assertTrue(s is PairingState.Failed)
    }

    @Test
    fun `onResult with accepted=true transitions to Paired`() = runTest {
        val m = PairingMachine()
        m.begin(ourDeviceId = "and", desktopDeviceId = "d", code = "123456", desktopInfo = desktop)
        val result = Envelope(
            id = "r1", type = MessageType.DEVICE_PAIR_RESULT,
            from = "d", to = "and", ts = "ts",
            payload = json.encodeToJsonElement(
                PairResultPayload.serializer(),
                PairResultPayload(accepted = true)
            )
        )
        m.onResult(result, ourDeviceId = "and")
        val s = m.state.first()
        assertTrue(s is PairingState.Paired)
    }

    @Test
    fun `onResult with accepted=false transitions to Failed with reason`() = runTest {
        val m = PairingMachine()
        m.begin(ourDeviceId = "and", desktopDeviceId = "d", code = "123456", desktopInfo = desktop)
        val result = Envelope(
            id = "r1", type = MessageType.DEVICE_PAIR_RESULT,
            from = "d", to = "and", ts = "ts",
            payload = json.encodeToJsonElement(
                PairResultPayload.serializer(),
                PairResultPayload(accepted = false, reason = "user denied")
            )
        )
        m.onResult(result, ourDeviceId = "and")
        val s = m.state.first()
        assertTrue(s is PairingState.Failed)
        assertEquals("user denied", (s as PairingState.Failed).reason)
    }
}
