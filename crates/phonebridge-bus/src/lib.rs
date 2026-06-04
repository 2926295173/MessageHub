//! In-process event bus. The daemon's modules publish events here; downstream
//! subscribers (UI, audit log, future automation rules) attach themselves.
//!
//! M1: stub with a typed `Bus` handle + subscribe/publish primitives.
//! M2: integrate with the WebSocket handler and SQLite audit log.
//! Future: expose the trait to a plugin host (WebAssembly, etc.).

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::broadcast;

use phonebridge_proto::Envelope;

/// Capacity of the broadcast channel. Tune for bursty event sources.
pub const DEFAULT_CHANNEL_CAPACITY: usize = 1024;

/// A single bus event: a fully-parsed wire envelope (or, optionally, an
/// internally-generated synthetic event in the future).
#[derive(Debug, Clone)]
pub struct BusEvent {
    /// The envelope that triggered this event.
    pub envelope: Envelope,
}

impl BusEvent {
    /// Wrap an envelope in a bus event.
    pub fn from_envelope(envelope: Envelope) -> Self {
        Self { envelope }
    }
}

/// Subscriber handle returned by [`Bus::subscribe`].
pub struct Subscriber {
    rx: broadcast::Receiver<Arc<BusEvent>>,
}

impl Subscriber {
    /// Receive the next event, awaiting if none is queued.
    pub async fn recv(&mut self) -> Result<Arc<BusEvent>, tokio::sync::broadcast::error::RecvError> {
        self.rx.recv().await
    }
}

/// A trait for types that handle bus events asynchronously.
///
/// Used to define the plugin hook surface (M2+).
#[async_trait]
pub trait EventHandler: Send + Sync + 'static {
    /// A human-readable name for logging.
    fn name(&self) -> &str;
    /// Process the event. Returning an error is logged but does not stop
    /// other handlers from running.
    async fn handle(&self, event: &BusEvent) -> Result<(), BusError>;
}

/// Central bus handle.
#[derive(Clone)]
pub struct Bus {
    tx: broadcast::Sender<Arc<BusEvent>>,
}

impl Bus {
    /// Create a new bus with the default capacity.
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_CHANNEL_CAPACITY)
    }

    /// Create a new bus with a custom capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        let (tx, _rx) = broadcast::channel(capacity);
        Self { tx }
    }

    /// Subscribe to all events.
    pub fn subscribe(&self) -> Subscriber {
        Subscriber { rx: self.tx.subscribe() }
    }

    /// Publish an event to all subscribers. Returns the number of receivers
    /// that received it.
    pub fn publish(&self, event: BusEvent) -> usize {
        self.tx.send(Arc::new(event)).unwrap_or(0)
    }

    /// Number of active subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.tx.receiver_count()
    }
}

impl Default for Bus {
    fn default() -> Self {
        Self::new()
    }
}

/// Errors from event handlers.
#[derive(Debug, thiserror::Error)]
pub enum BusError {
    /// Handler reported a failure.
    #[error("handler {0} failed: {1}")]
    Handler(String, String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use phonebridge_proto::{DeviceHello, DeviceType, MessageType};
    use uuid::Uuid;

    fn hello_envelope() -> Envelope {
        Envelope::new(
            MessageType::DeviceHello,
            Uuid::new_v4(),
            DeviceHello {
                name: "t".into(),
                device_type: DeviceType::Android,
                protocol_version: 1,
                pubkey: "AAAA".into(),
                port: None,
                manufacturer: None,
                model: None,
            },
        )
        .unwrap()
    }

    #[tokio::test]
    async fn publish_delivers_to_subscribers() {
        let bus = Bus::new();
        let mut sub1 = bus.subscribe();
        let mut sub2 = bus.subscribe();
        let env = hello_envelope();
        bus.publish(BusEvent::from_envelope(env.clone()));
        let got1 = sub1.recv().await.unwrap();
        let got2 = sub2.recv().await.unwrap();
        assert_eq!(got1.envelope.message_type, MessageType::DeviceHello);
        assert_eq!(got2.envelope.message_type, MessageType::DeviceHello);
    }

    struct CountingHandler {
        name: String,
        count: std::sync::Arc<std::sync::Mutex<u32>>,
    }

    #[async_trait]
    impl EventHandler for CountingHandler {
        fn name(&self) -> &str { &self.name }
        async fn handle(&self, _: &BusEvent) -> Result<(), BusError> {
            *self.count.lock().unwrap() += 1;
            Ok(())
        }
    }

    #[tokio::test]
    async fn handler_trait_round_trip() {
        let h = CountingHandler {
            name: "counter".into(),
            count: std::sync::Arc::new(std::sync::Mutex::new(0)),
        };
        let event = BusEvent::from_envelope(hello_envelope());
        h.handle(&event).await.unwrap();
        assert_eq!(*h.count.lock().unwrap(), 1);
    }
}
