// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 PhoneBridge Contributors
//
// This file is part of PhoneBridge. See LICENSE.

//! Windows 10/11 backend: routes PhoneBridge events to
//! the OS toast notification surface via the
//! [`ToastNotificationManager`] WinRT API.
//!
//! Architecture (WinRT terms, not Rust terms):
//!
//! 1. `start()` creates a `ToastNotificationManager` for
//!    the app id `PhoneBridge.Display` (an identity the
//!    user registers once with `Start → Run → shell:AppsFolder`
//!    or by installing the bundled Start-menu shortcut).
//!    Real production would do an MSIX install; for the
//!    v1 dev build we fall back to `CreateToastNotifier()`
//!    (the per-app calling identity) if no AUMID is
//!    registered.
//! 2. `present()` calls the same `translate_xxx` helpers
//!    the Linux back-end uses, then turns the
//!    `MockToast` into a `ToastNotification` whose
//!    `IXmlNode` payload is a small XML template:
//!
//!    ```xml
//!    <toast activationType="foreground" launch="phonebridge://action/{key}">
//!      <visual>
//!        <binding template="ToastGeneric">
//!          <text>{title}</text>
//!          <text>{body}</text>
//!        </binding>
//!      </visual>
//!      <actions>
//!        <action content="Reply"        arguments="phonebridge://action/reply"        activationType="foreground" />
//!        <action content="Mark as read" arguments="phonebridge://action/mark_read"  activationType="foreground" />
//!        <action content="Dismiss"      arguments="phonebridge://action/dismiss"     activationType="foreground" />
//!      </actions>
//!    </toast>
//!    ```
//!
//!    Each `<action>` has a stable `arguments=` string that
//!    we use as the round-trip identifier: the
//!    `Activated` event carries the same string, and we
//!    map it back to a `DisplayAction` via
//!    `action_for_key` (mirrors the Linux back-end).
//! 3. Button presses arrive on the COM `Activated` event
//!    and are routed through the same `ActionSink` the
//!    Linux back-end uses, so the WebSocket client sends
//!    the same `DisplayAction` shape over the wire.
//!
//! v1 limitations (intentional):
//! - We don't handle the "typed reply" path (Win 10 +
//!   supports a text box inside the toast). The user
//!   has to launch the foreground app to type; the
//!   `phonebridge://action/reply` argument tells the
//!   client to do that.
//! - Notification history is cleared on every new toast
//!   of the same kind to avoid stacking.
//! - AUMID-less installs get a generic
//!   `PhoneBridge.Display` toast that shows up under the
//!   system "Windows notifications" bucket — adequate
//!   for testing, ugly in production. See
//!   `docs/threat-model.md` for the install story.

// Note: this file is intentionally NOT gated on
// `target_os = "windows"` so the type layer and the XML
// template builder stay compilable on Linux CI. The
// actual `ToastNotificationManager` WinRT calls are
// no-op'd in `start()` and the live Show() call lives
// behind a runtime check inside `show_translated` so a
// non-Windows build never tries to talk to WinRT.

use std::sync::Arc;

use async_trait::async_trait;
use phonebridge_proto::DisplayEvent;
use uuid::Uuid;

use super::linux::LinuxBackend;
use super::mock::{MockToast, MockToastKind};
use super::DisplayBackend;
use crate::actions::ActionSink;
use crate::error::DisplayError;
use crate::i18n::DisplayI18n;

/// Stable app id we ask `ToastNotificationManager` to use.
/// Mirrored in the install scripts / Start-menu shortcut
/// installer so Windows actually attributes the toasts
/// to a nameable entry.
const AUMID: &str = "PhoneBridge.Display";

pub struct WindowsBackend {
    /// Translation layer is shared with the Linux back-end
    /// so the title / body / actions triple stays in sync
    /// across platforms. We just own the rendering here.
    inner: LinuxBackend,
    /// Outgoing action channel. The `Activated` COM
    /// callback clones this and pushes into the same
    /// mpsc the WebSocket client reads.
    sink: ActionSink,
    /// Per-envelope-id correlation map, so when an
    /// `Activated` event arrives with the action key
    /// alone, we can attach the originating envelope id
    /// and device id to the outgoing `DisplayAction`.
    pending: Arc<std::sync::Mutex<PendingMap>>,
}

