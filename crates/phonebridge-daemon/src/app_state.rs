//! Shared application state.

use std::sync::Arc;

use phonebridge_core::Config;
use phonebridge_storage::Db;

/// Cheaply-cloneable handle passed to every axum handler.
#[derive(Clone)]
pub struct AppState {
    /// Resolved daemon config.
    pub config: Arc<Config>,
    /// Database pool.
    pub db: Arc<Db>,
}

impl AppState {
    /// Construct a new state handle.
    pub fn new(config: Arc<Config>, db: Arc<Db>) -> Self {
        Self { config, db }
    }
}
