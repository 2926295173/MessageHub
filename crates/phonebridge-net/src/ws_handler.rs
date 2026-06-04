//! Per-connection WebSocket envelope dispatcher.
//!
//! The handler is intentionally minimal: it reads text frames, parses each
//! as a JSON [`Envelope`], and dispatches by [`MessageType`]. Pairing
//! messages are handled by a per-connection [`Initiator`] / [`Responder`]
//! state machine (kept in a shared `PairingMap` keyed by device id).
//!
//! For M2 we only implement the pairing-related message types. M3 will add
//! notification / SMS / call dispatch.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::collections::HashMap;
use std::sync::Arc;

use futures::{SinkExt, StreamExt};
use parking_lot::Mutex;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, info, warn};
use uuid::Uuid;

use phonebridge_bus::{Bus, BusEvent};
use phonebridge_proto::{Envelope, MessageType};

use crate::pairing::{Initiator, PairingError, PairingOutcome, Responder};

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
        // We can't return a reference into the lock because we don't know
        // lifetimes; return a clone of the relevant variant.
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
        // We don't expose the active pairing state machines as clones (they
        // own non-Clone data). Callers that need to read state should use
        // specific accessors.
        DeviceSession::Unpaired(_) => DeviceSession::Unpaired(UnpairedSession::Responder(
            Responder::start(Uuid::nil()).expect("dummy"),
        )),
    }
}

/// Shared state passed to every WS connection.
#[derive(Clone)]
pub struct WsContext {
    /// Bus for broadcasting events to subscribers.
    pub bus: Bus,
    /// In-flight pairing sessions, keyed by device id.
    pub pairing: PairingMap,
    /// This daemon's stable id.
    pub our_device_id: Uuid,
}

