package im.zyx.phonebridge.core.crypto

/**
 * Protocol-v1 constants for the pairing KDF. Mirror
 * `crates/phonebridge-crypto/src/pairing_code.rs`.
 *
 * The pairing code is derived as
 *
 * ```
 * shared_secret = ECDH(my_priv, peer_pub)       // 32 bytes
 * hkdf_salt     = "phonebridge/v1/pair"
 * hkdf_info     = "phonebridge/v1/code"
 * okm           = HKDF-SHA256(shared_secret, salt, info, 4)
 * code_int      = u32::from_be_bytes(okm) % 1_000_000
 * code          = format!("{:06}", code_int)
 * ```
 */
object PairingCodeSpec {
    const val HKDF_SALT: String = "phonebridge/v1/pair"
    const val HKDF_INFO: String = "phonebridge/v1/code"
    const val OKM_LEN: Int = 4
    const val MODULUS: Long = 1_000_000L
    const val CODE_LEN: Int = 6
}
