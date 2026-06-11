// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 PhoneBridge Contributors
//
// This file is part of PhoneBridge. See LICENSE and the dual-licensing
// notice in README.md for details.

//! PhoneBridge desktop message-center entry point.

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

use message_center::app_state::AppState;
use message_center::ws::{ws_console_upgrade, ws_upgrade};

#[derive(Parser, Debug)]
#[command(
    name = "message-center",
    version,
    about = "PhoneBridge message-center (the central broker for Android events)"
)]
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
    /// Print the current display-endpoint token to stdout
    /// and exit. The token authenticates the
    /// `deskdisplay` endpoint to the message-center's
    /// `/ws/display` route. Keep it secret.
    #[arg(long, conflicts_with_all = ["rotate_display_token", "revoke_display_token"])]
    print_display_token: bool,
    /// Generate a fresh display-endpoint token, overwriting
    /// the old one. All running display endpoints disconnect
    /// immediately. Prints the new token to stdout and exits.
    #[arg(long, conflicts_with_all = ["print_display_token", "revoke_display_token"])]
    rotate_display_token: bool,
    /// Revoke the current display-endpoint token. For v1
    /// this is identical to `--rotate-display-token`: a new
    /// random token replaces the old one. The CLI flag is
    /// kept for forward compatibility (a future revision
    /// might add a deny-list of revoked tokens instead of
    /// rotating).
    #[arg(long, conflicts_with_all = ["print_display_token", "rotate_display_token"])]
    revoke_display_token: bool,
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

    // --print-display-token / --rotate-display-token /
    // --revoke-display-token short-circuit before the rest of
    // startup: we only need the token (load or generate), the
    // DB / cert / mDNS layers aren't required for these
    // subcommands.
    if args.print_display_token || args.rotate_display_token || args.revoke_display_token {
        let auth = message_center::display_auth::DisplayAuth::load_or_generate(&paths)
            .context("loading display-endpoint token")?;
        if args.rotate_display_token || args.revoke_display_token {
            auth.revoke().context("revoking display-endpoint token")?;
        }
        println!("{}", auth.current());
        eprintln!(
            "# Token file: {}\n\
             # Treat this value as a credential. Anyone with it\n\
             # can read events and send actions to the message-center.",
            auth.path().display()
        );
        return Ok(());
    }

    let config_path = args.config.clone().unwrap_or_else(|| paths.config_file());
    let mut config = Config::load_from_file(&config_path)
        .with_context(|| format!("loading config from {}", config_path.display()))?;

    // Capture whether the user explicitly passed `--bind` so the
    // `--no-tls` auto-shift below can leave their choice alone.
    let user_explicit_bind = args.bind.is_some();
    if let Some(bind) = args.bind {
        config.server.bind = bind;
    }

    // When the user opts into plain HTTP via `--no-tls` and did not
    // explicitly pick a port (`--bind` not passed), the default
    // `Config::server.bind` is `"0.0.0.0:8443"` — the *HTTPS* convention
    // port. Tools and operators see 8443 and assume TLS; browsers
    // and proxies will upgrade or warn. We do not want the
    // message-center squatting on a port that signals "secure" while
    // serving plain HTTP. Auto-shift to 8080 (the HTTP convention
    // port) in this case, and log the shift so the operator can
    // override explicitly.
    if args.no_tls && !user_explicit_bind {
        const HTTP_CONVENTION_PORT: &str = "0.0.0.0:8080";
        if config.server.bind == "0.0.0.0:8443" {
            tracing::info!(
                old = %config.server.bind,
                new = HTTP_CONVENTION_PORT,
                "--no-tls: defaulting bind to the HTTP-convention port \
                 (8443 is HTTPS-convention). Pass --bind to override."
            );
            config.server.bind = HTTP_CONVENTION_PORT.to_string();
        }
    }

    // Init logging (after config is loaded).
    logging::init(&config.logging).context("initializing logging")?;

    info!(
        version = env!("CARGO_PKG_VERSION"),
        "message-center starting"
    );
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
    let id_module =
        message_center::identity::load_or_create(&paths, args.device_id, args.name.as_deref())
            .context("loading message-center identity")?;
    info!(device_id = %id_module.device_id, fingerprint = %id_module.fingerprint, name = %id_module.name, "message-center identity ready");

    // Optionally run a one-shot pairing against a peer and exit. Used by
    // the e2e smoke test and for manual debugging.
    if let Some(peer) = args.pair_with {
        return message_center::pair_cli::run(peer, id_module, Arc::new(config)).await;
    }

    // Build shared state.
    let registry = phonebridge_net::DeviceRegistry::new();
    // Load (or generate) the display-endpoint auth token before
    // constructing AppState so the type system enforces that it
    // is always available. The token is persisted to
    // ~/.config/phonebridge/display.token with 0600 perms.
    let display_auth = message_center::display_auth::DisplayAuth::load_or_generate(&paths)
        .context("loading display-endpoint token")?;
    let state = AppState::new(
        Arc::new(config.clone()),
        Arc::new(db),
        registry.clone(),
        display_auth,
        id_module.device_id,
        id_module.public_key_b64,
        id_module.fingerprint,
        id_module.name,
    );
    let app = build_router(state.clone());

    // Start mDNS in the background.
    if config.discovery.enabled {
        match message_center::mdns_service::start(Arc::new(state.clone())) {
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
            message_center::cert_loader::load_or_generate(&cert_pem_path, &key_pem_path)
                .context("loading TLS identity")?,
        )
    };
    if let Some(id) = &identity {
        info!(fingerprint = %id.fingerprint, "TLS identity ready");
    } else {
        info!("TLS disabled (--no-tls)");
    }

    // Resolve the system locale once at startup so the web
    // console has a sensible default on a fresh visit. Logged
    // here (next to TLS / listen) so an operator can see what
    // the message-center's default will be without hitting the API.
    let system_locale = message_center::i18n::SystemLocale::detect();
    info!(
        default_locale = %system_locale.0,
        "web console default locale (from LANG / LC_ALL / LC_MESSAGES)"
    );

    // Parse bind address.
    let addr: SocketAddr = config
        .server
        .bind
        .parse()
        .with_context(|| format!("parsing bind address: {}", config.server.bind))?;

    // Run server.
    if let Some(id) = identity {
        message_center::tls::serve_https(addr, app, id).await?;
    } else {
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .with_context(|| format!("binding to {addr}"))?;
        info!(%addr, "listening (plain HTTP)");
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .context("axum::serve")?;
    }

    Ok(())
}

