//! Load or generate the daemon's long-term TLS identity.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use tracing::info;

use phonebridge_crypto::cert::{self, Identity};

/// Load an existing identity from `cert_pem` + `key_pem`, or generate a fresh
/// one (valid 10 years) and persist it.
pub fn load_or_generate(cert_pem: &Path, key_pem: &Path) -> Result<Identity> {
    if cert_pem.exists() && key_pem.exists() {
        let cert_pem_str = fs::read_to_string(cert_pem)
            .with_context(|| format!("reading cert {}", cert_pem.display()))?;
        let key_pem_str = fs::read_to_string(key_pem)
            .with_context(|| format!("reading key {}", key_pem.display()))?;

        // Recompute fingerprint from the on-disk cert.
        let cert_der = pem_to_der(&cert_pem_str)?;
        let fingerprint = phonebridge_crypto::fingerprint::cert_fingerprint_der(&cert_der);

        info!(fingerprint = %fingerprint, "loaded existing TLS identity");
        return Ok(Identity {
            cert_pem: cert_pem_str,
            key_pem: key_pem_str,
            fingerprint,
        });
    }

    info!("no existing TLS identity found; generating a new one (valid 10 years)");
    let id = cert::generate_self_signed("phonebridge-daemon", 3650)
        .map_err(|e| anyhow::anyhow!(e))?;
    if let Some(parent) = cert_pem.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(cert_pem, &id.cert_pem)
        .with_context(|| format!("writing cert {}", cert_pem.display()))?;
    fs::write(key_pem, &id.key_pem)
        .with_context(|| format!("writing key {}", key_pem.display()))?;
    // Set best-effort 0600 on key.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(key_pem)?.permissions();
        perms.set_mode(0o600);
        fs::set_permissions(key_pem, perms)?;
    }
    info!(fingerprint = %id.fingerprint, "wrote new TLS identity");
    Ok(id)
}

/// Extract the first DER CERTIFICATE blob from a PEM file.
fn pem_to_der(pem_str: &str) -> Result<Vec<u8>> {
    use base64::Engine;
    let mut in_cert = false;
    let mut b64 = String::new();
    for line in pem_str.lines() {
        let line = line.trim();
        if line.contains("BEGIN CERTIFICATE") {
            in_cert = true;
            continue;
        }
        if line.contains("END CERTIFICATE") {
            in_cert = false;
            break;
        }
        if in_cert {
            b64.push_str(line);
        }
    }
    if b64.is_empty() {
        anyhow::bail!("no CERTIFICATE block found");
    }
    let der = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .context("base64 decoding cert body")?;
    Ok(der)
}
