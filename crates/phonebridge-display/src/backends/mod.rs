// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 PhoneBridge Contributors
//
// This file is part of PhoneBridge. See LICENSE and the dual-licensing
// notice in README.md for details.

//! Platform-specific OS notification backends.
//!
//! The display service dispatches incoming `DisplayEvent`s
//! to a [`DisplayBackend`] which renders them via the host
//! OS's notification surface and, on user action, calls
//! back through the [`ActionSink`].

use async_trait::async_trait;
use phonebridge_proto::DisplayEvent;

use crate::actions::ActionSink;
use crate::config::DisplayConfig;
use crate::error::DisplayError;
use crate::i18n::DisplayI18n;

#[async_trait]
pub trait DisplayBackend: Send + Sync {
    /// Called once at startup. May establish D-Bus
    /// connections, register signal subscriptions, etc.
    /// Should return quickly.
    async fn start(&self) -> Result<(), DisplayError>;

    /// Render an incoming `DisplayEvent` on the host OS
    /// surface. The backend is free to drop / coalesce
    /// events it doesn't know how to render.
    async fn present(
        &self,
        event: &DisplayEvent,
        i18n: &DisplayI18n,
        actions: &ActionSink,
    ) -> Result<(), DisplayError>;

    /// Stop the backend (close D-Bus connections, etc.).
    async fn stop(&self) -> Result<(), DisplayError> {
        Ok(())
    }
}

/// Create the platform-appropriate backend. Falls back to
/// a no-op stub on platforms we don't support yet (e.g.
/// non-Linux Unix, Windows, macOS — PR9 + PR10 will plug
/// in those backends).
pub fn create(cfg: &DisplayConfig) -> Result<Box<dyn DisplayBackend>, DisplayError> {
    #[cfg(target_os = "linux")]
    {
        return Ok(Box::new(linux::LinuxBackend::new(cfg)?));
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = cfg;
        return Ok(Box::new(stub::StubBackend));
    }
}

pub mod linux;
pub mod stub;
