// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 PhoneBridge Contributors
//
// This file is part of PhoneBridge. See LICENSE.

//! In-process mock backend used by unit tests.
//!
//! Captures every `present_xxx` translation as a typed
//! [`MockToast`] so a test can assert exactly what the
//! real OS surface would have been told — without standing
//! up a D-Bus session bus, a Windows Runtime, or any other
//! platform integration.
//!
//! Always compiled (not cfg-gated) because:
//!   1. The Linux back-end tests want to run on CI's
//!      Linux runner without a real D-Bus session.
//!   2. The mock is `pub`, so a test in a sibling module
//!      can `pub use super::MockBackend` and pass it to
//!      the same dispatch logic the real back-end uses.
//!
//! Not registered in `create()`; tests construct
//! `MockBackend::new()` directly and route events through
//! the same `present_*` helpers the OS backends expose
//! (see the integration test in
//! `backends::tests::toast_for_sms_received`).

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use phonebridge_proto::{CallIncoming, CallState, DisplayEvent, NotificationReceived, SmsReceived};
use uuid::Uuid;

use super::linux::LinuxBackend;
use super::DisplayBackend;
use crate::actions::ActionSink;
use crate::error::DisplayError;
use crate::i18n::DisplayI18n;

/// One OS-surface call captured by the mock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MockToast {
    /// What the back-end asked the OS to display.
    pub kind: MockToastKind,
    /// Window title (the `summary` arg in
    /// `org.freedesktop.Notifications.Notify`,
    /// `ToastNotification`'s `<text1>`, etc.).
    pub title: String,
    /// Body text.
    pub body: String,
    /// Action button pairs (key, label) in display order.
    pub actions: Vec<(String, String)>,
    /// Event id this toast was rendered for, so tests
    /// can correlate a later user click with the original
    /// event.
    pub envelope_id: Uuid,
    /// Device the event came from.
    pub device_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MockToastKind {
    Notification,
    Sms,
    CallIncoming,
    CallOngoing,
    CallEnded,
    ActionResult,
    PhoneOffline,
}

impl MockToastKind {
    fn from_event_kind(kind: &str) -> Self {
        match kind {
            "notification.received" => Self::Notification,
            "sms.received" => Self::Sms,
            "call.incoming" => Self::CallIncoming,
            "call.state" => Self::CallOngoing, // refined below
            "action.result" => Self::ActionResult,
            "phone.offline" => Self::PhoneOffline,
            _ => Self::ActionResult,
        }
    }
}

#[derive(Default)]
struct Captured {
    toasts: Vec<MockToast>,
    /// Outgoing `DisplayAction`s the back-end pushed into
    /// the sink in response to (mocked) button clicks.
    actions: Vec<phonebridge_proto::DisplayAction>,
}

/// `DisplayBackend` impl that records every toast.
#[derive(Clone, Default)]
pub struct MockBackend {
    captured: Arc<Mutex<Captured>>,
}

impl MockBackend {
    pub fn new() -> Self {
        Self::default()
    }

    /// Snapshot of toasts shown so far.
    pub fn toasts(&self) -> Vec<MockToast> {
        self.captured.lock().unwrap().toasts.clone()
    }

    /// Snapshot of actions dispatched (e.g. as if a user
    /// clicked a button).
    pub fn actions(&self) -> Vec<phonebridge_proto::DisplayAction> {
        self.captured.lock().unwrap().actions.clone()
    }

    /// Reset history. Tests use this between cases.
    pub fn reset(&self) {
        let mut c = self.captured.lock().unwrap();
        c.toasts.clear();
        c.actions.clear();
    }

    /// Internal: record a toast.
    fn record(&self, toast: MockToast) {
        self.captured.lock().unwrap().toasts.push(toast);
    }

    /// Internal: record a user-action push. Wired in for
    /// the upcoming "action callback → sink" integration
    /// tests; not used yet by any `present_xxx` path
    /// (the real back-ends do their own `sink.try_send`
    /// inside the COM/D-Bus callback, after the action
    /// key is mapped through `action_for_key`).
    #[allow(dead_code)]
    fn record_action(&self, action: phonebridge_proto::DisplayAction) {
        self.captured.lock().unwrap().actions.push(action);
    }

