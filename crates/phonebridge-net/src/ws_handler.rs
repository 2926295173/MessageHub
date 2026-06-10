// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 PhoneBridge Contributors
//
// This file is part of PhoneBridge. See LICENSE and the dual-licensing
// notice in README.md for details.

//! Per-connection WebSocket envelope dispatcher.
//!
//! For M3, the handler:
//! - Registers the device in the [`DeviceRegistry`] after `device.hello`.
//! - Persists incoming `notification.received` / `sms.received` / `call.*`
//!   via the [`WsSink`] callback (the message-center implements this to write to
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
    CallHistory, CallIncoming,
    CallState, DeviceHello, DeviceType, Envelope, MessageType, NotificationDismissed,
    NotificationReceived, PairConfirm, SmsListResult, SmsReceived, SmsSendResult, Unpair,
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
///
/// Variants differ in size by design: an `UnpairedSession` carries a
/// full in-flight pairing state machine (ephemeral keys, shared
/// secret, expiry, cert material) which is ~344 B; a `PairedSession`
/// is just a small id+name+fingerprint record (~64 B). Boxing the
/// large variant would push the size delta onto every access and
/// force callers to handle `Box<UnpairedSession>`; the size cost is
/// acceptable given that at most a handful of devices are ever in
/// `Unpaired` at once. Allowed locally to silence the lint.
#[allow(clippy::large_enum_variant)]
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

/// A `device.pair.request` we received from a phone (phone is the
/// initiator) but the user has not yet accepted or rejected on the
/// web console. The desktop sits on this entry until the user
/// decides; on Accept we send `device.pair.confirm(true)` directly,
/// on Reject we send `device.pair.confirm(false)` and drop.
///
/// This is the no-code phone-initiated path: the desktop never asks
/// the user to type a code, and no `device.pair.challenge` is
/// generated. Trust flows from the user visually identifying the
/// requesting device (by name) on the web console.
#[derive(Debug, Clone)]
pub struct PendingIncoming {
    /// The phone that sent pair.request.
    pub device_id: Uuid,
    /// Display name (from the phone's device.hello).
    pub name: String,
    /// Wall-clock epoch ms when the request arrived.
    pub received_at: i64,
    /// Phone's ephemeral pubkey from pair.request, base64. Captured
    /// for protocol completeness; not used in the no-code flow.
    pub peer_ephemeral_pub_b64: Option<String>,
}

/// Shared map of pending phone-initiated pairing requests awaiting
/// user approval on the web console.
#[derive(Clone, Default)]
pub struct PendingIncomingMap {
    inner: Arc<Mutex<HashMap<Uuid, PendingIncoming>>>,
}

impl PendingIncomingMap {
    /// Create a new empty map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert (or replace) a pending entry.
    pub fn insert(&self, p: PendingIncoming) {
        let mut g = self.inner.lock();
        g.insert(p.device_id, p);
    }

    /// Remove a pending entry by device id.
    pub fn remove(&self, device_id: &Uuid) -> Option<PendingIncoming> {
        self.inner.lock().remove(device_id)
    }

    /// Read a snapshot of one entry.
    pub fn get(&self, device_id: &Uuid) -> Option<PendingIncoming> {
        self.inner.lock().get(device_id).cloned()
    }

    /// Snapshot of all pending entries.
    pub fn list(&self) -> Vec<PendingIncoming> {
        self.inner.lock().values().cloned().collect()
    }
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

    /// Remove and return the `Initiator` state machine for this device,
    /// if one is in flight. Used by the WS handler to drive the
    /// initiator state transitions on `pair.challenge` / `pair.confirm` /
    /// `pair.complete`. The caller is responsible for re-inserting
    /// the (mutated) initiator back into the map after the transition.
    pub fn take_unpaired_initiator(&self, device_id: &Uuid) -> Option<Initiator> {
        let mut g = self.inner.lock();
        match g.remove(device_id) {
            Some(DeviceSession::Unpaired(UnpairedSession::Initiator(i))) => Some(i),
            other => {
                if let Some(s) = other {
                    g.insert(*device_id, s);
                }
                None
            }
        }
    }

