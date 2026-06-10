// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 PhoneBridge Contributors
//
// This file is part of PhoneBridge. See LICENSE and the dual-licensing
// notice in README.md for details.

//! `phonebridge-display` — desktop notification surface for
//! PhoneBridge.
//!
//! This binary subscribes to `/ws/display` on the daemon and
//! surfaces phone events (notifications, SMS, calls, …) on
//! the host OS using the appropriate native API. It can
//! also send back quick-reply / mark-read / dismiss actions
//! over the same full-duplex connection.
//!
//! The crate is structured as a small library so the same
//! WS client can be embedded in tests; the binary in
//! `main.rs` is the entry point that loads config, picks a
//! platform backend, and runs the client loop.

#![forbid(unsafe_code)]
#![allow(missing_docs)]

pub mod actions;
pub mod backends;
pub mod client;
pub mod config;
pub mod error;
pub mod i18n;

pub use actions::ActionSink;
pub use backends::DisplayBackend;
pub use client::DisplayClient;
pub use config::DisplayConfig;
pub use error::DisplayError;
