// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 PhoneBridge Contributors
//
// This file is part of PhoneBridge. See LICENSE and the dual-licensing
// notice in README.md for details.

//! Server-side i18n dictionary for the embedded web console.
//!
//! The web console used to ship a hard-coded Chinese / English
//! dictionary in the JS bundle. That has two problems:
//!
//! 1. **Stale strings**: every UI tweak required a full
//!    `npm run build` + message-center restart cycle just to flip a
//!    word. The strings are user-facing and iterate fast, so
//!    this is a productivity hit.
//! 2. **Language proliferation**: the day someone wanted
//!    Japanese or Spanish, we'd be back to "edit, rebuild,
//!    redeploy, restart". The console is supposed to be
//!    available in the user's preferred language without
//!    shipping a separate build for each one.
//!
//! Moving the strings to the message-center makes them runtime
//! resources that can be updated, added to, or replaced
//! without touching the JS bundle. The frontend fetches the
//! dictionary at startup (and again after a locale change),
//! and a full page reload is the simplest way to make sure
//! every component re-renders against the new strings.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Router,
};
use serde::Serialize;
use std::collections::BTreeMap;

use crate::app_state::AppState;

/// One translation dictionary, as flat key→string map. The keys
/// are dot-separated (e.g. "nav.dashboard") so the frontend can
/// type them as a TypeScript-style string-literal union.
type Dict = BTreeMap<String, String>;

/// Helper: build a dictionary from a list of (key, value) pairs.
/// Using BTreeMap (not HashMap) gives us stable JSON output
/// order, which makes the bundle deterministic for debugging.
fn dict(entries: &[(&str, &str)]) -> Dict {
    entries
        .iter()
        .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
        .collect()
}

