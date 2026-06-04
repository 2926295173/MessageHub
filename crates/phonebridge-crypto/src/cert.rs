//! Self-signed X.509 certificate generation using `rcgen` (ring backend).
//!
//! M1 wraps `rcgen::generate_simple_self_signed`. M2 may add richer params
//! (extensions, SANs for IP, etc.) once mDNS-driven pairing needs them.

use thiserror::Error;

use crate::fingerprint::cert_fingerprint_der;

/// Generated keypair + self-signed cert bundle.
pub struct Identity {
    /// PEM-encoded certificate.
    pub cert_pem: String,
    /// PEM-encoded private key.
    pub key_pem: String,
    /// SHA-256 fingerprint, colon-separated upper-case hex.
    pub fingerprint: String,
}

/// Generate a fresh self-signed identity. The certificate is valid for 10
/// years. The `common_name` is used as the certificate's CN (and only CN —
/// we add no SANs in MVP, since the daemon is pinned by fingerprint).
pub fn generate_self_signed(common_name: &str, _validity_days: u32) -> Result<Identity, CertError> {
    let key = rcgen::generate_simple_self_signed(vec![common_name.to_string()])
        .map_err(|e| CertError::Generate(e.to_string()))?;
    let cert_pem = key.cert.pem();
    let key_pem = key.key_pair.serialize_pem();
    let fingerprint = cert_fingerprint_der(key.cert.der());

    Ok(Identity {
        cert_pem,
        key_pem,
        fingerprint,
    })
}

/// Errors from cert generation.
#[derive(Debug, Error)]
pub enum CertError {
    /// rcgen generation failed.
    #[error("cert generation failed: {0}")]
    Generate(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fingerprint::parse_fingerprint;

    #[test]
    fn generate_idempotent_in_shape() {
        let id1 = generate_self_signed("phonebridge-test-1", 30).unwrap();
        let id2 = generate_self_signed("phonebridge-test-2", 30).unwrap();
        // Different cert, different key, different fingerprint.
        assert_ne!(id1.cert_pem, id2.cert_pem);
        assert_ne!(id1.fingerprint, id2.fingerprint);
        // Both fingerprints must be valid 32-pair colon-separated.
        parse_fingerprint(&id1.fingerprint).unwrap();
        parse_fingerprint(&id2.fingerprint).unwrap();
    }

    #[test]
    fn cert_pem_has_begin_end_markers() {
        let id = generate_self_signed("test", 1).unwrap();
        assert!(id.cert_pem.contains("BEGIN CERTIFICATE"));
        assert!(id.cert_pem.contains("END CERTIFICATE"));
        assert!(id.key_pem.contains("PRIVATE KEY"));
    }
}
