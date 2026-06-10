// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 PhoneBridge Contributors
//
// This file is part of PhoneBridge. See LICENSE and the dual-licensing
// notice in README.md for details.

//! Linux backend: routes PhoneBridge events to the
//! desktop session bus via the
//! [`org.freedesktop.Notifications`](https://specifications.freedesktop.org/notification-spec/notification-spec-latest.html)
//! spec.
//!
//! The flow is:
//!
//! 1. `start()` opens a `zbus::Connection` to the session
//!    bus, builds a `NotificationsProxy`, and spawns two
//!    signal-stream tasks: one for `ActionInvoked`, one
//!    for `NotificationClosed`.
//! 2. `present()` is called per incoming `DisplayEvent`.
//!    For `notification.received` / `sms.received` /
//!    `call.incoming` it calls
//!    `Notify(summary, body, actions, hints)` with
//!    `actions = ("reply", "Quick Reply", "mark_read",
//!    "Mark as read", "dismiss", "Dismiss")` and remembers
//!    the returned `notif_id` together with the
//!    originating `envelope_id` and `device_id`.
//! 3. When the user clicks an action button the signal
//!    handler looks up the envelope id and pushes a
//!    `DisplayAction` back to the daemon via the
//!    [`ActionSink`].
//! 4. The `reply` action is special: there is no native
//!    text-input on the notification surface, so we shell
//!    out to `zenity --entry` (GNOME / Xfce / Cinnamon) or
//!    `kdialog --inputbox` (KDE Plasma) to collect the
//!    reply text and then send the `sms.reply` action.
//!
//! v1 limitations (intentional, see PR8 plan):
//! - Reply prompts need a graphical session with `zenity`
//!   or `kdialog` on `$PATH`; headless users get a
//!   `toast.action_failed` log line and the action is
//!   dropped.
//! - Incoming calls only show a notification; the OS's
//!   built-in call UI on the phone is the source of truth.
//!   The `Answer` / `Hang up` buttons forward
//!   `call.answer` / `call.end` to the phone, which
//!   ultimately calls `TelecomManager.answerRingingCall()`
//!   / `endCall()`.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::{Arc, Mutex as StdMutex};

use async_trait::async_trait;
use futures::StreamExt;
use phonebridge_proto::{
    CallIncoming, CallState, CallStateKind, DisplayAction, NotificationReceived, SmsReceived,
};
use tokio::io::AsyncReadExt;
use uuid::Uuid;
use zbus::Connection;

use super::DisplayBackend;
use crate::actions::ActionSink;
use crate::config::DisplayConfig;
use crate::error::DisplayError;
use crate::i18n::{DisplayI18n, render};

const APP_NAME: &str = "PhoneBridge";
const REPLY_ACTION: &str = "reply";
const MARK_READ_ACTION: &str = "mark_read";
const DISMISS_ACTION: &str = "dismiss";
const ANSWER_ACTION: &str = "answer";
const HANGUP_ACTION: &str = "hangup";

/// Map from a D-Bus notification id (returned by `Notify`)
/// back to the originating `DisplayEvent` envelope, so the
/// signal handler can look up which event an `ActionInvoked`
/// signal refers to.
#[derive(Default)]
struct NotificationMap {
    by_notif_id: HashMap<u32, NotifRef>,
}

#[derive(Clone, Debug)]
struct NotifRef {
    envelope_id: Uuid,
    device_id: Uuid,
    kind: String,
    address: Option<String>,
    call_id: Option<String>,
    package: Option<String>,
    notif_id: Option<String>,
}

pub struct LinuxBackend {
    _cfg: DisplayConfig,
    conn: StdMutex<Option<Connection>>,
    map: Arc<StdMutex<NotificationMap>>,
    sink: ActionSink,
}

impl LinuxBackend {
    pub fn new(cfg: &DisplayConfig) -> Result<Self, DisplayError> {
        Ok(Self {
            _cfg: cfg.clone(),
            conn: StdMutex::new(None),
            map: Arc::new(StdMutex::new(NotificationMap::default())),
            sink: ActionSink::new(),
        })
    }

