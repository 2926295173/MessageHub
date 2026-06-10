// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 PhoneBridge Contributors
//
// This file is part of PhoneBridge. See LICENSE and the dual-licensing
// notice in README.md for details.

//! WebSocket client that talks to `/ws/display` on the
//! daemon.
//!
//! Lifecycle:
//!
//! 1. Build the WS URL from the [`DisplayConfig`].
//! 2. Connect.
//! 3. Send the local action-sink channel to the backend so
//!    the D-Bus signal handler (or its macOS / Windows
//!    equivalent) can enqueue outgoing `DisplayAction`s.
//! 4. Spawn two tasks: a reader that receives
//!    `DisplayEvent`s from the daemon and dispatches them
//!    to the backend, and a writer that drains the action
//!    channel and pushes JSON lines back to the daemon.
//! 5. If either side fails, sleep with exponential
//!    backoff (capped at 30s) and reconnect. The action
//!    sink is unbound while the connection is down so the
//!    backend doesn't see spurious failures.

use std::time::Duration;

use futures::{SinkExt, StreamExt};
use phonebridge_proto::{DisplayAction, DisplayEvent};
use tokio::sync::mpsc;
use tokio::time::sleep;
use tokio_tungstenite::tungstenite::Message;
use tracing::{info, warn};

use crate::actions::ActionSink;
use crate::backends::DisplayBackend;
use crate::config::DisplayConfig;
use crate::error::DisplayError;
use crate::i18n::DisplayI18n;

const MAX_BACKOFF: Duration = Duration::from_secs(30);
const INITIAL_BACKOFF: Duration = Duration::from_millis(500);
const ACTION_BUFFER: usize = 32;

pub struct DisplayClient;

impl DisplayClient {
    /// Run the client until the runtime is shut down.
    /// The caller passes a backend (already started) and
    /// the i18n dictionary; this loop only handles the
    /// network / framing side.
    pub async fn run(
        cfg: DisplayConfig,
        backend: Box<dyn DisplayBackend>,
        i18n: DisplayI18n,
        sink: ActionSink,
    ) -> Result<(), DisplayError> {
        let mut backoff = INITIAL_BACKOFF;
        loop {
            match session(&cfg, backend.as_ref(), &i18n, &sink).await {
                Ok(()) => {
                    info!("display WS session ended cleanly; reconnecting");
                    backoff = INITIAL_BACKOFF;
                }
                Err(e) => {
                    warn!(error = %e, backoff_ms = backoff.as_millis() as u64,
                          "display WS session errored; backing off");
                    sleep(backoff).await;
                    backoff = std::cmp::min(backoff * 2, MAX_BACKOFF);
                }
            }
        }
    }
}

/// One session: connect, read events, write actions, exit
/// on the first error.
async fn session(
    cfg: &DisplayConfig,
    backend: &dyn DisplayBackend,
    i18n: &DisplayI18n,
    sink: &ActionSink,
) -> Result<(), DisplayError> {
    let url = cfg.ws_url()?;
    let url_str = url.as_str().to_string();
    info!(url = %url_str, "connecting to /ws/display");
    let (ws, _resp) = tokio_tungstenite::connect_async(url_str)
        .await
        .map_err(|e| DisplayError::WebSocket(format!("connect: {e}")))?;
    info!("/ws/display connected");

    // Hand the action sink its outgoing channel.
    let (action_tx, mut action_rx) = mpsc::channel::<DisplayAction>(ACTION_BUFFER);
    sink.bind(action_tx);

    // Forward incoming events to the backend, framed as
    // JSON lines (one event per line).
    let (mut writer, mut reader) = ws.split();
    let sink_writer = sink.clone();
    let writer_task = tokio::spawn(async move {
        while let Some(action) = action_rx.recv().await {
            let line = match serde_json::to_string(&action) {
                Ok(s) => s,
                Err(e) => {
                    warn!(error = %e, "action serialize failed; dropping");
                    continue;
                }
            };
            if writer
                .send(Message::Text(format!("{line}\n")))
                .await
                .is_err()
            {
                break;
            }
        }
        let _ = sink_writer;
    });

    while let Some(msg) = reader.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(e) => {
                sink.unbind();
                writer_task.abort();
                return Err(DisplayError::WebSocket(format!("recv: {e}")));
            }
        };
        let text = match msg {
            Message::Text(t) => t,
            Message::Binary(_) => continue,
            Message::Close(_) => {
                sink.unbind();
                writer_task.abort();
                return Ok(());
            }
            _ => continue,
        };
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let event: DisplayEvent = match serde_json::from_str(line) {
                Ok(e) => e,
                Err(e) => {
                    warn!(error = %e, raw = line, "display event parse failed");
                    continue;
                }
            };
            if let Err(e) = backend.present(&event, i18n, sink).await {
                warn!(error = %e, kind = %event.kind, "backend present failed");
            }
        }
    }

    sink.unbind();
    writer_task.abort();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles() {
        let mut b = INITIAL_BACKOFF;
        for _ in 0..10 {
            b = std::cmp::min(b * 2, MAX_BACKOFF);
        }
        assert_eq!(b, MAX_BACKOFF);
    }
}
