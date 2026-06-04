//! Per-connection WebSocket envelope dispatcher.
//!
//! For M3, the handler:
//! - Registers the device in the [`DeviceRegistry`] after `device.hello`.
//! - Persists incoming `notification.received` / `sms.received` / `call.*`
//!   via the [`WsSink`] callback (the daemon implements this to write to
//!   SQLite).
//! - Provides a [`ConnectionSink`] to the rest of the system so other
//!   modules can send envelopes back to the device.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use axum::extract::ws::Message as AxumMessage;
use futures::{SinkExt, StreamExt};
use parking_lot::Mutex;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, info, warn};
use uuid::Uuid;

use phonebridge_bus::{Bus, BusEvent};
use phonebridge_proto::{
    CallAnswerRequest, CallDialRequest, CallEndRequest, CallHistory, CallHistoryEntry, CallIncoming,
    CallState, DeviceHello, DeviceType, Envelope, MessageType, NotificationDismissed,
    NotificationReceived, PairAccept, PairChallenge, PairComplete, PairConfirm, PairReject,
    PairRequest, SmsListRequest, SmsListResult, SmsReceived, SmsSendRequest, SmsSendResult, Unpair,
};

use crate::pairing::{Initiator, PairingError, PairingOutcome, Responder};
use crate::registry::DeviceRegistry;

