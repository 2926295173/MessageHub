// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 PhoneBridge Contributors
//
// This file is part of PhoneBridge. See LICENSE and the dual-licensing
// notice in README.md for details.

//! `phonebridge-display` binary entry point.
//!
//! Usage:
//!   phonebridge-display [--config <path>] [--log <level>]
//!   phonebridge-display --print-config-path
//!   phonebridge-display --print-default-config
//!
//! The binary loads the config, then runs the WS client
//! loop with the platform-appropriate backend.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::EnvFilter;

use phonebridge_display::{actions, backends, client, config, i18n, DisplayBackend};

#[derive(Debug, Parser)]
#[command(
    name = "phonebridge-display",
    version,
    about = "PhoneBridge desktop notification service"
)]
struct Cli {
    /// Path to the config file (default:
    /// `$XDG_CONFIG_HOME/phonebridge/display.toml`).
    #[arg(long, global = true)]
    config: Option<PathBuf>,

    /// Print the path where the config file is expected
    /// to live, then exit.
    #[arg(long, default_value_t = false)]
    print_config_path: bool,

    /// Print a fully-commented starter config and exit.
    #[arg(long, default_value_t = false)]
    print_default_config: bool,

    /// Override the log level (`trace`, `debug`, `info`,
    /// `warn`, `error`).
    #[arg(long, global = true, default_value = "info")]
    log: String,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    if cli.print_config_path {
        println!("{}", config::default_config_path().display());
        return ExitCode::SUCCESS;
    }
    if cli.print_default_config {
        println!("{}", DEFAULT_CONFIG_DOC);
        return ExitCode::SUCCESS;
    }

    let env_filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .parse_lossy(&cli.log);
    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(false)
        .init();

    let load_result = match cli.config.as_deref() {
        Some(p) => config::DisplayConfig::load_from(p),
        None => config::DisplayConfig::load(),
    };
    let mut cfg = match load_result {
        Ok(c) => c,
        Err(e) => {
            eprintln!("phonebridge-display: {e}");
            return ExitCode::from(2);
        }
    };
    if let Some(p) = cli.config {
        cfg.config_path = p;
    }

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("phonebridge-display: runtime init failed: {e}");
            return ExitCode::from(2);
        }
    };
    runtime.block_on(async move {
        let dict = i18n::DisplayI18n::load(&cfg).await;
        let backend: Box<dyn DisplayBackend> = match backends::create(&cfg) {
            Ok(b) => b,
            Err(e) => {
                tracing::error!(error = %e, "backend init failed");
                return ExitCode::from(2);
            }
        };
        if let Err(e) = backend.start().await {
            tracing::error!(error = %e, "backend start failed");
            return ExitCode::from(2);
        }
        let sink = actions::ActionSink::new();
        match client::DisplayClient::run(cfg, backend, dict, sink).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                tracing::error!(error = %e, "client loop ended");
                ExitCode::from(1)
            }
        }
    })
}

const DEFAULT_CONFIG_DOC: &str = r#"# phonebridge-display configuration
#
# Location: $XDG_CONFIG_HOME/phonebridge/display.toml
# (default: ~/.config/phonebridge/display.toml)

[daemon]
# URL of the message-center HTTP / WS endpoint.
url = "http://127.0.0.1:8443"
# Either `token` (literal) or `token_file` (path). The
# latter is the typical setup: the daemon writes
# `~/.config/phonebridge/display.token` (mode 0600) and
# this service reads it directly.
token_file = "/root/.config/phonebridge/display.token"

[i18n]
# `auto` (default) — fetch from daemon, fall back to
# LANG-derived builtin.
locale = "auto"
"#;