    /// Remove and return the `Responder` state machine for this device,
    /// if one is in flight. Symmetric with `take_unpaired_initiator`.
    pub fn take_unpaired_responder(&self, device_id: &Uuid) -> Option<Responder> {
        let mut g = self.inner.lock();
        match g.remove(device_id) {
            Some(DeviceSession::Unpaired(UnpairedSession::Responder(r))) => Some(r),
            other => {
                if let Some(s) = other {
                    g.insert(*device_id, s);
                }
                None
            }
        }
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
    async fn on_notification(
        &self,
        envelope_id: Uuid,
        device_id: Uuid,
        env: &NotificationReceived,
    );
    /// Persist a `notification.dismissed` envelope.
    async fn on_notification_dismissed(
        &self,
        envelope_id: Uuid,
        device_id: Uuid,
        env: &NotificationDismissed,
    );
    /// Persist a `sms.received` envelope.
    async fn on_sms_received(
        &self,
        envelope_id: Uuid,
        device_id: Uuid,
        env: &SmsReceived,
    );
    /// Persist a `sms.send.result` envelope and resolve any pending send.
    async fn on_sms_send_result(
        &self,
        envelope_id: Uuid,
        device_id: Uuid,
        env: &SmsSendResult,
    );
    /// Persist a `call.state` envelope (state transition).
    async fn on_call_state(
        &self,
        envelope_id: Uuid,
        device_id: Uuid,
        env: &CallState,
    );
    /// Persist a `call.incoming` envelope.
    async fn on_call_incoming(
        &self,
        envelope_id: Uuid,
        device_id: Uuid,
        env: &CallIncoming,
    );
    /// Persist a `call.history` envelope.
    async fn on_call_history(
        &self,
        envelope_id: Uuid,
        device_id: Uuid,
        env: &CallHistory,
    );
    /// Persist an `sms.list.result` envelope.
    async fn on_sms_list_result(
        &self,
        envelope_id: Uuid,
        device_id: Uuid,
        env: &SmsListResult,
    );
    /// Persist a `device.hello` envelope (update device row + last_seen).
    async fn on_hello(
        &self,
        envelope_id: Uuid,
        device_id: Uuid,
        env: &DeviceHello,
    );
    /// Persist a `device.unpair` envelope.
    async fn on_unpair(
        &self,
        envelope_id: Uuid,
        device_id: Uuid,
        env: &Unpair,
    );
    /// Called on connection close. Audit log + cleanup.
    async fn on_disconnect(&self, device_id: Uuid);
}

/// No-op sink used in tests.
pub struct NoopSink;

#[async_trait]
impl WsSink for NoopSink {
    async fn on_notification(&self, _: Uuid, _: Uuid, _: &NotificationReceived) {}
    async fn on_notification_dismissed(&self, _: Uuid, _: Uuid, _: &NotificationDismissed) {}
    async fn on_sms_received(&self, _: Uuid, _: Uuid, _: &SmsReceived) {}
    async fn on_sms_send_result(&self, _: Uuid, _: Uuid, _: &SmsSendResult) {}
    async fn on_call_state(&self, _: Uuid, _: Uuid, _: &CallState) {}
    async fn on_call_incoming(&self, _: Uuid, _: Uuid, _: &CallIncoming) {}
    async fn on_call_history(&self, _: Uuid, _: Uuid, _: &CallHistory) {}
    async fn on_sms_list_result(&self, _: Uuid, _: Uuid, _: &SmsListResult) {}
    async fn on_hello(&self, _: Uuid, _: Uuid, _: &DeviceHello) {}
    async fn on_unpair(&self, _: Uuid, _: Uuid, _: &Unpair) {}
    async fn on_disconnect(&self, _: Uuid) {}
}

/// Shared state passed to every WS connection.
#[derive(Clone)]
pub struct WsContext {
    /// Bus for broadcasting events to subscribers.
    pub bus: Bus,
    /// In-flight pairing sessions, keyed by device id.
    pub pairing: PairingMap,
    /// Pending phone-initiated pairing requests awaiting user approval.
    pub pending_incoming: PendingIncomingMap,
    /// Downstream send registry (shared across the message-center process).
    pub registry: DeviceRegistry,
    /// Sink for non-pairing persistence.
    pub sink: Arc<dyn WsSink + Send + Sync>,
    /// This message-center’s stable id.
    pub our_device_id: Uuid,
    /// This message-center's display name. Sent in `device.hello` on
    /// every accepted WebSocket so the phone can show the
    /// human-readable name ("rk3588", "office-ubuntu") in its UI
    /// without re-deriving it from the host:port it dialed.
    pub our_name: String,
    /// Base64 of the message-center's long-term ECDH P-256 public key
    /// (SubjectPublicKeyInfo, 65 bytes uncompressed). Sent in
    /// `device.hello` as the `pubkey` field so the phone can verify
    /// the identity cert the WebSocket is pinned to. Mirrors the
    /// `DeviceHello.pubkey` field on the phone side.
    pub our_public_key_b64: String,
    /// Port the message-center is listening on for new WebSocket
    /// connections. Sent in `device.hello.port` so the phone (and
    /// any debug tooling) can show where to dial the daemon.
    pub our_listen_port: Option<u16>,
}

impl WsContext {
    /// Construct a new WS context with a fresh per-connection bus/pairing
    /// map, but a shared registry. The identity fields default to
    /// empty/None — production callers should use
    /// [`WsContext::with_identity`] so the resulting `device.hello`
    /// carries the right name and pubkey.
    pub fn new(our_device_id: Uuid, sink: Arc<dyn WsSink + Send + Sync>, registry: DeviceRegistry) -> Self {
        Self {
            bus: Bus::new(),
            pairing: PairingMap::new(),
            pending_incoming: PendingIncomingMap::new(),
            registry,
            sink,
            our_device_id,
            our_name: String::new(),
            our_public_key_b64: String::new(),
            our_listen_port: None,
        }
    }

