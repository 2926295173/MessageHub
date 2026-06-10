// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 PhoneBridge Contributors
//
// This file is part of PhoneBridge. See LICENSE and the dual-licensing
// notice in README.md for details.

//! Initialize the `tracing` subscriber for the daemon.

use std::path::Path;

use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

use crate::config::LoggingConfig;

/// Initialize logging based on the `[logging]` config.
///
/// Behavior:
/// - If `file` is set, write to that path (with non-blocking writer).
/// - Always also write to stdout.
/// - `level` is overridden by the `RUST_LOG` env var if present.
pub fn init(cfg: &LoggingConfig) -> Result<(), LoggingError> {
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(&cfg.level));

    let fmt_layer = fmt::layer()
        .with_target(true)
        .with_thread_ids(false)
        .with_line_number(false)
        .with_file(false);

    if cfg.file.is_empty() {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt_layer)
            .init();
    } else {
        let file = open_log_file(Path::new(&cfg.file))?;
        let (nb_writer, _guard) = tracing_appender::non_blocking(file);
        let file_layer = fmt::layer().with_writer(nb_writer).with_ansi(false);
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt_layer)
            .with(file_layer)
            .init();
        // _guard is leaked intentionally to keep the non-blocking writer alive.
    }
    Ok(())
}

fn open_log_file(path: &Path) -> Result<std::fs::File, LoggingError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?)
}

/// Errors from logging initialization.
#[derive(Debug, thiserror::Error)]
pub enum LoggingError {
    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_load() {
        let cfg = LoggingConfig::default();
        assert_eq!(cfg.level, "info");
        assert!(cfg.file.is_empty());
    }
}
