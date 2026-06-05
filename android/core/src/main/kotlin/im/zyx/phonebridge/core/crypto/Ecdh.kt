package im.zyx.phonebridge.core.crypto

import java.math.BigInteger
import java.security.AlgorithmParameters
import java.security.KeyFactory
import java.security.KeyPair
import java.security.KeyPairGenerator
import java.security.interfaces.ECPublicKey
import java.security.spec.ECGenParameterSpec
import java.security.spec.ECParameterSpec
import java.security.spec.ECPoint
import java.security.spec.ECPublicKeySpec

/**
 * ECDH P-256 keypair generation + shared-secret agreement.
 *
 * Wire format: an uncompressed public key is rendered as
 *   `0x04 || X(32 bytes big-endian) || Y(32 bytes big-endian)`
 * which is then base64 (URL-safe, no padding) encoded. The Rust side
 * (`phonebridge-crypto/src/ecdh.rs`) produces the same shape.
 *
 * The Android JCE handles the EC math natively (P-256 == secp256r1
 * == prime256v1). No BouncyCastle is required for the key agreement
 * itself; we only need BouncyCastle later for X.509 cert generation.
 */
object Ecdh {
    private const val ALGO = "EC"
    private val PARAMS: ECParameterSpec by lazy {
        val ap = AlgorithmParameters.getInstance(ALGO)
        ap.init(ECGenParameterSpec("secp256r1"))
        ap.getParameterSpec(ECParameterSpec::class.java)
    }

    /**
     * Generate a fresh P-256 keypair.
     */
    fun generateKeyPair(): KeyPair {
        val kpg = KeyPairGenerator.getInstance(ALGO)
        kpg.initialize(ECGenParameterSpec("secp256r1"))
        return kpg.generateKeyPair()
    }

    /**
     * Render an [ECPublicKey] as the wire format: 65 bytes
     * `0x04 || X(32) || Y(32)`. Returns the byte array.
     */
    fun uncompressedBytes(pub: ECPublicKey): ByteArray {
        val point = pub.w
        val x = point.affineX.toByteArray()
        val y = point.affineY.toByteArray()
        require(x.size <= 33 && y.size <= 33) { "EC affine coords out of range" }
        val out = ByteArray(65)
        out[0] = 0x04
        // BigInteger.toByteArray() is signed two's complement; we need
        // unsigned 32 bytes. If the top bit of the magnitude is set
        // the result has a leading 0x00 we can drop; otherwise we
        // right-align into a 32-byte slot.
        writeUnsigned32(x, out, 1)
        writeUnsigned32(y, out, 33)
        return out
    }

    private fun writeUnsigned32(src: ByteArray, dst: ByteArray, dstOff: Int) {
        // Take the last 32 bytes of the BigInteger representation.
        val len = minOf(src.size, 32)
        val srcOff = src.size - len
        System.arraycopy(src, srcOff, dst, dstOff + (32 - len), len)
    }

    /**
     * Encode an uncompressed pubkey to base64 (URL-safe, no padding).
     */
    fun toBase64(pub: ECPublicKey): String =
        java.util.Base64.getUrlEncoder().withoutPadding().encodeToString(uncompressedBytes(pub))

    /**
     * Decode a base64 (URL-safe, no padding) string of length 65
     * starting with `0x04` into a public key.
     *
     * @throws IllegalArgumentException if the input is malformed.
     */
    fun publicKeyFromBase64(b64: String): ECPublicKey {
        val raw = java.util.Base64.getUrlDecoder().decode(b64)
        require(raw.size == 65) { "pubkey must be 65 bytes, got ${raw.size}" }
        require(raw[0] == 0x04.toByte()) { "uncompressed pubkey must start with 0x04" }
        val x = BigInteger(1, raw.copyOfRange(1, 33))
        val y = BigInteger(1, raw.copyOfRange(33, 65))
        val spec = ECPublicKeySpec(ECPoint(x, y), PARAMS)
        return KeyFactory.getInstance(ALGO).generatePublic(spec) as ECPublicKey
    }

    /**
     * Compute the 32-byte ECDH shared secret. Consumes nothing
     * (keypairs are reused; the JCE doesn't have ring's "consume
     * private key" semantic).
     */
    fun agree(myPrivate: java.security.interfaces.ECPrivateKey, peerPublic: ECPublicKey): ByteArray {
        val ka = javax.crypto.KeyAgreement.getInstance("ECDH")
        ka.init(myPrivate)
        ka.doPhase(peerPublic, true)
        val secret = ka.generateSecret()
        require(secret.size == 32) { "ECDH shared secret must be 32 bytes (got ${secret.size})" }
        return secret
    }
}