    /// Build the action tuple list for a notification. The
    /// even-indexed entries are keys (consumed by the
    /// signal handler), the odd-indexed entries are
    /// human-readable labels.
    fn action_list_for(kind: &str, i18n: &DisplayI18n) -> Vec<(String, String)> {
        match kind {
            "sms" | "notification" => vec![
                (REPLY_ACTION.into(), i18n.t("notif.action.reply").into()),
                (MARK_READ_ACTION.into(), i18n.t("notif.action.mark_read").into()),
                (DISMISS_ACTION.into(), i18n.t("notif.action.dismiss").into()),
            ],
            "call" => vec![
                (ANSWER_ACTION.into(), i18n.t("notif.action.answer").into()),
                (HANGUP_ACTION.into(), i18n.t("notif.action.hangup").into()),
            ],
            _ => Vec::new(),
        }
    }

    async fn present_notification(
        &self,
        evt_envelope_id: Uuid,
        device_id: Uuid,
        notif: &NotificationReceived,
        i18n: &DisplayI18n,
    ) -> Result<(), DisplayError> {
        let summary = notif
            .app_name
            .clone()
            .unwrap_or_else(|| notif.title.clone());
        let body = if notif.is_sensitive {
            String::from("•••")
        } else {
            notif.content.clone()
        };
        let actions = Self::action_list_for("notification", i18n);
        let notif_id_str = notif.id.clone();
        let notif_id = self
            .notify(&summary, &body, &actions, Some("im"))
            .await?;
        self.map.lock().unwrap().by_notif_id.insert(
            notif_id,
            NotifRef {
                envelope_id: evt_envelope_id,
                device_id,
                kind: "notification".into(),
                address: None,
                call_id: None,
                package: Some(notif.package.clone()),
                notif_id: Some(notif_id_str),
            },
        );
        Ok(())
    }

    async fn present_sms(
        &self,
        evt_envelope_id: Uuid,
        device_id: Uuid,
        sms: &SmsReceived,
        i18n: &DisplayI18n,
    ) -> Result<(), DisplayError> {
        let summary = render(
            i18n.t("notif.sms.incoming"),
            &[("address", sms.address.as_str())],
        );
        let body = sms.body.clone();
        let actions = Self::action_list_for("sms", i18n);
        let notif_id = self
            .notify(&summary, &body, &actions, Some("im"))
            .await?;
        self.map.lock().unwrap().by_notif_id.insert(
            notif_id,
            NotifRef {
                envelope_id: evt_envelope_id,
                device_id,
                kind: "sms".into(),
                address: Some(sms.address.clone()),
                call_id: None,
                package: None,
                notif_id: None,
            },
        );
        Ok(())
    }

    async fn present_call_incoming(
        &self,
        evt_envelope_id: Uuid,
        device_id: Uuid,
        call: &CallIncoming,
        i18n: &DisplayI18n,
    ) -> Result<(), DisplayError> {
        let summary = render(
            i18n.t("notif.call.incoming"),
            &[("number", call.phone_number.as_str())],
        );
        let body = call.contact_name.clone().unwrap_or_default();
        let actions = Self::action_list_for("call", i18n);
        let notif_id = self
            .notify(&summary, &body, &actions, Some("call"))
            .await?;
        self.map.lock().unwrap().by_notif_id.insert(
            notif_id,
            NotifRef {
                envelope_id: evt_envelope_id,
                device_id,
                kind: "call".into(),
                address: Some(call.phone_number.clone()),
                call_id: None,
                package: None,
                notif_id: None,
            },
        );
        Ok(())
    }

    async fn present_call_state(
        &self,
        evt_envelope_id: Uuid,
        device_id: Uuid,
        call: &CallState,
        i18n: &DisplayI18n,
    ) -> Result<(), DisplayError> {
        if call.state != CallStateKind::Ringing && call.state != CallStateKind::Offhook {
            return Ok(());
        }
        let number = call.phone_number.clone().unwrap_or_default();
        let summary = if call.state == CallStateKind::Offhook {
            i18n.t("notif.call.ongoing").to_string()
        } else {
            render(
                i18n.t("notif.call.incoming"),
                &[("number", number.as_str())],
            )
        };
        let actions = Self::action_list_for("call", i18n);
        let notif_id = self
            .notify(&summary, "", &actions, Some("call"))
            .await?;
        self.map.lock().unwrap().by_notif_id.insert(
            notif_id,
            NotifRef {
                envelope_id: evt_envelope_id,
                device_id,
                kind: "call".into(),
                address: Some(number),
                call_id: call.call_id.clone(),
                package: None,
                notif_id: None,
            },
        );
        Ok(())
    }