    /// Internal: dispatch one event through the same
    /// `LinuxBackend` translation logic the real OS
    /// surface uses, but instead of going to D-Bus we
    /// capture the args. This is what makes the mock
    /// actually test the translation code, not just a
    /// parallel re-implementation.
    ///
    /// Returns the same `MockToast` for the convenience of
    /// the caller; the per-event record is also pushed
    /// into the captured history.
    pub async fn present_via_linux_logic(
        &self,
        event: &DisplayEvent,
        i18n: &DisplayI18n,
    ) -> Result<Option<MockToast>, DisplayError> {
        // We need a real `LinuxBackend` to get the
        // translation logic, but we don't want it to
        // touch D-Bus. The cheapest way is to instantiate
        // it (which only sets up struct fields, no I/O in
        // `new()`), and then drive its `present_*` methods
        // directly. The Linux back-end's `present()` does
        // the JSON payload decoding once; we duplicate
        // that decode here so the test sees the same
        // dispatch path the real back-end does.
        let backend = LinuxBackend::new_for_test();
        match event.kind.as_str() {
            "notification.received" => {
                let n: NotificationReceived = serde_json::from_value(event.payload.clone())
                    .map_err(|e| DisplayError::Protocol(format!("notification.received: {e}")))?;
                let t =
                    backend.translate_notification(event.envelope_id, event.device_id, &n, i18n)?;
                self.record(t.clone());
                Ok(Some(t))
            }
            "sms.received" => {
                let s: SmsReceived = serde_json::from_value(event.payload.clone())
                    .map_err(|e| DisplayError::Protocol(format!("sms.received: {e}")))?;
                let t = backend.translate_sms(event.envelope_id, event.device_id, &s, i18n)?;
                self.record(t.clone());
                Ok(Some(t))
            }
            "call.incoming" => {
                let c: CallIncoming = serde_json::from_value(event.payload.clone())
                    .map_err(|e| DisplayError::Protocol(format!("call.incoming: {e}")))?;
                let t = backend.translate_call_incoming(
                    event.envelope_id,
                    event.device_id,
                    &c,
                    i18n,
                )?;
                self.record(t.clone());
                Ok(Some(t))
            }
            "call.state" => {
                let c: CallState = serde_json::from_value(event.payload.clone())
                    .map_err(|e| DisplayError::Protocol(format!("call.state: {e}")))?;
                if let Some(t) =
                    backend.translate_call_state(event.envelope_id, event.device_id, &c, i18n)?
                {
                    self.record(t.clone());
                    Ok(Some(t))
                } else {
                    Ok(None)
                }
            }
            "action.result" => {
                let t = backend.translate_action_result(
                    event.envelope_id,
                    event.device_id,
                    &event.payload,
                    i18n,
                )?;
                self.record(t.clone());
                Ok(Some(t))
            }
            "phone.offline" => {
                let t = backend.translate_phone_offline(
                    event.envelope_id,
                    event.device_id,
                    &event.payload,
                    i18n,
                )?;
                self.record(t.clone());
                Ok(Some(t))
            }
            _ => Ok(None),
        }
    }
}

#[async_trait]
impl DisplayBackend for MockBackend {
    async fn start(&self) -> Result<(), DisplayError> {
        // No-op: the mock has no external surface to
        // subscribe to.
        Ok(())
    }

    async fn present(
        &self,
        event: &DisplayEvent,
        i18n: &DisplayI18n,
        _actions: &ActionSink,
    ) -> Result<(), DisplayError> {
        self.present_via_linux_logic(event, i18n).await?;
        Ok(())
    }

    async fn stop(&self) -> Result<(), DisplayError> {
        Ok(())
    }
}

/// Convenience: classify a `MockToast` kind from a
/// `DisplayEvent.kind` string. Tests can use this to
/// assert the dispatch path went to the right translator
/// without poking at the back-end internals.
pub fn classify(kind: &str) -> MockToastKind {
    MockToastKind::from_event_kind(kind)
}

