// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 PhoneBridge Contributors
//
// This file is part of PhoneBridge. See LICENSE and the dual-licensing
// notice in README.md for details.

//! Minimal i18n for the desktop notification surface.
//!
//! The strings here are intentionally small — only the
//! labels we inject into the OS notification (action
//! buttons, app name, generic headers) are translated. The
//! actual notification *body* comes from the phone, in the
//! language the sender used.
//!
//! Lookup priority:
//! 1. Strings from the daemon (fetched via
//!    [`DisplayConfig::i18n_url`]) keyed by the locale
//!    supplied by the user / detected from the environment.
//! 2. Built-in fallback for `zh` and `en` (so the app stays
//!    usable when the daemon is offline at startup).
//!
//! We deliberately do NOT use the web console's i18n
//! dictionary because the keys are different surfaces
//! (the console has nav / table headers, the display
//! surface only has notification labels).

use std::collections::HashMap;

use serde::Deserialize;

use crate::config::DisplayConfig;

#[derive(Debug, Clone, Default, Deserialize)]
struct I18nResponse {
    #[serde(default)]
    default_locale: Option<String>,
    #[serde(default)]
    dictionary: HashMap<String, String>,
}

#[derive(Debug, Clone, Default)]
pub struct DisplayI18n {
    dict: HashMap<String, String>,
}

impl DisplayI18n {
    /// Fetch the dictionary from the daemon. The
    /// `requested_locale` is sent to the daemon; on success
    /// we use the response, on failure we fall back to the
    /// built-in `en`.
    pub async fn load(cfg: &DisplayConfig) -> Self {
        let url = match cfg.i18n_url(&cfg.locale) {
            Ok(u) => u,
            Err(_) => return Self::builtin_en(),
        };
        match reqwest::Client::new()
            .get(url)
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await
        {
            Ok(resp) => match resp.json::<I18nResponse>().await {
                Ok(body) => {
                    let mut s = DisplayI18n { dict: body.dictionary };
                    // Merge in built-in fallbacks for any
                    // missing key — ensures we never display
                    // a raw `notif.action.reply` key.
                    for (k, v) in builtin_en_dict() {
                        s.dict.entry(k).or_insert(v);
                    }
                    s
                }
                Err(_) => Self::builtin_en(),
            },
            Err(_) => Self::builtin_en(),
        }
    }

    /// Built-in English dictionary (used as the
    /// unreachable-daemon fallback).
    pub fn builtin_en() -> Self {
        DisplayI18n { dict: builtin_en_dict() }
    }

    /// Built-in Chinese dictionary (used as the
    /// unreachable-daemon fallback when the user has
    /// LANG=zh).
    pub fn builtin_zh() -> Self {
        DisplayI18n { dict: builtin_zh_dict() }
    }

    /// Detect the locale from environment variables in
    /// priority `LC_ALL` → `LC_MESSAGES` → `LANG` and
    /// return either the `zh` or `en` built-in.
    pub fn detect_from_env() -> Self {
        let raw = std::env::var("LC_ALL")
            .or_else(|_| std::env::var("LC_MESSAGES"))
            .or_else(|_| std::env::var("LANG"))
            .unwrap_or_default();
        if raw.to_lowercase().starts_with("zh") {
            Self::builtin_zh()
        } else {
            Self::builtin_en()
        }
    }

    pub fn t<'a>(&'a self, key: &'a str) -> &'a str {
        self.dict.get(key).map(String::as_str).unwrap_or(key)
    }

    /// Number of keys loaded (used by tests).
    pub fn len(&self) -> usize {
        self.dict.len()
    }
}

fn builtin_en_dict() -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert("notif.app_name".into(), "PhoneBridge".into());
    m.insert("notif.action.reply".into(), "Reply".into());
    m.insert("notif.action.mark_read".into(), "Mark as read".into());
    m.insert("notif.action.dismiss".into(), "Dismiss".into());
    m.insert("notif.action.answer".into(), "Answer".into());
    m.insert("notif.action.hangup".into(), "Hang up".into());
    m.insert("notif.sms.incoming".into(), "SMS from {address}".into());
    m.insert("notif.call.incoming".into(), "Incoming call from {number}".into());
    m.insert("notif.call.ongoing".into(), "Ongoing call".into());
    m.insert("prompt.reply.title".into(), "Reply to {address}".into());
    m.insert("prompt.reply.label".into(), "Message".into());
    m.insert("toast.action_sent".into(), "Reply sent".into());
    m.insert("toast.action_failed".into(), "Action failed: {message}".into());
    m.insert("toast.phone_offline".into(), "Phone offline; action not delivered".into());
    m
}

