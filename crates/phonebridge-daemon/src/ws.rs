//! WebSocket upgrade handlers:
//! - `/ws`         — Android device connections.
//! - `/ws/console` — Web console live-push (re-broadcasts daemon bus events).

#![forbid(unsafe_code)]
#![allow(missing_docs)]

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, State};
use axum::response::IntoResponse;
use futures::{SinkExt, StreamExt};
use tracing::{info, warn};
use uuid::Uuid;

use phonebridge_net::ws_handler::{self, WsContext, WsSink};

use crate::app_state::AppState;
use crate::console_bus::ConsoleBus;
use crate::daemon_sink::DaemonSink;

/// `GET /ws` — upgrade to a WebSocket connection from an Android client.
pub async fn ws_upgrade(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    let sink: Arc<dyn WsSink + Send + Sync> = Arc::new(DaemonSink::new(
        state.db.clone(),
        state.console_bus.clone(),
    ));
    let ctx = WsContext::new(state.our_device_id, sink, state.registry.clone());
    let peer_for_log = peer;
    ws.on_upgrade(move |socket| async move {
        if let Err(e) = ws_handler::handle_axum_connection(socket, peer_for_log, ctx).await {
            tracing::warn!(%peer_for_log, "ws handler ended with error: {e}");
        }
    })
}

/// `GET /ws/console` — upgrade to a WebSocket connection from the web
/// console. Pushes events from the daemon's `ConsoleBus` to the client.
pub async fn ws_console_upgrade(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    let bus: ConsoleBus = state.console_bus.clone();
    let peer_for_log = peer;
    ws.on_upgrade(move |socket| async move {
        run_console_axum(socket, peer_for_log, bus).await;
    })
}

/// Drive a single console WS connection from an axum WebSocket.
async fn run_console_axum(socket: WebSocket, peer: SocketAddr, bus: ConsoleBus) {
    let (mut sink, mut stream) = socket.split();
    let mut sub = bus.subscribe();
    info!(%peer, "console ws: client connected ({} total)", bus.subscriber_count());

    // Send a hello event immediately so the client can confirm.
    let hello = serde_json::json!({
        "kind": "console.hello",
        "device_id": "00000000-0000-0000-0000-000000000000",
        "envelope_id": "00000000-0000-0000-0000-000000000000",
        "timestamp": chrono::Utc::now().timestamp_millis(),
        "summary": {"server": "phonebridge-daemon", "version": env!("CARGO_PKG_VERSION")}
    });
    if let Err(e) = sink
        .send(Message::Text(hello.to_string()))
        .await
    {
        warn!(%peer, "console ws send hello failed: {e}");
        return;
    }

    loop {
        tokio::select! {
            evt = sub.recv() => {
                match evt {
                    Ok(e) => {
                        let json = match serde_json::to_string(&e) {
                            Ok(s) => s,
                            Err(er) => {
                                warn!("console event serialize: {er}");
                                continue;
                            }
                        };
                        if let Err(e) = sink.send(Message::Text(json)).await {
                            warn!(%peer, "console ws send failed: {e}");
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
            frame = stream.next() => {
                match frame {
                    Some(Ok(_)) => continue,
                    Some(Err(e)) => {
                        warn!(%peer, "console ws frame error: {e}");
                        break;
                    }
                    None => break,
                }
            }
        }
    }
    info!(%peer, "console ws: client disconnected ({} total)", bus.subscriber_count());
}

#[allow(dead_code)]
fn _peer_marker(_p: &Uuid) {}

/// Test-only: construct a [`phonebridge_net::ws_handler::WsContext`] from a
/// `Db` and `ConsoleBus` for unit tests.
#[doc(hidden)]
pub fn test_context(
    our_id: Uuid,
    db: std::sync::Arc<phonebridge_storage::Db>,
) -> phonebridge_net::ws_handler::WsContext {
    let sink: std::sync::Arc<dyn phonebridge_net::WsSink + Send + Sync> =
        std::sync::Arc::new(DaemonSink::new(db, ConsoleBus::default()));
    phonebridge_net::ws_handler::WsContext::new(
        our_id,
        sink,
        phonebridge_net::DeviceRegistry::new(),
    )
}
