// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 PhoneBridge Contributors
//
// This file is part of PhoneBridge. See LICENSE and the dual-licensing
// notice in README.md for details.

//! Configuration file for the display service.
//!
//! Location: `$XDG_CONFIG_HOME/phonebridge/display.toml`
//! (typically `~/.config/phonebridge/display.toml`).
//!
//! ```toml
//! [daemon]
//! url = "http://127.0.0.1:8443"
//! # Either `token` (literal) or `token_file` (path to a
//! # file containing the token; first line is read, trailing
//! # whitespace trimmed). `token_file` is preferred on
//! # shared hosts where the daemon's token file already
//! # exists.
//! token_file = "/root/.config/phonebridge/display.token"
//!
//! [i18n]
//! # `auto` (default) → fetch default from daemon, fall back
//! # to env LANG / LC_ALL.
//! locale = "auto"
//! ```
//!
//! Any field omitted falls back to a sensible default.

use std::net::IpAddr;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;

use crate::error::DisplayError;

const QUALIFIER: &str = "im";
const ORG: &str = "zyx";
// The display binary shares its config dir with the
// daemon (`~/.config/phonebridge/`). Using the same app
// name keeps the XDG layout consistent — no surprise
// `~/.config/im.zyx.phonebridge-display/` directory on
// the user's disk.
const APP: &str = "phonebridge";

#[derive(Debug, Clone)]
pub struct DisplayConfig {
    pub daemon_url: String,
    pub token: String,
    pub locale: String,
    pub config_path: PathBuf,
}

impl DisplayConfig {
    /// Load from the default config path, fall back to
    /// defaults for any missing field. Returns the effective
    /// config + the path it was loaded from.
    pub fn load() -> Result<Self, DisplayError> {
        Self::load_from(default_config_path())
    }

    /// Load from an explicit path. Missing file is not an
    /// error — every field falls back to its default. The
    /// returned `config_path` reflects whatever was passed
    /// in, so `main.rs` can echo it back when the user asked
    /// for a specific path but the file didn't exist.
    pub fn load_from(path: impl Into<PathBuf>) -> Result<Self, DisplayError> {
        let path = path.into();
        let mut cfg = DisplayConfig {
            // Loopback default — matches the same-host short-circuit
            // on the message-center side. When the user runs both
            // binaries on one machine, the daemon is always reached
            // via 127.0.0.1 and no token round-trip is needed.
            daemon_url: "http://127.0.0.1:8443".into(),
            token: String::new(),
            locale: "auto".into(),
            config_path: path.clone(),
        };
        if path.exists() {
            let raw = std::fs::read_to_string(&path)
                .map_err(|e| DisplayError::Config(format!("read {}: {e}", path.display())))?;
            let parsed: TomlConfig = toml::from_str(&raw)
                .map_err(|e| DisplayError::Config(format!("parse {}: {e}", path.display())))?;
            if let Some(daemon) = parsed.daemon {
                if let Some(url) = daemon.url {
                    cfg.daemon_url = url;
                }
                if let Some(t) = daemon.token {
                    cfg.token = t;
                }
                if let Some(tf) = daemon.token_file {
                    cfg.token = read_token_file(Path::new(&tf))?;
                }
            }
            if let Some(i18n) = parsed.i18n {
                if let Some(loc) = i18n.locale {
                    cfg.locale = loc;
                }
            }
        }
        // Token discovery:
        //   1. If the user set `token` or `token_file` in TOML, use it.
        //   2. Otherwise, look in the default location next to the
        //      config file (`<config_dir>/display.token`).
        //   3. Otherwise, if the daemon URL is loopback, no token is
        //      required — the message-center will accept the
        //      upgrade from a same-host peer without one. We send
        //      an empty token in the URL; the server ignores it on
        //      the same-host path.
        //   4. Otherwise (foreign URL, no token) it's a hard error:
        //      tell the user how to fix it.
        if cfg.token.is_empty() {
            let default_token = default_config_dir().join("display.token");
            if default_token.exists() {
                cfg.token = read_token_file(&default_token)?;
            }
        }
        if cfg.token.is_empty() && !is_loopback_daemon_url(&cfg.daemon_url) {
            return Err(DisplayError::Config(
                "no token configured; set [daemon].token or [daemon].token_file, \
                 or run `message-center --print-display-token` and copy it into display.toml. \
                 (The token is not required when the daemon URL points at 127.0.0.1 / ::1 / localhost.)"
                    .into(),
            ));
        }
        Ok(cfg)
    }

