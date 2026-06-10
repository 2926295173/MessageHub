// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 PhoneBridge Contributors
//
// This file is part of PhoneBridge. See LICENSE and the dual-licensing
// notice in README.md for details.

//! No-op backend used on platforms we don't yet support
//! (macOS, Windows). Logs the event to the tracing
//! subscriber and otherwise drops it on the floor.
//!
//! PR9 (macOS) and PR10 (Windows) will replace this with
//! the real `objc2` / `winrt` backends.

use async_trait::async_trait;
use phonebridge_proto::DisplayEvent;

use super::DisplayBackend;
use crate::actions::ActionSink;
use crate::error::DisplayError;
use crate::i18n::DisplayI18n;

pub struct StubBackend;

#[async_trait]
impl DisplayBackend for StubBackend {
    async fn start(&self) -> Result<(), DisplayError> {
        tracing::info!("stub backend: no platform notification surface to bind to");
        Ok(())
    }

    async fn present(
        &self,
        event: &DisplayEvent,
        _i18n: &DisplayI18n,
        _actions: &ActionSink,
    ) -> Result<(), DisplayError> {
        tracing::info!(
            kind = %event.kind,
            device_id = %event.device_id,
            "stub backend would have surfaced event: {}",
            serde_json::to_string(&event.payload).unwrap_or_default()
        );
        Ok(())
    }
}
