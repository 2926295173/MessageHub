package im.zyx.phonebridge.sms

import android.telephony.SmsMessage
import io.mockk.every
import io.mockk.mockk
import im.zyx.phonebridge.core.protocol.MessageType
import im.zyx.phonebridge.core.protocol.SmsReceivedPayload
import im.zyx.phonebridge.core.protocol.json
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Unit test for the pure envelope-building function in [SmsReceiver].
 *
 * The SmsReceiver's BroadcastReceiver machinery (intent parsing,
 * Hilt injection) is exercised on-device via `am broadcast` once
 * the user enables notification/SMS access in Settings. These
 * tests verify the protocol-level envelope building in isolation,
 * which is what the daemon ultimately cares about.
 */
class SmsReceiverTest {

    @Test
    fun `single-part SMS builds valid envelope`() {
        val p = mockSmsMessage(sender = "+15555550100", body = "Hello from test")
        val env = SmsReceiver.buildSmsReceivedEnvelope(
            parts = listOf(p),
            ourDeviceId = "android-uuid-1"
        )!!
        assertEquals(MessageType.SMS_RECEIVED, env.type)
        assertEquals("android-uuid-1", env.device_id)
        val payload = json.decodeFromJsonElement(SmsReceivedPayload.serializer(), env.payload)
        assertEquals("+15555550100", payload.address)
        assertEquals("Hello from test", payload.body)
        assertTrue("id should be a UUID", payload.id.length == 36)
        assertEquals(payload.received_at, p.timestampMillis)
    }

    @Test
    fun `multipart SMS concatenates bodies in order`() {
        val part1 = mockSmsMessage(sender = "+15555550100", body = "Hello ")
        val part2 = mockSmsMessage(sender = "+15555550100", body = "from ")
        val part3 = mockSmsMessage(sender = "+15555550100", body = "test")
        val env = SmsReceiver.buildSmsReceivedEnvelope(
            parts = listOf(part1, part2, part3),
            ourDeviceId = "android-uuid-1"
        )!!
        val payload = json.decodeFromJsonElement(SmsReceivedPayload.serializer(), env.payload)
        assertEquals("Hello from test", payload.body)
        assertEquals("+15555550100", payload.address)
        // received_at uses the first part's timestamp
        assertEquals(part1.timestampMillis, payload.received_at)
    }

    @Test
    fun `empty parts list returns null`() {
        val env = SmsReceiver.buildSmsReceivedEnvelope(
            parts = emptyList(),
            ourDeviceId = "android-uuid-1"
        )
        assertNull(env)
    }

    @Test
    fun `null originating address returns null`() {
        val p = mockk<SmsMessage>()
        every { p.displayOriginatingAddress } returns null
        every { p.displayMessageBody } returns "body"
        every { p.timestampMillis } returns 12345L
        val env = SmsReceiver.buildSmsReceivedEnvelope(
            parts = listOf(p),
            ourDeviceId = "android-uuid-1"
        )
        assertNull(env)
    }

    private fun mockSmsMessage(sender: String, body: String, tsMs: Long = 1_700_000_000_000L): SmsMessage {
        val m = mockk<SmsMessage>()
        every { m.displayOriginatingAddress } returns sender
        every { m.displayMessageBody } returns body
        every { m.timestampMillis } returns tsMs
        return m
    }
}