    /// Build a `WsContext` carrying the message-center's identity so
    /// `handle_connection` can construct an authentic `device.hello`
    /// on every accepted WebSocket.
    pub fn with_identity(
        our_device_id: Uuid,
        our_name: String,
        our_public_key_b64: String,
        our_listen_port: Option<u16>,
        sink: Arc<dyn WsSink + Send + Sync>,
        registry: DeviceRegistry,
    ) -> Self {
        Self {
            bus: Bus::new(),
            pairing: PairingMap::new(),
            pending_incoming: PendingIncomingMap::new(),
            registry,
            sink,
            our_device_id,
            our_name,
            our_public_key_b64,
            our_listen_port,
        }
    }
}

/// Build the `device.hello` envelope the message-center sends on
/// every new WebSocket. Returns Err if the [DeviceHello] payload
/// fails to serialize (it never should — all fields are valid).
///
/// We send this so the phone can render the daemon's human-readable
/// name ("rk3588", "office-ubuntu") in its UI without re-deriving
/// it from the host:port it dialed. The `pubkey` mirrors the
/// `DeviceHello.pubkey` field on the phone side and is what the
/// phone uses to verify the cert the WebSocket is pinned to
/// (`handshake-time` TLS pinning, see docs/threat-model.md).
fn build_desktop_hello(ctx: &WsContext) -> serde_json::Result<Envelope> {
    let payload = DeviceHello {
        name: ctx.our_name.clone(),
        device_type: DeviceType::Desktop,
        protocol_version: 1,
        pubkey: ctx.our_public_key_b64.clone(),
        port: ctx.our_listen_port,
        manufacturer: None,
        model: None,
        hardware_id: None,
    };
    Envelope::new(MessageType::DeviceHello, ctx.our_device_id, payload)
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

    // The phone expects a `device.hello` from us on every new
    // WebSocket so it can render the daemon's display name in its
    // drawer (otherwise it would have to fall back to the host:port
    // it dialed, which is opaque to the user). The hello is
    // independent of the phone's own hello; the two are
    // interchangeable directionally.
    if !ctx.our_name.is_empty() {
        let hello = build_desktop_hello(&ctx);
        if let Ok(env) = hello {
            if let Err(e) = out_tx.send(env).await {
                warn!(%peer_addr, "ws: failed to enqueue desktop hello: {e}");
            } else {
                info!(name=%ctx.our_name, "ws: sent device.hello (desktop)");
            }
        }
    }

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
                    ctx.sink.on_hello(env.id, env.device_id, h).await;
                }
                // Register in the registry so other code can send.
                *device_id_holder.lock() = Some(env.device_id);
                ctx.registry.register(env.device_id, out_tx.clone());
            }
            MessageType::NotificationReceived => {
                if let Ok(n) = env.parse_payload::<NotificationReceived>() {
                    ctx.sink.on_notification(env.id, env.device_id, &n).await;
                }
            }
            MessageType::NotificationDismissed => {
                if let Ok(n) = env.parse_payload::<NotificationDismissed>() {
                    ctx.sink.on_notification_dismissed(env.id, env.device_id, &n).await;
                }
            }
            MessageType::SmsReceived => {
                if let Ok(s) = env.parse_payload::<SmsReceived>() {
                    ctx.sink.on_sms_received(env.id, env.device_id, &s).await;
                }
            }
            MessageType::SmsSendResult => {
                if let Ok(s) = env.parse_payload::<SmsSendResult>() {
                    ctx.sink.on_sms_send_result(env.id, env.device_id, &s).await;
                }
            }
            MessageType::CallState => {
                if let Ok(c) = env.parse_payload::<CallState>() {
                    ctx.sink.on_call_state(env.id, env.device_id, &c).await;
                }
            }
            MessageType::CallIncoming => {
                if let Ok(c) = env.parse_payload::<CallIncoming>() {
                    ctx.sink.on_call_incoming(env.id, env.device_id, &c).await;
                }
            }
            MessageType::CallHistory => {
                if let Ok(c) = env.parse_payload::<CallHistory>() {
                    ctx.sink.on_call_history(env.id, env.device_id, &c).await;
                }
            }
            MessageType::SmsListResult => {
                if let Ok(r) = env.parse_payload::<SmsListResult>() {
                    ctx.sink.on_sms_list_result(env.id, env.device_id, &r).await;
                }
            }
            MessageType::DeviceUnpair => {
                if let Ok(u) = env.parse_payload::<Unpair>() {
                    ctx.sink.on_unpair(env.id, env.device_id, &u).await;
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

    // Send the desktop's `device.hello` first so the phone has our
    // name/pubkey as soon as the handshake completes. See
    // `build_desktop_hello` for the rationale. The axum path
    // reuses the same builder so the two handlers stay in sync.
    if !ctx.our_name.is_empty() {
        let hello = build_desktop_hello(&ctx);
        if let Ok(env) = hello {
            if let Err(e) = out_tx.send(env).await {
                warn!(%peer_addr, "ws: failed to enqueue desktop hello (axum): {e}");
            } else {
                info!(name=%ctx.our_name, "ws(axum): sent device.hello (desktop)");
            }
        }
    }

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
                    ctx.sink.on_hello(env.id, env.device_id, &h).await;
                }
                *device_id_holder.lock() = Some(env.device_id);
                ctx.registry.register(env.device_id, out_tx.clone());
            }
            MessageType::NotificationReceived => {
                if let Ok(n) = env.parse_payload::<NotificationReceived>() {
                    ctx.sink.on_notification(env.id, env.device_id, &n).await;
                }
            }
            MessageType::NotificationDismissed => {
                if let Ok(n) = env.parse_payload::<NotificationDismissed>() {
                    ctx.sink.on_notification_dismissed(env.id, env.device_id, &n).await;
                }
            }
            MessageType::SmsReceived => {
                if let Ok(s) = env.parse_payload::<SmsReceived>() {
                    ctx.sink.on_sms_received(env.id, env.device_id, &s).await;
                }
            }
            MessageType::SmsSendResult => {
                if let Ok(s) = env.parse_payload::<SmsSendResult>() {
                    ctx.sink.on_sms_send_result(env.id, env.device_id, &s).await;
                }
            }
            MessageType::CallState => {
                if let Ok(c) = env.parse_payload::<CallState>() {
                    ctx.sink.on_call_state(env.id, env.device_id, &c).await;
                }
            }
            MessageType::CallIncoming => {
                if let Ok(c) = env.parse_payload::<CallIncoming>() {
                    ctx.sink.on_call_incoming(env.id, env.device_id, &c).await;
                }
            }
            MessageType::CallHistory => {
                if let Ok(c) = env.parse_payload::<CallHistory>() {
                    ctx.sink.on_call_history(env.id, env.device_id, &c).await;
                }
            }
            MessageType::SmsListResult => {
                if let Ok(r) = env.parse_payload::<SmsListResult>() {
                    ctx.sink.on_sms_list_result(env.id, env.device_id, &r).await;
                }
            }
            MessageType::DeviceUnpair => {
                if let Ok(u) = env.parse_payload::<Unpair>() {
                    ctx.sink.on_unpair(env.id, env.device_id, &u).await;
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
            // The phone is the initiator here. Mark the request as
            // "pending user approval" on the web console (no code is
            // generated — the user identifies the phone by name and
            // clicks Accept/Reject). On Accept, the REST handler
            // builds and sends a `device.pair.confirm(true)` envelope
            // directly. On Reject, it sends `device.pair.confirm(false)`
            // and drops the entry. After Accept, when the phone replies
            // with `device.pair.complete`, the existing dispatcher
            // handles the cert exchange.
            let peer_id = env.device_id;
            let req: phonebridge_proto::PairRequest = env
                .parse_payload()
                .unwrap_or(phonebridge_proto::PairRequest { ephemeral_pubkey: String::new() });
            ctx.pending_incoming.insert(PendingIncoming {
                device_id: peer_id,
                name: format!("device-{}", &peer_id.to_string()[..8]),
                received_at: chrono::Utc::now().timestamp_millis(),
                peer_ephemeral_pub_b64: Some(req.ephemeral_pubkey).filter(|s| !s.is_empty()),
            });
            info!(%peer_id, "ws: phone-initiated pair.request parked for user approval");
            None
        }
        MessageType::DevicePairChallenge => {
            // The phone (responder) derived the code from our ephemeral
            // pub + theirs, and is showing it to the user. We just need
            // to record the peer's ephemeral pub + code in the Initiator
            // state machine so we can later build pair.complete once the
            // user clicks Accept on the phone.
            //
            // We intentionally do NOT send pair.accept here. The phone
            // is the trusted UI surface (per the project threat model:
            // the desktop may be compromised, the phone is always safe),
            // so the user's click on the phone — which sends
            // pair.confirm(true) directly — is the canonical
            // confirmation. There is no code-typed-by-the-desktop step
            // any more.
            let peer_id = env.device_id;
            let mut init = match ctx.pairing.take_unpaired_initiator(&peer_id) {
                Some(i) => i,
                None => {
                    warn!(%peer_id, "ws: pair.challenge for unknown initiator session");
                    return None;
                }
            };
            if let Err(e) = init.on_challenge(env, our_id) {
                let reply = init.build_reject_envelope(our_id, &e.to_string()).ok();
                ctx.pairing.insert(
                    peer_id,
                    DeviceSession::Unpaired(UnpairedSession::Initiator(init)),
                );
                return reply;
            }
            // Stash the (now post-challenge) Initiator back. The next
            // event we expect is either pair.confirm(true) from the
            // user accepting on the phone, or pair.confirm(false) if
            // they reject.
            ctx.pairing.insert(
                peer_id,
                DeviceSession::Unpaired(UnpairedSession::Initiator(init)),
            );
            None
        }
        MessageType::DevicePairConfirm => {
            let peer_id = env.device_id;
            let init = match ctx.pairing.take_unpaired_initiator(&peer_id) {
                Some(i) => i,
                None => {
                    warn!(%peer_id, "ws: pair.confirm for unknown initiator session");
                    return None;
                }
            };
            let confirm: PairConfirm = env
                .parse_payload()
                .unwrap_or(PairConfirm { accepted: false });
            if confirm.accepted {
                let reply = init.build_complete_envelope(our_id).ok();
                ctx.pairing.insert(
                    peer_id,
                    DeviceSession::Unpaired(UnpairedSession::Initiator(init)),
                );
                reply
            } else {
                // User rejected on Android. Drop the initiator; the
                // device stays unpaired.
                None
            }
        }
        MessageType::DevicePairComplete => {
            let peer_id = env.device_id;
            // First try as Initiator (desktop-driven flow).
            if let Some(init) = ctx.pairing.take_unpaired_initiator(&peer_id) {
                let outcome: Result<PairingOutcome, _> = init.on_complete(env);
                match outcome {
                    Ok(o) => {
                        info!(%peer_id, fingerprint = %o.peer_fingerprint, "ws: paired (initiator)");
                        ctx.pairing.insert(
                            peer_id,
                            DeviceSession::Paired(PairedSession {
                                device_id: peer_id,
                                name: "(unknown)".into(),
                                cert_fingerprint: o.peer_fingerprint.clone(),
                            }),
                        );
                    }
                    Err(e) => {
                        warn!(%peer_id, "ws: pair.complete rejected (initiator): {e}");
                    }
                }
                return None;
            }
            // Else, the device.hello inserted a Responder for us, and
            // the Android is now sending us its cert after the user
            // accepted. Pull it out.
            let responder = ctx.pairing.take_unpaired_responder(&peer_id);
            match responder {
                Some(r) => match r.on_complete(env) {
                    Ok(o) => {
                        info!(%peer_id, fingerprint = %o.peer_fingerprint, "ws: paired (responder)");
                        ctx.pairing.insert(
                            peer_id,
                            DeviceSession::Paired(PairedSession {
                                device_id: peer_id,
                                name: "(unknown)".into(),
                                cert_fingerprint: o.peer_fingerprint.clone(),
                            }),
                        );
                    }
                    Err(e) => {
                        warn!(%peer_id, "ws: pair.complete rejected (responder): {e}");
                    }
                },
                None => {
                    warn!(%peer_id, "ws: pair.complete for unknown session");
                }
            }
            None
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
    use phonebridge_proto::{DeviceHello, DeviceType};
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
                hardware_id: None,
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
                hardware_id: None,
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
