// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 PhoneBridge Contributors
//
// This file is part of PhoneBridge. See LICENSE and the dual-licensing
// notice in README.md for details.

//! MessageCenter's long-term identity: device id, ECDH keypair, cert, fingerprint.

use std::path::Path;

use anyhow::{Context, Result};
use uuid::Uuid;

use phonebridge_core::paths::AppPaths;
use phonebridge_crypto::cert;
use ring::rand::SystemRandom;

/// MessageCenter's persistent identity.
pub struct CenterIdentity {
    /// Stable UUIDv4.
    pub device_id: Uuid,
    /// Display name (e.g. hostname).
    pub name: String,
    /// PEM-encoded self-signed cert.
    pub cert_pem: String,
    /// SHA-256 fingerprint of the cert.
    pub fingerprint: String,
    /// Base64 of the long-term public key.
    pub public_key_b64: String,
}

const DEVICE_ID_FILE: &str = "device_id";
const DEVICE_NAME_FILE: &str = "device_name";
const DEVICE_KEY_FILE: &str = "device_key.pem";
const DEVICE_CERT_FILE: &str = "device_cert.pem";

/// Load or create the message-center identity, persisting it under `{data_dir}`.
pub fn load_or_create(
    paths: &AppPaths,
    override_id: Option<Uuid>,
    override_name: Option<&str>,
) -> Result<CenterIdentity> {
    let id_path = paths.data_dir.join(DEVICE_ID_FILE);
    let name_path = paths.data_dir.join(DEVICE_NAME_FILE);
    let cert_path = paths.data_dir.join(DEVICE_CERT_FILE);
    let key_path = paths.data_dir.join(DEVICE_KEY_FILE);

    let device_id = match override_id {
        Some(id) => {
            std::fs::write(&id_path, id.to_string())?;
            id
        }
        None => match std::fs::read_to_string(&id_path) {
            Ok(s) => Uuid::parse_str(s.trim())?,
            Err(_) => {
                let id = Uuid::new_v4();
                std::fs::write(&id_path, id.to_string())?;
                id
            }
        },
    };

    let name = match override_name {
        Some(n) => {
            std::fs::write(&name_path, n)?;
            n.to_string()
        }
        None => match std::fs::read_to_string(&name_path) {
            Ok(s) => s.trim().to_string(),
            Err(_) => {
                let h = std::env::var("HOSTNAME")
                    .ok()
                    .or_else(|| {
                        std::fs::read_to_string("/etc/hostname")
                            .ok()
                            .map(|s| s.trim().to_string())
                    })
                    .unwrap_or_else(|| "phonebridge".to_string());
                std::fs::write(&name_path, &h)?;
                h
            }
        },
    };

    if cert_path.exists() && key_path.exists() {
        // Reload existing cert.
        let cert_pem = std::fs::read_to_string(&cert_path)?;
        let fingerprint = fingerprint_from_pem(&cert_pem)?;
        let public_key_b64 = pubkey_from_pem(&cert_pem)?;
        return Ok(CenterIdentity {
            device_id,
            name,
            cert_pem,
            fingerprint,
            public_key_b64,
        });
    }

    // Generate fresh.
    let id = cert::generate_self_signed("message-center", 3650).map_err(|e| anyhow::anyhow!(e))?;
    std::fs::write(&cert_path, &id.cert_pem)?;
    std::fs::write(&key_path, &id.key_pem)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&key_path)?.permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(&key_path, perms)?;
    }
    let public_key_b64 = pubkey_from_pem(&id.cert_pem)?;
    Ok(CenterIdentity {
        device_id,
        name,
        cert_pem: id.cert_pem,
        fingerprint: id.fingerprint,
        public_key_b64,
    })
}

/// Compute the SHA-256 fingerprint of the first CERTIFICATE in a PEM.
pub fn fingerprint_from_pem(pem: &str) -> Result<String> {
    let der = pem_body(pem, "CERTIFICATE")?;
    Ok(phonebridge_crypto::fingerprint::cert_fingerprint_der(&der))
}

/// Extract the SubjectPublicKeyInfo (65 bytes uncompressed P-256 point) from
/// the first CERTIFICATE in a PEM, then base64-encode it (no padding).
pub fn pubkey_from_pem(pem: &str) -> Result<String> {
    use base64::Engine;
    let der = pem_body(pem, "CERTIFICATE")?;
    // Walk the DER looking for the 0x04 || X || Y sequence. We don't
    // bother with full ASN.1 parsing — for our self-signed P-256 certs
    // the uncompressed point appears as a fixed 65-byte sequence at a
    // well-known position in the SPKI.
    for i in 0..der.len().saturating_sub(65) {
        if der[i] == 0x04 && der[i + 1] != 0x00 {
            let candidate = &der[i..i + 65];
            if candidate.iter().any(|&b| b != 0) {
                let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(candidate);
                return Ok(b64);
            }
        }
    }
    anyhow::bail!("could not find P-256 public key in cert DER")
}

fn pem_body(pem: &str, label: &str) -> Result<Vec<u8>> {
    use base64::Engine;
    let begin = format!("-----BEGIN {label}-----");
    let end = format!("-----END {label}-----");
    let mut in_block = false;
    let mut b64 = String::new();
    for line in pem.lines() {
        let line = line.trim();
        if line == begin {
            in_block = true;
            continue;
        }
        if line == end {
            break;
        }
        if in_block {
            b64.push_str(line);
        }
    }
    if b64.is_empty() {
        anyhow::bail!("no {label} block in PEM");
    }
    let der = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .with_context(|| format!("base64 decoding {label}"))?;
    Ok(der)
}

/// Re-export for tests.
pub fn _random_for_test() -> SystemRandom {
    SystemRandom::new()
}

#[allow(dead_code)]
fn _suppress_unused(_p: &Path) {
    let _ = std::path::PathBuf::new();
}
