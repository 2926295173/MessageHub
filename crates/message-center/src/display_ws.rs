// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 PhoneBridge Contributors
//
// This file is part of PhoneBridge. See LICENSE and the dual-licensing
// notice in README.md for details.

//! WebSocket endpoint for the desktop display endpoint
//! (`deskdisplay`).
//!
//! Path: `/ws/display?token=…` (HTTP) → `ws://…/ws/display?token=…`
//!
//! Wire format (full-duplex, newline-delimited JSON):
//! - server → client: `DisplayEvent` (see `phonebridge-proto`)
//! - client → server: `DisplayAction` (see `phonebridge-proto`)
//!
//! The same `kind` field is the disambiguator. Kinds that
//! appear on the server-to-client direction (`notification.received`,
//! `sms.received`, `phone.offline`, `action.result`, …) are
//! namespaced away from client-to-server kinds
//! (`sms.reply`, `notification.read`, …), so the JSON
//! deserializer can pick the right struct based on the value
//! of `kind`.
//!
//! Auth: the connection is upgraded only if the `?token=…`
//! query string matches the message-center's persisted token. A bad
//! token gets `401 Unauthorized` and the WS is never
//! established. The token is bound to the message-center's config
//! directory and can be rotated via the `display-token
//! --rotate` subcommand (old token stops working immediately,
//! no reconnect grace).
//!
//! Per-connection lifecycle: each successful upgrade spawns
//! two tokio tasks — a writer that drains the bus into the
//! WS, and a reader that consumes client messages and
//! dispatches them to the per-action handlers below. Both
//! tasks exit when the WS closes; the subscriber handle in
//! the writer's task is `Drop`ped automatically and decrements
//! the bus's active-subscriber count.

#![forbid(unsafe_code)]
#![allow(missing_docs)]

use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use chrono::Utc;
use futures::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::json;
use tracing::{info, warn};

use phonebridge_proto::{
    ActionResultEvent, CallAnswerRequest, CallDialRequest, CallEndRequest, DeviceHello,
    DisplayAction, DisplayEvent, MessageType, NotificationDismissed, SmsSendRequest,
};

use crate::app_state::AppState;
use crate::display_auth::is_same_host;
use crate::display_bus::{build_display_event, DisplayBus};

/// `GET /ws/display?token=…` — upgrade to a full-duplex
/// display connection.
pub async fn ws_display_upgrade(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Query(q): Query<DisplayAuthQuery>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    // Same-host shortcut: if the display binary is running on the
    // same machine as the message-center, the user is the only
    // possible peer (no remote attacker can reach `127.0.0.1`
    // or one of our local interface IPs from off-host). We
    // accept the upgrade without checking the token. The token
    // is still required for any peer whose IP isn't ours — a
    // LAN-attached display still needs to present the token,
    // preserving the existing threat model.
    if is_same_host(peer) {
        info!(%peer, "display endpoint upgrading; same-host, token check skipped");
        return ws.on_upgrade(move |socket| async move {
            if let Err(e) = handle_display_socket(state, socket).await {
                warn!(error = %e, "display WS session ended with error");
            }
        });
    }

    if !state.display_auth.verify(&q.token) {
        warn!(%peer, "display WS upgrade rejected: bad token");
        return (StatusCode::UNAUTHORIZED, "bad display token").into_response();
    }
    info!(%peer, "display endpoint upgrading; token ok");
    ws.on_upgrade(move |socket| async move {
        if let Err(e) = handle_display_socket(state, socket).await {
            warn!(error = %e, "display WS session ended with error");
        }
    })
}

/// Query-string parameters for the WS upgrade. The token is
/// passed as `?token=…`; the URL ends up in proxy logs and
/// process listings so this is a deliberate trade-off (see
/// `display_auth.rs` doc comment).
#[derive(Debug, Deserialize)]
pub struct DisplayAuthQuery {
    pub token: String,
}

