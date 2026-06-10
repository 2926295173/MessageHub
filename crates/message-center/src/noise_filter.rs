// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 PhoneBridge Contributors
//
// This file is part of PhoneBridge. See LICENSE and the dual-licensing
// notice in README.md for details.

//! Noise filter — drop a small set of known-noise events from the
//! `display_bus`.
//!
//! v1 scope: a hardcoded deny list, first-match semantics, default
//! allow. The 7 predicates are deliberately hand-picked; new ones
//! can be added to [`NOISE_FILTERS`] without touching the call site
//! in [`center_sink`].
//!
//! ## What gets dropped
//!
//! The filter is consulted *after* [`DisplayEvent`] is built (the
//! `payload` is already serialized JSON at that point), so a noisy
//! kind OR a noisy payload field will match. The 7 predicates are:
//!
//! | Name              | Drops                                                |
//! |-------------------|------------------------------------------------------|
//! | `is_heartbeat`    | `device.heartbeat` (latency liveness)                |
//! | `is_pair_internal`| any `device.pair.*` (6-step handshake)              |
//! | `is_info_update`  | `device.info.update` (battery, network)              |
//! | `is_unpair`       | `device.unpair`                                       |
//! | `is_own_pkg`      | our own `notification.received` (debug + release)    |
//! | `is_sys_noise`    | `notification.received` from system packages        |
//! | `is_transient_cat`| `notification.received` with progress / service cat |
//!
//! ## What passes through (default allow)
//!
//! Everything else, including: `notification.received` from real apps
//! (WhatsApp / WeChat / mail / calendar), `notification.dismissed`,
//! `sms.received`, `sms.send.result`, `sms.list.result`,
//! `call.incoming`, `call.state`, `device.hello`. A user who wants
//! to drop *more* events needs to add a predicate to
//! [`NOISE_FILTERS`] (or open a feature request — a small TOML-driven
//! rules engine is intentionally **out of scope** for v1).
//!
//! ## Design notes
//!
//! - **No config**: predicates are hardcoded. This is deliberate — the
//!   filter is a small static allow-on-default set of known-noise
//!   patterns; making it configurable would invite subtle
//!   mis-configurations for little gain.
//! - **First-match wins**: predicates are checked in array order.
//!   The 4 kind-based predicates come first (cheap, exact), the 3
//!   payload-based predicates last (require a string lookup). Adding
//!   a new predicate is just an `&[..]` entry.
//! - **Case-sensitive** for `package` and `category`: Android
//!   package names are lowercase by convention and the category
//!   strings are drawn from a small fixed set; we avoid the cost of
//!   a unicode-aware case fold.
//! - **Field-missing safety**: a `payload` missing `package` or
//!   `category` is treated as "no match" — the predicate returns
//!   `false` and the event passes through. Better to over-show than
//!   to crash on a malformed event.

#![forbid(unsafe_code)]
#![allow(missing_docs)]

use phonebridge_proto::DisplayEvent;

/// Our own Android application package names, both the release
/// (`im.zyx.phonebridge`) and the debug variant
/// (`im.zyx.phonebridge.debug`). We do not want our own
/// notifications to flash the desktop — the user can see them on
/// the phone. Exposed for tests.
pub const OWN_PACKAGES: &[&str] = &["im.zyx.phonebridge", "im.zyx.phonebridge.debug"];

/// Android system packages whose notifications carry no useful
/// content. `com.android.systemui` posts the "Charging" / "USB
/// debugging connected" style toast, `com.android.shell` posts
/// `adb shell`-issued notifications during dev, the
/// `com.android.providers.downloads` package posts the
/// "Downloading..." flow, and the bare `android` namespace catches
/// anything that escapes the manifest-defined packages.
/// Exposed for tests.
pub const SYSTEM_NOISE_PACKAGES: &[&str] = &[
    "com.android.systemui",
    "com.android.shell",
    "com.android.providers.downloads",
    "android",
];

/// Android notification categories that are transient and would
/// only briefly flash the desktop (progress bars, foreground
/// services, transport events, status bar housekeeping).
/// Exposed for tests.
pub const TRANSIENT_CATEGORIES: &[&str] = &["progress", "service", "transport", "status"];

