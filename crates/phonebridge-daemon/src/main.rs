//! PhoneBridge desktop daemon entry point.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use axum::Router;
use clap::Parser;
use tracing::info;
use uuid::Uuid;

use phonebridge_core::{config::Config, logging, paths::AppPaths};
use phonebridge_storage::Db;

use phonebridge_daemon::app_state::AppState;
use phonebridge_daemon::ws::ws_upgrade;

#[derive(Parser, Debug)]
#[command(name = "phonebridge-daemon", version, about = "PhoneBridge desktop daemon")]
struct Args {
    /// Path to the config TOML. Defaults to ~/.config/phonebridge/config.toml.
    #[arg(long, env = "PHONEBRIDGE_CONFIG")]
    config: Option<PathBuf>,
    /// Override the bind address (e.g. `0.0.0.0:8443`).
    #[arg(long)]
    bind: Option<String>,
    /// Skip TLS (use plain HTTP/WS). Only for development.
    #[arg(long)]
    no_tls: bool,
    /// Run a one-shot pairing handshake against `addr` (e.g. `192.168.1.5:8443`)
    /// and exit. Used by `scripts/e2e-smoke.sh` to verify the wire protocol.
    #[arg(long)]
    pair_with: Option<SocketAddr>,
    /// Our device id (UUIDv4). If omitted, a persistent one is read from
    /// `{data_dir}/device_id` or generated and saved.
    #[arg(long)]
    device_id: Option<Uuid>,
    /// Our display name. Defaults to the host's hostname.
    #[arg(long)]
    name: Option<String>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Install the rustls process-level CryptoProvider (ring backend) before
    // any TLS code runs.
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

    // Load or generate our long-term identity.
    let id_module = phonebridge_daemon::identity::load_or_create(&paths, args.device_id, args.name.as_deref())
        .context("loading daemon identity")?;
    info!(device_id = %id_module.device_id, fingerprint = %id_module.fingerprint, name = %id_module.name, "daemon identity ready");

    // Optionally run a one-shot pairing against a peer and exit. Used by
    // the e2e smoke test and for manual debugging.
    if let Some(peer) = args.pair_with {
        return phonebridge_daemon::pair_cli::run(peer, id_module, Arc::new(config)).await;
    }

    // Build shared state.
    let registry = phonebridge_net::DeviceRegistry::new();
    let state = AppState::new(
        Arc::new(config.clone()),
        Arc::new(db),
        registry.clone(),
        id_module.device_id,
        id_module.public_key_b64,
        id_module.fingerprint,
        id_module.name,
    );
    let app = build_router(state.clone());

    // Start mDNS in the background.
    if config.discovery.enabled {
        match phonebridge_daemon::mdns_service::start(Arc::new(state.clone())) {
            Ok(_mdns) => {
                info!("mDNS service started");
            }
            Err(e) => {
                tracing::warn!("mDNS service failed to start: {e}");
            }
        }
    }

    // Load or generate TLS identity for the server.
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
            phonebridge_daemon::cert_loader::load_or_generate(&cert_pem_path, &key_pem_path)
                .context("loading TLS identity")?,
        )
    };
    if let Some(id) = &identity {
        info!(fingerprint = %id.fingerprint, "TLS identity ready");
    } else {
        info!("TLS disabled (--no-tls)");
    }

    // Parse bind address.
    let addr: SocketAddr = config
        .server
        .bind
        .parse()
        .with_context(|| format!("parsing bind address: {}", config.server.bind))?;

    // Run server.
    if let Some(id) = identity {
        phonebridge_daemon::tls::serve_https(addr, app, id).await?;
    } else {
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .with_context(|| format!("binding to {addr}"))?;
        info!(%addr, "listening (plain HTTP)");
        axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
            .await
            .context("axum::serve")?;
    }

    Ok(())
}

/// Build the axum router (used by both TLS and plain-HTTP paths).
fn build_router(state: AppState) -> Router {
    use tower_http::trace::TraceLayer;

    Router::new()
        .nest("/api/v1", phonebridge_daemon::rest::router())
        .merge(phonebridge_daemon::static_files::router())
        .route("/ws", axum::routing::get(ws_upgrade))
        .with_state(state)
        .layer(TraceLayer::new_for_http())
}
