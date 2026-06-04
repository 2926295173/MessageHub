//! Console bus + WebSocket live-push handler.
//!
//! `phonebridge_bus::Bus` is per-connection. For the web console's
//! live-push, we need a **process-wide** bus that all WS handlers publish
//! to and the `/ws/console` endpoint subscribes from.

#![forbid(unsafe_code)]
#![allow(missing_docs)]

use std::sync::Arc;

use futures::{SinkExt, StreamExt};
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::Value;
use tokio::sync::broadcast;
use tracing::warn;
use uuid::Uuid;

use phonebridge_proto::Envelope;

/// One event published to the console.
#[derive(Debug, Clone, Serialize)]
pub struct ConsoleEvent {
    /// `notification.received`, `sms.received`, `device.hello`, etc.
    pub kind: String,
    /// Device id this event relates to.
    pub device_id: Uuid,
    /// Original envelope id.
    pub envelope_id: Uuid,
    /// Unix epoch ms.
    pub timestamp: i64,
    /// Best-effort summary fields (e.g. `{"package": "com.test", "title": "..."}`).
    pub summary: Value,
}

impl ConsoleEvent {
    /// Build a ConsoleEvent from an Envelope, extracting a tiny summary.
    pub fn from_envelope(env: &Envelope) -> Self {
        let mut summary = serde_json::Map::new();
        let kind = env.message_type.as_str().to_string();
        match env.message_type {
            phonebridge_proto::MessageType::NotificationReceived => {
                if let Ok(n) = env.parse_payload::<phonebridge_proto::NotificationReceived>() {
                    summary.insert("package".into(), Value::String(n.package));
                    summary.insert("title".into(), Value::String(n.title));
                    summary.insert("app_name".into(), n.app_name.map(Value::String).unwrap_or(Value::Null));
                }
            }
            phonebridge_proto::MessageType::SmsReceived => {
                if let Ok(s) = env.parse_payload::<phonebridge_proto::SmsReceived>() {
                    summary.insert("address".into(), Value::String(s.address));
                    summary.insert("body".into(), Value::String(s.body));
                }
            }
            phonebridge_proto::MessageType::CallIncoming | phonebridge_proto::MessageType::CallState => {
                // (Use raw payload as summary.)
                summary.insert("raw".into(), env.payload.clone());
            }
            phonebridge_proto::MessageType::DeviceHello => {
                if let Ok(h) = env.parse_payload::<phonebridge_proto::DeviceHello>() {
                    summary.insert("name".into(), Value::String(h.name));
                }
            }
            phonebridge_proto::MessageType::DeviceUnpair => {}
            _ => {}
        }
        Self {
            kind,
            device_id: env.device_id,
            envelope_id: env.id,
            timestamp: env.ts,
            summary: Value::Object(summary),
        }
    }
}

/// A process-wide broadcast bus for the web console.
#[derive(Clone)]
pub struct ConsoleBus {
    tx: broadcast::Sender<ConsoleEvent>,
    /// Number of current subscribers (for the dashboard tile).
    subscriber_count: Arc<Mutex<usize>>,
}

impl ConsoleBus {
    /// Create a new console bus with the given channel capacity.
    pub fn new(capacity: usize) -> Self {
        let (tx, _rx) = broadcast::channel(capacity);
        Self {
            tx,
            subscriber_count: Arc::new(Mutex::new(0)),
        }
    }

    /// Publish an event. Drops events if no subscriber (best-effort).
    pub fn publish(&self, env: &Envelope) {
        let evt = ConsoleEvent::from_envelope(env);
        if let Err(e) = self.tx.send(evt) {
            // Only log occasionally; this is expected if no console is
            // connected.
            if self.subscriber_count.lock().gt(&0) {
                warn!("console bus publish failed: {e}");
            }
        }
    }

    /// Subscribe to all events.
    pub fn subscribe(&self) -> ConsoleSubscriber {
        let rx = self.tx.subscribe();
        *self.subscriber_count.lock() += 1;
        ConsoleSubscriber {
            rx,
            counter: self.subscriber_count.clone(),
        }
    }

    /// Number of active subscribers.
    pub fn subscriber_count(&self) -> usize {
        *self.subscriber_count.lock()
    }
}

impl Default for ConsoleBus {
    fn default() -> Self {
        Self::new(1024)
    }
}

/// A subscription that automatically decrements the count on drop.
pub struct ConsoleSubscriber {
    rx: broadcast::Receiver<ConsoleEvent>,
    counter: Arc<Mutex<usize>>,
}

impl ConsoleSubscriber {
    /// Await the next event.
    pub async fn recv(&mut self) -> Result<ConsoleEvent, broadcast::error::RecvError> {
        self.rx.recv().await
    }
}

impl Drop for ConsoleSubscriber {
    fn drop(&mut self) {
        let mut g = self.counter.lock();
        if *g > 0 {
            *g -= 1;
        }
    }
}