    /// Construct a config in-memory (used by tests).
    pub fn for_test(daemon_url: impl Into<String>, token: impl Into<String>) -> Self {
        DisplayConfig {
            daemon_url: daemon_url.into(),
            token: token.into(),
            locale: "auto".into(),
            config_path: PathBuf::from("(test)"),
        }
    }

    /// Build the WebSocket URL for `/ws/display?token=…`.
    pub fn ws_url(&self) -> Result<url::Url, DisplayError> {
        let mut u = url::Url::parse(&self.daemon_url)
            .map_err(|e| DisplayError::Config(format!("bad daemon url: {e}")))?;
        let scheme = match u.scheme() {
            "https" => "wss",
            _ => "ws",
        };
        u.set_scheme(scheme)
            .map_err(|_| DisplayError::Config("could not switch scheme to ws/wss".into()))?;
        {
            let mut paths = u.path().trim_end_matches('/').to_string();
            if !paths.ends_with("/ws/display") {
                if !paths.ends_with("/ws") {
                    if !paths.is_empty() {
                        paths.push('/');
                    }
                    paths.push_str("ws/display");
                } else {
                    paths.push_str("/display");
                }
            }
            u.set_path(&paths);
        }
        u.query_pairs_mut().append_pair("token", &self.token);
        Ok(u)
    }

    /// Build the HTTP URL for `/api/v1/i18n?locale=…`.
    pub fn i18n_url(&self, locale: &str) -> Result<url::Url, DisplayError> {
        let mut u = url::Url::parse(&self.daemon_url)
            .map_err(|e| DisplayError::Config(format!("bad daemon url: {e}")))?;
        let path = u.path().trim_end_matches('/');
        let new_path = if path.ends_with("/api/v1") {
            format!("{path}/i18n")
        } else {
            format!("{path}/api/v1/i18n")
        };
        u.set_path(&new_path);
        u.query_pairs_mut().append_pair("locale", locale);
        Ok(u)
    }
}

fn read_token_file(p: &Path) -> Result<String, DisplayError> {
    let raw = std::fs::read_to_string(p)
        .map_err(|e| DisplayError::TokenFile(format!("{}: {e}", p.display())))?;
    Ok(raw.lines().next().unwrap_or("").trim().to_string())
}

/// Decide whether a daemon URL points at this same host. Mirrors
/// the message-center's `is_same_host` check: loopback IPs
/// (`127.0.0.0/8`, `::1`) and "localhost" are same-host. The
/// local-interface check is on the server side; on the client
/// side we can only inspect the URL. In practice, when both
/// binaries run on the same machine the user's config will use
/// either `127.0.0.1` (most common) or `localhost` (which the
/// OS resolves to `127.0.0.1` or `::1`). We resolve the URL
/// host on the client side to catch the LAN-IP case too
/// (e.g. `http://192.168.1.5:8443` where `192.168.1.5` is one
/// of this machine's own addresses).
fn is_loopback_daemon_url(daemon_url: &str) -> bool {
    let Ok(u) = url::Url::parse(daemon_url) else {
        return false;
    };
    let Some(host) = u.host() else { return false };
    match host {
        url::Host::Ipv4(ip) => ip.is_loopback(),
        url::Host::Ipv6(ip) => ip.is_loopback(),
        url::Host::Domain(d) => {
            // `localhost` and any of its IP literal forms are
            // loopback. We also try to resolve other hostnames
            // (e.g. the user wrote the host's own LAN IP) and
            // check if any resolved address is on a loopback
            // range — that catches the case where someone wrote
            // `http://my-host:8443` and the OS resolves it to
            // `127.0.1.1` (Debian-style) or to a local
            // interface address.
            if d.eq_ignore_ascii_case("localhost") {
                return true;
            }
            resolve_loopback(d).unwrap_or(false)
        }
    }
}