fn builtin_zh_dict() -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert("notif.app_name".into(), "PhoneBridge".into());
    m.insert("notif.action.reply".into(), "回复".into());
    m.insert("notif.action.mark_read".into(), "标记已读".into());
    m.insert("notif.action.dismiss".into(), "忽略".into());
    m.insert("notif.action.answer".into(), "接听".into());
    m.insert("notif.action.hangup".into(), "挂断".into());
    m.insert("notif.sms.incoming".into(), "{address} 的短信".into());
    m.insert("notif.call.incoming".into(), "{number} 来电".into());
    m.insert("notif.call.ongoing".into(), "通话中".into());
    m.insert("prompt.reply.title".into(), "回复 {address}".into());
    m.insert("prompt.reply.label".into(), "消息".into());
    m.insert("toast.action_sent".into(), "回复已发送".into());
    m.insert("toast.action_failed".into(), "操作失败：{message}".into());
    m.insert("toast.phone_offline".into(), "手机离线，操作未送达".into());
    m
}

/// Apply simple `{key}` substitution to a template. Unknown
/// placeholders are left in place.
pub fn render(template: &str, vars: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let bytes = template.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            if let Some(close) = template[i + 1..].find('}') {
                let key = &template[i + 1..i + 1 + close];
                if let Some((_, v)) = vars.iter().find(|(k, _)| *k == key) {
                    out.push_str(v);
                } else {
                    out.push('{');
                    out.push_str(key);
                    out.push('}');
                }
                i += 1 + close + 1;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_substitutes() {
        let s = render("hello {name}!", &[("name", "world")]);
        assert_eq!(s, "hello world!");
    }

    #[test]
    fn render_leaves_unknown() {
        let s = render("a {x} b {y} c", &[("x", "X")]);
        assert_eq!(s, "a X b {y} c");
    }

    #[test]
    fn render_handles_partial_brace() {
        let s = render("a { b", &[]);
        assert_eq!(s, "a { b");
    }

    #[test]
    fn builtin_en_has_required_keys() {
        let m = builtin_en_dict();
        for k in [
            "notif.app_name",
            "notif.action.reply",
            "notif.action.mark_read",
            "notif.action.dismiss",
            "notif.action.answer",
            "notif.action.hangup",
            "notif.sms.incoming",
            "notif.call.incoming",
            "toast.action_sent",
            "toast.action_failed",
        ] {
            assert!(m.contains_key(k), "missing key {k}");
        }
    }

    #[test]
    fn builtin_zh_has_required_keys() {
        let m = builtin_zh_dict();
        for k in [
            "notif.app_name",
            "notif.action.reply",
            "notif.action.mark_read",
            "notif.action.dismiss",
            "notif.action.answer",
            "notif.action.hangup",
            "notif.sms.incoming",
            "notif.call.incoming",
            "toast.action_sent",
            "toast.action_failed",
        ] {
            assert!(m.contains_key(k), "missing key {k}");
        }
    }

    #[test]
    fn detect_picks_zh_for_builtin_zh() {
        // We don't touch env in tests (Rust 1.78+ made
        // env::set_var unsafe and #![forbid(unsafe_code)]
        // makes it awkward). Instead, just verify the
        // built-in Chinese dict is wired correctly.
        let i = DisplayI18n::builtin_zh();
        assert_eq!(i.t("notif.action.reply"), "回复");
    }

    #[test]
    fn detect_picks_en_for_builtin_en() {
        let i = DisplayI18n::builtin_en();
        assert_eq!(i.t("notif.action.reply"), "Reply");
    }

    #[test]
    fn builtin_keys_match_count() {
        // Guard against accidental divergence between zh
        // and en key sets.
        assert_eq!(builtin_en_dict().len(), builtin_zh_dict().len());
    }
}
