package im.zyx.phonebridge.core.crypto

import javax.crypto.Mac
import javax.crypto.spec.SecretKeySpec

/**
 * RFC 5869 HKDF using HMAC-SHA256. Used to derive the 6-digit
 * pairing code from a 32-byte ECDH shared secret.
 */
object Hkdf {
    private const val HMAC = "HmacSHA256"
    private const val HASH_LEN = 32  // SHA-256 output size

    /**
     * Derive [len] bytes of output keying material. Max output is
     * 255 * 32 = 8160 bytes (RFC 5869 bound on T(i) iterations).
     */
    fun hkdfSha256(secret: ByteArray, salt: ByteArray, info: ByteArray, len: Int): ByteArray {
        require(len in 1..(255 * HASH_LEN)) { "HKDF output length out of range: $len" }
        // 1. Extract: PRK = HMAC-SHA256(salt, secret)
        val extractMac = Mac.getInstance(HMAC)
        extractMac.init(SecretKeySpec(salt, HMAC))
        val prk = extractMac.doFinal(secret)
        // 2. Expand. T(0) is empty. T(i) = HMAC(PRK, T(i-1) || info || i).
        val out = ByteArray(len)
        var written = 0
        var prev = ByteArray(0)
        var counter: Byte = 0
        val expandMac = Mac.getInstance(HMAC)
        expandMac.init(SecretKeySpec(prk, HMAC))
        while (written < len) {
            counter = (counter + 1).toByte()
            expandMac.reset()
            expandMac.update(prev)
            expandMac.update(info)
            expandMac.update(counter)
            prev = expandMac.doFinal()
            val take = minOf(HASH_LEN, len - written)
            System.arraycopy(prev, 0, out, written, take)
            written += take
        }
        return out
    }
}