/// Hardcoded deny predicates. Each entry is a name (used in
/// tracing / logs) and a function returning `true` if the event
/// should be dropped. Evaluated in array order; the first match
/// wins. **No match → event passes through** (default allow).
///
/// Add a new noise class by appending a tuple here. The
/// [`should_filter`] function does not need to be edited.
#[allow(clippy::type_complexity)]
const NOISE_FILTERS: &[(&str, fn(&DisplayEvent) -> bool)] = &[
    // ---- kind-based: cheap, no payload access ----
    ("is_heartbeat", |e| e.kind == "device.heartbeat"),
    ("is_pair_internal", |e| e.kind.starts_with("device.pair.")),
    ("is_info_update", |e| e.kind == "device.info.update"),
    ("is_unpair", |e| e.kind == "device.unpair"),
    // ---- payload-based: require string lookup on payload ----
    ("is_own_pkg", |e| {
        e.kind == "notification.received"
            && payload_str(&e.payload, "package")
                .map(|p| OWN_PACKAGES.contains(&p))
                .unwrap_or(false)
    }),
    ("is_sys_noise", |e| {
        e.kind == "notification.received"
            && payload_str(&e.payload, "package")
                .map(|p| SYSTEM_NOISE_PACKAGES.contains(&p))
                .unwrap_or(false)
    }),
    ("is_transient_cat", |e| {
        e.kind == "notification.received"
            && payload_str(&e.payload, "category")
                .map(|c| TRANSIENT_CATEGORIES.contains(&c))
                .unwrap_or(false)
    }),
];

/// Read a string field from the `DisplayEvent.payload` JSON, returning
/// `None` if the field is missing or not a string.
fn payload_str<'a>(payload: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    payload.get(key).and_then(|v| v.as_str())
}