fn zh() -> Dict {
    dict(&[
        // nav
        ("nav.dashboard", "仪表盘"),
        ("nav.devices", "设备"),
        ("nav.pairings", "配对"),
        ("nav.notifications", "通知"),
        ("nav.sms", "短信"),
        ("nav.calls", "通话"),
        ("nav.settings", "设置"),
        ("nav.about", "关于"),
        // layout
        ("layout.console_subtitle", "Web 控制台"),
        ("layout.online_count", "{online}/{paired} 在线"),
        // dashboard
        ("dashboard.loading", "正在加载仪表盘…"),
        ("dashboard.load_error", "加载失败"),
        ("dashboard.title", "仪表盘"),
        ("dashboard.subtitle", "所有已配对 Android 设备的实时状态。"),
        ("dashboard.paired", "已配对设备"),
        ("dashboard.online", "当前在线"),
        ("dashboard.unread", "未读通知"),
        ("dashboard.unread_hint", "共 {n} 条"),
        ("dashboard.sms_conv", "短信会话"),
        ("dashboard.sms_hint", "共 {n} 条消息"),
        ("dashboard.calls_24h", "通话（近 24 小时）"),
        ("dashboard.calls_hint", "{ringing} 响铃中 · {missed} 未接"),
        ("dashboard.recent_notifs", "最近通知"),
        ("dashboard.recent_sms", "最近短信"),
        ("dashboard.recent_calls", "最近通话"),
        ("dashboard.no_notifs", "暂无通知。"),
        ("dashboard.no_sms", "暂无短信。"),
        ("dashboard.no_calls", "暂无通话。"),
        ("dashboard.view_all", "查看全部 →"),
        // live
        ("live.title", "实时活动"),
        ("live.live", "已连接"),
        ("live.disconnected", "已断开"),
        ("live.streamed_via", "通过 /ws/console 实时推送。"),
        ("live.online_part", "当前 {online}/{paired} 在线。"),
        ("live.empty", "暂无事件。连接 Android 设备后，通知、短信和通话会实时显示在这里。"),
        ("live.ev_console_hello", "已连接（{server} v{version}）"),
        ("live.ev_device_hello", "设备 {name} 已连接"),
        ("live.ev_notif", "[{app}] {title}"),
        ("live.ev_sms", "来自 {addr} 的短信：{body}"),
        ("live.ev_call_incoming", "通话：呼入"),
        ("live.ev_call_state", "通话：状态变化"),
        ("live.ev_unpair", "设备已取消配对"),
        // devices
        ("devices.title", "设备"),
        ("devices.subtitle", "与本守护进程配对的 Android 客户端。当前在线 {n} 台。"),
        ("devices.col.name", "名称"),
        ("devices.col.paired", "配对状态"),
        ("devices.col.last_seen", "最后在线"),
        ("devices.loading", "加载中…"),
        ("devices.empty", "暂无设备。请打开 PhoneBridge Android 客户端发起配对。"),
        ("devices.badge_paired", "已配对"),
        ("devices.badge_discovered", "已发现"),
        ("devices.unpair_btn", "取消配对"),
        ("devices.unpair_confirm", "确定要取消配对 {name} 吗？"),
        ("devices.unpair_btn_paired", "已配对"),
        ("devices.btn_pair", "配对"),
        ("devices.btn_paired", "已配对"),
        // pairings
        ("pairings.title", "配对"),
        ("pairings.subtitle", "点击下方设备的 配对 按钮，Android 端会显示一个 4 位配对码；在此输入该码并点击 接受 即完成配对。"),
        ("pairings.subtitle_hint", "配对码由 Android 客户端使用 ECDH P-256 + HKDF-SHA256 从共享密钥派生（参见 docs/protocol-v1.md §4.2）。"),
        ("pairings.col.device", "设备"),
        ("pairings.col.status", "配对状态"),
        ("pairings.col.action", "操作"),
        ("pairings.loading", "加载中…"),
        ("pairings.empty", "暂无设备。请打开 PhoneBridge Android 客户端，让其出现在此列表中。"),
        ("pairings.badge_paired", "已配对"),
        ("pairings.badge_unpaired", "未配对"),
        ("pairings.btn_pair", "配对"),
        ("pairings.btn_paired", "已配对"),
        ("pairings.waiting_hint", "等待手机确认"),
        ("pairings.waiting_cancel", "取消配对"),
        ("pairings.start_info", "已向 Android 设备 {name} 发送配对请求，请在该设备的屏幕上：核对显示的 4 位验证码并点击 接受 完成配对。"),
        ("pairings.incoming_title", "入站配对请求"),
        ("pairings.incoming_hint", "请确认是您本人的手机发起的请求。接受后无需输入任何验证码。"),
        ("pairings.incoming_accept", "接受"),
        ("pairings.incoming_reject", "拒绝"),
        // notifications
        ("notif.title", "通知"),
        ("notif.subtitle", "从 Android 设备同步的全部通知。点击 关闭 可在设备上移除该通知（同时在守护进程中标记为已读）。"),
        ("notif.unread_only", "仅未读"),
        ("notif.filter_placeholder", "按应用包名筛选…"),
        ("notif.loading", "加载中…"),
        ("notif.empty", "没有匹配的通知。"),
        ("notif.sensitive", "敏感"),
        ("notif.hidden", "（内容已隐藏）"),
        ("notif.mark_read", "标记已读"),
        ("notif.dismiss", "关闭"),
        ("notif.dismissing", "关闭中…"),
        ("notif.dismiss_title", "向设备发送 notification.dismissed"),
        // sms
        ("sms.title", "短信"),
        ("sms.subtitle", "通过 Android 设备查看与发送短信。"),
        ("sms.all_devices", "全部设备"),
        ("sms.conversations", "会话（{n}）"),
        ("sms.no_conversations", "暂无会话。"),
        ("sms.pick_conversation", "请选择一个会话"),
        ("sms.no_conversation_selected", "请在左侧选择一个会话。"),
        ("sms.no_messages", "该会话暂无消息。"),
        ("sms.placeholder_to", "收件人"),
        ("sms.placeholder_body", "消息内容…"),
        ("sms.send_via", "将通过 {name} 发送。Android 端确认后此会话会自动刷新。"),
        ("sms.send", "发送"),
        ("sms.select_device_first", "请先选择一台设备"),
        // calls
        ("calls.title", "通话"),
        ("calls.subtitle", "来自 Android 设备的近期通话记录。也可以在此发起外呼。"),
        ("calls.place_call", "拨打电话"),
        ("calls.device", "设备"),
        ("calls.number", "号码"),
        ("calls.dial", "拨打"),
        ("calls.col.time", "时间"),
        ("calls.col.number", "号码"),
        ("calls.col.direction", "方向"),
        ("calls.col.state", "状态"),
        ("calls.col.duration", "时长"),
        ("calls.col.sim", "SIM 卡"),
        ("calls.loading", "加载中…"),
        ("calls.empty", "暂无通话。"),
        ("calls.dir_in", "呼入"),
        ("calls.dir_out", "呼出"),
        ("calls.dir_missed", "未接"),
        // settings
        ("settings.title", "设置"),
        ("settings.subtitle", "控制台语言与近期审计日志。"),
        ("settings.col.time", "时间"),
        ("settings.col.event", "事件"),
        ("settings.col.device", "设备"),
        ("settings.col.detail", "详情"),
        ("settings.loading", "加载中…"),
        ("settings.audit_title", "近期审计日志"),
        ("settings.audit_empty", "暂无记录。"),
        ("settings.language", "界面语言"),
        ("settings.language_hint", "选择中文或英文，下次访问自动记忆。"),
        ("settings.copy", "复制"),
        ("settings.copied", "已复制"),
        // about
        ("about.title", "关于"),
        ("about.subtitle", "本守护进程的身份信息与外部链接。"),
        ("about.identity", "本守护进程"),
        ("about.col.id", "设备 ID"),
        ("about.col.name", "名称"),
        ("about.col.fingerprint", "TLS 指纹（SHA-256）"),
        ("about.col.pubkey", "公钥（P-256, base64）"),
        ("about.col.version", "版本"),
        ("about.api_docs", "API 文档（Swagger UI）"),
        ("about.api_docs_hint", "打开新的标签页，浏览 REST + WebSocket 端点的 OpenAPI 定义。"),
        ("about.source", "源代码"),
        ("about.license", "许可证：GPL-3.0-or-later"),
        // language
        ("lang.zh", "中文"),
        ("lang.en", "English"),
    ])
}

