package im.zyx.phonebridge.core.crypto

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class CertGenTest {

    @Test
    fun `pem contains BEGIN CERTIFICATE and END CERTIFICATE`() {
        val kp = Ecdh.generateKeyPair()
        val cert = CertGen.generateSelfSigned("test-android", kp, validityDays = 1)
        assertTrue("PEM missing BEGIN CERTIFICATE:\n${cert.pem}",
            cert.pem.contains("BEGIN CERTIFICATE"))
        assertTrue("PEM missing END CERTIFICATE:\n${cert.pem}",
            cert.pem.contains("END CERTIFICATE"))
    }

    @Test
    fun `fingerprint is 32 colon-separated UPPERCASE hex pairs`() {
        val kp = Ecdh.generateKeyPair()
        val cert = CertGen.generateSelfSigned("test-android", kp, validityDays = 1)
        val fp = cert.fingerprint
        assertEquals(32 * 3 - 1, fp.length)  // 32 pairs * 2 + 31 colons
        assertEquals(31, fp.count { it == ':' })
        val parts = fp.split(':')
        assertEquals(32, parts.size)
        parts.forEach { p ->
            assertEquals(2, p.length)
            assertTrue("non-hex pair '$p'", p.all { it in '0'..'9' || it in 'A'..'F' })
        }
    }

    @Test
    fun `fingerprint is deterministic for the same cert`() {
        val kp = Ecdh.generateKeyPair()
        val a = CertGen.generateSelfSigned("test-android", kp, validityDays = 1)
        // Re-derive the fingerprint from the same DER bytes.
        val b = CertGen.fingerprint(a.der)
        assertEquals(a.fingerprint, b)
    }

    @Test
    fun `two different keys produce different fingerprints`() {
        val a = CertGen.generateSelfSigned("a", Ecdh.generateKeyPair(), 1)
        val b = CertGen.generateSelfSigned("b", Ecdh.generateKeyPair(), 1)
        assertNotEquals(a.fingerprint, b.fingerprint)
    }
}
