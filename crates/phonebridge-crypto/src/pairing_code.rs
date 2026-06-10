// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 PhoneBridge Contributors
//
// This file is part of PhoneBridge. See LICENSE and the dual-licensing
// notice in README.md for details.

//! Derive the 4-digit decimal pairing code via HKDF-SHA256.
//!
//! See `docs/protocol-v1.md` §4.2 for the wire-level spec.
//!
//! ```text
//! shared_secret = ECDH(my_priv, peer_pub)
//! hkdf_salt     = "phonebridge/v1/pair"        (21 bytes UTF-8)
//! hkdf_info     = "phonebridge/v1/code"        (21 bytes UTF-8)
//! okm           = HKDF-SHA256(shared_secret, salt, info, 4)
//! code_int      = u32::from_be_bytes(okm) % 10_000
//! code          = format!("{:04}", code_int)
//! ```

use hkdf::Hkdf;
use sha2::Sha256;

use crate::ecdh::SharedSecret;

/// Salt used in the pairing HKDF. Fixed constant per protocol v1.
pub const HKDF_SALT: &[u8] = b"phonebridge/v1/pair";

/// Info used in the pairing HKDF. Fixed constant per protocol v1.
pub const HKDF_INFO: &[u8] = b"phonebridge/v1/code";

/// Number of OKM bytes to extract (4 → fits a u32 mod 10_000).
pub const OKM_LEN: usize = 4;

/// Derive the 4-digit decimal code from a 32-byte shared secret.
///
/// The output is always a 4-character string in `[0000, 9999]`.
pub fn derive_pairing_code(shared_secret: &SharedSecret) -> String {
    let hk = Hkdf::<Sha256>::new(Some(HKDF_SALT), shared_secret);
    let mut okm = [0u8; OKM_LEN];
    hk.expand(HKDF_INFO, &mut okm)
        .expect("HKDF expand with 4 bytes is always valid");
    let n = u32::from_be_bytes(okm);
    format!("{:04}", n % 10_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 5869 test vector 1 (basic) — used to sanity-check the HKDF plumbing.
    /// Note: PhoneBridge uses a different salt/info; this just verifies the
    /// `Hkdf` crate is wired correctly.
    #[test]
    fn hkdf_rfc5869_vector_a01() {
        let ikm = hex_literal::hex!("0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b0b");
        let salt = hex_literal::hex!("000102030405060708090a0b0c");
        let info = hex_literal::hex!("f0f1f2f3f4f5f6f7f8f9");
        let expected = hex_literal::hex!(
            "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865"
        );

        let hk = Hkdf::<Sha256>::new(Some(&salt), &ikm);
        let mut okm = [0u8; 42];
        hk.expand(&info, &mut okm).unwrap();
        assert_eq!(okm[..], expected[..]);
    }

    #[test]
    fn pairing_code_is_4_digits() {
        let shared = [0u8; 32];
        let code = derive_pairing_code(&shared);
        assert_eq!(code.len(), 4);
        assert!(code.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn pairing_code_deterministic() {
        let shared = [42u8; 32];
        let a = derive_pairing_code(&shared);
        let b = derive_pairing_code(&shared);
        assert_eq!(a, b);
    }

    /// Cross-check: two peers derive the same code from the same ECDH secret.
    #[test]
    fn pairing_code_matches_across_peers() {
        use crate::ecdh::EphemeralKeyPair;
        let alice = EphemeralKeyPair::generate().unwrap();
        let bob = EphemeralKeyPair::generate().unwrap();
        let alice_pub = alice.public_key();
        let bob_pub = bob.public_key();
        let alice_shared = alice.agree(&bob_pub).unwrap();
        let bob_shared = bob.agree(&alice_pub).unwrap();
        assert_eq!(alice_shared, bob_shared);
        let alice_code = derive_pairing_code(&alice_shared);
        let bob_code = derive_pairing_code(&bob_shared);
        assert_eq!(alice_code, bob_code);
    }

    /// Two different ECDH exchanges give different codes.
    #[test]
    fn different_keys_give_different_codes() {
        let a = derive_pairing_code(&[1u8; 32]);
        let b = derive_pairing_code(&[2u8; 32]);
        assert_ne!(a, b);
    }

    /// Distribution check (loose): 100 random codes should mostly be unique.
    /// With 10K space and 100 samples, ~40% expected collisions; we only
    /// assert ≥10 unique to catch broken KDF wiring.
    #[test]
    fn codes_look_unique_in_small_sample() {
        use crate::ecdh::EphemeralKeyPair;
        let mut codes = std::collections::HashSet::new();
        for _ in 0..100 {
            let alice = EphemeralKeyPair::generate().unwrap();
            let bob = EphemeralKeyPair::generate().unwrap();
            let bob_pub = bob.public_key();
            let shared = alice.agree(&bob_pub).unwrap();
            codes.insert(derive_pairing_code(&shared));
        }
        // Allow many collisions; just verify the KDF is producing many
        // distinct outputs (i.e. not stuck on a single value).
        assert!(
            codes.len() >= 10,
            "only {} unique codes out of 100 — KDF looks broken",
            codes.len()
        );
    }
}