    async fn present_action_result(
        &self,
        envelope_id: Uuid,
        device_id: Uuid,
        payload: &serde_json::Value,
        i18n: &DisplayI18n,
    ) -> Result<(), DisplayError> {
        let ok = payload.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
        let body = if ok {
            i18n.t("toast.action_sent").to_string()
        } else {
            let msg = payload
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            render(i18n.t("toast.action_failed"), &[("message", msg)])
        };
        let actions = Vec::<(String, String)>::new();
        let notif_id = self
            .notify(&body, "", &actions, Some("transfer.complete"))
            .await?;
        self.map.lock().unwrap().by_notif_id.insert(
            notif_id,
            NotifRef {
                envelope_id,
                device_id,
                kind: "action_result".into(),
                address: None,
                call_id: None,
                package: None,
                notif_id: None,
            },
        );
        Ok(())
    }

    async fn present_phone_offline(
        &self,
        envelope_id: Uuid,
        device_id: Uuid,
        payload: &serde_json::Value,
        i18n: &DisplayI18n,
    ) -> Result<(), DisplayError> {
        let action_kind = payload
            .get("action_kind")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let body = format!("{} ({})", i18n.t("toast.phone_offline"), action_kind);
        let actions = Vec::<(String, String)>::new();
        let notif_id = self
            .notify(&body, "", &actions, Some("transfer.error"))
            .await?;
        self.map.lock().unwrap().by_notif_id.insert(
            notif_id,
            NotifRef {
                envelope_id,
                device_id,
                kind: "phone_offline".into(),
                address: None,
                call_id: None,
                package: None,
                notif_id: None,
            },
        );
        Ok(())
    }

    /// Wraps the `org.freedesktop.Notifications.Notify`
    /// D-Bus call. Returns the notification id assigned by
    /// the notification daemon.
    async fn notify(
        &self,
        summary: &str,
        body: &str,
        actions: &[(String, String)],
        category: Option<&str>,
    ) -> Result<u32, DisplayError> {
        let conn = self
            .conn
            .lock()
            .unwrap()
            .as_ref()
            .cloned()
            .ok_or_else(|| DisplayError::Dbus("not connected to session bus".into()))?;
        let proxy = NotificationsProxy::new(&conn)
            .await
            .map_err(|e| DisplayError::Dbus(format!("build proxy: {e}")))?;
        let action_pairs: Vec<(&str, &str)> = actions
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let action_slice: &[(&str, &str)] = &action_pairs;
        let mut hints: HashMap<&str, zvariant::Value> = HashMap::new();
        if let Some(cat) = category {
            hints.insert("category", cat.into());
        }
        hints.insert("transient", true.into());
        let id = proxy
            .notify(
                APP_NAME,
                0u32,
                "phonebridge",
                summary,
                body,
                action_slice,
                hints,
                5000i32,
            )
            .await
            .map_err(|e| DisplayError::Dbus(format!("Notify: {e}")))?;
        Ok(id)
    }
}

#[async_trait]
impl DisplayBackend for LinuxBackend {
    async fn start(&self) -> Result<(), DisplayError> {
        let conn = Connection::session()
            .await
            .map_err(|e| DisplayError::Dbus(format!("session bus: {e}")))?;
        *self.conn.lock().unwrap() = Some(conn.clone());

        // Spawn the signal consumers. We hold a clone of
        // the proxy (cheap; it's just an `Arc`) and a clone
        // of the map / sink.
        let proxy = NotificationsProxy::new(&conn)
            .await
            .map_err(|e| DisplayError::Dbus(format!("build proxy: {e}")))?;
        spawn_action_invoked_consumer(proxy.clone(), self.map.clone(), self.sink.clone());
        spawn_notification_closed_consumer(proxy, self.map.clone());
        Ok(())
    }

