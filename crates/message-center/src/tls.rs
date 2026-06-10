// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 PhoneBridge Contributors
//
// This file is part of PhoneBridge. See LICENSE and the dual-licensing
// notice in README.md for details.

//! TLS server using `axum-server` + `rustls`.

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::Router;
use axum_server::tls_rustls::RustlsConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::ServerConfig;
use tracing::info;

use phonebridge_crypto::cert::Identity;

/// Serve the given axum app over HTTPS on `addr` using `identity` for TLS.
pub async fn serve_https(addr: SocketAddr, app: Router, identity: Identity) -> Result<()> {
    let certs: Vec<CertificateDer<'static>> =
        rustls_pemfile::certs(&mut identity.cert_pem.as_bytes())
            .collect::<Result<Vec<_>, _>>()
            .context("parsing server cert chain")?;
    if certs.is_empty() {
        anyhow::bail!("no certificates found in identity cert PEM");
    }
    let key: PrivateKeyDer<'static> = rustls_pemfile::private_key(&mut identity.key_pem.as_bytes())
        .context("parsing server private key")?
        .context("no private key in identity key PEM")?;

    let cfg = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .context("invalid cert/key combination")?;
    let rustls_cfg = RustlsConfig::from_config(Arc::new(cfg));

    info!(%addr, "listening (HTTPS / TLS)");
    axum_server::bind_rustls(addr, rustls_cfg)
        .serve(app.into_make_service_with_connect_info::<SocketAddr>())
        .await
        .context("axum_server::bind_rustls")?;
    Ok(())
}
