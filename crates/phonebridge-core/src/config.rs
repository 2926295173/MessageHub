// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 PhoneBridge Contributors
//
// This file is part of PhoneBridge. See LICENSE and the dual-licensing
// notice in README.md for details.

//! Daemon config (TOML).

use std::path::Path;

use serde::{Deserialize, Serialize};

/// Top-level daemon configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    /// Server (TLS) section.
    #[serde(default)]
    pub server: ServerConfig,
    /// mDNS discovery section.
    #[serde(default)]
    pub discovery: DiscoveryConfig,
    /// Storage section.
    #[serde(default)]
    pub storage: StorageConfig,
    /// Logging section.
    #[serde(default)]
    pub logging: LoggingConfig,
}

impl Config {
    /// Load from a TOML file at `path`. Returns defaults if the file is absent.
    pub fn load_from_file(path: &Path) -> Result<Self, ConfigError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let s = std::fs::read_to_string(path)?;
        let cfg: Config = toml::from_str(&s)?;
        Ok(cfg)
    }

    /// Load from a TOML string.
    pub fn load_from_str(s: &str) -> Result<Self, ConfigError> {
        Ok(toml::from_str(s)?)
    }

    /// Save to a TOML file.
    pub fn save_to_file(&self, path: &Path) -> Result<(), ConfigError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let s = toml::to_string_pretty(self)?;
        std::fs::write(path, s)?;
        Ok(())
    }
}

/// `[server]` section.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Bind address, e.g. `0.0.0.0:8443`.
    #[serde(default = "default_bind")]
    pub bind: String,
    /// Optional explicit cert path. Empty = use data_dir/daemon.cert.pem.
    #[serde(default)]
    pub cert_path: String,
    /// Optional explicit key path. Empty = use data_dir/daemon.key.pem.
    #[serde(default)]
    pub key_path: String,
    /// External hostname/IP to advertise via mDNS. Empty = auto-detect.
    #[serde(default)]
    pub advertise_host: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind: default_bind(),
            cert_path: String::new(),
            key_path: String::new(),
            advertise_host: String::new(),
        }
    }
}

fn default_bind() -> String {
    "0.0.0.0:8443".to_string()
}

/// `[discovery]` section.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryConfig {
    /// mDNS service type, default `_phonebridge._tcp`.
    #[serde(default = "default_service_type")]
    pub service_type: String,
    /// Whether to enable mDNS advertising + browsing.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Optional: bind to a specific interface.
    #[serde(default)]
    pub interface: Option<String>,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            service_type: default_service_type(),
            enabled: true,
            interface: None,
        }
    }
}

fn default_service_type() -> String {
    "_phonebridge._tcp".to_string()
}

fn default_true() -> bool {
    true
}

/// `[storage]` section.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StorageConfig {
    /// SQLite path; empty = use `{data_dir}/phonebridge.db`.
    #[serde(default)]
    pub db_path: String,
}

/// `[logging]` section.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Log level: `trace` / `debug` / `info` / `warn` / `error`.
    #[serde(default = "default_log_level")]
    pub level: String,
    /// Log file path; empty = stdout only.
    #[serde(default)]
    pub file: String,
    /// Max log file size before rotation in MB.
    #[serde(default = "default_log_size")]
    pub max_log_size_mb: u32,
    /// Number of log files to keep.
    #[serde(default = "default_log_files")]
    pub max_log_files: u32,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            file: String::new(),
            max_log_size_mb: default_log_size(),
            max_log_files: default_log_files(),
        }
    }
}

fn default_log_level() -> String {
    "info".to_string()
}
fn default_log_size() -> u32 {
    10
}
fn default_log_files() -> u32 {
    5
}

/// Errors from config loading / saving.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// TOML parse error.
    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),
    /// TOML serialize error.
    #[error("TOML serialize error: {0}")]
    TomlSer(#[from] toml::ser::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_round_trip() {
        let cfg = Config::default();
        let s = toml::to_string(&cfg).unwrap();
        let back = Config::load_from_str(&s).unwrap();
        assert_eq!(back.server.bind, "0.0.0.0:8443");
        assert_eq!(back.discovery.service_type, "_phonebridge._tcp");
        assert!(back.discovery.enabled);
        assert_eq!(back.logging.level, "info");
    }

    #[test]
    fn missing_file_returns_defaults() {
        let p = std::path::Path::new("/nonexistent/path/config.toml");
        let cfg = Config::load_from_file(p).unwrap();
        assert_eq!(cfg.server.bind, "0.0.0.0:8443");
    }

    #[test]
    fn partial_overlay() {
        let s = r#"
            [server]
            bind = "127.0.0.1:9443"
        "#;
        let cfg = Config::load_from_str(s).unwrap();
        assert_eq!(cfg.server.bind, "127.0.0.1:9443");
        assert_eq!(cfg.discovery.service_type, "_phonebridge._tcp"); // default
    }

    #[test]
    fn save_and_reload() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let cfg = Config {
            server: ServerConfig { bind: "0.0.0.0:9999".into(), ..ServerConfig::default() },
            ..Config::default()
        };
        cfg.save_to_file(&path).unwrap();
        let back = Config::load_from_file(&path).unwrap();
        assert_eq!(back.server.bind, "0.0.0.0:9999");
    }
}
