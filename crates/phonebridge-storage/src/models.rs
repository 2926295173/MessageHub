//! Row structs that mirror the SQL schema.

use serde::{Deserialize, Serialize};
use sqlx::FromRow;

// ============================================================================
// devices
// ============================================================================

/// One row of the `devices` table.
#[derive(Debug, Clone, PartialEq, Eq, FromRow, Serialize, Deserialize)]
pub struct DeviceRow {
    /// Auto-increment primary key.
    pub id: i64,
    /// Human-readable device name.
    pub name: String,
    /// Stable UUIDv4 device id (the one used on the wire as `device_id`).
    pub device_id: uuid::Uuid,
    /// Base64 of the long-term ECDH P-256 public key.
    pub public_key: String,
    /// Last-seen Unix epoch seconds.
    pub last_seen: i64,
    /// Whether the device is paired (vs. discovered but unpaired).
    pub paired: bool,
}

// ============================================================================
// pairings
// ============================================================================

/// One row of the `pairings` table.
#[derive(Debug, Clone, PartialEq, Eq, FromRow, Serialize, Deserialize)]
pub struct PairingRow {
    /// Auto-increment primary key.
    pub id: i64,
    /// Paired device's UUIDv4 id.
    pub device_id: uuid::Uuid,
    /// PEM-encoded cert we pinned for the peer.
    pub cert_pem: String,
    /// SHA-256 fingerprint of the peer's cert.
    pub cert_fingerprint: String,
    /// When pairing completed (epoch seconds).
    pub paired_at: i64,
}

// ============================================================================
// notifications
// ============================================================================

/// One row of the `notifications` table.
#[derive(Debug, Clone, PartialEq, Eq, FromRow, Serialize, Deserialize)]
pub struct NotificationRow {
    /// Per-device notification id.
    pub id: String,
    /// Owning device.
    pub device_id: uuid::Uuid,
    /// App package name.
    pub package_name: String,
    /// Optional human-readable app name.
    pub app_name: Option<String>,
    /// Title.
    pub title: String,
    /// Content body.
    pub content: String,
    /// Unix epoch ms when posted.
    pub posted_at: i64,
    /// True if `FLAG_NO_PEEK` / locked-screen SECRET.
    pub is_sensitive: bool,
    /// Optional Android notification category.
    pub category: Option<String>,
    /// Whether the user marked it read in the web console.
    pub read: bool,
}