/// Build the axum router (used by both TLS and plain-HTTP paths).
fn build_router(state: AppState) -> Router {
    use tower_http::cors::{Any, CorsLayer};
    use tower_http::trace::TraceLayer;

    // All sub-routers are typed for AppState so we can `merge` them
    // freely, then apply `with_state` at the end. i18n is a pure
    // stateless router so it lives outside the `nest("/api/v1", ...)`
    // closure — we attach it to the v1 group below alongside the
    // rest API.
    let api = message_center::rest::router().merge(message_center::i18n::router());
    let api_docs = message_center::openapi::router();
    let static_assets = message_center::static_files::router();

    // Permissive CORS. PhoneBridge is a LAN-only tool — there is no
    // public attack surface — but the embedded Next.js console
    // needs cross-origin to call the API when the page is opened
    // from a host that is not the message-center (e.g.
    // https://192.168.123.186:8443/console/ from your laptop).
    //
    // We deliberately allow all origins, all methods, all standard
    // headers, and the credentials mode is `include` so the
    // browser will send auth cookies if the user enables them
    // later. This is a single-user, network-isolated message-center, not
    // a public API.
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
        .allow_credentials(false)
        .max_age(std::time::Duration::from_secs(3600));

    Router::new()
        .nest("/api/v1", api)
        .route("/ws", axum::routing::get(ws_upgrade))
        .route("/ws/console", axum::routing::get(ws_console_upgrade))
        .route(
            "/ws/display",
            axum::routing::get(message_center::display_ws::ws_display_upgrade),
        )
        .merge(api_docs)
        .merge(static_assets)
        .with_state(state)
        // TraceLayer first so the request log sees the headers
        // before CORS trims the response.
        .layer(TraceLayer::new_for_http())
        .layer(cors)
}