/// Run a single console WS connection. Pushes events until the client
/// disconnects.
pub async fn run_console_ws<S>(
    stream: S,
    peer: std::net::SocketAddr,
    bus: ConsoleBus,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let ws = match tokio_tungstenite::accept_async(stream).await {
        Ok(ws) => ws,
        Err(e) => {
            warn!(%peer, "console ws accept failed: {e}");
            return;
        }
    };
    let (mut sink, mut stream) = ws.split();
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
        .send(tokio_tungstenite::tungstenite::Message::Text(hello.to_string()))
        .await
    {
        warn!(%peer, "console ws send hello failed: {e}");
        return;
    }

    // Concurrently forward events and read client frames.
    loop {
        tokio::select! {
            evt = sub.recv() => {
                match evt {
                    Ok(e) => {
                        let json = match serde_json::to_string(&e) {
                            Ok(s) => s,
                            Err(e) => {
                                warn!("console event serialize: {e}");
                                continue;
                            }
                        };
                        if let Err(e) = sink.send(tokio_tungstenite::tungstenite::Message::Text(json)).await {
                            warn!(%peer, "console ws send failed: {e}");
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
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

use tracing::info;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_from_hello_envelope() {
        use phonebridge_proto::{DeviceHello, DeviceType, Envelope, MessageType};
        let env = Envelope::new(
            MessageType::DeviceHello,
            Uuid::new_v4(),
            DeviceHello {
                name: "Test".into(),
                device_type: DeviceType::Android,
                protocol_version: 1,
                pubkey: "AAAA".into(),
                port: None,
                manufacturer: None,
                model: None,
            },
        )
        .unwrap();
        let evt = ConsoleEvent::from_envelope(&env);
        assert_eq!(evt.kind, "device.hello");
        assert_eq!(evt.summary["name"], "Test");
    }

    #[test]
    fn event_from_notification_envelope() {
        use phonebridge_proto::{Envelope, MessageType, NotificationReceived};
        let env = Envelope::new(
            MessageType::NotificationReceived,
            Uuid::new_v4(),
            NotificationReceived {
                id: "n1".into(),
                package: "com.test".into(),
                app_name: Some("Test".into()),
                title: "Hello".into(),
                content: "World".into(),
                posted_at: 0,
                is_sensitive: false,
                category: None,
            },
        )
        .unwrap();
        let evt = ConsoleEvent::from_envelope(&env);
        assert_eq!(evt.kind, "notification.received");
        assert_eq!(evt.summary["package"], "com.test");
        assert_eq!(evt.summary["title"], "Hello");
    }

    #[tokio::test]
    async fn publishes_via_sink_and_receives() {
        use phonebridge_proto::{Envelope, MessageType, NotificationReceived};
        use std::sync::Arc;
        use tokio::time::timeout;
        let bus = ConsoleBus::new(8);
        let mut sub = bus.subscribe();

        let env = Envelope::new(
            MessageType::NotificationReceived,
            Uuid::new_v4(),
            NotificationReceived {
                id: "n1".into(),
                package: "x".into(),
                app_name: None,
                title: "T".into(),
                content: "C".into(),
                posted_at: 0,
                is_sensitive: false,
                category: None,
            },
        )
        .unwrap();
        bus.publish(&env);
        // With a subscriber alive, the publish should be delivered.
        let evt = timeout(
            tokio::time::Duration::from_secs(2),
            sub.recv(),
        )
        .await
        .expect("recv timeout")
        .expect("channel closed");
        assert_eq!(evt.kind, "notification.received");
        assert_eq!(evt.summary["package"], "x");
        // No need to manually increment counters; just check the subscriber
        // count semantics.
        let _ = Arc::new(()); // suppress unused import
    }

    #[tokio::test]
    async fn publish_and_subscribe() {
        use phonebridge_proto::{DeviceHello, DeviceType, Envelope, MessageType};
        use tokio::time::timeout;
        let bus = ConsoleBus::new(16);
        assert_eq!(bus.subscriber_count(), 0);
        let mut sub = bus.subscribe();
        assert_eq!(bus.subscriber_count(), 1);
        let env = Envelope::new(
            MessageType::DeviceHello,
            Uuid::new_v4(),
            DeviceHello {
                name: "T".into(),
                device_type: DeviceType::Android,
                protocol_version: 1,
                pubkey: "A".into(),
                port: None,
                manufacturer: None,
                model: None,
            },
        )
        .unwrap();
        bus.publish(&env);
        let evt = timeout(tokio::time::Duration::from_secs(1), sub.recv())
            .await
            .expect("timeout")
            .expect("channel closed");
        assert_eq!(evt.kind, "device.hello");
        drop(sub);
        assert_eq!(bus.subscriber_count(), 0);
    }
}
