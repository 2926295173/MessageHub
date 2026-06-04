//! Shared application state.

use std::sync::Arc;

use parking_lot::RwLock;
use phonebridge_core::Config;
use phonebridge_net::{DeviceRegistry, PairingMap};
use phonebridge_storage::Db;
use uuid::Uuid;

use crate::console_bus::ConsoleBus;

/// Cheaply-cloneable handle passed to every axum handler.
#[derive(Clone)]
pub struct AppState {
    /// Resolved daemon config.
    pub config: Arc<Config>,
    /// Database pool.
    pub db: Arc<Db>,
    /// Per-device cert fingerprints (pinned at pairing time).
    pub pin_store: Arc<RwLock<std::collections::HashMap<Uuid, String>>>,
    /// In-flight pairing sessions.
    pub pairing: PairingMap,
    /// Downstream send registry.
    pub registry: DeviceRegistry,
    /// Process-wide console bus (web console live push).
    pub console_bus: ConsoleBus,
    /// This daemon's stable UUIDv4 id (generated on first run, persisted).
    pub our_device_id: Uuid,
    /// Public key of the daemon's long-term cert (base64).
    pub our_public_key_b64: Arc<RwLock<String>>,
    /// The daemon's long-term cert fingerprint.
    pub our_fingerprint: Arc<RwLock<String>>,
    /// This daemon's display name (for mDNS).
    pub our_name: Arc<RwLock<String>>,
}

impl AppState {
    /// Construct a new state handle.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        config: Arc<Config>,
        db: Arc<Db>,
        registry: DeviceRegistry,
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
            registry,
            console_bus: ConsoleBus::default(),
            our_device_id,
            our_public_key_b64: Arc::new(RwLock::new(our_public_key_b64)),
            our_fingerprint: Arc::new(RwLock::new(our_fingerprint)),
            our_name: Arc::new(RwLock::new(our_name)),
        }
    }
}
