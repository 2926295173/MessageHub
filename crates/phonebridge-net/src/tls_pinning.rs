// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 PhoneBridge Contributors
//
// This file is part of PhoneBridge. See LICENSE and the dual-licensing
// notice in README.md for details.

//! TLS pinning: validate the client cert presented during a WebSocket
//! handshake against a stored fingerprint.
//!
//! The message-center uses `with_no_client_auth()` in rustls, so the WS server does
//! **not** require a client cert. We accept any cert at the TLS layer and
//! then validate the fingerprint at the application layer (after reading
//! the first `device.hello` envelope, which carries the device id we use
//! to look up the pinned fingerprint).

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use rustls::pki_types::CertificateDer;
use std::sync::Arc;
use thiserror::Error;

/// A pin store: maps a device id (UUIDv4) to a cert fingerprint.
pub type PinStore = Arc<parking_lot::RwLock<std::collections::HashMap<uuid::Uuid, String>>>;

/// Errors from fingerprint verification.
#[derive(Debug, Error)]
pub enum PinError {
    /// The cert chain is empty.
    #[error("no client certificate provided")]
    NoCert,
    /// The cert's fingerprint doesn't match the pinned value.
    #[error("fingerprint mismatch (expected={expected}, got={got})")]
    Mismatch {
        /// Expected fingerprint (colon-separated hex).
        expected: String,
        /// Computed fingerprint.
        got: String,
    },
    /// The peer did not provide a `device.hello` with a known device id.
    #[error("unknown device id")]
    UnknownDevice,
}

/// Compute the SHA-256 fingerprint of a DER cert and return it as
/// 32 colon-separated upper-case hex pairs.
pub fn cert_fingerprint(cert_der: &[u8]) -> Result<String, PinError> {
    Ok(phonebridge_crypto::fingerprint::cert_fingerprint_der(
        cert_der,
    ))
}

/// Verify a client cert against the pinned fingerprint for `device_id`.
pub fn verify_pin(
    store: &PinStore,
    device_id: uuid::Uuid,
    client_certs: &[CertificateDer<'_>],
) -> Result<(), PinError> {
    let leaf = client_certs.first().ok_or(PinError::NoCert)?;
    let got = cert_fingerprint(leaf.as_ref())?;
    let expected = {
        let r = store.read();
        r.get(&device_id).cloned()
    };
    match expected {
        None => Err(PinError::UnknownDevice),
        Some(pinned) if pinned == got => Ok(()),
        Some(pinned) => Err(PinError::Mismatch {
            expected: pinned,
            got,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phonebridge_crypto::cert;

    #[test]
    fn fingerprint_format_is_32_pairs() {
        let id = cert::generate_self_signed("test", 1).unwrap();
        let der = pem_to_der(&id.cert_pem);
        let fp = cert_fingerprint(&der).unwrap();
        assert_eq!(fp.len(), 95);
        assert_eq!(fp.matches(':').count(), 31);
    }

    #[test]
    fn verify_pin_matches_and_mismatches() {
        let id = cert::generate_self_signed("test", 1).unwrap();
        let der = pem_to_der(&id.cert_pem);
        let cert = CertificateDer::from(der);

        let store: PinStore = Arc::new(parking_lot::RwLock::new(std::collections::HashMap::new()));
        let device_id = uuid::Uuid::new_v4();
        store.write().insert(device_id, id.fingerprint.clone());

        // Match
        let r = verify_pin(&store, device_id, std::slice::from_ref(&cert));
        assert!(r.is_ok(), "expected ok, got {r:?}");

        // Mismatch
        store.write().insert(device_id, "DE:AD:BE:EF".repeat(8));
        let r = verify_pin(&store, device_id, std::slice::from_ref(&cert));
        assert!(matches!(r, Err(PinError::Mismatch { .. })));

        // Unknown device
        let r = verify_pin(&store, uuid::Uuid::new_v4(), std::slice::from_ref(&cert));
        assert!(matches!(r, Err(PinError::UnknownDevice)));
    }

    fn pem_to_der(pem_str: &str) -> Vec<u8> {
        use base64::Engine;
        let mut b64 = String::new();
        let mut in_cert = false;
        for line in pem_str.lines() {
            let line = line.trim();
            if line.contains("BEGIN CERTIFICATE") {
                in_cert = true;
                continue;
            }
            if line.contains("END CERTIFICATE") {
                break;
            }
            if in_cert {
                b64.push_str(line);
            }
        }
        base64::engine::general_purpose::STANDARD
            .decode(b64)
            .expect("base64 cert body")
    }
}