fn en() -> Dict {
    dict(&[
        // nav
        ("nav.dashboard", "Dashboard"),
        ("nav.devices", "Devices"),
        ("nav.pairings", "Pairings"),
        ("nav.notifications", "Notifications"),
        ("nav.sms", "SMS"),
        ("nav.calls", "Calls"),
        ("nav.settings", "Settings"),
        ("nav.about", "About"),
        // layout
        ("layout.console_subtitle", "Web Console"),
        ("layout.online_count", "{online}/{paired} online"),
        // dashboard
        ("dashboard.loading", "Loading dashboard…"),
        ("dashboard.load_error", "Failed to load"),
        ("dashboard.title", "Dashboard"),
        ("dashboard.subtitle", "Live snapshot of all paired Android devices."),
        ("dashboard.paired", "Paired devices"),
        ("dashboard.online", "Online now"),
        ("dashboard.unread", "Unread notifications"),
        ("dashboard.unread_hint", "{n} total"),
        ("dashboard.sms_conv", "SMS conversations"),
        ("dashboard.sms_hint", "{n} messages"),
        ("dashboard.calls_24h", "Calls (24h)"),
        ("dashboard.calls_hint", "{ringing} ringing · {missed} missed"),
        ("dashboard.recent_notifs", "Recent notifications"),
        ("dashboard.recent_sms", "Recent SMS"),
        ("dashboard.recent_calls", "Recent calls"),
        ("dashboard.no_notifs", "No notifications yet."),
        ("dashboard.no_sms", "No SMS yet."),
        ("dashboard.no_calls", "No calls yet."),
        ("dashboard.view_all", "View all →"),
        // live
        ("live.title", "Live activity"),
        ("live.live", "live"),
        ("live.disconnected", "disconnected"),
        ("live.streamed_via", "Streamed via /ws/console."),
        ("live.online_part", "{online}/{paired} online."),
        ("live.empty", "No events yet. Connect an Android device to see notifications, SMS, and calls stream in here."),
        ("live.ev_console_hello", "connected ({server} v{version})"),
        ("live.ev_device_hello", "device {name} connected"),
        ("live.ev_notif", "[{app}] {title}"),
        ("live.ev_sms", "SMS from {addr}: {body}"),
        ("live.ev_call_incoming", "call: incoming"),
        ("live.ev_call_state", "call: state change"),
        ("live.ev_unpair", "device unpaired"),
        // devices
        ("devices.title", "Devices"),
        ("devices.subtitle", "Android clients paired with this message-center. {n} currently online."),
        ("devices.col.name", "Name"),
        ("devices.col.paired", "Paired"),
        ("devices.col.last_seen", "Last seen"),
        ("devices.loading", "Loading…"),
        ("devices.empty", "No devices yet. Open the PhoneBridge Android app to pair."),
        ("devices.badge_paired", "paired"),
        ("devices.badge_discovered", "discovered"),
        ("devices.unpair_btn", "Unpair"),
        ("devices.unpair_confirm", "Unpair {name}?"),
        ("devices.unpair_btn_paired", "paired"),
        ("devices.btn_pair", "Pair"),
        ("devices.btn_paired", "paired"),
        // pairings
        ("pairings.title", "Pairings"),
        ("pairings.subtitle", "Click Pair on a device below. A 4-digit code will appear on the Android device; click Accept on the phone to complete pairing."),
        ("pairings.subtitle_hint", "The code is derived from ECDH P-256 + HKDF-SHA256 on the shared secret (see docs/protocol-v1.md §4.2)."),
        ("pairings.col.device", "Device"),
        ("pairings.col.status", "Paired"),
        ("pairings.col.action", "Action"),
        ("pairings.loading", "Loading…"),
        ("pairings.empty", "No devices yet. Open the PhoneBridge Android app to make it appear here."),
        ("pairings.badge_paired", "paired"),
        ("pairings.badge_unpaired", "unpaired"),
        ("pairings.btn_pair", "Pair"),
        ("pairings.btn_paired", "Already paired"),
        ("pairings.waiting_hint", "Waiting for phone"),
        ("pairings.waiting_cancel", "Cancel"),
        ("pairings.start_info", "Pair request sent to {name}. The phone should show a 4-digit code; click Accept there to complete."),
        ("pairings.incoming_title", "Incoming pair requests"),
        ("pairings.incoming_hint", "Make sure this is your own phone. No verification code is required after Accept."),
        ("pairings.incoming_accept", "Accept"),
        ("pairings.incoming_reject", "Reject"),
        // notifications
        ("notif.title", "Notifications"),
        ("notif.subtitle", "All notifications synced from your Android devices. Click Dismiss to remove a notification from the device (also marks it read on the message-center)."),
        ("notif.unread_only", "Unread only"),
        ("notif.filter_placeholder", "Filter by app package…"),
        ("notif.loading", "Loading…"),
        ("notif.empty", "No notifications match."),
        ("notif.sensitive", "sensitive"),
        ("notif.hidden", "(content hidden)"),
        ("notif.mark_read", "Mark read"),
        ("notif.dismiss", "Dismiss"),
        ("notif.dismissing", "Dismissing…"),
        ("notif.dismiss_title", "Sends notification.dismissed to the device"),
        // sms
        ("sms.title", "SMS"),
        ("sms.subtitle", "Browse and send SMS via your Android devices."),
        ("sms.all_devices", "All devices"),
        ("sms.conversations", "Conversations ({n})"),
        ("sms.no_conversations", "No conversations yet."),
        ("sms.pick_conversation", "Select a conversation"),
        ("sms.no_conversation_selected", "Pick a conversation on the left."),
        ("sms.no_messages", "No messages in this conversation."),
        ("sms.placeholder_to", "To"),
        ("sms.placeholder_body", "Message…"),
        ("sms.send_via", "Sends via {name}. Conversation will refresh on Android confirm."),
        ("sms.send", "Send"),
        ("sms.select_device_first", "Select a device first"),
        // calls
        ("calls.title", "Calls"),
        ("calls.subtitle", "Recent call log from your Android devices. You can also place outgoing calls."),
        ("calls.place_call", "Place a call"),
        ("calls.device", "Device"),
        ("calls.number", "Number"),
        ("calls.dial", "Dial"),
        ("calls.col.time", "Time"),
        ("calls.col.number", "Number"),
        ("calls.col.direction", "Direction"),
        ("calls.col.state", "State"),
        ("calls.col.duration", "Duration"),
        ("calls.col.sim", "SIM"),
        ("calls.loading", "Loading…"),
        ("calls.empty", "No calls yet."),
        ("calls.dir_in", "in"),
        ("calls.dir_out", "out"),
        ("calls.dir_missed", "missed"),
        // settings
        ("settings.title", "Settings"),
        ("settings.subtitle", "Console language and recent audit log."),
        ("settings.col.time", "Time"),
        ("settings.col.event", "Event"),
        ("settings.col.device", "Device"),
        ("settings.col.detail", "Detail"),
        ("settings.loading", "Loading…"),
        ("settings.audit_title", "Recent audit log"),
        ("settings.audit_empty", "No entries yet."),
        ("settings.language", "Language"),
        ("settings.language_hint", "Choose Chinese or English. The choice is remembered for next time."),
        ("settings.copy", "Copy"),
        ("settings.copied", "Copied"),
        // about
        ("about.title", "About"),
        ("about.subtitle", "This message-center's identity and external links."),
        ("about.identity", "This message-center"),
        ("about.col.id", "Device id"),
        ("about.col.name", "Name"),
        ("about.col.fingerprint", "TLS fingerprint (SHA-256)"),
        ("about.col.pubkey", "Public key (P-256, base64)"),
        ("about.col.version", "Version"),
        ("about.api_docs", "API documentation (Swagger UI)"),
        ("about.api_docs_hint", "Opens in a new tab — browse the OpenAPI definition for the REST + WebSocket endpoints."),
        ("about.source", "Source code"),
        ("about.license", "License: GPL-3.0-or-later"),
        // language
        ("lang.zh", "中文"),
        ("lang.en", "English"),
    ])
}