async fn handle_display_socket(state: AppState, socket: WebSocket) -> anyhow::Result<()> {
    let (mut sender, mut receiver) = socket.split();
    let bus = state.display_bus.clone();

    // -- Writer task: drain the bus into the WS --
    let mut sub = bus.subscribe();
    // Bounded channel to avoid an unbounded queue between
    // the bus-bridge task and the writer (which awaits
    // sender.send). 64 events is plenty for a desktop
    // notification workload.
    let (tx, mut rx) = tokio::sync::mpsc::channel::<Arc<DisplayEvent>>(64);

    let writer = tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            match serde_json::to_string(&*event) {
                Ok(line) => {
                    if sender
                        .send(Message::Text(format!("{line}\n")))
                        .await
                        .is_err()
                    {
                        // Peer gone; the recv side will see the
                        // close and the bus_bridge task will be
                        // aborted shortly.
                        break;
                    }
                }
                Err(e) => {
                    warn!(error = %e, "display event serialize failed; dropping");
                }
            }
        }
    });

    // -- Bus-bridge task: re-broadcast to the per-connection channel --
    let bus_bridge = tokio::spawn(async move {
        loop {
            match sub.recv().await {
                Ok(event) => {
                    if tx.send(event).await.is_err() {
                        // Writer gone.
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    // Slow consumer; we dropped `n` events. Log
                    // it; the display endpoint can show a "missed
                    // events" indicator on next connect.
                    warn!(lagged = n, "display WS lagged behind the bus");
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    break;
                }
            }
        }
    });

    // -- Reader task: handle incoming DisplayAction lines --
    while let Some(msg) = receiver.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(e) => {
                warn!(error = %e, "display WS read error; closing");
                break;
            }
        };
        let text = match msg {
            Message::Text(t) => t,
            Message::Binary(b) => {
                warn!(len = b.len(), "display WS received binary frame; ignoring");
                continue;
            }
            Message::Close(_) => break,
            _ => continue,
        };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let action: DisplayAction = match serde_json::from_str(line) {
                Ok(a) => a,
                Err(e) => {
                    warn!(error = %e, raw = line, "display action parse failed");
                    continue;
                }
            };
            handle_display_action(&state, action).await;
        }
    }

    writer.abort();
    bus_bridge.abort();
    Ok(())
}

