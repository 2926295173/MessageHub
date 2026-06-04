//! PhoneBridge desktop daemon entry point.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use axum::Router;
use clap::Parser;
use tracing::info;

mod app_state;
mod cert_loader;
mod rest;
mod static_files;
mod tls;

use phonebridge_core::{config::Config, logging, paths::AppPaths};
use phonebridge_storage::Db;

use crate::app_state::AppState;

#[derive(Parser, Debug)]
#[command(name = "phonebridge-daemon", version, about = "PhoneBridge desktop daemon")]
struct Args {
    /// Path to the config TOML. Defaults to ~/.config/phonebridge/config.toml.
    #[arg(long, env = "PHONEBRIDGE_CONFIG")]
    config: Option<PathBuf>,
    /// Override the bind address (e.g. `0.0.0.0:8443`).
    #[arg(long)]
    bind: Option<String>,
    /// Skip TLS (use plain HTTP/WS). Only for development; refuse if a paired
    /// device is present in the database.
    #[arg(long)]
    no_tls: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Install the rustls process-level CryptoProvider (ring backend) before
    // any TLS code runs. Required by rustls 0.23 when both `ring` and
    // `aws_lc_rs` features are reachable. We ignore the error: it just means
    // a provider was already installed.
    let _ = rustls::crypto::ring::default_provider().install_default();

    // Resolve config + data paths early so we can pass them around.
    let paths = AppPaths::resolve().context("resolving app paths")?;
    paths.ensure().context("creating app directories")?;

    let config_path = args
        .config
        .clone()
        .unwrap_or_else(|| paths.config_file());
    let mut config = Config::load_from_file(&config_path)
        .with_context(|| format!("loading config from {}", config_path.display()))?;

    if let Some(bind) = args.bind {
        config.server.bind = bind;
    }

    // Init logging (after config is loaded).
    logging::init(&config.logging).context("initializing logging")?;

    info!(version = env!("CARGO_PKG_VERSION"), "phonebridge-daemon starting");
    info!(config = %config_path.display(), "config loaded");
    info!(bind = %config.server.bind, "binding");

    // Open DB.
    let db_path = if config.storage.db_path.is_empty() {
        paths.db_file()
    } else {
        PathBuf::from(&config.storage.db_path)
    };
    let db = Db::open(&db_path).await.context("opening database")?;
    db.migrate().await.context("running migrations")?;
    info!(db = %db_path.display(), "database ready");

    // Load or generate TLS identity.
    let cert_pem_path = if config.server.cert_path.is_empty() {
        paths.cert_file()
    } else {
        PathBuf::from(&config.server.cert_path)
    };
    let key_pem_path = if config.server.key_path.is_empty() {
        paths.key_file()
    } else {
        PathBuf::from(&config.server.key_path)
    };
    let identity = if args.no_tls {
        None
    } else {
        Some(
            cert_loader::load_or_generate(&cert_pem_path, &key_pem_path)
                .context("loading TLS identity")?,
        )
    };
    if let Some(id) = &identity {
        info!(fingerprint = %id.fingerprint, "TLS identity ready");
    } else {
        info!("TLS disabled (--no-tls)");
    }

    // Build shared state.
    let state = AppState::new(Arc::new(config.clone()), Arc::new(db));
    let app = build_router(state);

    // Parse bind address.
    let addr: SocketAddr = config
        .server
        .bind
        .parse()
        .with_context(|| format!("parsing bind address: {}", config.server.bind))?;

    // Run server.
    if let Some(id) = identity {
        tls::serve_https(addr, app, id).await?;
    } else {
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .with_context(|| format!("binding to {addr}"))?;
        info!(%addr, "listening (plain HTTP)");
        axum::serve(listener, app).await.context("axum::serve")?;
    }

    Ok(())
}

/// Build the axum router (used by both TLS and plain-HTTP paths).
fn build_router(state: AppState) -> Router {
    use tower_http::trace::TraceLayer;

    Router::new()
        .nest("/api/v1", rest::router())
        .merge(static_files::router())
        .with_state(state)
        .layer(TraceLayer::new_for_http())
}
