//! Networking surface for the daemon.
//!
//! - [`mdns`]: browse for and advertise `_phonebridge._tcp` services.
//! - [`pairing`]: pairing state machine for both initiator and responder roles.
//! - [`registry`]: tracks currently-connected devices for downstream
//!   sends (daemon → android).
//! - [`ws_handler`]: per-connection envelope dispatcher (used by the daemon).
//! - [`tls_pinning`]: validate an incoming WebSocket connection's client cert
//!   against a stored fingerprint.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod mdns;
pub mod pairing;
pub mod registry;
pub mod tls_pinning;
pub mod ws_handler;

pub use pairing::{Initiator, PairingError, PairingOutcome, Responder};
pub use registry::{DeviceRegistry, DownstreamError};
pub use ws_handler::{
    ConnectionSink, DeviceSession, NoopSink, PairedSession, PairingMap, SinkError, UnpairedSession,
    WsContext, WsSink,
};

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
