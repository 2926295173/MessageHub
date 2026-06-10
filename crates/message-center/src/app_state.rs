// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 PhoneBridge Contributors
//
// This file is part of PhoneBridge. See LICENSE and the dual-licensing
// notice in README.md for details.

//! Shared application state.

use std::sync::Arc;

use parking_lot::RwLock;
use phonebridge_core::Config;
use phonebridge_net::{DeviceRegistry, PairingMap, PendingIncomingMap};
use phonebridge_storage::Db;
use uuid::Uuid;

use crate::console_bus::ConsoleBus;
use crate::display_auth::DisplayAuth;
use crate::display_bus::DisplayBus;

/// Cheaply-cloneable handle passed to every axum handler.
#[derive(Clone)]
pub struct AppState {
    /// Resolved message-center config.
    pub config: Arc<Config>,
    /// Database pool.
    pub db: Arc<Db>,
    /// Per-device cert fingerprints (pinned at pairing time).
    pub pin_store: Arc<RwLock<std::collections::HashMap<Uuid, String>>>,
    /// In-flight pairing sessions.
    pub pairing: PairingMap,
    /// Pending phone-initiated pairing requests awaiting user approval
    /// on the web console.
    pub pending_incoming: PendingIncomingMap,
    /// Downstream send registry.
    pub registry: DeviceRegistry,
    /// Process-wide console bus (web console live push).
    pub console_bus: ConsoleBus,
    /// Process-wide display bus (desktop notification endpoint).
    pub display_bus: DisplayBus,
    /// Display-endpoint token, with rotate/revoke support.
    pub display_auth: DisplayAuth,
    /// This message-center's stable UUIDv4 id (generated on first run, persisted).
    pub our_device_id: Uuid,
    /// Public key of the message-center's long-term cert (base64).
    pub our_public_key_b64: Arc<RwLock<String>>,
    /// The message-center's long-term cert fingerprint.
    pub our_fingerprint: Arc<RwLock<String>>,
    /// This message-center's display name (for mDNS).
    pub our_name: Arc<RwLock<String>>,
}

impl AppState {
    /// Construct a new state handle. Caller is responsible for
    /// having loaded (or generated) the display-endpoint token
    /// first and passing the resulting [`DisplayAuth`] in.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: Arc<Config>,
        db: Arc<Db>,
        registry: DeviceRegistry,
        display_auth: DisplayAuth,
        our_device_id: Uuid,
        our_public_key_b64: String,
        our_fingerprint: String,
        our_name: String,
    ) -> Self {
        Self {
            config,
            db,
            pin_store: Arc::new(RwLock::new(std::collections::HashMap::new())),
            pairing: PairingMap::new(),
            pending_incoming: PendingIncomingMap::new(),
            registry,
            console_bus: ConsoleBus::default(),
            display_bus: DisplayBus::default(),
            display_auth,
            our_device_id,
            our_public_key_b64: Arc::new(RwLock::new(our_public_key_b64)),
            our_fingerprint: Arc::new(RwLock::new(our_fingerprint)),
            our_name: Arc::new(RwLock::new(our_name)),
        }
    }
}
