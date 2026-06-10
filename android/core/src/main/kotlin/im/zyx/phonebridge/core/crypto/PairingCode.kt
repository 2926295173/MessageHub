package im.zyx.phonebridge.core.crypto

/**
 * Derive a 4-digit decimal pairing code from a 32-byte ECDH shared
 * secret. Wire-compatible with the Rust `pairing_code::derive_pairing_code`.
 */
object PairingCode {
    /**
     * @return a 4-character string in `[0000, 9999]`.
     */
    fun derive(sharedSecret: ByteArray): String {
        require(sharedSecret.size == 32) { "shared secret must be 32 bytes" }
        val okm = Hkdf.hkdfSha256(
            secret = sharedSecret,
            salt = PairingCodeSpec.HKDF_SALT.toByteArray(Charsets.UTF_8),
            info = PairingCodeSpec.HKDF_INFO.toByteArray(Charsets.UTF_8),
            len = PairingCodeSpec.OKM_LEN
        )
        // u32::from_be_bytes
        val n = ((okm[0].toLong() and 0xFF) shl 24) or
                ((okm[1].toLong() and 0xFF) shl 16) or
                ((okm[2].toLong() and 0xFF) shl 8) or
                (okm[3].toLong() and 0xFF)
        val code = n % PairingCodeSpec.MODULUS
        return code.toString().padStart(PairingCodeSpec.CODE_LEN, '0')
    }
}
