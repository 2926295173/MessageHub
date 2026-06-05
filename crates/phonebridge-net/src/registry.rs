//! Registry of currently-connected devices, for sending envelopes
//! downstream (daemon → android).
//!
//! Each connection that completes `device.hello` registers an outbound
//! channel. The channel is removed when the connection closes.
//!
//! Outbound sends are best-effort: if the channel is full or the receiver
//! is gone, the send fails and the caller (e.g. the REST handler) is
//! expected to surface a 502/503 to the user.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use thiserror::Error;
use tokio::sync::mpsc;
use tracing::warn;
use uuid::Uuid;

use phonebridge_proto::Envelope;

/// Errors from downstream sends.
#[derive(Debug, Error)]
pub enum DownstreamError {
    /// The device is not currently connected.
    #[error("device {0} not connected")]
    NotConnected(Uuid),
    /// The channel is full; the send would block.
    #[error("device {0} send queue full")]
    QueueFull(Uuid),
}

/// A registry of `device_id -> mpsc::Sender<Envelope>` for outbound
/// downstream messages.
#[derive(Clone, Default)]
pub struct DeviceRegistry {
    inner: Arc<Mutex<HashMap<Uuid, mpsc::Sender<Envelope>>>>,
}

impl DeviceRegistry {
    /// Create an empty registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a device with its outbound channel. The caller (the WS
    /// handler) keeps a clone of the receiver and feeds it from the
    /// outbound task.
    pub fn register(&self, device_id: Uuid, tx: mpsc::Sender<Envelope>) {
        let mut g = self.inner.lock();
        g.insert(device_id, tx);
    }

    /// Remove a device's channel (e.g. on disconnect).
    pub fn unregister(&self, device_id: &Uuid) {
        let mut g = self.inner.lock();
        g.remove(device_id);
    }

    /// Try to send an envelope to a device.
    pub async fn try_send(&self, device_id: Uuid, env: Envelope) -> Result<(), DownstreamError> {
        let tx = {
            let g = self.inner.lock();
            g.get(&device_id).cloned()
        };
        let tx = tx.ok_or(DownstreamError::NotConnected(device_id))?;
        tx.try_send(env)
            .map_err(|_| DownstreamError::QueueFull(device_id))
    }

    /// Number of currently-registered devices.
    pub fn connected_count(&self) -> usize {
        self.inner.lock().len()
    }

    /// List of currently-connected device ids.
    pub fn connected_ids(&self) -> Vec<Uuid> {
        self.inner.lock().keys().copied().collect()
    }

    /// Best-effort fire-and-forget send. Logs on failure; useful when the
    /// caller doesn't care about the result (e.g. heartbeats).
    pub fn fire(&self, device_id: Uuid, env: Envelope) {
        let tx = {
            let g = self.inner.lock();
            g.get(&device_id).cloned()
        };
        if let Some(tx) = tx {
            if let Err(e) = tx.try_send(env) {
                warn!(%device_id, "fire-and-forget send failed: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    use phonebridge_proto::{DeviceHello, DeviceType, MessageType};

    fn dummy_env() -> Envelope {
        Envelope::new(
            MessageType::DeviceHeartbeat,
            Uuid::new_v4(),
            DeviceHello {
                name: "t".into(),
                device_type: DeviceType::Android,
                protocol_version: 1,
                pubkey: "A".into(),
                port: None,
                manufacturer: None,
                model: None,
            },
        )
        .unwrap()
    }

    #[tokio::test]
    async fn register_unregister_try_send() {
        let r = DeviceRegistry::new();
        let id = Uuid::new_v4();
        let (tx, mut rx) = mpsc::channel(8);
        r.register(id, tx);
        assert_eq!(r.connected_count(), 1);

        r.try_send(id, dummy_env()).await.unwrap();
        let got = rx.recv().await.unwrap();
        assert_eq!(got.message_type, MessageType::DeviceHeartbeat);

        r.unregister(&id);
        assert_eq!(r.connected_count(), 0);
        let r2 = r.try_send(id, dummy_env()).await;
        assert!(matches!(r2, Err(DownstreamError::NotConnected(_))));
    }

    #[tokio::test]
    async fn queue_full_surfaces_error() {
        let r = DeviceRegistry::new();
        let id = Uuid::new_v4();
        let (tx, _rx) = mpsc::channel(1);
        r.register(id, tx);
        // Fill the queue.
        r.try_send(id, dummy_env()).await.unwrap();
        // Next send should fail with QueueFull.
        let r2 = r.try_send(id, dummy_env()).await;
        assert!(matches!(r2, Err(DownstreamError::QueueFull(_))));
    }

    #[tokio::test]
    async fn fire_swallows_errors() {
        let r = DeviceRegistry::new();
        let id = Uuid::new_v4();
        // No registration, fire should be a no-op (no panic).
        r.fire(id, dummy_env());
    }
}