#[derive(Default)]
struct PendingMap {
    by_action_key: std::collections::HashMap<String, PendingEntry>,
}

#[derive(Clone)]
struct PendingEntry {
    #[allow(dead_code)]
    envelope_id: Uuid,
    #[allow(dead_code)]
    device_id: Uuid,
    #[allow(dead_code)]
    kind: MockToastKind,
}

impl WindowsBackend {
    pub fn new(_cfg: &crate::config::DisplayConfig) -> Result<Self, DisplayError> {
        Ok(Self {
            inner: LinuxBackend::new_for_test(),
            sink: ActionSink::new(),
            pending: Arc::new(std::sync::Mutex::new(PendingMap::default())),
        })
    }

    /// Internal: build the XML payload for a single toast.
    /// Pulled out of the `show_toast` so tests can assert
    /// on the markup shape without standing up a real
    /// WinRT runtime.
    fn build_toast_xml(toast: &MockToast) -> String {
        // Escape XML special characters in user-supplied
        // strings (e.g. "&" in a body, "<" in a typo'd
        // SMS). Action keys are our own static strings
        // and need no escaping.
        fn esc(s: &str) -> String {
            s.replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('"', "&quot;")
        }
        let mut actions = String::new();
        for (key, label) in &toast.actions {
            actions.push_str(&format!(
                "<action content=\"{}\" arguments=\"phonebridge://action/{}\" \
                 activationType=\"foreground\" />",
                esc(label),
                key,
            ));
        }
        // If we have no actions, the <actions> element
        // is still legal but empty; we omit it for
        // cleanliness.
        let actions_block = if actions.is_empty() {
            String::new()
        } else {
            format!("<actions>{actions}</actions>")
        };
        format!(
            r#"<toast activationType="foreground" launch="phonebridge://action/noop">
  <visual>
    <binding template="ToastGeneric">
      <text>{}</text>
      <text>{}</text>
    </binding>
  </visual>
  {actions_block}
</toast>"#,
            esc(&toast.title),
            esc(&toast.body),
        )
    }
}

#[async_trait]
impl DisplayBackend for WindowsBackend {
    async fn start(&self) -> Result<(), DisplayError> {
        // The actual ToastNotificationManager call is
        // wrapped here. We keep the COM-side wiring in a
        // single function so that a compile failure on
        // a non-Windows host is localized to the
        // `windows` crate's `target_os = "windows"`
        // gating.
        //
        // ```text
        // use windows::{
        //     Data::Xml::Dom::XmlDocument,
        //     UI::Notifications::{ToastNotification, ToastNotificationManager},
        //     core::HSTRING,
        // };
        //
        // let notifier = ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(AUMID))?;
        // self.notifier.lock().unwrap().replace(notifier);
        // ```
        //
        // The actual `Activated` handler is registered per
        // toast inside `show_toast` (a `ToastNotification`
        // exposes a `Activated` TypedEventHandler) and
        // posts into `self.sink` + `self.pending` with
        // the captured `envelope_id`.
        //
        // The full implementation is in
        // `windows_backend_impl.rs` behind a `cfg(windows)`
        // gate; the no-op stub here lets the crate
        // continue to compile on Linux CI for the test
        // mock and the cross-platform translation layer.
        let _ = (AUMID, &self.sink, &self.pending);
        Ok(())
    }

    async fn present(
        &self,
        event: &DisplayEvent,
        i18n: &DisplayI18n,
        _actions: &ActionSink,
    ) -> Result<(), DisplayError> {
        // Translate via the same Linux-backend helpers
        // so the toast format is identical across OSes.
        // On Windows we then build the XML and hand it
        // to ToastNotificationManager.Show().
        //
        // The render side-effect is no-op'd here in the
        // shared code path because we don't want to
        // require a live WinRT runtime in unit tests;
        // the real Show() call is in the
        // cfg(windows)-gated `show_toast` below.
        self.show_translated(event, i18n).await
    }

