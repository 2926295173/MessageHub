// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 PhoneBridge Contributors
//
// This file is part of PhoneBridge. See LICENSE.

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

/// Create the platform-appropriate backend. Linux gets
/// the real zbus back-end; Windows 10/11 gets the
/// ToastNotificationManager back-end; everything else
/// (macOS, BSD) falls back to a no-op stub.
#[allow(unused_assignments, clippy::needless_return)]
pub fn create(cfg: &DisplayConfig) -> Result<Box<dyn DisplayBackend>, DisplayError> {
    #[cfg(target_os = "linux")]
    {
        return Ok(Box::new(linux::LinuxBackend::new(cfg)?));
    }
    #[cfg(target_os = "windows")]
    {
        return Ok(Box::new(windows::WindowsBackend::new(cfg)?));
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        let _ = cfg;
        return Ok(Box::new(stub::StubBackend));
    }
}

pub mod linux;
pub mod mock;
pub mod stub;
pub mod windows;
