//! WebSocket upgrade handler at /ws.

use std::net::SocketAddr;

use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{ConnectInfo, State};
use axum::response::IntoResponse;

use phonebridge_net::ws_handler::{self, WsContext};

use crate::app_state::AppState;

/// `GET /ws` — upgrade to a WebSocket connection.
pub async fn ws_upgrade(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    let ctx = WsContext::new(state.our_device_id);
    let peer_for_log = peer;
    ws.on_upgrade(move |socket| async move {
        if let Err(e) = ws_handler::handle_axum_connection(socket, peer_for_log, ctx).await {
            tracing::warn!(%peer_for_log, "ws handler ended with error: {e}");
        }
    })
}
