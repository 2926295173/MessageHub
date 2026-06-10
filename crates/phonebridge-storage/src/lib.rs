// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 PhoneBridge Contributors
//
// This file is part of PhoneBridge. See LICENSE and the dual-licensing
// notice in README.md for details.

//! SQLite storage layer.
//!
//! - [`migrations`]: SQL files run on startup.
//! - [`models`]: row structs and DTOs.
//! - [`pool`]: the [`Db`] connection pool wrapper.

#![forbid(unsafe_code)]
#![allow(missing_docs)]

pub mod models;
pub mod pool;

pub use pool::Db;