/// The list of locales the message-center ships with. New languages are
/// added by extending [zh] / [en] and matching here.
const LOCALES: &[&str] = &["zh", "en"];

/**
 * Best-effort detection of the host system's preferred locale.
 *
 * The web console does not run inside a browser tab on the
 * message-center's host, so it cannot use `navigator.language` to pick
 * the right default. Instead we read the standard Unix
 * environment variables, in priority order:
 *
 *   1. `LC_ALL`   — explicit override, almost always set on
 *      purpose-built deployments.
 *   2. `LC_MESSAGES` — locale category for program messages.
 *   3. `LANG`     — the historical default; almost every
 *      desktop distro sets this (e.g. `zh_CN.UTF-8`).
 *
 * The raw value is typically a string like `zh_CN.UTF-8` or
 * `en_US.utf8`; we extract the language prefix (the part
 * before the first `_`) and check it against [LOCALES]. Anything
 * not shipped falls back to `en` so the console is never
 * stuck rendering raw i18n keys.
 *
 * The result is computed once at startup and shared via
 * [SystemLocale] (a `OnceLock`) — reading the env at request
 * time would be more flexible but also less deterministic for
 * tests and would re-do the same string parsing on every WS
 * ping.
 */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemLocale(pub &'static str);

impl SystemLocale {
    /// Compute the system locale at message-center startup. Safe to
    /// call multiple times — the result is memoized.
    pub fn detect() -> Self {
        use std::sync::OnceLock;
        static CACHE: OnceLock<SystemLocale> = OnceLock::new();
        *CACHE.get_or_init(|| SystemLocale(detect_inner()))
    }
}

fn detect_inner() -> &'static str {
    let raw = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .find_map(|k| std::env::var(k).ok())
        .unwrap_or_default();
    let lang = raw
        .split(['_', '.', '@'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match lang.as_str() {
        "zh" | "chinese" => "zh",
        // "en" matches `en`, `en_US`, `english`, etc.
        "en" | "english" => "en",
        // Unknown — fall back to English so the user sees
        // real strings rather than raw i18n keys.
        _ => "en",
    }
}

/// Response shape for `GET /api/v1/i18n?locale=…`.
#[derive(Serialize)]
struct I18nResponse {
    /// The locale the dictionary in this response is for.
    /// Equals the requested `?locale`, the resolved default
    /// (when no query param was supplied), or `en` as a final
    /// fallback if the requested locale is not shipped.
    locale: String,
    /// All key→string pairs. Variable placeholders use the
    /// `{name}` convention; the frontend's `t()` does the
    /// interpolation.
    strings: Dict,
    /// The list of locales the message-center knows about. The
    /// frontend uses this to populate the language picker.
    available: Vec<String>,
    /// The locale the message-center would pick by default — i.e. the
    /// system locale detected at startup. The frontend uses
    /// this as the initial selection on a fresh visit (before
    /// the user has picked anything in localStorage).
    default_locale: String,
}

#[derive(serde::Deserialize)]
struct I18nQuery {
    #[serde(default)]
    locale: Option<String>,
}

async fn get_i18n(State(_state): State<AppState>, Query(q): Query<I18nQuery>) -> impl IntoResponse {
    let default = SystemLocale::detect().0.to_string();
    // No `?locale` → fall through to the message-center's system default.
    // The frontend hits this path on a fresh visit when the user
    // has not yet chosen a language.
    let requested = q.locale.as_deref().unwrap_or(&default);
    let (locale, dict) = match requested {
        "zh" => ("zh", zh()),
        // Unknown / empty / unrecognised → English so the user
        // sees real strings rather than raw i18n keys.
        _ => ("en", en()),
    };
    (
        StatusCode::OK,
        axum::Json(I18nResponse {
            locale: locale.to_string(),
            strings: dict,
            available: LOCALES.iter().map(|s| s.to_string()).collect(),
            default_locale: default,
        }),
    )
}

/// Mount the `/api/v1/i18n` route. Pass to the message-center's
/// `build_router` like any other sub-router. Returns a
/// `Router<AppState>` even though the handler is stateless —
/// it lets us `.merge(...)` it with the rest router under the
/// same `/api/v1` mount.
pub fn router() -> Router<AppState> {
    Router::new().route("/i18n", get(get_i18n))
}
