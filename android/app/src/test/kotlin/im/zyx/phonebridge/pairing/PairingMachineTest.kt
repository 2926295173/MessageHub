package im.zyx.phonebridge.pairing

import im.zyx.phonebridge.core.crypto.Ecdh
import im.zyx.phonebridge.core.protocol.Envelope
import im.zyx.phonebridge.core.protocol.MessageType
import im.zyx.phonebridge.core.protocol.PairAcceptPayload
import im.zyx.phonebridge.core.protocol.PairChallengePayload
import im.zyx.phonebridge.core.protocol.PairCompletePayload
import im.zyx.phonebridge.core.protocol.PairConfirmPayload
import im.zyx.phonebridge.core.protocol.PairRejectPayload
import im.zyx.phonebridge.core.protocol.PairRequestPayload
import im.zyx.phonebridge.core.protocol.json
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.test.runTest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.security.interfaces.ECPublicKey

class PairingMachineTest {

    private fun makeRequest(): Envelope {
        val kp = Ecdh.generateKeyPair()
        val pubB64 = Ecdh.toBase64(kp.public as ECPublicKey)
        return Envelope(
            v = 1,
            id = "req-1",
            type = MessageType.DEVICE_PAIR_REQUEST,
            device_id = "daemon-id",
            ts = 1L,
            payload = json.encodeToJsonElement(
                PairRequestPayload.serializer(),
                PairRequestPayload(ephemeral_pubkey = pubB64)
            )
        )
    }

    @Test
    fun `onRequest derives a 6-digit code and replies with challenge`() = runTest {
        val m = PairingMachine()
        val challenge = m.onRequest(makeRequest())
        assertNotNull(challenge)
        assertEquals(MessageType.DEVICE_PAIR_CHALLENGE, challenge!!.type)
        val payload = json.decodeFromJsonElement(
            PairChallengePayload.serializer(), challenge.payload
        )
        assertEquals(6, payload.code.length)
        assertTrue(payload.code.all { it.isDigit() })
        val s = m.state.first()
        assertTrue(s is PairingState.ShowingCode)
        assertEquals(payload.code, (s as PairingState.ShowingCode).code)
    }

    @Test
    fun `onRequest with bad pubkey transitions to Failed`() = runTest {
        val m = PairingMachine()
        val env = Envelope(
            v = 1, id = "x", type = MessageType.DEVICE_PAIR_REQUEST, device_id = "d", ts = 1L,
            payload = json.encodeToJsonElement(
                PairRequestPayload.serializer(),
                PairRequestPayload(ephemeral_pubkey = "AAAA")
            )
        )
        val r = m.onRequest(env)
        assertEquals(null, r)
        val s = m.state.first()
        assertTrue(s is PairingState.Failed)
    }

    @Test
    fun `onAccept sends confirm and transitions to Confirming`() = runTest {
        val m = PairingMachine()
        m.onRequest(makeRequest()) ?: error("expected challenge")
        val accept = Envelope(
            v = 1, id = "acc-1", type = MessageType.DEVICE_PAIR_ACCEPT,
            device_id = "daemon-id", ts = 2L,
            payload = json.encodeToJsonElement(
                PairAcceptPayload.serializer(), PairAcceptPayload()
            )
        )
        val confirm = m.onAccept(accept, ourDeviceId = "and-id")
        val payload = json.decodeFromJsonElement(
            PairConfirmPayload.serializer(), confirm.payload
        )
        assertTrue(payload.accepted)
        val s = m.state.first()
        assertTrue(s is PairingState.Confirming)
    }

    @Test
    fun `onComplete validates and sends our own complete`() = runTest {
        val m = PairingMachine()
        m.onRequest(makeRequest())
        val accept = Envelope(
            v = 1, id = "acc", type = MessageType.DEVICE_PAIR_ACCEPT,
            device_id = "daemon-id", ts = 2L,
            payload = json.encodeToJsonElement(
                PairAcceptPayload.serializer(), PairAcceptPayload()
            )
        )
        m.onAccept(accept, ourDeviceId = "and-id")
        val fakePeerCert = im.zyx.phonebridge.core.crypto.CertGen.generateSelfSigned(
            "test", Ecdh.generateKeyPair(), 1
        )
        val complete = Envelope(
            v = 1, id = "comp-1", type = MessageType.DEVICE_PAIR_COMPLETE,
            device_id = "daemon-id", ts = 3L,
            payload = json.encodeToJsonElement(
                PairCompletePayload.serializer(),
                PairCompletePayload(
                    cert_pem = fakePeerCert.pem,
                    cert_fingerprint = fakePeerCert.fingerprint
                )
            )
        )
        val reply = m.onComplete(complete, ourDeviceId = "and-id")
        assertNotNull(reply)
        assertEquals(MessageType.DEVICE_PAIR_COMPLETE, reply!!.type)
        val ourCert = json.decodeFromJsonElement(
            PairCompletePayload.serializer(), reply.payload
        )
        assertTrue(ourCert.cert_pem.contains("BEGIN CERTIFICATE"))
        // 32 colon-separated UPPERCASE hex pairs
        assertEquals(32 * 3 - 1, ourCert.cert_fingerprint.length)
        assertEquals(31, ourCert.cert_fingerprint.count { it == ':' })
        val s = m.state.first()
        assertTrue(s is PairingState.Paired)
    }

    @Test
    fun `onComplete with malformed PEM transitions to Failed`() = runTest {
        val m = PairingMachine()
        m.onRequest(makeRequest())
        val accept = Envelope(
            v = 1, id = "acc", type = MessageType.DEVICE_PAIR_ACCEPT,
            device_id = "d", ts = 1L,
            payload = json.encodeToJsonElement(
                PairAcceptPayload.serializer(), PairAcceptPayload()
            )
        )
        m.onAccept(accept, ourDeviceId = "and-id")
        val bad = Envelope(
            v = 1, id = "c", type = MessageType.DEVICE_PAIR_COMPLETE,
            device_id = "d", ts = 1L,
            payload = json.encodeToJsonElement(
                PairCompletePayload.serializer(),
                PairCompletePayload(cert_pem = "not a cert", cert_fingerprint = "AB".repeat(32))
            )
        )
        val reply = m.onComplete(bad, ourDeviceId = "and-id")
        assertEquals(null, reply)
        val s = m.state.first()
        assertTrue(s is PairingState.Failed)
    }

    @Test
    fun `onReject transitions to Failed with reason`() = runTest {
        val m = PairingMachine()
        m.onRequest(makeRequest())
        val reject = Envelope(
            v = 1, id = "r", type = MessageType.DEVICE_PAIR_REJECT,
            device_id = "d", ts = 1L,
            payload = json.encodeToJsonElement(
                PairRejectPayload.serializer(),
                PairRejectPayload(reason = "user said no")
            )
        )
        m.onReject(reject)
        val s = m.state.first()
        assertTrue(s is PairingState.Failed)
        assertEquals("user said no", (s as PairingState.Failed).reason)
    }
}