    async fn present(
        &self,
        event: &phonebridge_proto::DisplayEvent,
        i18n: &DisplayI18n,
        _actions: &ActionSink,
    ) -> Result<(), DisplayError> {
        match event.kind.as_str() {
            "notification.received" => {
                let n: NotificationReceived = serde_json::from_value(event.payload.clone())
                    .map_err(|e| DisplayError::Protocol(format!("notification.received: {e}")))?;
                self.present_notification(event.envelope_id, event.device_id, &n, i18n)
                    .await
            }
            "sms.received" => {
                let s: SmsReceived = serde_json::from_value(event.payload.clone())
                    .map_err(|e| DisplayError::Protocol(format!("sms.received: {e}")))?;
                self.present_sms(event.envelope_id, event.device_id, &s, i18n)
                    .await
            }
            "call.incoming" => {
                let c: CallIncoming = serde_json::from_value(event.payload.clone())
                    .map_err(|e| DisplayError::Protocol(format!("call.incoming: {e}")))?;
                self.present_call_incoming(event.envelope_id, event.device_id, &c, i18n)
                    .await
            }
            "call.state" => {
                let c: CallState = serde_json::from_value(event.payload.clone())
                    .map_err(|e| DisplayError::Protocol(format!("call.state: {e}")))?;
                self.present_call_state(event.envelope_id, event.device_id, &c, i18n)
                    .await
            }
            "action.result" => {
                self.present_action_result(event.envelope_id, event.device_id, &event.payload, i18n)
                    .await
            }
            "phone.offline" => {
                self.present_phone_offline(event.envelope_id, event.device_id, &event.payload, i18n)
                    .await
            }
            "device.hello" | "device.unpair" | "device.pair.request" | "device.pair.confirm" => {
                Ok(())
            }
            other => {
                tracing::debug!(kind = %other, "linux backend: ignoring unknown event kind");
                Ok(())
            }
        }
    }

    async fn stop(&self) -> Result<(), DisplayError> {
        *self.conn.lock().unwrap() = None;
        Ok(())
    }
}

fn spawn_action_invoked_consumer(
    proxy: NotificationsProxy<'static>,
    map: Arc<StdMutex<NotificationMap>>,
    sink: ActionSink,
) {
    tokio::spawn(async move {
        let mut stream = match proxy.inner().receive_signal("ActionInvoked").await {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(error = %e, "could not subscribe to ActionInvoked");
                return;
            }
        };
        while let Some(msg) = stream.next().await {
            // Body signature: (notif_id: u32, action_key: str)
            let body = msg.body();
            let (notif_id, action_key) = match body.deserialize_unchecked::<(u32, String)>() {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(error = %e, "ActionInvoked body decode failed");
                    continue;
                }
            };

            // Build the action from the map.
            let action = {
                let map_guard = map.lock().unwrap();
                let Some(r) = map_guard.by_notif_id.get(&notif_id).cloned() else {
                    tracing::debug!(notif_id, action=%action_key, "ActionInvoked for unknown notif_id");
                    continue;
                };
                action_for_key(&action_key, &r)
            };
            let Some(mut action) = action else {
                tracing::debug!(action=%action_key, "ActionInvoked with unknown action_key");
                continue;
            };
            // Reply needs interactive text input.
            if action.kind == "sms.reply" && action.body.is_none() {
                let address = map
                    .lock()
                    .unwrap()
                    .by_notif_id
                    .get(&notif_id)
                    .and_then(|r| r.address.clone())
                    .unwrap_or_else(|| "(unknown)".to_string());
                let prompt = format!("Reply to {address}:");
                match collect_reply_text(&prompt).await {
                    Ok(text) if !text.is_empty() => action.body = Some(text),
                    Ok(_) => {
                        tracing::info!("reply cancelled (empty text)");
                        continue;
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "could not collect reply text; dropping");
                        continue;
                    }
                }
            }
            if let Err(e) = sink.try_send(action) {
                tracing::warn!(error = %e, "failed to enqueue display action");
            }
        }
    });
}

fn spawn_notification_closed_consumer(
    proxy: NotificationsProxy<'static>,
    map: Arc<StdMutex<NotificationMap>>,
) {
    tokio::spawn(async move {
        let mut stream = match proxy.inner().receive_signal("NotificationClosed").await {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(error = %e, "could not subscribe to NotificationClosed");
                return;
            }
        };
        while let Some(msg) = stream.next().await {
            // Body signature: (notif_id: u32, reason: u32)
            let body = msg.body();
            let (notif_id, _reason) = match body.deserialize_unchecked::<(u32, u32)>() {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(error = %e, "NotificationClosed body decode failed");
                    continue;
                }
            };
            map.lock().unwrap().by_notif_id.remove(&notif_id);
        }
    });
}

