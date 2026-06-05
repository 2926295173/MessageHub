package im.zyx.phonebridge.core.crypto

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.security.interfaces.ECPublicKey

class EcdhTest {

    @Test
    fun `uncompressed pubkey is 65 bytes starting with 0x04`() {
        val kp = Ecdh.generateKeyPair()
        val pub = kp.public as ECPublicKey
        val bytes = Ecdh.uncompressedBytes(pub)
        assertEquals(65, bytes.size)
        assertEquals(0x04.toByte(), bytes[0])
    }

    @Test
    fun `base64 round-trip of public key`() {
        val kp = Ecdh.generateKeyPair()
        val pub = kp.public as ECPublicKey
        val b64 = Ecdh.toBase64(pub)
        val back = Ecdh.publicKeyFromBase64(b64)
        assertEquals(pub.w.affineX, back.w.affineX)
        assertEquals(pub.w.affineY, back.w.affineY)
    }

    @Test
    fun `publicKeyFromBase64 rejects wrong length`() {
        val r = runCatching { Ecdh.publicKeyFromBase64("AAAA") }
        assertTrue(r.isFailure)
    }

    @Test
    fun `publicKeyFromBase64 rejects non-0x04 prefix`() {
        val kp = Ecdh.generateKeyPair()
        val bytes = Ecdh.uncompressedBytes(kp.public as ECPublicKey)
        bytes[0] = 0x02  // compressed form
        val b64 = java.util.Base64.getUrlEncoder().withoutPadding().encodeToString(bytes)
        val r = runCatching { Ecdh.publicKeyFromBase64(b64) }
        assertTrue("expected failure on 0x02 prefix, got ${r.getOrNull()}", r.isFailure)
    }

    @Test
    fun `two parties derive the same 32-byte ECDH secret`() {
        val a = Ecdh.generateKeyPair()
        val b = Ecdh.generateKeyPair()
        val aPub = a.public as ECPublicKey
        val bPub = b.public as ECPublicKey
        val aShared = Ecdh.agree(a.private as java.security.interfaces.ECPrivateKey, bPub)
        val bShared = Ecdh.agree(b.private as java.security.interfaces.ECPrivateKey, aPub)
        assertEquals(32, aShared.size)
        assertEquals(32, bShared.size)
        assertTrue("ECDH shared secrets must match", aShared.contentEquals(bShared))
    }
}
