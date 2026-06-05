package im.zyx.phonebridge.core.crypto

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class HkdfTest {

    @Test
    fun `RFC 5869 test vector A_1`() {
        // From RFC 5869 §A.1
        val ikm = hex("0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b")
        val salt = hex("000102030405060708090a0b0c")
        val info = hex("f0f1f2f3f4f5f6f7f8f9")
        val expected = hex("3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865")
        val okm = Hkdf.hkdfSha256(ikm, salt, info, 42)
        assertTrue("HKDF mismatch: got ${okm.toHex()}, expected ${expected.toHex()}",
            okm.contentEquals(expected))
    }

    @Test
    fun `extract+expand produces 4 bytes for code use`() {
        val secret = ByteArray(32) { it.toByte() }
        val okm = Hkdf.hkdfSha256(
            secret = secret,
            salt = "phonebridge/v1/pair".toByteArray(),
            info = "phonebridge/v1/code".toByteArray(),
            len = 4
        )
        assertEquals(4, okm.size)
    }

    @Test
    fun `deterministic for same input`() {
        val s = ByteArray(32) { 7 }
        val a = Hkdf.hkdfSha256(s, "a".toByteArray(), "b".toByteArray(), 4)
        val b = Hkdf.hkdfSha256(s, "a".toByteArray(), "b".toByteArray(), 4)
        assertTrue(a.contentEquals(b))
    }

    @Test
    fun `different salt or info produces different output`() {
        val s = ByteArray(32) { 7 }
        val a = Hkdf.hkdfSha256(s, "salt-1".toByteArray(), "info".toByteArray(), 4)
        val b = Hkdf.hkdfSha256(s, "salt-2".toByteArray(), "info".toByteArray(), 4)
        val c = Hkdf.hkdfSha256(s, "salt-1".toByteArray(), "info2".toByteArray(), 4)
        assertNotEquals(a.toList(), b.toList())
        assertNotEquals(a.toList(), c.toList())
    }

    private fun hex(s: String): ByteArray {
        val out = ByteArray(s.length / 2)
        for (i in 0 until s.length / 2) {
            out[i] = s.substring(i * 2, i * 2 + 2).toInt(16).toByte()
        }
        return out
    }

    private fun ByteArray.toHex(): String =
        joinToString("") { "%02x".format(it) }
}
