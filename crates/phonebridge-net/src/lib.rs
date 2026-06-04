//! Networking surface for the daemon.
//!
//! - [`mdns`]: browse for and advertise `_phonebridge._tcp` services.
//! - [`pairing`]: pairing state machine for both initiator and responder roles.
//! - [`ws_handler`]: per-connection envelope dispatcher (used by the daemon).
//! - [`tls_pinning`]: validate an incoming WebSocket connection's client cert
//!   against a stored fingerprint.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod mdns;
pub mod pairing;
pub mod tls_pinning;
pub mod ws_handler;

pub use pairing::{Initiator, PairingError, PairingOutcome, Responder};
pub use ws_handler::{DeviceSession, PairedSession, PairingMap, UnpairedSession, WsContext};

/// A discovered device on the LAN (alias for the mDNS-discovered form).
#[derive(Debug, Clone)]
pub struct DiscoveredDevice {
    /// Stable id of the device.
    pub device_id: uuid::Uuid,
    /// Human-readable name.
    pub name: String,
    /// Last advertised address.
    pub address: std::net::SocketAddr,
    /// TXT record key/value pairs.
    pub txt: Vec<(String, String)>,
}

/// Errors from the network layer.
#[derive(Debug, thiserror::Error)]
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
    /// Pairing state machine error.
    #[error("pairing: {0}")]
    Pairing(#[from] pairing::PairingError),
}

/// Result of parsing a single WebSocket text frame.
#[derive(Debug)]
pub enum FrameOutcome {
    /// A valid envelope.
    Envelope(phonebridge_proto::Envelope),
    /// A ping — respond with a pong at the tungstenite layer.
    Ping,
    /// A pong — ignore.
    Pong,
    /// A close frame — propagate.
    Close,
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
