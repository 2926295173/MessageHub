// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 PhoneBridge Contributors
//
// This file is part of PhoneBridge. See LICENSE and the dual-licensing
// notice in README.md for details.

//! Resolve platform-specific config and data directories for PhoneBridge.

use std::path::{Path, PathBuf};

/// Resolved paths for config + data + cache + state.
#[derive(Debug, Clone)]
pub struct AppPaths {
    /// Base config dir (e.g. `~/.config/phonebridge` on Linux).
    pub config_dir: PathBuf,
    /// Base data dir (e.g. `~/.local/share/phonebridge` on Linux).
    pub data_dir: PathBuf,
    /// Log directory (same as data dir on Linux for now).
    pub log_dir: PathBuf,
}

impl AppPaths {
    /// Compute platform-standard paths.
    ///
    /// Honors `$PHONEBRIDGE_CONFIG_DIR` and `$PHONEBRIDGE_DATA_DIR` if set,
    /// otherwise falls back to `directories::ProjectDirs`.
    pub fn resolve() -> Result<Self, PathError> {
        let config_dir = std::env::var("PHONEBRIDGE_CONFIG_DIR")
            .ok()
            .map(|s| expand_tilde(&s))
            .or_else(|| {
                directories::ProjectDirs::from("im", "zyx", "phonebridge")
                    .map(|d| d.config_dir().to_path_buf())
            })
            .ok_or(PathError::NoConfigDir)?;

        let data_dir = std::env::var("PHONEBRIDGE_DATA_DIR")
            .ok()
            .map(|s| expand_tilde(&s))
            .or_else(|| {
                directories::ProjectDirs::from("im", "zyx", "phonebridge")
                    .map(|d| d.data_dir().to_path_buf())
            })
            .ok_or(PathError::NoDataDir)?;

        let log_dir = data_dir.clone();

        Ok(Self {
            config_dir,
            data_dir,
            log_dir,
        })
    }

    /// Make sure all directories exist on disk.
    pub fn ensure(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.config_dir)?;
        std::fs::create_dir_all(&self.data_dir)?;
        std::fs::create_dir_all(&self.log_dir)?;
        Ok(())
    }

    /// Path to `config.toml`.
    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    /// Path to the SQLite database file.
    pub fn db_file(&self) -> PathBuf {
        self.data_dir.join("phonebridge.db")
    }

    /// Path to the daemon's long-term TLS cert.
    pub fn cert_file(&self) -> PathBuf {
        self.data_dir.join("daemon.cert.pem")
    }

    /// Path to the daemon's long-term TLS key.
    pub fn key_file(&self) -> PathBuf {
        self.data_dir.join("daemon.key.pem")
    }
}

/// Expand a leading `~` to `$HOME`.
pub fn expand_tilde(input: &str) -> PathBuf {
    if let Some(rest) = input.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return Path::new(&home).join(rest);
        }
    } else if input == "~" {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home);
        }
    }
    PathBuf::from(input)
}

/// Errors from path resolution.
#[derive(Debug, thiserror::Error)]
pub enum PathError {
    /// No suitable config directory could be resolved.
    #[error("could not determine config directory (set PHONEBRIDGE_CONFIG_DIR or $HOME)")]
    NoConfigDir,
    /// No suitable data directory could be resolved.
    #[error("could not determine data directory (set PHONEBRIDGE_DATA_DIR or $HOME)")]
    NoDataDir,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_tilde_handles_paths() {
        let original = std::env::var_os("HOME");
        std::env::set_var("HOME", "/tmp/fake-home");
        assert_eq!(expand_tilde("~"), PathBuf::from("/tmp/fake-home"));
        assert_eq!(expand_tilde("~/x"), PathBuf::from("/tmp/fake-home/x"));
        assert_eq!(expand_tilde("/abs/path"), PathBuf::from("/abs/path"));
        if let Some(orig) = original {
            std::env::set_var("HOME", orig);
        } else {
            std::env::remove_var("HOME");
        }
    }

    #[test]
    fn app_paths_resolve_with_overrides() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var(
            "PHONEBRIDGE_CONFIG_DIR",
            tmp.path().join("c").to_str().unwrap(),
        );
        std::env::set_var(
            "PHONEBRIDGE_DATA_DIR",
            tmp.path().join("d").to_str().unwrap(),
        );
        let paths = AppPaths::resolve().unwrap();
        paths.ensure().unwrap();
        assert!(paths.config_dir.exists());
        assert!(paths.data_dir.exists());
        std::env::remove_var("PHONEBRIDGE_CONFIG_DIR");
        std::env::remove_var("PHONEBRIDGE_DATA_DIR");
    }
}
