// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 PhoneBridge Contributors
//
// This file is part of PhoneBridge. See LICENSE and the dual-licensing
// notice in README.md for details.

//! WebSocket upgrade handlers:
//! - `/ws`         — Android device connections.
//! - `/ws/console` — Web console live-push (re-broadcasts message-center bus events).

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
use crate::center_sink::CenterSink;
use crate::console_bus::ConsoleBus;

/// `GET /ws` — upgrade to a WebSocket connection from an Android client.
pub async fn ws_upgrade(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    let sink: Arc<dyn WsSink + Send + Sync> = Arc::new(CenterSink::new(
        state.db.clone(),
        state.console_bus.clone(),
        state.display_bus.clone(),
    ));
    // Pull the identity + listen port out of AppState under the
    // short RwLock sections so we can hand them to the connection
    // (and from there into the device.hello we send on accept).
    // The name and pubkey are read at request time; if the user
    // renames the daemon via Settings later, the next WS will see
    // the new name.
    let our_name = state.our_name.read().clone();
    let our_public_key_b64 = state.our_public_key_b64.read().clone();
    let our_listen_port = listen_port_from_bind(&state.config.server.bind);
    // Share the AppState's pairing map with this connection so that
    // REST handlers (e.g. POST /pair/start, /pair/accept) can drive
    // the Initiator state machine from outside the WS context.
    let mut ctx = WsContext::with_identity(
        state.our_device_id,
        our_name,
        our_public_key_b64,
        our_listen_port,
        sink,
        state.registry.clone(),
    );
    ctx.pairing = state.pairing.clone();
    ctx.pending_incoming = state.pending_incoming.clone();
    let peer_for_log = peer;
    ws.on_upgrade(move |socket| async move {
        if let Err(e) = ws_handler::handle_axum_connection(socket, peer_for_log, ctx).await {
            tracing::warn!(%peer_for_log, "ws handler ended with error: {e}");
        }
    })
}

/// Parse `host:port` → port. Returns None on malformed input —
/// callers should treat that as "we don't know where to dial" and
/// skip the `port` field in the outbound device.hello.
fn listen_port_from_bind(bind: &str) -> Option<u16> {
    // The bind string is `ip:port` (e.g. "0.0.0.0:8443") or
    // sometimes an IPv6 literal ("[::1]:8443"). Use SocketAddr's
    // parser which handles both shapes.
    bind.parse::<std::net::SocketAddr>().ok().map(|a| a.port())
}

/// `GET /ws/console` — upgrade to a WebSocket connection from the web
/// console. Pushes events from the message-center’s `ConsoleBus` to the client.
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
        "summary": {"server": "message-center", "version": env!("CARGO_PKG_VERSION")}
    });
    if let Err(e) = sink.send(Message::Text(hello.to_string())).await {
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
        std::sync::Arc::new(CenterSink::new(
            db,
            ConsoleBus::default(),
            crate::display_bus::DisplayBus::default(),
        ));
    phonebridge_net::ws_handler::WsContext::new(
        our_id,
        sink,
        phonebridge_net::DeviceRegistry::new(),
    )
}
