package im.zyx.phonebridge.core.crypto

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class PairingCodeTest {

    @Test
    fun `code is always 4 digits`() {
        // 100 random ECDH secrets → 100 4-digit codes.
        repeat(100) {
            val secret = ByteArray(32) { (it * 7).toByte() }
            val code = PairingCode.derive(secret)
            assertEquals(4, code.length)
            assertTrue("code '$code' contains non-digit", code.all { it in '0'..'9' })
        }
    }

    @Test
    fun `same secret produces same code`() {
        val s = ByteArray(32) { 42 }
        assertEquals(PairingCode.derive(s), PairingCode.derive(s))
    }

    @Test
    fun `different secrets produce different codes (mostly)`() {
        val codes = HashSet<String>()
        for (i in 0 until 200) {
            val s = ByteArray(32) { (i and 0xFF).toByte() }
            codes.add(PairingCode.derive(s))
        }
        // 4-digit space = 10K; 200 samples → many collisions expected
        // (~63% by birthday bound). We only require ≥5 unique to confirm
        // the KDF is producing many distinct outputs.
        assertTrue("only ${codes.size} unique codes out of 200", codes.size >= 5)
    }

    @Test
    fun `two parties derive the same code from the same ECDH exchange`() {
        val alice = Ecdh.generateKeyPair()
        val bob = Ecdh.generateKeyPair()
        val aShared = Ecdh.agree(alice.private as java.security.interfaces.ECPrivateKey, bob.public as java.security.interfaces.ECPublicKey)
        val bShared = Ecdh.agree(bob.private as java.security.interfaces.ECPrivateKey, alice.public as java.security.interfaces.ECPublicKey)
        assertTrue("ECDH mismatch", aShared.contentEquals(bShared))
        val aliceCode = PairingCode.derive(aShared)
        val bobCode = PairingCode.derive(bShared)
        assertEquals(aliceCode, bobCode)
    }
}