#[cfg(test)]
mod tests {
    //! These tests answer the question "what toast would my
    //! boss actually see when an Android event lands?".
    //! They run on any host (no D-Bus / WinRT required)
    //! because they only exercise the translation layer.
    //!
    //! Pattern for every test: construct a synthetic
    //! `DisplayEvent`, hand it to `MockBackend`, then
    //! snapshot `mock.toasts()` and assert on the
    //! `title` / `body` / `actions` triple. If a test ever
    //! fails because the toast format drifted, that's the
    //! signal to update the user-facing docs and the
    //! i18n string table together.

    use super::*;
    use phonebridge_proto::{
        CallIncoming, CallState, CallStateKind, DisplayEvent, NotificationReceived, SmsReceived,
    };
    use uuid::Uuid;

    fn evt(kind: &str, payload: serde_json::Value) -> DisplayEvent {
        DisplayEvent {
            kind: kind.into(),
            envelope_id: Uuid::new_v4(),
            device_id: Uuid::new_v4(),
            payload,
            timestamp: 0,
            summary: serde_json::Value::Null,
        }
    }

    /// Spin up a builtin English dictionary. The
    /// `for_test` constructor is what the real back-end
    /// would do if no daemon-supplied dictionary arrives
    /// over `/api/v1/i18n`.
    fn en() -> DisplayI18n {
        DisplayI18n::builtin_en()
    }

