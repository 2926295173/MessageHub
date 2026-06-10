// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 PhoneBridge Contributors
//
// This file is part of PhoneBridge. See LICENSE and the dual-licensing
// notice in README.md for details.

//! ECDH P-256 keypair generation and shared-secret derivation.

use ring::agreement::{self, agree_ephemeral, EphemeralPrivateKey, UnparsedPublicKey};
use thiserror::Error;

/// 32-byte shared secret.
pub type SharedSecret = [u8; 32];

/// Length of an uncompressed P-256 public key (X9.62 form: 0x04 || X || Y).
pub const PUBLIC_KEY_LEN: usize = 65;

/// An ephemeral ECDH P-256 keypair.
pub struct EphemeralKeyPair {
    private: EphemeralPrivateKey,
    public_bytes: [u8; PUBLIC_KEY_LEN],
}

impl EphemeralKeyPair {
    /// Generate a fresh ephemeral keypair.
    pub fn generate() -> Result<Self, EcdhError> {
        let rng = ring::rand::SystemRandom::new();
        let private = EphemeralPrivateKey::generate(&agreement::ECDH_P256, &rng)
            .map_err(|_| EcdhError::Keygen)?;
        let pub_ref = private
            .compute_public_key()
            .map_err(|_| EcdhError::PublicKey)?;
        let pub_slice = pub_ref.as_ref();
        if pub_slice.len() != PUBLIC_KEY_LEN {
            return Err(EcdhError::PublicKeyLength(pub_slice.len()));
        }
        let mut public_bytes = [0u8; PUBLIC_KEY_LEN];
        public_bytes.copy_from_slice(pub_slice);
        Ok(Self {
            private,
            public_bytes,
        })
    }

    /// The 65-byte uncompressed public key (`0x04 || X || Y`).
    pub fn public_key(&self) -> PublicKey {
        PublicKey(self.public_bytes)
    }

    /// Derive a 32-byte shared secret with the peer.
    ///
    /// Consumes the keypair (ring's `EphemeralPrivateKey` is not cloneable
    /// by design — a fresh one is generated for each pairing).
    pub fn agree(self, peer_public: &PublicKey) -> Result<SharedSecret, EcdhError> {
        let peer = UnparsedPublicKey::new(&agreement::ECDH_P256, peer_public.0);
        agree_ephemeral(self.private, &peer, |shared| {
            let len = shared.len();
            if len != 32 {
                return Err(EcdhError::SharedSecretLength(len));
            }
            let mut out = [0u8; 32];
            out.copy_from_slice(shared);
            Ok(out)
        })
        .map_err(|_| EcdhError::Agreement)?
    }
}

/// 65-byte uncompressed P-256 public key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublicKey(pub [u8; PUBLIC_KEY_LEN]);

impl PublicKey {
    /// Try to parse from a 65-byte slice.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, EcdhError> {
        if bytes.len() != PUBLIC_KEY_LEN {
            return Err(EcdhError::PublicKeyLength(bytes.len()));
        }
        if bytes[0] != 0x04 {
            return Err(EcdhError::PublicKeyFormat);
        }
        let mut arr = [0u8; PUBLIC_KEY_LEN];
        arr.copy_from_slice(bytes);
        Ok(Self(arr))
    }

    /// Encode as base64 (no padding).
    pub fn to_base64(&self) -> String {
        use base64::engine::Engine;
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(self.0)
    }

    /// Parse from base64 (no padding).
    pub fn from_base64(s: &str) -> Result<Self, EcdhError> {
        use base64::engine::Engine;
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(s)
            .map_err(|_| EcdhError::Base64)?;
        Self::from_bytes(&bytes)
    }

    /// As raw bytes.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// Errors from ECDH operations.
#[derive(Debug, Error)]
pub enum EcdhError {
    /// Keypair generation failed (system RNG).
    #[error("ECDH keygen failed")]
    Keygen,
    /// Could not extract the public key.
    #[error("could not extract public key")]
    PublicKey,
    /// Public key has the wrong length.
    #[error("public key has wrong length: {0}")]
    PublicKeyLength(usize),
    /// Public key has the wrong format (must start with 0x04).
    #[error("public key must be uncompressed (leading 0x04)")]
    PublicKeyFormat,
    /// Shared secret has the wrong length.
    #[error("shared secret has wrong length: {0}")]
    SharedSecretLength(usize),
    /// ECDH agreement failed (ring Unspecified).
    #[error("ECDH agreement failed")]
    Agreement,
    /// Base64 decode failed.
    #[error("base64 decode failed")]
    Base64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_parties_derive_same_secret() {
        let alice = EphemeralKeyPair::generate().unwrap();
        let bob = EphemeralKeyPair::generate().unwrap();
        let alice_pub = alice.public_key();
        let bob_pub = bob.public_key();
        let alice_shared = alice.agree(&bob_pub).unwrap();
        let bob_shared = bob.agree(&alice_pub).unwrap();
        assert_eq!(
            alice_shared, bob_shared,
            "ECDH shared secret must match on both sides"
        );
    }

    #[test]
    fn public_key_starts_with_0x04() {
        let kp = EphemeralKeyPair::generate().unwrap();
        let pk = kp.public_key();
        assert_eq!(pk.0[0], 0x04, "P-256 uncompressed must start with 0x04");
        assert_eq!(pk.0.len(), 65);
    }

    #[test]
    fn base64_round_trip() {
        let kp = EphemeralKeyPair::generate().unwrap();
        let pk = kp.public_key();
        let s = pk.to_base64();
        let back = PublicKey::from_base64(&s).unwrap();
        assert_eq!(back, pk);
    }

    #[test]
    fn from_bytes_rejects_short_input() {
        let r = PublicKey::from_bytes(&[0u8; 10]);
        assert!(matches!(r, Err(EcdhError::PublicKeyLength(10))));
    }

    #[test]
    fn from_bytes_rejects_wrong_prefix() {
        let mut bytes = [0u8; 65];
        bytes[0] = 0x02; // compressed form
        let r = PublicKey::from_bytes(&bytes);
        assert!(matches!(r, Err(EcdhError::PublicKeyFormat)));
    }
}
