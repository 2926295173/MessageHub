// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 PhoneBridge Contributors
//
// This file is part of PhoneBridge. See LICENSE and the dual-licensing
// notice in README.md for details.

//! Library surface for the message-center. Re-exports modules needed by integration
//! tests and the main binary.

#![forbid(unsafe_code)]
#![allow(missing_docs)]

pub mod app_state;
pub mod center_sink;
pub mod cert_loader;
pub mod console_bus;
pub mod display_auth;
pub mod display_bus;
pub mod display_ws;
pub mod i18n;
pub mod identity;
pub mod mdns_service;
pub mod noise_filter;
pub mod openapi;
pub mod pair_cli;
pub mod rest;
pub mod static_files;
pub mod tls;
pub mod ws;

pub use ws::test_context;

pub use app_state::AppState;
pub use center_sink::CenterSink;