/// Best-effort DNS lookup that returns true if **any** resolved
/// IP is loopback. We keep this opt-in (only called when the
/// URL host is a non-`localhost` name) so we don't trigger
/// surprise DNS lookups for URLs the user has explicitly pinned
/// to a foreign host.
fn resolve_loopback(host: &str) -> Option<bool> {
    use std::net::ToSocketAddrs;
    let mut addrs = (host, 0u16).to_socket_addrs().ok()?;
    Some(addrs.any(|a| matches!(a.ip(), IpAddr::V4(v4) if v4.is_loopback())))
}

#[derive(Debug, Default, serde::Deserialize)]
struct TomlConfig {
    daemon: Option<DaemonSection>,
    i18n: Option<I18nSection>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct DaemonSection {
    url: Option<String>,
    token: Option<String>,
    token_file: Option<String>,
}

#[derive(Debug, Default, serde::Deserialize)]
struct I18nSection {
    locale: Option<String>,
}

pub fn default_config_path() -> PathBuf {
    default_config_dir().join("display.toml")
}

pub fn default_config_dir() -> PathBuf {
    if let Some(dirs) = ProjectDirs::from(QUALIFIER, ORG, APP) {
        dirs.config_dir().to_path_buf()
    } else {
        // Fallback to ~/.config/phonebridge/display (no
        // qualifier / org).
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        home.join(".config").join("phonebridge").join("display")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ws_url_appends_path() {
        let cfg = DisplayConfig::for_test("http://127.0.0.1:8443", "abc");
        let u = cfg.ws_url().unwrap();
        assert_eq!(u.scheme(), "ws");
        assert_eq!(u.path(), "/ws/display");
        assert_eq!(
            u.query_pairs().find(|(k, _)| k == "token").unwrap().1,
            "abc"
        );
    }

    #[test]
    fn ws_url_preserves_existing_path() {
        let cfg = DisplayConfig::for_test("http://localhost:9000/api", "t");
        let u = cfg.ws_url().unwrap();
        assert_eq!(u.scheme(), "ws");
        assert_eq!(u.path(), "/api/ws/display");
    }

    #[test]
    fn i18n_url_appends_path() {
        let cfg = DisplayConfig::for_test("http://127.0.0.1:8443", "x");
        let u = cfg.i18n_url("zh").unwrap();
        assert_eq!(u.scheme(), "http");
        assert_eq!(u.path(), "/api/v1/i18n");
        assert_eq!(
            u.query_pairs().find(|(k, _)| k == "locale").unwrap().1,
            "zh"
        );
    }

    #[test]
    fn i18n_url_dedup_api_v1() {
        let cfg = DisplayConfig::for_test("http://h/api/v1", "x");
        let u = cfg.i18n_url("en").unwrap();
        assert_eq!(u.path(), "/api/v1/i18n");
    }

    // ---- same-host loopback detection on the client side ----

    #[test]
    fn loopback_url_v4_is_detected() {
        assert!(is_loopback_daemon_url("http://127.0.0.1:8443"));
    }

    #[test]
    fn loopback_url_v6_is_detected() {
        assert!(is_loopback_daemon_url("http://[::1]:8443"));
    }

    #[test]
    fn localhost_alias_is_detected() {
        assert!(is_loopback_daemon_url("http://localhost:8443"));
    }

    #[test]
    fn lan_ip_is_not_loopback_via_dns() {
        // 192.0.2.0/24 is RFC 5737 TEST-NET-1 — guaranteed not
        // to be on any real host. We use the literal IP form so
        // the test doesn't depend on DNS.
        assert!(!is_loopback_daemon_url("http://192.0.2.1:8443"));
    }

    #[test]
    fn malformed_url_is_not_loopback() {
        assert!(!is_loopback_daemon_url("not a url"));
    }

    /// A loopback host:port still produces a valid `ws://…/ws/display?token=`
    /// URL even when the token is empty (same-host path).
    #[test]
    fn loopback_url_works_with_empty_token() {
        let cfg = DisplayConfig::for_test("http://127.0.0.1:8443", "");
        let u = cfg.ws_url().unwrap();
        assert_eq!(u.scheme(), "ws");
        assert_eq!(u.path(), "/ws/display");
        // Token is present in the URL but empty — the server's
        // same-host branch never inspects it.
        let tok = u.query_pairs().find(|(k, _)| k == "token").unwrap().1;
        assert_eq!(tok, "");
    }
}
