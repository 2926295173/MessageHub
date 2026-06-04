//! SHA-256 fingerprint of a DER-encoded X.509 certificate.
//!
//! Output format: 32 bytes, rendered as 32 colon-separated upper-case hex
//! pairs, e.g. `AB:CD:EF:...`. This is the canonical PhoneBridge format.

use ring::digest::{SHA256, digest};
use thiserror::Error;

/// Length of a SHA-256 digest.
pub const SHA256_LEN: usize = 32;

/// Compute the SHA-256 fingerprint of a DER certificate and return it as
/// 32 colon-separated upper-case hex pairs.
pub fn cert_fingerprint_der(der: &[u8]) -> String {
    let d = digest(&SHA256, der);
    let bytes = d.as_ref();
    debug_assert_eq!(bytes.len(), SHA256_LEN);
    let mut s = String::with_capacity(SHA256_LEN * 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 {
            s.push(':');
        }
        s.push_str(&format!("{:02X}", b));
    }
    s
}

/// Parse a colon-separated hex fingerprint and return the 32 raw bytes.
pub fn parse_fingerprint(s: &str) -> Result<[u8; SHA256_LEN], FingerprintError> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != SHA256_LEN {
        return Err(FingerprintError::PairCount(parts.len()));
    }
    let mut out = [0u8; SHA256_LEN];
    for (i, p) in parts.iter().enumerate() {
        if p.len() != 2 {
            return Err(FingerprintError::PairLength(i, p.len()));
        }
        out[i] = u8::from_str_radix(p, 16).map_err(|_| FingerprintError::PairHex(i))?;
    }
    Ok(out)
}

/// Errors from fingerprint parsing.
#[derive(Debug, Error)]
pub enum FingerprintError {
    /// Wrong number of colon-separated pairs.
    #[error("fingerprint must have {} pairs, got {0}", SHA256_LEN)]
    PairCount(usize),
    /// A pair has the wrong length.
    #[error("pair {0} has length {1}, expected 2")]
    PairLength(usize, usize),
    /// A pair is not valid hex.
    #[error("pair {0} is not valid hex")]
    PairHex(usize),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_deterministic() {
        let der = b"hello world DER-encoded cert";
        let a = cert_fingerprint_der(der);
        let b = cert_fingerprint_der(der);
        assert_eq!(a, b);
    }

    #[test]
    fn fingerprint_is_64_chars() {
        let f = cert_fingerprint_der(b"x");
        assert_eq!(f.len(), SHA256_LEN * 3 - 1); // 32 pairs * 2 chars + 31 colons
        assert_eq!(f.matches(':').count(), 31);
    }

    #[test]
    fn fingerprint_parse_round_trip() {
        let f = cert_fingerprint_der(b"x");
        let bytes = parse_fingerprint(&f).unwrap();
        assert_eq!(bytes.len(), 32);
    }

    #[test]
    fn fingerprint_parse_rejects_short() {
        let r = parse_fingerprint("AB:CD");
        assert!(r.is_err());
    }
}
