// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 PhoneBridge Contributors
//
// This file is part of PhoneBridge. See LICENSE and the dual-licensing
// notice in README.md for details.

//! Core utilities shared by the daemon binary and other crates:
//! - [`config`]: load / save / validate the daemon config TOML.
//! - [`paths`]: resolve XDG / platform config + data directories.
//! - [`logging`]: initialize the `tracing` subscriber.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod config;
pub mod logging;
pub mod paths;

pub use config::{Config, LoggingConfig, ServerConfig, DiscoveryConfig, StorageConfig};
pub use paths::{AppPaths, expand_tilde};