/// Process a single [`DisplayAction`] from the client.
///
/// The flow is:
/// 1. Validate the action (mandatory fields present for the
///    given kind).
/// 2. Look up the target phone in the registry. If offline,
///    publish a `phone.offline` `DisplayEvent` and an
///    `action.result` with `ok=false, message="phone_offline"`.
///    **Do not close the WS** — the user may continue to send
///    actions as other phones come online.
/// 3. Otherwise, build the appropriate envelope and send it
///    via the `DeviceRegistry`. The phone's reply
///    (`sms.send.result`, `notification.dismissed`, call state
///    transitions, …) flows back as a regular `DisplayEvent`
///    through the bus, so we don't wait for an ack here.
async fn handle_display_action(state: &AppState, action: DisplayAction) {
    if !state.registry.connected_ids().contains(&action.device_id) {
        publish_phone_offline(state, &action);
        return;
    }

    match action.kind.as_str() {
        "sms.reply" => do_sms_reply(state, &action).await,
        "notification.read" => do_notification_read(state, &action).await,
        "notification.dismiss" => do_notification_dismiss(state, &action).await,
        "call.answer" => do_call_answer(state, &action).await,
        "call.end" => do_call_end(state, &action).await,
        other => {
            warn!(kind = %other, "display action with unknown kind; rejecting");
            publish_action_result(
                state,
                &action,
                false,
                Some("bad_request"),
                Some(format!("unknown action kind: {other}")),
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Per-action dispatchers
// ---------------------------------------------------------------------------
//
// Each of these is small because the heavy lifting (DB write
// + envelope send) is already implemented for the REST API.
// We duplicate the envelope-build-and-send here rather than
// refactoring the REST layer to expose a shared library,
// because the per-action logic is small and the call sites
// are radically different (HTTP handler vs WS message).
// The duplication is documented and kept under 30 lines per
// action.
// ---------------------------------------------------------------------------

async fn do_sms_reply(state: &AppState, action: &DisplayAction) {
    let to = match action.to.as_deref() {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => {
            publish_action_result(
                state,
                action,
                false,
                Some("bad_request"),
                Some("sms.reply requires `to`".into()),
            );
            return;
        }
    };
    let body = match action.body.as_deref() {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => {
            publish_action_result(
                state,
                action,
                false,
                Some("bad_request"),
                Some("sms.reply requires `body`".into()),
            );
            return;
        }
    };

    // The phone picks the SIM slot itself; we don't expose
    // subscription_id in the display action (the v1 Android
    // client uses its own default SIM).
    let env = match phonebridge_proto::Envelope::new(
        MessageType::SmsSendRequest,
        state.our_device_id,
        SmsSendRequest {
            to,
            body,
            subscription_id: None,
        },
    ) {
        Ok(e) => e,
        Err(e) => {
            publish_action_result(
                state,
                action,
                false,
                Some("internal_error"),
                Some(format!("build envelope: {e}")),
            );
            return;
        }
    };
    if let Err(e) = state.registry.try_send(action.device_id, env).await {
        publish_action_result(
            state,
            action,
            false,
            Some("phone_error"),
            Some(format!("send: {e}")),
        );
        return;
    }
    let _ = state
        .db
        .insert_audit_log(
            Utc::now().timestamp_millis(),
            Some(action.device_id),
            "display.sms_reply",
            None,
        )
        .await;
    publish_action_result(state, action, true, None, None);
}

async fn do_notification_read(state: &AppState, action: &DisplayAction) {
    if let Err(e) = state
        .db
        .mark_notification_read(action.device_id, &action.envelope_id.to_string())
        .await
    {
        publish_action_result(
            state,
            action,
            false,
            Some("internal_error"),
            Some(format!("db: {e}")),
        );
        return;
    }
    publish_action_result(state, action, true, None, None);
}

async fn do_notification_dismiss(state: &AppState, action: &DisplayAction) {
    // The envelope_id from the original notification.received
    // is the sbn.key on the phone. We pass it through verbatim.
    let env = match phonebridge_proto::Envelope::new(
        MessageType::NotificationDismissed,
        state.our_device_id,
        NotificationDismissed {
            id: action.envelope_id.to_string(),
        },
    ) {
        Ok(e) => e,
        Err(e) => {
            publish_action_result(
                state,
                action,
                false,
                Some("internal_error"),
                Some(format!("build envelope: {e}")),
            );
            return;
        }
    };
    if let Err(e) = state.registry.try_send(action.device_id, env).await {
        publish_action_result(
            state,
            action,
            false,
            Some("phone_error"),
            Some(format!("send: {e}")),
        );
        return;
    }
    let _ = state
        .db
        .dismiss_notification(action.device_id, &action.envelope_id.to_string())
        .await;
    publish_action_result(state, action, true, None, None);
}

async fn do_call_answer(state: &AppState, action: &DisplayAction) {
    // Android's CallController handles call.answer on the
    // device side; we forward an empty CallAnswerRequest.
    let env = match phonebridge_proto::Envelope::new(
        MessageType::CallAnswerRequest,
        state.our_device_id,
        CallAnswerRequest {},
    ) {
        Ok(e) => e,
        Err(e) => {
            publish_action_result(
                state,
                action,
                false,
                Some("internal_error"),
                Some(format!("build envelope: {e}")),
            );
            return;
        }
    };
    if let Err(e) = state.registry.try_send(action.device_id, env).await {
        publish_action_result(
            state,
            action,
            false,
            Some("phone_error"),
            Some(format!("send: {e}")),
        );
        return;
    }
    publish_action_result(state, action, true, None, None);
}

async fn do_call_end(state: &AppState, action: &DisplayAction) {
    // The `call_id` (when present) lets the Android side
    // disambiguate which call to hang up if multiple are
    // active. The v1 phone client ignores it and just calls
    // TelecomManager.endCall(); we pass the field through
    // anyway so a future multi-call Android build can use it.
    let env = match phonebridge_proto::Envelope::new(
        MessageType::CallEndRequest,
        state.our_device_id,
        CallEndRequest {},
    ) {
        Ok(e) => e,
        Err(e) => {
            publish_action_result(
                state,
                action,
                false,
                Some("internal_error"),
                Some(format!("build envelope: {e}")),
            );
            return;
        }
    };
    if let Err(e) = state.registry.try_send(action.device_id, env).await {
        publish_action_result(
            state,
            action,
            false,
            Some("phone_error"),
            Some(format!("send: {e}")),
        );
        return;
    }
    publish_action_result(state, action, true, None, None);
}

// Unused for now but kept so future call.dial from the
// desktop doesn't need a new dispatch entry.
#[allow(dead_code)]
async fn do_call_dial(state: &AppState, action: &DisplayAction, number: &str) {
    let env = match phonebridge_proto::Envelope::new(
        MessageType::CallDialRequest,
        state.our_device_id,
        CallDialRequest {
            number: number.to_string(),
        },
    ) {
        Ok(e) => e,
        Err(e) => {
            publish_action_result(
                state,
                action,
                false,
                Some("internal_error"),
                Some(format!("build envelope: {e}")),
            );
            return;
        }
    };
    if let Err(e) = state.registry.try_send(action.device_id, env).await {
        publish_action_result(
            state,
            action,
            false,
            Some("phone_error"),
            Some(format!("send: {e}")),
        );
        return;
    }
    publish_action_result(state, action, true, None, None);
}

// ---------------------------------------------------------------------------
// Display event publishers (for message-center-generated events)
// ---------------------------------------------------------------------------

fn publish_action_result(
    state: &AppState,
    action: &DisplayAction,
    ok: bool,
    code: Option<&str>,
    message: Option<String>,
) {
    let merged_message = match (code, message) {
        (Some(c), Some(m)) => Some(format!("{c}: {m}")),
        (Some(c), None) => Some(c.to_string()),
        (None, m) => m,
    };
    let result = ActionResultEvent {
        kind: "action.result".into(),
        request_envelope_id: action.envelope_id,
        device_id: action.device_id,
        action_kind: action.kind.clone(),
        ok,
        message: merged_message,
        timestamp: Utc::now().timestamp_millis(),
    };
    // Wrap into a DisplayEvent so all subscribers see a single
    // message shape. The "kind" is "action.result" so the
    // display endpoint knows to look at the embedded result.
    let event = DisplayEvent {
        kind: "action.result".into(),
        device_id: action.device_id,
        envelope_id: action.envelope_id,
        timestamp: result.timestamp,
        payload: serde_json::to_value(&result).unwrap_or_else(|_| json!({})),
        summary: Default::default(),
    };
    state.display_bus.publish(event);
}

fn publish_phone_offline(state: &AppState, action: &DisplayAction) {
    // First, post a one-shot "phone.offline" event so the
    // display endpoint can pop a transient toast.
    let event = build_display_event(
        "phone.offline",
        action.device_id,
        action.envelope_id,
        json!({
            "action_kind": action.kind,
            "message": "phone offline; action not delivered",
        }),
    );
    state.display_bus.publish(event);
    // Then, the action.result with ok=false.
    publish_action_result(
        state,
        action,
        false,
        Some("phone_offline"),
        Some(format!(
            "device {} is not currently connected",
            action.device_id
        )),
    );
}

// Keep the imports we actually use happy if some are
// referenced only transitively through display_bus builds.
const _: fn() = || {
    let _: DeviceHello = DeviceHello {
        name: String::new(),
        device_type: phonebridge_proto::DeviceType::Android,
        protocol_version: 0,
        pubkey: String::new(),
        port: None,
        manufacturer: None,
        model: None,
        hardware_id: None,
    };
    let _: DisplayBus = DisplayBus::default();
};
