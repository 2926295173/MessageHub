// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 PhoneBridge Contributors
//
// This file is part of PhoneBridge. See LICENSE and the dual-licensing
// notice in README.md for details.

//! Outgoing `DisplayAction` channel.
//!
//! The platform backend calls into the [`ActionSink`] to
//! send a quick-reply / mark-read / dismiss back to the
//! daemon. The sink is just a `mpsc::Sender<DisplayAction>`
//! wrapped behind an `Arc<Mutex<…>>` so the backend can be
//! `Send + Sync` without owning the runtime.

use std::sync::{Arc, Mutex};

use phonebridge_proto::DisplayAction;
use tokio::sync::mpsc;

use crate::error::DisplayError;

#[derive(Clone)]
pub struct ActionSink {
    inner: Arc<Mutex<Option<mpsc::Sender<DisplayAction>>>>,
}

impl ActionSink {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(None)),
        }
    }

    /// Called by the WS client once it has its outgoing
    /// channel up. Replaces any prior binding.
    pub fn bind(&self, tx: mpsc::Sender<DisplayAction>) {
        *self.inner.lock().expect("action sink poisoned") = Some(tx);
    }

    /// Called by the WS client when the connection drops,
    /// so the backend stops trying to send until the next
    /// bind.
    pub fn unbind(&self) {
        *self.inner.lock().expect("action sink poisoned") = None;
    }

    /// Send an action. Returns `Err` if the sink is not
    /// currently bound (daemon unreachable / not yet
    /// connected).
    pub fn try_send(&self, action: DisplayAction) -> Result<(), DisplayError> {
        let guard = self.inner.lock().expect("action sink poisoned");
        let tx = guard
            .as_ref()
            .ok_or_else(|| DisplayError::WebSocket("action sink unbound".into()))?;
        tx.try_send(action).map_err(|e| match e {
            mpsc::error::TrySendError::Full(_) => {
                DisplayError::WebSocket("action channel full".into())
            }
            mpsc::error::TrySendError::Closed(_) => {
                DisplayError::WebSocket("action channel closed".into())
            }
        })
    }
}

impl Default for ActionSink {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phonebridge_proto::DisplayAction;
    use uuid::Uuid;

    #[test]
    fn unbound_returns_err() {
        let sink = ActionSink::new();
        let r = sink.try_send(DisplayAction {
            kind: "notification.read".into(),
            envelope_id: Uuid::new_v4(),
            device_id: Uuid::new_v4(),
            to: None,
            body: None,
            call_id: None,
        });
        assert!(r.is_err());
    }

    #[tokio::test]
    async fn bound_forwards_action() {
        let sink = ActionSink::new();
        let (tx, mut rx) = mpsc::channel::<DisplayAction>(8);
        sink.bind(tx);
        let env = Uuid::new_v4();
        let dev = Uuid::new_v4();
        sink.try_send(DisplayAction {
            kind: "notification.read".into(),
            envelope_id: env,
            device_id: dev,
            to: None,
            body: None,
            call_id: None,
        })
        .unwrap();
        let got = rx.recv().await.unwrap();
        assert_eq!(got.kind, "notification.read");
        assert_eq!(got.envelope_id, env);
        assert_eq!(got.device_id, dev);
    }
}
