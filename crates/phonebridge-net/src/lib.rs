//! Networking surface for the daemon.
//!
//! M1: define types and traits, but no real implementations.
//! M2: mDNS browse + respond; WebSocket upgrade + envelope routing; TLS
//!     pinning during handshake.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::net::SocketAddr;

use async_trait::async_trait;
use thiserror::Error;
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use phonebridge_proto::Envelope;

/// A TLS-upgraded WebSocket stream (client or server side).
pub type PhoneBridgeStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// A discovered device on the LAN.
#[derive(Debug, Clone)]
pub struct DiscoveredDevice {
    /// Stable id of the device.
    pub device_id: uuid::Uuid,
    /// Human-readable name.
    pub name: String,
    /// Last advertised address.
    pub address: SocketAddr,
    /// TXT record key/value pairs.
    pub txt: Vec<(String, String)>,
}

/// Errors from the network layer.
#[derive(Debug, Error)]
pub enum NetError {
    /// mDNS browse / respond failed.
    #[error("mDNS error: {0}")]
    Mdns(String),
    /// WebSocket I/O failed.
    #[error("WebSocket error: {0}")]
    WebSocket(#[from] tokio_tungstenite::tungstenite::Error),
    /// JSON encode/decode failed.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    /// Protocol-level error (e.g. wrong version, schema violation).
    #[error("protocol error: {0}")]
    Protocol(String),
}

/// Result of parsing a single WebSocket text frame.
#[derive(Debug)]
pub enum FrameOutcome {
    /// A valid envelope.
    Envelope(Envelope),
    /// A ping — respond with a pong at the tungstenite layer.
    Ping,
    /// A pong — ignore.
    Pong,
    /// A close frame — propagate.
    Close,
}

/// Trait for the WebSocket message handler. M2 will implement this for the
/// daemon's pairing + message router.
#[async_trait]
pub trait EnvelopeHandler: Send + Sync + 'static {
    /// Handle a single incoming envelope from a connected device.
    async fn handle(
        &self,
        envelope: Envelope,
        ctx: &mut ConnectionContext<'_>,
    ) -> Result<Option<Envelope>, NetError>;
}

/// Per-connection context the handler can use to push envelopes back to the
/// client or close the socket.
pub struct ConnectionContext<'a> {
    /// The remote peer address.
    pub peer: SocketAddr,
    /// The peer device's UUIDv4 id (set after `device.hello`).
    pub device_id: Option<uuid::Uuid>,
    /// Sink for outgoing envelopes on this connection.
    pub sink: &'a mut tokio::sync::mpsc::Sender<Envelope>,
}

impl<'a> ConnectionContext<'a> {
    /// Try to enqueue an outgoing envelope (non-blocking).
    pub fn try_send(&mut self, env: Envelope) -> Result<(), NetError> {
        self.sink.try_send(env).map_err(|e| NetError::Protocol(e.to_string()))
    }
}

/// Marker trait for the mDNS service browser (M2 implements).
#[async_trait]
pub trait DiscoveryBrowser: Send + Sync + 'static {
    /// Register a callback that fires for every discovered device.
    async fn browse<F>(&self, callback: F) -> Result<(), NetError>
    where
        F: Fn(DiscoveredDevice) + Send + Sync + 'static;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovered_device_debug() {
        let d = DiscoveredDevice {
            device_id: uuid::Uuid::nil(),
            name: "Pixel".into(),
            address: "127.0.0.1:8443".parse().unwrap(),
            txt: vec![("pubkey".into(), "AAAA".into())],
        };
        let s = format!("{d:?}");
        assert!(s.contains("Pixel"));
    }
}
