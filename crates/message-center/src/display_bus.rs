// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 PhoneBridge Contributors
//
// This file is part of PhoneBridge. See LICENSE and the dual-licensing
// notice in README.md for details.

//! `DisplayBus` — process-wide broadcast bus for the
//! `deskdisplay` endpoint. Carries the richer
//! [`phonebridge_proto::DisplayEvent`] shape (full payload,
//! not the lossy `ConsoleEvent.summary`).
//!
//! Why a separate bus from `ConsoleBus`? Two reasons:
//! 1. The web console only needs a tiny summary for its live
//!    feed; the desktop display needs the full payload to
//!    render rich notifications and inline-reply text fields.
//!    Different subscribers, different message shapes.
//! 2. The display bus also carries *message-center-generated* events
//!    (`phone.offline`, `action.result`) that the web console
//!    doesn't care about. Keeping them on a separate channel
//!    keeps each subscriber's parse path minimal.

#![forbid(unsafe_code)]
#![allow(missing_docs)]

use std::sync::Arc;

use parking_lot::Mutex;
use serde::Serialize;
use tokio::sync::broadcast;
use tracing::warn;

use phonebridge_proto::DisplayEvent;

/// Capacity of the broadcast channel. 1024 is more than enough
/// for a single-user tool — bursts of >1024 events imply the
/// message-center is misbehaving, not a real load pattern.
pub const DEFAULT_CHANNEL_CAPACITY: usize = 1024;

/// Cheaply-cloneable handle to the bus, stored in [`AppState`].
#[derive(Clone)]
pub struct DisplayBus {
    tx: broadcast::Sender<Arc<DisplayEvent>>,
    /// Number of active subscribers. Surfaced via the health
    /// endpoint and used to skip publish work when nobody is
    /// listening.
    subscriber_count: Arc<Mutex<usize>>,
}

impl DisplayBus {
    /// Create a new bus with the given channel capacity.
    pub fn new(capacity: usize) -> Self {
        let (tx, _rx) = broadcast::channel(capacity);
        Self {
            tx,
            subscriber_count: Arc::new(Mutex::new(0)),
        }
    }

    /// Subscribe to all events. Each subscriber gets its own
    /// `broadcast::Receiver`; if the receiver falls behind by
    /// more than the channel capacity, it gets
    /// `RecvError::Lagged` and skips ahead.
    pub fn subscribe(&self) -> DisplaySubscriber {
        let mut count = self.subscriber_count.lock();
        *count += 1;
        DisplaySubscriber {
            rx: self.tx.subscribe(),
            count: Arc::clone(&self.subscriber_count),
        }
    }

    /// Number of active subscribers.
    pub fn subscriber_count(&self) -> usize {
        *self.subscriber_count.lock()
    }

    /// Publish an event. Returns the number of receivers that
    /// got it. If no subscribers, this is a cheap no-op (we
    /// still build the Arc but don't queue it).
    pub fn publish(&self, event: DisplayEvent) -> usize {
        if *self.subscriber_count.lock() == 0 {
            return 0;
        }
        let n = self.tx.send(Arc::new(event)).unwrap_or(0);
        if n == 0 {
            warn!("display bus publish: no active receivers (race)");
        }
        n
    }
}

impl Default for DisplayBus {
    fn default() -> Self {
        Self::new(DEFAULT_CHANNEL_CAPACITY)
    }
}

/// A subscriber handle, returned by [`DisplayBus::subscribe`].
/// Decrements the active count when dropped.
pub struct DisplaySubscriber {
    rx: broadcast::Receiver<Arc<DisplayEvent>>,
    count: Arc<Mutex<usize>>,
}

impl DisplaySubscriber {
    /// Receive the next event, awaiting if none is queued.
    pub async fn recv(&mut self) -> Result<Arc<DisplayEvent>, broadcast::error::RecvError> {
        self.rx.recv().await
    }
}

impl Drop for DisplaySubscriber {
    fn drop(&mut self) {
        // We only ever decrement, never go negative, because
        // every subscribe() bumps the count. The lock is
        // bounded (parking_lot Mutex) and the operation is
        // O(1), so Drop is safe to hold the lock.
        let mut count = self.count.lock();
        *count = count.saturating_sub(1);
    }
}

/// Wrapper for the message-center-internal "send a DisplayEvent" path.
/// Used by `display_ws.rs` for `phone.offline` and
/// `action.result` events that originate in the message-center (not
/// from a phone envelope).
pub fn build_display_event(
    kind: impl Into<String>,
    device_id: uuid::Uuid,
    envelope_id: uuid::Uuid,
    payload: serde_json::Value,
) -> DisplayEvent {
    DisplayEvent {
        kind: kind.into(),
        device_id,
        envelope_id,
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0),
        payload,
        summary: Default::default(),
    }
}

/// Silences the unused-import lint when `Serialize` is not
/// otherwise referenced in this file (it is, via DisplayEvent
/// derive, but a future refactor might want this re-export).
#[allow(dead_code)]
fn _ensure_serialize_imported<T: Serialize>() {}
