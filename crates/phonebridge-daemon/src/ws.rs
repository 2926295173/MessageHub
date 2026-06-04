//! WebSocket upgrade handler at /ws.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::ws::WebSocketUpgrade;
use axum::extract::{ConnectInfo, State};
use axum::response::IntoResponse;

use phonebridge_net::ws_handler::{self, WsContext, WsSink};
use phonebridge_storage::Db;

use crate::app_state::AppState;
use crate::daemon_sink::DaemonSink;

/// `GET /ws` — upgrade to a WebSocket connection.
pub async fn ws_upgrade(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    let sink: Arc<dyn WsSink + Send + Sync> = Arc::new(DaemonSink::new(state.db.clone()));
    let ctx = WsContext::new(state.our_device_id, sink, state.registry.clone());
    let peer_for_log = peer;
    ws.on_upgrade(move |socket| async move {
        if let Err(e) = ws_handler::handle_axum_connection(socket, peer_for_log, ctx).await {
            tracing::warn!(%peer_for_log, "ws handler ended with error: {e}");
        }
    })
}

/// Test-only: construct a [`WsContext`] from a `Db` for unit tests.
#[doc(hidden)]
pub fn test_context(our_id: uuid::Uuid, db: Arc<Db>) -> WsContext {
    let sink: Arc<dyn WsSink + Send + Sync> = Arc::new(DaemonSink::new(db));
    WsContext::new(our_id, sink, phonebridge_net::DeviceRegistry::new())
}
