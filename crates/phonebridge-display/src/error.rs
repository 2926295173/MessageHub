// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 PhoneBridge Contributors
//
// This file is part of PhoneBridge. See LICENSE and the dual-licensing
// notice in README.md for details.

//! Top-level error type for the display service.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum DisplayError {
    /// The config file could not be read or parsed.
    #[error("config: {0}")]
    Config(String),

    /// The token file could not be read.
    #[error("token file: {0}")]
    TokenFile(String),

    /// The HTTP client could not reach the daemon.
    #[error("http: {0}")]
    Http(String),

    /// The WebSocket could not be (re-)established.
    #[error("websocket: {0}")]
    WebSocket(String),

    /// The D-Bus session bus could not be reached or the
    /// notification daemon rejected a call.
    #[error("dbus: {0}")]
    Dbus(String),

    /// The shell prompt used to collect a quick-reply text
    /// (zenity / kdialog) failed.
    #[error("prompt: {0}")]
    Prompt(String),

    /// An inbound `DisplayEvent` was malformed.
    #[error("protocol: {0}")]
    Protocol(String),

    /// Any other I/O error.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// Catch-all.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
