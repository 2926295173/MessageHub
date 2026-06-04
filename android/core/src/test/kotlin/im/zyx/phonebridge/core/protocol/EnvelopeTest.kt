package im.zyx.phonebridge.core.protocol

import org.junit.Assert.assertEquals
import org.junit.Test

class EnvelopeTest {

    @Test
    fun `envelope round-trips through json`() {
        val env = Envelope(
            id = "11111111-1111-1111-1111-111111111111",
            type = MessageType.SMS_RECEIVED,
            from = "android-1",
            to = "daemon-1",
            ts = "2026-06-04T15:00:00Z",
            payload = json.encodeToJsonElement(
                SmsReceivedPayload.serializer(),
                SmsReceivedPayload(
                    smsId = "sms-1",
                    address = "+15555550100",
                    body = "hello world",
                    receivedAt = 1717500000000L
                )
            )
        )
        val text = json.encodeToString(Envelope.serializer(), env)
        val back = json.decodeFromString(Envelope.serializer(), text)
        assertEquals(env, back)
        assertEquals(MessageType.SMS_RECEIVED, back.type)
    }

    @Test
    fun `payload decoder returns the typed struct`() {
        val payload = SmsReceivedPayload(
            smsId = "x", address = "+1", body = "hi", receivedAt = 0
        )
        val env = Envelope(
            id = "id", type = MessageType.SMS_RECEIVED, from = "a", to = "b",
            ts = "ts",
            payload = json.encodeToJsonElement(SmsReceivedPayload.serializer(), payload)
        )
        val decoded = json.decodeFromJsonElement(
            SmsReceivedPayload.serializer(), env.payload
        )
        assertEquals(payload, decoded)
    }
}