/// Errors from the WS handler.
#[derive(Debug, Error)]
pub enum WsError {
    /// WebSocket I/O error.
    #[error("ws: {0}")]
    Ws(#[from] tokio_tungstenite::tungstenite::Error),
    /// axum WS error.
    #[error("axum ws: {0}")]
    Axum(#[from] axum::Error),
    /// JSON error.
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    /// Pairing state machine error.
    #[error("pairing: {0}")]
    Pairing(#[from] PairingError),
    /// Protocol-level rejection.
    #[error("protocol: {0}")]
    Protocol(String),
}

/// Per-device state held in the shared `PairingMap`.
pub enum DeviceSession {
    /// Not yet paired; carries either an Initiator (we're starting) or a
    /// Responder (we just received hello) state machine.
    Unpaired(UnpairedSession),
    /// Paired: persistent session, no in-flight state machine.
    Paired(PairedSession),
}

/// One half of a pending pairing.
pub enum UnpairedSession {
    /// We are the initiator (desktop clicked "Pair").
    Initiator(Initiator),
    /// We are the responder (Android opened the WS first; we just got
    /// `device.hello` and are waiting for the user's click on Android).
    Responder(Responder),
}

/// Bookkeeping for a paired connection.
#[derive(Debug, Clone)]
pub struct PairedSession {
    /// The paired device's id.
    pub device_id: Uuid,
    /// The paired device's display name.
    pub name: String,
    /// The paired device's pinned cert fingerprint.
    pub cert_fingerprint: String,
}

/// Shared map of `device_id -> DeviceSession`. Cloned cheaply (Arc inside).
#[derive(Clone, Default)]
pub struct PairingMap {
    inner: Arc<Mutex<HashMap<Uuid, DeviceSession>>>,
}

impl PairingMap {
    /// Create an empty map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a new session, replacing any existing one.
    pub fn insert(&self, device_id: Uuid, session: DeviceSession) {
        let mut g = self.inner.lock();
        g.insert(device_id, session);
    }

    /// Get a snapshot of the session for inspection.
    pub fn get(&self, device_id: &Uuid) -> Option<DeviceSession> {
        let g = self.inner.lock();
        g.get(device_id).map(clone_session)
    }

    /// Remove a session.
    pub fn remove(&self, device_id: &Uuid) -> Option<DeviceSession> {
        self.inner.lock().remove(device_id)
    }

    /// List all currently-paired devices.
    pub fn list_paired(&self) -> Vec<(Uuid, PairedSession)> {
        let g = self.inner.lock();
        g.iter()
            .filter_map(|(id, s)| match s {
                DeviceSession::Paired(p) => Some((*id, p.clone())),
                _ => None,
            })
            .collect()
    }
}

fn clone_session(s: &DeviceSession) -> DeviceSession {
    match s {
        DeviceSession::Paired(p) => DeviceSession::Paired(p.clone()),
        DeviceSession::Unpaired(_) => {
            DeviceSession::Unpaired(UnpairedSession::Responder(Responder::start(Uuid::nil()).expect("dummy")))
        }
    }
}

/// Errors from [`ConnectionSink`].
#[derive(Debug, Error)]
pub enum SinkError {
    /// Channel closed.
    #[error("sink closed")]
    Closed,
}

/// A sink for outgoing envelopes on a single connection. Cheap to clone.
#[derive(Clone)]
pub struct ConnectionSink {
    inner: mpsc::Sender<Envelope>,
}

impl ConnectionSink {
    /// Construct a new sink from an mpsc sender.
    pub fn new(inner: mpsc::Sender<Envelope>) -> Self {
        Self { inner }
    }
    /// Try to enqueue an envelope (non-blocking).
    pub fn try_send(&self, env: Envelope) -> Result<(), SinkError> {
        self.inner.try_send(env).map_err(|_| SinkError::Closed)
    }
    /// Awaiting send.
    pub async fn send(&self, env: Envelope) -> Result<(), SinkError> {
        self.inner.send(env).await.map_err(|_| SinkError::Closed)
    }
}

/// A callback the WS handler invokes for non-pairing messages that need to
/// be persisted or routed.
#[async_trait]
pub trait WsSink: Send + Sync + 'static {
    /// Persist a `notification.received` envelope.
    async fn on_notification(&self, device_id: Uuid, env: &NotificationReceived);
    /// Persist a `notification.dismissed` envelope.
    async fn on_notification_dismissed(
        &self,
        device_id: Uuid,
        env: &NotificationDismissed,
    );
    /// Persist a `sms.received` envelope.
    async fn on_sms_received(&self, device_id: Uuid, env: &SmsReceived);
    /// Persist a `sms.send.result` envelope and resolve any pending send.
    async fn on_sms_send_result(&self, device_id: Uuid, env: &SmsSendResult);
    /// Persist a `call.state` envelope (state transition).
    async fn on_call_state(&self, device_id: Uuid, env: &CallState);
    /// Persist a `call.incoming` envelope.
    async fn on_call_incoming(&self, device_id: Uuid, env: &CallIncoming);
    /// Persist a `call.history` envelope.
    async fn on_call_history(&self, device_id: Uuid, env: &CallHistory);
    /// Persist an `sms.list.result` envelope.
    async fn on_sms_list_result(&self, device_id: Uuid, env: &SmsListResult);
    /// Persist a `device.hello` envelope (update device row + last_seen).
    async fn on_hello(&self, device_id: Uuid, env: &DeviceHello);
    /// Persist a `device.unpair` envelope.
    async fn on_unpair(&self, device_id: Uuid, env: &Unpair);
    /// Called on connection close. Audit log + cleanup.
    async fn on_disconnect(&self, device_id: Uuid);
}

/// No-op sink used in tests.
pub struct NoopSink;

#[async_trait]
impl WsSink for NoopSink {
    async fn on_notification(&self, _: Uuid, _: &NotificationReceived) {}
    async fn on_notification_dismissed(&self, _: Uuid, _: &NotificationDismissed) {}
    async fn on_sms_received(&self, _: Uuid, _: &SmsReceived) {}
    async fn on_sms_send_result(&self, _: Uuid, _: &SmsSendResult) {}
    async fn on_call_state(&self, _: Uuid, _: &CallState) {}
    async fn on_call_incoming(&self, _: Uuid, _: &CallIncoming) {}
    async fn on_call_history(&self, _: Uuid, _: &CallHistory) {}
    async fn on_sms_list_result(&self, _: Uuid, _: &SmsListResult) {}
    async fn on_hello(&self, _: Uuid, _: &DeviceHello) {}
    async fn on_unpair(&self, _: Uuid, _: &Unpair) {}
    async fn on_disconnect(&self, _: Uuid) {}
}

/// Shared state passed to every WS connection.
#[derive(Clone)]
pub struct WsContext {
    /// Bus for broadcasting events to subscribers.
    pub bus: Bus,
    /// In-flight pairing sessions, keyed by device id.
    pub pairing: PairingMap,
    /// Downstream send registry (shared across the daemon process).
    pub registry: DeviceRegistry,
    /// Sink for non-pairing persistence.
    pub sink: Arc<dyn WsSink + Send + Sync>,
    /// This daemon's stable id.
    pub our_device_id: Uuid,
}

impl WsContext {
    /// Construct a new WS context with a fresh per-connection bus/pairing
    /// map, but a shared registry.
    pub fn new(our_device_id: Uuid, sink: Arc<dyn WsSink + Send + Sync>, registry: DeviceRegistry) -> Self {
        Self {
            bus: Bus::new(),
            pairing: PairingMap::new(),
            registry,
            sink,
            our_device_id,
        }
    }
}

/// Drive a single WebSocket connection. Returns when the peer disconnects
/// or an unrecoverable error occurs.
pub async fn handle_connection<S>(
    stream: S,
    peer_addr: std::net::SocketAddr,
    ctx: WsContext,
) -> Result<(), WsError>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let ws = tokio_tungstenite::accept_async(stream).await?;
    let (mut sink, mut stream) = ws.split();
    info!(%peer_addr, "ws: connection accepted");

    let (out_tx, mut out_rx) = mpsc::channel::<Envelope>(32);
    let device_id_holder: Arc<Mutex<Option<Uuid>>> = Arc::new(Mutex::new(None));
    let dev_id_writer = device_id_holder.clone();
    let writer = tokio::spawn(async move {
        while let Some(env) = out_rx.recv().await {
            if let Some(d) = *dev_id_writer.lock() {
                debug!(%d, "ws: outbound envelope");
            }
            let s = env.to_json();
            if let Err(e) = sink.send(Message::Text(s)).await {
                warn!(%peer_addr, "ws send error: {e}");
                break;
            }
        }
    });

    while let Some(frame) = stream.next().await {
        let frame = match frame? {
            Message::Text(t) => t,
            Message::Close(_) => {
                debug!(%peer_addr, "ws: peer closed");
                break;
            }
            Message::Ping(p) => {
                debug!(%peer_addr, "ws: ping ({} bytes)", p.len());
                continue;
            }
            other => {
                debug!(%peer_addr, "ws: ignoring frame {other:?}");
                continue;
            }
        };

        let env: Envelope = match serde_json::from_str(&frame) {
            Ok(e) => e,
            Err(e) => {
                warn!(%peer_addr, "ws: invalid JSON frame: {e}");
                continue;
            }
        };

        // Per-message side effects.
        match env.message_type {
            MessageType::DeviceHello => {
                // Persist device row.
                let hello = env
                    .parse_payload::<DeviceHello>()
                    .ok();
                if let Some(ref h) = hello {
                    ctx.sink.on_hello(env.device_id, h).await;
                }
                // Register in the registry so other code can send.
                *device_id_holder.lock() = Some(env.device_id);
                ctx.registry.register(env.device_id, out_tx.clone());
            }
            MessageType::NotificationReceived => {
                if let Ok(n) = env.parse_payload::<NotificationReceived>() {
                    ctx.sink.on_notification(env.device_id, &n).await;
                }
            }
            MessageType::NotificationDismissed => {
                if let Ok(n) = env.parse_payload::<NotificationDismissed>() {
                    ctx.sink.on_notification_dismissed(env.device_id, &n).await;
                }
            }
            MessageType::SmsReceived => {
                if let Ok(s) = env.parse_payload::<SmsReceived>() {
                    ctx.sink.on_sms_received(env.device_id, &s).await;
                }
            }
            MessageType::SmsSendResult => {
                if let Ok(s) = env.parse_payload::<SmsSendResult>() {
                    ctx.sink.on_sms_send_result(env.device_id, &s).await;
                }
            }
            MessageType::CallState => {
                if let Ok(c) = env.parse_payload::<CallState>() {
                    ctx.sink.on_call_state(env.device_id, &c).await;
                }
            }
            MessageType::CallIncoming => {
                if let Ok(c) = env.parse_payload::<CallIncoming>() {
                    ctx.sink.on_call_incoming(env.device_id, &c).await;
                }
            }
            MessageType::CallHistory => {
                if let Ok(c) = env.parse_payload::<CallHistory>() {
                    ctx.sink.on_call_history(env.device_id, &c).await;
                }
            }
            MessageType::SmsListResult => {
                if let Ok(r) = env.parse_payload::<SmsListResult>() {
                    ctx.sink.on_sms_list_result(env.device_id, &r).await;
                }
            }
            MessageType::DeviceUnpair => {
                if let Ok(u) = env.parse_payload::<Unpair>() {
                    ctx.sink.on_unpair(env.device_id, &u).await;
                }
            }
            _ => {}
        }

        if let Some(reply) = dispatch(&env, &ctx).await {
            if let Err(e) = out_tx.send(reply).await {
                warn!(%peer_addr, "ws: outbox full: {e}");
            }
        }

        // Always publish to the bus for subscribers (web console, audit log).
        ctx.bus.publish(BusEvent::from_envelope(env));
    }

    // Cleanup: unregister the device.
    let id_to_cleanup = *device_id_holder.lock();
    if let Some(id) = id_to_cleanup {
        ctx.registry.unregister(&id);
        ctx.sink.on_disconnect(id).await;
    }
    drop(out_tx);
    let _ = writer.await;
    info!(%peer_addr, "ws: connection closed");
    Ok(())
}

/// Drive a single WebSocket connection from an axum WebSocket.
pub async fn handle_axum_connection(
    socket: axum::extract::ws::WebSocket,
    peer_addr: std::net::SocketAddr,
    ctx: WsContext,
) -> Result<(), WsError> {
    let (mut sink, mut stream) = socket.split();
    let (out_tx, mut out_rx) = mpsc::channel::<Envelope>(32);
    let device_id_holder: Arc<Mutex<Option<Uuid>>> = Arc::new(Mutex::new(None));
    let dev_id_writer = device_id_holder.clone();
    let writer = tokio::spawn(async move {
        while let Some(env) = out_rx.recv().await {
            if let Some(d) = *dev_id_writer.lock() {
                debug!(%d, "ws: outbound envelope");
            }
            let s = env.to_json();
            if let Err(e) = sink.send(AxumMessage::Text(s)).await {
                warn!(%peer_addr, "ws send error: {e}");
                break;
            }
        }
    });
    while let Some(frame) = stream.next().await {
        let frame = match frame? {
            AxumMessage::Text(t) => t,
            AxumMessage::Close(_) => break,
            other => {
                debug!(%peer_addr, "ws: ignoring frame {other:?}");
                continue;
            }
        };
        let env: Envelope = match serde_json::from_str(&frame) {
            Ok(e) => e,
            Err(e) => {
                warn!(%peer_addr, "ws: invalid JSON frame: {e}");
                continue;
            }
        };

        match env.message_type {
            MessageType::DeviceHello => {
                if let Ok(h) = env.parse_payload::<DeviceHello>() {
                    ctx.sink.on_hello(env.device_id, &h).await;
                }
                *device_id_holder.lock() = Some(env.device_id);
                ctx.registry.register(env.device_id, out_tx.clone());
            }
            MessageType::NotificationReceived => {
                if let Ok(n) = env.parse_payload::<NotificationReceived>() {
                    ctx.sink.on_notification(env.device_id, &n).await;
                }
            }
            MessageType::NotificationDismissed => {
                if let Ok(n) = env.parse_payload::<NotificationDismissed>() {
                    ctx.sink.on_notification_dismissed(env.device_id, &n).await;
                }
            }
            MessageType::SmsReceived => {
                if let Ok(s) = env.parse_payload::<SmsReceived>() {
                    ctx.sink.on_sms_received(env.device_id, &s).await;
                }
            }
            MessageType::SmsSendResult => {
                if let Ok(s) = env.parse_payload::<SmsSendResult>() {
                    ctx.sink.on_sms_send_result(env.device_id, &s).await;
                }
            }
            MessageType::CallState => {
                if let Ok(c) = env.parse_payload::<CallState>() {
                    ctx.sink.on_call_state(env.device_id, &c).await;
                }
            }
            MessageType::CallIncoming => {
                if let Ok(c) = env.parse_payload::<CallIncoming>() {
                    ctx.sink.on_call_incoming(env.device_id, &c).await;
                }
            }
            MessageType::CallHistory => {
                if let Ok(c) = env.parse_payload::<CallHistory>() {
                    ctx.sink.on_call_history(env.device_id, &c).await;
                }
            }
            MessageType::SmsListResult => {
                if let Ok(r) = env.parse_payload::<SmsListResult>() {
                    ctx.sink.on_sms_list_result(env.device_id, &r).await;
                }
            }
            MessageType::DeviceUnpair => {
                if let Ok(u) = env.parse_payload::<Unpair>() {
                    ctx.sink.on_unpair(env.device_id, &u).await;
                }
            }
            _ => {}
        }

        if let Some(reply) = dispatch(&env, &ctx).await {
            if let Err(e) = out_tx.send(reply).await {
                warn!(%peer_addr, "ws: outbox full: {e}");
            }
        }

        ctx.bus.publish(BusEvent::from_envelope(env));
    }
    let id_to_cleanup = *device_id_holder.lock();
    if let Some(id) = id_to_cleanup {
        ctx.registry.unregister(&id);
        ctx.sink.on_disconnect(id).await;
    }
    drop(out_tx);
    let _ = writer.await;
    Ok(())
}

/// Dispatch a single envelope to the right handler. Returns an optional
/// envelope to send back.
async fn dispatch(env: &Envelope, ctx: &WsContext) -> Option<Envelope> {
    let our_id = ctx.our_device_id;
    match env.message_type {
        MessageType::DeviceHello => {
            // First contact. Insert a Responder state machine.
            let peer_id = env.device_id;
            if ctx.pairing.get(&peer_id).is_none() {
                let r = match Responder::start(peer_id) {
                    Ok(r) => r,
                    Err(e) => {
                        warn!("ws: failed to start Responder: {e}");
                        return None;
                    }
                };
                ctx.pairing
                    .insert(peer_id, DeviceSession::Unpaired(UnpairedSession::Responder(r)));
                info!(%peer_id, "ws: device.hello received, awaiting pair.request");
            }
            None
        }
        MessageType::DeviceHeartbeat => Envelope::new(
            MessageType::DeviceHeartbeat,
            our_id,
            phonebridge_proto::DeviceHeartbeat::default(),
        )
        .ok(),
        MessageType::DevicePairRequest => {
            warn!("ws: received device.pair.request from peer; ignoring (M3 daemon is responder only)");
            None
        }
        MessageType::DevicePairChallenge => {
            let peer_id = env.device_id;
            let session = ctx.pairing.get(&peer_id);
            let initiator = match session {
                Some(DeviceSession::Unpaired(UnpairedSession::Initiator(i))) => Some(i),
                _ => None,
            };
            if let Some(mut init) = initiator {
                ctx.pairing.remove(&peer_id);
                let r = init.on_challenge(env, our_id);
                let reply = match &r {
                    Ok(_) => init.build_accept_envelope(our_id).ok(),
                    Err(e) => init.build_reject_envelope(our_id, &e.to_string()).ok(),
                };
                ctx.pairing.insert(peer_id, DeviceSession::Unpaired(UnpairedSession::Initiator(init)));
                reply
            } else {
                warn!(%peer_id, "ws: pair.challenge for unknown initiator session");
                None
            }
        }
        MessageType::DevicePairConfirm => {
            let peer_id = env.device_id;
            let session = ctx.pairing.get(&peer_id);
            if let Some(DeviceSession::Unpaired(UnpairedSession::Initiator(init))) = session {
                ctx.pairing.remove(&peer_id);
                let confirm: PairConfirm = env.parse_payload().unwrap_or(PairConfirm { accepted: false });
                if confirm.accepted {
                    let reply = init.build_complete_envelope(our_id).ok();
                    ctx.pairing.insert(peer_id, DeviceSession::Unpaired(UnpairedSession::Initiator(init)));
                    reply
                } else {
                    None
                }
            } else {
                warn!(%peer_id, "ws: pair.confirm for unknown session");
                None
            }
        }
        MessageType::DevicePairComplete => {
            let peer_id = env.device_id;
            let session = ctx.pairing.get(&peer_id);
            match session {
                Some(DeviceSession::Unpaired(UnpairedSession::Initiator(init))) => {
                    ctx.pairing.remove(&peer_id);
                    let outcome: Result<PairingOutcome, _> = init.on_complete(env);
                    match outcome {
                        Ok(o) => {
                            info!(%peer_id, fingerprint = %o.peer_fingerprint, "ws: paired (initiator)");
                            ctx.pairing.insert(peer_id, DeviceSession::Paired(PairedSession {
                                device_id: peer_id,
                                name: "(unknown)".into(),
                                cert_fingerprint: o.peer_fingerprint.clone(),
                            }));
                            None
                        }
                        Err(e) => {
                            warn!(%peer_id, "ws: pair.complete rejected: {e}");
                            None
                        }
                    }
                }
                Some(DeviceSession::Unpaired(UnpairedSession::Responder(r))) => {
                    ctx.pairing.remove(&peer_id);
                    match r.on_complete(env) {
                        Ok(o) => {
                            info!(%peer_id, fingerprint = %o.peer_fingerprint, "ws: paired (responder)");
                            ctx.pairing.insert(peer_id, DeviceSession::Paired(PairedSession {
                                device_id: peer_id,
                                name: "(unknown)".into(),
                                cert_fingerprint: o.peer_fingerprint.clone(),
                            }));
                            None
                        }
                        Err(e) => {
                            warn!(%peer_id, "ws: pair.complete rejected: {e}");
                            None
                        }
                    }
                }
                _ => {
                    warn!(%peer_id, "ws: pair.complete for unknown session");
                    None
                }
            }
        }
        MessageType::DevicePairAccept
        | MessageType::DevicePairReject => None,
        _ => {
            debug!(message_type = %env.message_type, "ws: message handled in M3 dispatcher");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phonebridge_proto::DeviceHello;
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::Message;
    use futures::SinkExt;

    /// Open a TCP listener and return its address. The caller drives the
    /// accept + connect.
    async fn listen_addr() -> (TcpListener, std::net::SocketAddr) {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        (l, addr)
    }

    #[tokio::test]
    async fn connection_round_trip_with_noop_sink() {
        let (l, addr) = listen_addr().await;
        let server = tokio::spawn(async move {
            let (s, _) = l.accept().await.unwrap();
            s
        });
        let client = tokio::net::TcpStream::connect(addr).await.unwrap();
        let server_stream = server.await.unwrap();

        let ctx = WsContext::new(
            Uuid::new_v4(),
            Arc::new(NoopSink) as Arc<dyn WsSink + Send + Sync>,
            DeviceRegistry::new(),
        );
        let task = tokio::spawn(handle_connection(server_stream, addr, ctx));

        let mut ws = tokio_tungstenite::client_async("ws://localhost/", client)
            .await
            .unwrap()
            .0;
        let hello = Envelope::new(
            MessageType::DeviceHello,
            Uuid::new_v4(),
            DeviceHello {
                name: "test".into(),
                device_type: DeviceType::Android,
                protocol_version: 1,
                pubkey: "AAAA".into(),
                port: Some(18443),
                manufacturer: None,
                model: None,
            },
        )
        .unwrap();
        ws.send(Message::Text(hello.to_json())).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let _ = ws.send(Message::Close(None)).await;
        let _ = task.await;
    }

    /// Test that the registry gets populated after device.hello.
    #[tokio::test]
    async fn registry_populated_on_hello() {
        let (l, addr) = listen_addr().await;
        let server = tokio::spawn(async move {
            let (s, _) = l.accept().await.unwrap();
            s
        });
        let client = tokio::net::TcpStream::connect(addr).await.unwrap();
        let server_stream = server.await.unwrap();

        let ctx = WsContext::new(
            Uuid::new_v4(),
            Arc::new(NoopSink) as Arc<dyn WsSink + Send + Sync>,
            DeviceRegistry::new(),
        );
        let registry = ctx.registry.clone();
        let task = tokio::spawn(handle_connection(server_stream, addr, ctx));

        let mut ws = tokio_tungstenite::client_async("ws://localhost/", client)
            .await
            .unwrap()
            .0;
        let device_id = Uuid::new_v4();
        let hello = Envelope::new(
            MessageType::DeviceHello,
            device_id,
            DeviceHello {
                name: "test".into(),
                device_type: DeviceType::Android,
                protocol_version: 1,
                pubkey: "AAAA".into(),
                port: Some(18443),
                manufacturer: None,
                model: None,
            },
        )
        .unwrap();
        ws.send(Message::Text(hello.to_json())).await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        assert_eq!(registry.connected_count(), 1, "device should be registered after hello");
        assert_eq!(registry.connected_ids(), vec![device_id]);

        let _ = ws.send(Message::Close(None)).await;
        let _ = task.await;
        assert_eq!(registry.connected_count(), 0, "device should be unregistered on close");
    }
}