/// Return the name of the first matching deny predicate, or
/// `None` if the event should pass through to the display bus.
///
/// Returning `None` is the **default** — most events should pass
/// through, the filter is a small allow-list in the deny direction.
pub fn should_filter(event: &DisplayEvent) -> Option<&'static str> {
    for (name, pred) in NOISE_FILTERS {
        if pred(event) {
            return Some(*name);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use phonebridge_proto::DisplayEvent;
    use serde_json::{json, Value};
    use uuid::Uuid;

    fn ev(kind: &str, payload: Value) -> DisplayEvent {
        DisplayEvent {
            kind: kind.into(),
            device_id: Uuid::new_v4(),
            envelope_id: Uuid::new_v4(),
            timestamp: 0,
            payload,
            summary: Default::default(),
        }
    }

    // =========================================================================
    // 4 kind-based deny predicates
    // =========================================================================

    #[test]
    fn heartbeat_filtered() {
        let e = ev("device.heartbeat", json!({}));
        assert_eq!(should_filter(&e), Some("is_heartbeat"));
    }

    #[test]
    fn pair_internal_filtered_all_subkinds() {
        for kind in [
            "device.pair.request",
            "device.pair.challenge",
            "device.pair.confirm",
            "device.pair.accept",
            "device.pair.reject",
            "device.pair.complete",
        ] {
            let e = ev(kind, json!({}));
            assert_eq!(
                should_filter(&e),
                Some("is_pair_internal"),
                "expected {kind} to be filtered"
            );
        }
    }

    #[test]
    fn info_update_filtered() {
        let e = ev("device.info.update", json!({}));
        assert_eq!(should_filter(&e), Some("is_info_update"));
    }

    #[test]
    fn unpair_filtered() {
        let e = ev("device.unpair", json!({}));
        assert_eq!(should_filter(&e), Some("is_unpair"));
    }

    // =========================================================================
    // 3 payload-based deny predicates
    // =========================================================================

    #[test]
    fn own_pkg_filtered_debug_variant() {
        let e = ev(
            "notification.received",
            json!({"package": "im.zyx.phonebridge.debug"}),
        );
        assert_eq!(should_filter(&e), Some("is_own_pkg"));
    }

    #[test]
    fn own_pkg_filtered_release_variant() {
        let e = ev(
            "notification.received",
            json!({"package": "im.zyx.phonebridge"}),
        );
        assert_eq!(should_filter(&e), Some("is_own_pkg"));
    }

    #[test]
    fn sys_noise_filtered_systemui() {
        let e = ev(
            "notification.received",
            json!({"package": "com.android.systemui", "title": "Charging"}),
        );
        assert_eq!(should_filter(&e), Some("is_sys_noise"));
    }

    #[test]
    fn sys_noise_filtered_shell() {
        let e = ev(
            "notification.received",
            json!({"package": "com.android.shell"}),
        );
        assert_eq!(should_filter(&e), Some("is_sys_noise"));
    }

    #[test]
    fn sys_noise_filtered_downloads() {
        let e = ev(
            "notification.received",
            json!({"package": "com.android.providers.downloads"}),
        );
        assert_eq!(should_filter(&e), Some("is_sys_noise"));
    }

    #[test]
    fn sys_noise_filtered_bare_android_namespace() {
        let e = ev("notification.received", json!({"package": "android"}));
        assert_eq!(should_filter(&e), Some("is_sys_noise"));
    }

    #[test]
    fn transient_category_progress_filtered() {
        let e = ev(
            "notification.received",
            json!({"package": "com.example.app", "category": "progress"}),
        );
        assert_eq!(should_filter(&e), Some("is_transient_cat"));
    }

    #[test]
    fn transient_category_service_filtered() {
        let e = ev(
            "notification.received",
            json!({"package": "com.example.app", "category": "service"}),
        );
        assert_eq!(should_filter(&e), Some("is_transient_cat"));
    }

    // =========================================================================
    // Default-allow behaviour: events that should pass through
    // =========================================================================

    #[test]
    fn normal_notification_passes() {
        let e = ev(
            "notification.received",
            json!({"package": "com.whatsapp", "title": "Alice", "content": "Hey"}),
        );
        assert_eq!(should_filter(&e), None);
    }

    #[test]
    fn sms_passes() {
        let e = ev(
            "sms.received",
            json!({"address": "+8613800138000", "body": "hi"}),
        );
        assert_eq!(should_filter(&e), None);
    }

    #[test]
    fn call_incoming_passes() {
        let e = ev(
            "call.incoming",
            json!({"phone_number": "+8613800138000", "contact_name": "Alice"}),
        );
        assert_eq!(should_filter(&e), None);
    }

    #[test]
    fn call_state_passes() {
        let e = ev(
            "call.state",
            json!({"state": "offhook", "phone_number": "+8613800138000"}),
        );
        assert_eq!(should_filter(&e), None);
    }

    #[test]
    fn sms_send_result_passes() {
        let e = ev(
            "sms.send.result",
            json!({"request_id": Uuid::new_v4(), "ok": true}),
        );
        assert_eq!(should_filter(&e), None);
    }

    #[test]
    fn notification_dismissed_passes() {
        let e = ev("notification.dismissed", json!({"id": "notif-1"}));
        assert_eq!(should_filter(&e), None);
    }

    #[test]
    fn device_hello_passes() {
        let e = ev(
            "device.hello",
            json!({"name": "phone", "device_type": "android", "protocol_version": 1, "pubkey": ""}),
        );
        assert_eq!(should_filter(&e), None);
    }

    // =========================================================================
    // Edge cases
    // =========================================================================

    #[test]
    fn missing_fields_in_notification_dont_panic_and_pass() {
        // `notification.received` with no `package` or `category`:
        // payload-based predicates all return false; event passes.
        let e = ev("notification.received", json!({"title": "orphan"}));
        assert_eq!(should_filter(&e), None);
    }

    #[test]
    fn non_string_package_field_falls_through() {
        // Defensive: a payload where `package` is a number (malformed
        // upstream) should not panic; we treat it as "no match".
        let e = ev(
            "notification.received",
            json!({"package": 12345, "title": "weird"}),
        );
        assert_eq!(should_filter(&e), None);
    }

    #[test]
    fn non_string_category_field_falls_through() {
        let e = ev(
            "notification.received",
            json!({"package": "com.example", "category": ["array"]}),
        );
        assert_eq!(should_filter(&e), None);
    }

    #[test]
    fn empty_payload_object_passes() {
        let e = ev("device.hello", json!({}));
        assert_eq!(should_filter(&e), None);
    }

    #[test]
    fn unknown_event_kind_passes() {
        // Future kinds we haven't filtered: pass through.
        let e = ev("some.future.kind", json!({}));
        assert_eq!(should_filter(&e), None);
    }

    // =========================================================================
    // First-match semantics
    // =========================================================================

    #[test]
    fn first_match_wins_among_kind_filters() {
        // `device.pair.request` could be matched by `is_pair_internal`
        // (first) or by some hypothetical later predicate. We only have
        // kind-based predicates that are mutually exclusive, so verify
        // the actual match: pair_internal.
        let e = ev("device.pair.request", json!({}));
        assert_eq!(should_filter(&e), Some("is_pair_internal"));
    }

    #[test]
    fn kind_filter_takes_priority_over_payload_filter() {
        // Even if we also had a payload filter for some hypothetical
        // case, the kind filter runs first and wins. Here we just
        // verify that a notification with our own package *but* with
        // kind="sms.received" (which is not notification.received) is
        // NOT filtered by is_own_pkg.
        let e = ev(
            "sms.received",
            json!({"package": "im.zyx.phonebridge.debug", "address": "+86138"}),
        );
        assert_eq!(should_filter(&e), None);
    }

    // =========================================================================
    // Constants sanity (guard against typos)
    // =========================================================================

    #[test]
    fn constants_contain_expected_entries() {
        assert!(OWN_PACKAGES.contains(&"im.zyx.phonebridge"));
        assert!(OWN_PACKAGES.contains(&"im.zyx.phonebridge.debug"));
        assert!(SYSTEM_NOISE_PACKAGES.contains(&"com.android.systemui"));
        assert!(TRANSIENT_CATEGORIES.contains(&"progress"));
        assert!(TRANSIENT_CATEGORIES.contains(&"service"));
    }

    #[test]
    fn filter_array_has_seven_entries() {
        // This is a regression guard: if someone adds a new predicate,
        // they should be aware they're changing the count. If the
        // count is intentional (e.g. an 8th filter is added), update
        // this test to match.
        assert_eq!(NOISE_FILTERS.len(), 7, "expected 7 deny predicates");
    }
}