    #[tokio::test]
    async fn sms_received_renders_with_address_and_three_actions() {
        let mock = MockBackend::new();
        let event = evt(
            "sms.received",
            serde_json::to_value(SmsReceived {
                id: "s42".into(),
                address: "+8613812345678".into(),
                body: "今晚开会请准备材料".into(),
                received_at: 0,
                sim_slot: None,
                subscription_id: None,
            })
            .unwrap(),
        );
        let toast = mock
            .present_via_linux_logic(&event, &en())
            .await
            .unwrap()
            .expect("sms.received should produce a toast");
        assert_eq!(toast.kind, MockToastKind::Sms);
        // The title is "SMS from <address>"; we test the
        // address substitution by string-contains rather
        // than exact match so we can later add country code
        // normalization without rewriting this test.
        assert!(
            toast.title.contains("+8613812345678"),
            "title should include the address, got {:?}",
            toast.title
        );
        assert_eq!(toast.body, "今晚开会请准备材料");
        let keys: Vec<&str> = toast.actions.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, vec!["reply", "mark_read", "dismiss"]);
    }

    #[tokio::test]
    async fn notification_sensitive_body_is_redacted() {
        let mock = MockBackend::new();
        let event = evt(
            "notification.received",
            serde_json::to_value(NotificationReceived {
                id: "n42".into(),
                package: "im.wechat".into(),
                app_name: Some("WeChat".into()),
                title: "老板".into(),
                content: "secret pin 1234".into(),
                posted_at: 0,
                is_sensitive: true,
                category: None,
            })
            .unwrap(),
        );
        let toast = mock
            .present_via_linux_logic(&event, &en())
            .await
            .unwrap()
            .expect("notification should produce a toast");
        // App name (when set) is preferred over title.
        assert_eq!(toast.title, "WeChat");
        // Sensitive content is replaced with bullet dots,
        // NOT the raw pin.
        assert_eq!(toast.body, "•••");
    }

    #[tokio::test]
    async fn notification_non_sensitive_shows_real_body() {
        let mock = MockBackend::new();
        let event = evt(
            "notification.received",
            serde_json::to_value(NotificationReceived {
                id: "n43".into(),
                package: "im.wechat".into(),
                app_name: None,
                title: "WeChat".into(),
                content: "新消息 1 条".into(),
                posted_at: 0,
                is_sensitive: false,
                category: None,
            })
            .unwrap(),
        );
        let toast = mock
            .present_via_linux_logic(&event, &en())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(toast.title, "WeChat");
        assert_eq!(toast.body, "新消息 1 条");
    }

    #[tokio::test]
    async fn call_incoming_renders_with_number_and_two_actions() {
        let mock = MockBackend::new();
        let event = evt(
            "call.incoming",
            serde_json::to_value(CallIncoming {
                phone_number: "+8613800001111".into(),
                contact_name: Some("老板".into()),
                sim_slot: None,
            })
            .unwrap(),
        );
        let toast = mock
            .present_via_linux_logic(&event, &en())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(toast.kind, MockToastKind::CallIncoming);
        assert!(toast.title.contains("+8613800001111"));
        assert_eq!(toast.body, "老板");
        let keys: Vec<&str> = toast.actions.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, vec!["answer", "hangup"]);
    }

    #[tokio::test]
    async fn call_state_ongoing_has_no_body_no_reply() {
        let mock = MockBackend::new();
        let event = evt(
            "call.state",
            serde_json::to_value(CallState {
                state: CallStateKind::Offhook,
                phone_number: Some("+8613800002222".into()),
                call_id: Some("c2".into()),
                contact_name: None,
                sim_slot: None,
            })
            .unwrap(),
        );
        let toast = mock
            .present_via_linux_logic(&event, &en())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(toast.kind, MockToastKind::CallOngoing);
        // Ongoing-call toast has empty body and the
        // answer/hangup buttons, just like the incoming
        // call — there is no "reply" action on calls.
        assert_eq!(toast.body, "");
        let keys: Vec<&str> = toast.actions.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, vec!["answer", "hangup"]);
    }

    #[tokio::test]
    async fn call_state_idle_emits_no_toast() {
        let mock = MockBackend::new();
        let event = evt(
            "call.state",
            serde_json::to_value(CallState {
                state: CallStateKind::Idle,
                phone_number: Some("+8613800003333".into()),
                call_id: Some("c3".into()),
                contact_name: None,
                sim_slot: None,
            })
            .unwrap(),
        );
        let toast = mock.present_via_linux_logic(&event, &en()).await.unwrap();
        assert!(toast.is_none(), "idle call.state should be a no-op");
        assert!(
            mock.toasts().is_empty(),
            "captured history should also be empty"
        );
    }

    #[tokio::test]
    async fn action_result_ok_and_failure_paths() {
        // Success: short ack toast, no actions.
        let mock = MockBackend::new();
        let ok_event = evt(
            "action.result",
            serde_json::json!({ "ok": true, "message": "" }),
        );
        let t_ok = mock
            .present_via_linux_logic(&ok_event, &en())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(t_ok.kind, MockToastKind::ActionResult);
        assert!(
            !t_ok.title.is_empty(),
            "ok result should still show a confirmation toast"
        );
        assert!(t_ok.actions.is_empty());

        // Failure: includes the server-provided message.
        let mock = MockBackend::new();
        let fail_event = evt(
            "action.result",
            serde_json::json!({ "ok": false, "message": "device timeout" }),
        );
        let t_fail = mock
            .present_via_linux_logic(&fail_event, &en())
            .await
            .unwrap()
            .unwrap();
        assert!(
            t_fail.title.contains("device timeout"),
            "failure toast should include the message, got {:?}",
            t_fail.title
        );
    }

    #[tokio::test]
    async fn phone_offline_includes_action_kind() {
        let mock = MockBackend::new();
        let event = evt(
            "phone.offline",
            serde_json::json!({ "action_kind": "sms.reply" }),
        );
        let toast = mock
            .present_via_linux_logic(&event, &en())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(toast.kind, MockToastKind::PhoneOffline);
        assert!(
            toast.title.contains("sms.reply"),
            "should include the action kind so the user knows what failed"
        );
    }

    #[tokio::test]
    async fn captured_history_accumulates_then_resets() {
        let mock = MockBackend::new();
        let e1 = evt(
            "sms.received",
            serde_json::to_value(SmsReceived {
                id: "sa".into(),
                address: "+8613811111111".into(),
                body: "hi".into(),
                received_at: 0,
                sim_slot: None,
                subscription_id: None,
            })
            .unwrap(),
        );
        let e2 = evt(
            "sms.received",
            serde_json::to_value(SmsReceived {
                id: "sb".into(),
                address: "+8613822222222".into(),
                body: "there".into(),
                received_at: 0,
                sim_slot: None,
                subscription_id: None,
            })
            .unwrap(),
        );
        mock.present_via_linux_logic(&e1, &en()).await.unwrap();
        mock.present_via_linux_logic(&e2, &en()).await.unwrap();
        assert_eq!(mock.toasts().len(), 2);
        mock.reset();
        assert!(mock.toasts().is_empty());
    }
}