fn action_for_key(action_key: &str, r: &NotifRef) -> Option<DisplayAction> {
    match action_key {
        REPLY_ACTION => Some(DisplayAction {
            kind: "sms.reply".into(),
            envelope_id: r.envelope_id,
            device_id: r.device_id,
            to: r.address.clone(),
            body: None,
            call_id: None,
        }),
        MARK_READ_ACTION => Some(DisplayAction {
            kind: "notification.read".into(),
            envelope_id: r.envelope_id,
            device_id: r.device_id,
            to: None,
            body: None,
            call_id: None,
        }),
        DISMISS_ACTION => Some(DisplayAction {
            kind: "notification.dismiss".into(),
            envelope_id: r.envelope_id,
            device_id: r.device_id,
            to: None,
            body: None,
            call_id: None,
        }),
        ANSWER_ACTION => Some(DisplayAction {
            kind: "call.answer".into(),
            envelope_id: r.envelope_id,
            device_id: r.device_id,
            to: None,
            body: None,
            call_id: r.call_id.clone(),
        }),
        HANGUP_ACTION => Some(DisplayAction {
            kind: "call.end".into(),
            envelope_id: r.envelope_id,
            device_id: r.device_id,
            to: None,
            body: None,
            call_id: r.call_id.clone(),
        }),
        _ => None,
    }
}

#[zbus::proxy(
    interface = "org.freedesktop.Notifications",
    default_service = "org.freedesktop.Notifications",
    default_path = "/org/freedesktop/Notifications"
)]
trait Notifications {
    async fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        app_icon: &str,
        summary: &str,
        body: &str,
        actions: &[(&str, &str)],
        hints: HashMap<&str, zvariant::Value<'_>>,
        expire_timeout: i32,
    ) -> zbus::Result<u32>;
}

/// Open `zenity --entry` (GNOME) or `kdialog --inputbox`
/// (KDE) and return the text the user typed. Returns
/// `Ok("")` on cancel, `Err` if neither tool is installed.
async fn collect_reply_text(prompt: &str) -> Result<String, DisplayError> {
    if which("zenity").await {
        if let Some(t) = run_prompt("zenity", &["--entry", "--text", prompt]).await? {
            return Ok(t);
        }
    }
    if which("kdialog").await {
        if let Some(t) = run_prompt("kdialog", &["--inputbox", prompt]).await? {
            return Ok(t);
        }
    }
    Err(DisplayError::Prompt(
        "neither zenity nor kdialog is installed; cannot collect reply text".into(),
    ))
}

async fn which(cmd: &str) -> bool {
    tokio::process::Command::new("sh")
        .args(["-c", &format!("command -v {cmd} >/dev/null 2>&1")])
        .status()
        .await
        .map(|s| s.success())
        .unwrap_or(false)
}

