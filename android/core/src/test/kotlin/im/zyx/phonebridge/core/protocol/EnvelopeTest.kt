package im.zyx.phonebridge.core.protocol

import im.zyx.phonebridge.core.protocol.json
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class EnvelopeTest {

    @Test
    fun `envelope round-trips through json`() {
        val env = Envelope(
            id = "11111111-1111-1111-1111-111111111111",
            type = MessageType.SMS_RECEIVED,
            device_id = "android-1",
            ts = 1717500000000L,
            payload = json.encodeToJsonElement(
                SmsReceivedPayload.serializer(),
                SmsReceivedPayload(
                    id = "sms-1",
                    address = "+15555550100",
                    body = "hello world",
                    received_at = 1717500000000L
                )
            )
        )
        val text = json.encodeToString(Envelope.serializer(), env)
        val back = json.decodeFromString(Envelope.serializer(), text)
        assertEquals(env, back)
        assertEquals(MessageType.SMS_RECEIVED, back.type)
        // Verify the exact wire field names match the Rust side.
        assertTrue(text.contains("\"device_id\":\"android-1\""))
        assertTrue(text.contains("\"ts\":1717500000000"))
        assertTrue(text.contains("\"v\":1"))
    }

    @Test
    fun `payload decoder returns the typed struct`() {
        val payload = SmsReceivedPayload(
            id = "x", address = "+1", body = "hi", received_at = 0
        )
        val env = Envelope(
            id = "id", type = MessageType.SMS_RECEIVED, device_id = "a",
            ts = 1L,
            payload = json.encodeToJsonElement(SmsReceivedPayload.serializer(), payload)
        )
        val decoded = json.decodeFromJsonElement(
            SmsReceivedPayload.serializer(), env.payload
        )
        assertEquals(payload, decoded)
    }

    @Test
    fun `PairRequest has only ephemeral_pubkey`() {
        val p = PairRequestPayload(ephemeral_pubkey = "abcd")
        val text = json.encodeToString(PairRequestPayload.serializer(), p)
        assertTrue(text.contains("\"ephemeral_pubkey\":\"abcd\""))
    }

    @Test
    fun `DeviceType serializes lowercase`() {
        assertEquals("\"android\"",
            json.encodeToString(DeviceType.serializer(), DeviceType.Android))
        assertEquals("\"desktop\"",
            json.encodeToString(DeviceType.serializer(), DeviceType.Desktop))
    }

    @Test
    fun `CallStateKind serializes lowercase`() {
        assertEquals("\"idle\"",
            json.encodeToString(CallStateKind.serializer(), CallStateKind.Idle))
        assertEquals("\"ringing\"",
            json.encodeToString(CallStateKind.serializer(), CallStateKind.Ringing))
        assertEquals("\"offhook\"",
            json.encodeToString(CallStateKind.serializer(), CallStateKind.Offhook))
    }
}