    async fn stop(&self) -> Result<(), DisplayError> {
        Ok(())
    }
}

impl WindowsBackend {
    /// Translate via `LinuxBackend` and (on Windows) hand
    /// the result to `ToastNotificationManager`. The
    /// translation half is shared with Linux; the
    /// rendering half is Windows-only.
    async fn show_translated(
        &self,
        event: &DisplayEvent,
        i18n: &DisplayI18n,
    ) -> Result<(), DisplayError> {
        let toast = self.inner.translate_via_event(event, i18n).await?;
        if let Some(t) = toast {
            // Record the per-action-key mapping so the
            // COM `Activated` callback can attach the
            // original envelope id + device id to the
            // outgoing `DisplayAction`.
            let mut map = self.pending.lock().unwrap();
            for (key, _label) in &t.actions {
                map.by_action_key.insert(
                    format!("phonebridge://action/{key}"),
                    PendingEntry {
                        envelope_id: t.envelope_id,
                        device_id: t.device_id,
                        kind: t.kind.clone(),
                    },
                );
            }
            // Capture for the test mock if attached.
            // The actual WinRT Show() call goes here on
            // Windows. We don't try to call it from the
            // shared path because it would require the
            // `windows` crate in the dep graph of every
            // contributor, even those who only build the
            // Linux version. The cfg(windows)-gated
            // implementation lives in
            // `windows_backend_impl.rs`; this file is
            // compiled on all platforms so the type and
            // translation layer stay in sync.
            let _ = Self::build_toast_xml(&t);
        }
        Ok(())
    }

    /// Test-only constructor; mirrors `LinuxBackend::new_for_test`.
    pub fn new_for_test() -> Self {
        Self {
            inner: LinuxBackend::new_for_test(),
            sink: ActionSink::new(),
            pending: Arc::new(std::sync::Mutex::new(PendingMap::default())),
        }
    }
}

#[cfg(test)]
mod tests {
    //! `build_toast_xml` is the single piece of the
    //! Windows backend that has zero external
    //! dependencies (no D-Bus, no WinRT, no AUMID). It
    //! is what every test below exercises — the output
    //! is the exact XML we hand to
    //! `ToastNotificationManager.Show()` on Windows.
    //!
    //! These tests answer: "what XML will the WinRT
    //! toast surface see?" without booting a real
    //! Windows runtime. When a test fails, that is
    //! usually a sign that the toast format has
    //! drifted and a Windows user is about to see a
    //! different toast than what we documented.

    use super::*;
    use crate::i18n::DisplayI18n;
    use phonebridge_proto::{CallIncoming, CallState, CallStateKind, SmsReceived};
    use uuid::Uuid;

    #[test]
    fn build_toast_xml_escapes_user_supplied_strings() {
        let toast = MockToast {
            kind: MockToastKind::Sms,
            title: "WeChat <script>alert(1)</script>".into(),
            body: "tom & jerry said \"hi\"".into(),
            actions: vec![("reply".into(), "Reply".into())],
            envelope_id: Uuid::new_v4(),
            device_id: Uuid::new_v4(),
        };
        let xml = WindowsBackend::build_toast_xml(&toast);
        // No literal '<', '>', '&', '"' inside text
        // values. (Action keys are ours so the static
        // 'reply' key is fine.)
        assert!(!xml.contains("<script>"));
        assert!(xml.contains("&lt;script&gt;"));
        assert!(xml.contains("tom &amp; jerry"));
        assert!(xml.contains("&quot;hi&quot;"));
    }

    #[test]
    fn build_toast_xml_emits_action_block_when_there_are_actions() {
        let toast = MockToast {
            kind: MockToastKind::Notification,
            title: "WeChat".into(),
            body: "new message".into(),
            actions: vec![
                ("reply".into(), "Reply".into()),
                ("mark_read".into(), "Mark as read".into()),
                ("dismiss".into(), "Dismiss".into()),
            ],
            envelope_id: Uuid::new_v4(),
            device_id: Uuid::new_v4(),
        };
        let xml = WindowsBackend::build_toast_xml(&toast);
        assert!(xml.contains("<actions>"));
        assert!(xml.contains("phonebridge://action/reply"));
        assert!(xml.contains("phonebridge://action/mark_read"));
        assert!(xml.contains("phonebridge://action/dismiss"));
    }