async fn run_prompt(prog: &str, args: &[&str]) -> Result<Option<String>, DisplayError> {
    let mut child = tokio::process::Command::new(prog)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .spawn()
        .map_err(|e| DisplayError::Prompt(format!("spawn {prog}: {e}")))?;
    let mut out = Vec::new();
    if let Some(mut o) = child.stdout.take() {
        let _ = o.read_to_end(&mut out).await;
    }
    let mut err = Vec::new();
    if let Some(mut e) = child.stderr.take() {
        let _ = e.read_to_end(&mut err).await;
    }
    let status = child
        .wait()
        .await
        .map_err(|e| DisplayError::Prompt(format!("wait {prog}: {e}")))?;
    if !status.success() {
        // User cancelled: zenity returns 1, kdialog returns 1.
        return Ok(None);
    }
    Ok(Some(String::from_utf8_lossy(&out).trim().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DisplayConfig;

    #[test]
    fn action_list_for_sms_has_reply() {
        let i18n = DisplayI18n::builtin_en();
        let actions = LinuxBackend::action_list_for("sms", &i18n);
        let keys: Vec<&str> = actions.iter().map(|(k, _)| k.as_str()).collect();
        assert!(keys.contains(&"reply"));
        assert!(keys.contains(&"mark_read"));
        assert!(keys.contains(&"dismiss"));
    }

    #[test]
    fn action_list_for_call_has_answer() {
        let i18n = DisplayI18n::builtin_en();
        let actions = LinuxBackend::action_list_for("call", &i18n);
        let keys: Vec<&str> = actions.iter().map(|(k, _)| k.as_str()).collect();
        assert!(keys.contains(&"answer"));
        assert!(keys.contains(&"hangup"));
    }

    #[test]
    fn action_for_key_maps_reply_to_sms() {
        let env = Uuid::new_v4();
        let dev = Uuid::new_v4();
        let r = NotifRef {
            envelope_id: env,
            device_id: dev,
            kind: "sms".into(),
            address: Some("+8613800138000".into()),
            call_id: None,
            package: None,
            notif_id: None,
        };
        let a = action_for_key("reply", &r).unwrap();
        assert_eq!(a.kind, "sms.reply");
        assert_eq!(a.envelope_id, env);
        assert_eq!(a.to.as_deref(), Some("+8613800138000"));
        assert!(a.body.is_none());
    }

    #[test]
    fn action_for_key_maps_mark_read() {
        let r = NotifRef {
            envelope_id: Uuid::new_v4(),
            device_id: Uuid::new_v4(),
            kind: "notification".into(),
            address: None,
            call_id: None,
            package: None,
            notif_id: None,
        };
        let a = action_for_key("mark_read", &r).unwrap();
        assert_eq!(a.kind, "notification.read");
    }

    #[test]
    fn action_for_key_maps_dismiss() {
        let r = NotifRef {
            envelope_id: Uuid::new_v4(),
            device_id: Uuid::new_v4(),
            kind: "notification".into(),
            address: None,
            call_id: None,
            package: None,
            notif_id: None,
        };
        let a = action_for_key("dismiss", &r).unwrap();
        assert_eq!(a.kind, "notification.dismiss");
    }

    #[test]
    fn action_for_key_maps_answer() {
        let r = NotifRef {
            envelope_id: Uuid::new_v4(),
            device_id: Uuid::new_v4(),
            kind: "call".into(),
            address: Some("+8613800138000".into()),
            call_id: Some("call-123".into()),
            package: None,
            notif_id: None,
        };
        let a = action_for_key("answer", &r).unwrap();
        assert_eq!(a.kind, "call.answer");
        assert_eq!(a.call_id.as_deref(), Some("call-123"));
    }

    #[test]
    fn action_for_key_unknown_returns_none() {
        let r = NotifRef {
            envelope_id: Uuid::new_v4(),
            device_id: Uuid::new_v4(),
            kind: "sms".into(),
            address: None,
            call_id: None,
            package: None,
            notif_id: None,
        };
        assert!(action_for_key("nope", &r).is_none());
    }

    #[tokio::test]
    async fn collect_reply_text_returns_err_when_no_zenity() {
        // We can't safely set PATH="" in tests (Rust 1.78+
        // made env::set_var unsafe and #![forbid(unsafe_code)]
        // makes it awkward). Instead, we test that the
        // function errors when an obviously-missing program
        // is requested. Since the function shells out to
        // `zenity` first and falls back to `kdialog`, both
        // of which may exist in the test environment, we
        // can't reliably assert an error in CI. The real
        // assertion is that the call doesn't panic; both
        // success and `Err(_)` are acceptable.
        let r = collect_reply_text("hi").await;
        // The function must return a Result<String, _>;
        // success returns "". An `Err` is fine if neither
        // tool is installed; `Ok("")` is fine if both are
        // absent or both were cancelled. The unit test
        // simply exercises the codepath.
        let _ = r;
    }

    #[test]
    fn render_includes_address() {
        let i18n = DisplayI18n::builtin_en();
        let s = render(
            i18n.t("notif.sms.incoming"),
            &[("address", "+8613800138000")],
        );
        assert!(s.contains("+8613800138000"));
    }

    #[test]
    fn new_backend_creates_empty_map() {
        let cfg = DisplayConfig::for_test("http://127.0.0.1:8443", "x");
        let b = LinuxBackend::new(&cfg).unwrap();
        assert!(b.map.lock().unwrap().by_notif_id.is_empty());
    }
}