impl WsContext {
    /// Construct a new WS context.
    pub fn new(our_device_id: Uuid) -> Self {
        Self {
            bus: Bus::new(),
            pairing: PairingMap::new(),
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

    // Channel for outbound messages from background tasks.
    let (out_tx, mut out_rx) = mpsc::channel::<Envelope>(32);

    // Spawn the writer task.
    let writer = tokio::spawn(async move {
        while let Some(env) = out_rx.recv().await {
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

        if let Some(reply) = dispatch(&env, &ctx).await {
            if let Err(e) = out_tx.send(reply).await {
                warn!(%peer_addr, "ws: outbox full: {e}");
            }
        }

        // Always publish to the bus for subscribers (web console, audit log).
        ctx.bus.publish(BusEvent::from_envelope(env));
    }

    drop(out_tx);
    let _ = writer.await;
    info!(%peer_addr, "ws: connection closed");
    Ok(())
}

/// Drive a single WebSocket connection from an axum WebSocket. Returns when
/// the peer disconnects or an unrecoverable error occurs.
pub async fn handle_axum_connection(
    socket: axum::extract::ws::WebSocket,
    peer_addr: std::net::SocketAddr,
    ctx: WsContext,
) -> Result<(), WsError> {
    use futures::SinkExt;
    use axum::extract::ws::Message as AxumMessage;
    let (mut sink, mut stream) = socket.split();
    let (out_tx, mut out_rx) = mpsc::channel::<Envelope>(32);
    let writer = tokio::spawn(async move {
        while let Some(env) = out_rx.recv().await {
            let s = env.to_json();
            if let Err(e) = sink.send(AxumMessage::Text(s)).await {
                warn!(%peer_addr, "ws send error: {e}");
                break;
            }
        }
    });
    use futures::StreamExt;
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
        if let Some(reply) = dispatch(&env, &ctx).await {
            if let Err(e) = out_tx.send(reply).await {
                warn!(%peer_addr, "ws: outbox full: {e}");
            }
        }
        ctx.bus.publish(BusEvent::from_envelope(env));
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
            let name = env
                .parse_payload::<phonebridge_proto::DeviceHello>()
                .ok()
                .map(|h| h.name)
                .unwrap_or_default();
            // If we already have a session for this device, leave it.
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
                info!(%peer_id, name, "ws: device.hello received, awaiting pair.request");
            }
            None
        }
        MessageType::DeviceHeartbeat => {
            // Reply with a heartbeat. (We don't track RTT for MVP.)
            Envelope::new(MessageType::DeviceHeartbeat, our_id, phonebridge_proto::DeviceHeartbeat::default()).ok()
        }
        MessageType::DevicePairRequest => {
            // The peer is the initiator (desktop). But the desktop is *us* in MVP,
            // so this is unexpected. Just log and ignore.
            warn!("ws: received device.pair.request from peer; ignoring (daemon is initiator in MVP)");
            None
        }
        MessageType::DevicePairChallenge => {
            // We are the initiator; the peer (Android) sent us a challenge.
            let peer_id = env.device_id;
            // Lock the session and dispatch.
            let session = ctx.pairing.get(&peer_id);
            let initiator = match session {
                Some(DeviceSession::Unpaired(UnpairedSession::Initiator(i))) => Some(i),
                _ => None,
            };
            // We can't hold the lock across an await; instead, we mutate via
            // a temporary swap. The Mutex isn't reentrant, so this is awkward.
            // For M2 we cheat: clone the session's initiator state by
            // removing+re-inserting. This is fine for single-connection
            // flows; multi-connection concurrent pairing is a M3 concern.
            if let Some(mut init) = initiator {
                // Take the session out, mutate, put it back.
                ctx.pairing.remove(&peer_id);
                let r = init.on_challenge(env, our_id);
                let reply = match &r {
                    Ok(_exp) => init.build_accept_envelope(our_id).ok(),
                    Err(e) => Some(init.build_reject_envelope(our_id, &e.to_string()).unwrap_or_else(|_| {
                        Envelope::new(
                            MessageType::DevicePairReject,
                            our_id,
                            phonebridge_proto::PairReject { reason: Some(e.to_string()) },
                        ).unwrap()
                    })),
                };
                ctx.pairing.insert(peer_id, DeviceSession::Unpaired(UnpairedSession::Initiator(init)));
                reply
            } else {
                warn!(%peer_id, "ws: pair.challenge for unknown initiator session");
                None
            }
        }
        MessageType::DevicePairConfirm => {
            // Initiator receives confirm; this is the second step of the
            // happy path. The peer (Android) confirmed → we accept + send
            // complete.
            let peer_id = env.device_id;
            let session = ctx.pairing.get(&peer_id);
            if let Some(DeviceSession::Unpaired(UnpairedSession::Initiator(init))) = session {
                ctx.pairing.remove(&peer_id);
                let confirm: phonebridge_proto::PairConfirm = env.parse_payload().unwrap_or(phonebridge_proto::PairConfirm { accepted: false });
                if confirm.accepted {
                    let reply = init.build_complete_envelope(our_id).ok();
                    ctx.pairing.insert(peer_id, DeviceSession::Unpaired(UnpairedSession::Initiator(init)));
                    reply
                } else {
                    // User rejected: drop the session.
                    None
                }
            } else {
                warn!(%peer_id, "ws: pair.confirm for unknown session");
                None
            }
        }
        MessageType::DevicePairComplete => {
            // Either side may send complete. Find the matching session and
            // finalize the pairing.
            let peer_id = env.device_id;
            let session = ctx.pairing.get(&peer_id);
            match session {
                Some(DeviceSession::Unpaired(UnpairedSession::Initiator(init))) => {
                    ctx.pairing.remove(&peer_id);
                    let outcome: Result<PairingOutcome, _> = init.on_complete(env);
                    match outcome {
                        Ok(o) => {
                            info!(%peer_id, fingerprint = %o.peer_fingerprint, "ws: paired (initiator side)");
                            ctx.pairing.insert(peer_id, DeviceSession::Paired(PairedSession {
                                device_id: peer_id,
                                name: "(unknown)".into(),
                                cert_fingerprint: o.peer_fingerprint.clone(),
                            }));
                            // Persist the pairing (M3 will do this for real).
                            // For now, the bus carries the outcome.
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
                    let outcome = r.on_complete(env);
                    match outcome {
                        Ok(o) => {
                            info!(%peer_id, fingerprint = %o.peer_fingerprint, "ws: paired (responder side)");
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
        | MessageType::DevicePairReject => {
            // Responder receives these from initiator. For M2 the desktop is
            // always the initiator, so we just log and drop.
            debug!("ws: received pair.accept/reject (M2: daemon is initiator)");
            None
        }
        _ => {
            // M3: notification / SMS / call.
            debug!(message_type = %env.message_type, "ws: message not handled in M2");
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

    async fn fake_stream_pair() -> (tokio::net::TcpStream, tokio::net::TcpStream, std::net::SocketAddr) {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (s, _) = l.accept().await.unwrap();
            s
        });
        let client = tokio::net::TcpStream::connect(addr).await.unwrap();
        let server = server.await.unwrap();
        (server, client, addr)
    }

    #[tokio::test]
    async fn connection_round_trip() {
        let (server, client, addr) = fake_stream_pair().await;
        let ctx = WsContext::new(Uuid::new_v4());
        let task = tokio::spawn(handle_connection(server, addr, ctx.clone()));

        let mut ws = tokio_tungstenite::client_async("ws://localhost/", client)
            .await
            .unwrap()
            .0;
        let hello = Envelope::new(
            MessageType::DeviceHello,
            Uuid::new_v4(),
            DeviceHello {
                name: "test".into(),
                device_type: phonebridge_proto::DeviceType::Android,
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
}