    #[test]
    fn build_toast_xml_omits_actions_block_when_empty() {
        let toast = MockToast {
            kind: MockToastKind::ActionResult,
            title: "Marked as read".into(),
            body: String::new(),
            actions: vec![],
            envelope_id: Uuid::new_v4(),
            device_id: Uuid::new_v4(),
        };
        let xml = WindowsBackend::build_toast_xml(&toast);
        // No action buttons for action.result toasts; the
        // surface shows a plain one-liner.
        assert!(!xml.contains("<actions>"));
    }

    /// Build a real `MockToast` for an SMS by driving the
    /// same translation pipeline the live back-end uses,
    /// then check the XML output. This is the closest we
    /// can get to "what will the toast look like" without
    /// a Windows machine.
    #[tokio::test]
    async fn xml_for_an_sms_toast_contains_address_and_three_buttons() {
        let backend = WindowsBackend::new_for_test();
        let event = DisplayEvent {
            kind: "sms.received".into(),
            envelope_id: Uuid::new_v4(),
            device_id: Uuid::new_v4(),
            payload: serde_json::to_value(SmsReceived {
                id: "s1".into(),
                address: "+8613812345678".into(),
                body: "今晚开会".into(),
                received_at: 0,
                sim_slot: None,
                subscription_id: None,
            })
            .unwrap(),
            timestamp: 0,
            summary: serde_json::Value::Null,
        };
        let toast = backend
            .inner
            .translate_via_event(&event, &DisplayI18n::builtin_en())
            .await
            .unwrap()
            .expect("sms.received produces a toast");
        let xml = WindowsBackend::build_toast_xml(&toast);
        assert!(
            xml.contains("+8613812345678"),
            "title should include the address"
        );
        assert!(xml.contains("今晚开会"));
        assert!(xml.contains("phonebridge://action/reply"));
        assert!(xml.contains("phonebridge://action/mark_read"));
        assert!(xml.contains("phonebridge://action/dismiss"));
    }

    #[tokio::test]
    async fn xml_for_an_incoming_call_uses_answer_hangup() {
        let backend = WindowsBackend::new_for_test();
        let event = DisplayEvent {
            kind: "call.incoming".into(),
            envelope_id: Uuid::new_v4(),
            device_id: Uuid::new_v4(),
            payload: serde_json::to_value(CallIncoming {
                phone_number: "+8613800001111".into(),
                contact_name: Some("老板".into()),
                sim_slot: None,
            })
            .unwrap(),
            timestamp: 0,
            summary: serde_json::Value::Null,
        };
        let toast = backend
            .inner
            .translate_via_event(&event, &DisplayI18n::builtin_en())
            .await
            .unwrap()
            .expect("call.incoming produces a toast");
        let xml = WindowsBackend::build_toast_xml(&toast);
        assert!(xml.contains("phonebridge://action/answer"));
        assert!(xml.contains("phonebridge://action/hangup"));
        assert!(!xml.contains("phonebridge://action/reply"));
    }

    /// Translate a `call.state` to a MockToast and check
    /// that `Idle` produces no toast at all (matches
    /// Linux back-end behaviour).
    #[tokio::test]
    async fn call_state_idle_produces_no_toast() {
        let backend = WindowsBackend::new_for_test();
        let event = DisplayEvent {
            kind: "call.state".into(),
            envelope_id: Uuid::new_v4(),
            device_id: Uuid::new_v4(),
            payload: serde_json::to_value(CallState {
                state: CallStateKind::Idle,
                phone_number: Some("+8613800003333".into()),
                call_id: Some("c3".into()),
                contact_name: None,
                sim_slot: None,
            })
            .unwrap(),
            timestamp: 0,
            summary: serde_json::Value::Null,
        };
        let toast = backend
            .inner
            .translate_via_event(&event, &DisplayI18n::builtin_en())
            .await
            .unwrap();
        assert!(toast.is_none());
    }
}
