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

// ============================================================================
// sms_messages
// ============================================================================

/// Direction of an SMS: incoming (received) or outgoing (sent).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SmsDirection {
    /// Received from the phone network.
    In,
    /// Sent by us (via the daemon or phone).
    Out,
}

impl SmsDirection {
    /// As a string for SQL.
    pub fn as_str(&self) -> &'static str {
        match self {
            SmsDirection::In => "in",
            SmsDirection::Out => "out",
        }
    }
    /// Parse from a SQL string.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "in" => Some(SmsDirection::In),
            "out" => Some(SmsDirection::Out),
            _ => None,
        }
    }
}

/// One row of the `sms_messages` table.
#[derive(Debug, Clone, PartialEq, Eq, FromRow, Serialize, Deserialize)]
pub struct SmsRow {
    /// Per-device SMS id.
    pub id: String,
    /// Owning device.
    pub device_id: uuid::Uuid,
    /// SIM slot 0 or 1.
    pub sim_slot: Option<u8>,
    /// Phone number (or short code).
    pub phone_number: String,
    /// Message body.
    pub body: String,
    /// Direction (in/out).
    pub direction: String,
    /// Epoch ms.
    pub timestamp: i64,
}

// ============================================================================
// calls
// ============================================================================

/// Call state stored in the `calls` table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CallStateStr {
    /// Incoming ring.
    Ringing,
    /// Off-hook (active or dialing).
    Offhook,
    /// Idle / ended.
    Idle,
}

impl CallStateStr {
    pub fn as_str(&self) -> &'static str {
        match self {
            CallStateStr::Ringing => "ringing",
            CallStateStr::Offhook => "offhook",
            CallStateStr::Idle => "idle",
        }
    }
}

/// Call direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CallDirStr {
    /// Incoming.
    Incoming,
    /// Outgoing.
    Outgoing,
    /// Missed.
    Missed,
}

impl CallDirStr {
    pub fn as_str(&self) -> &'static str {
        match self {
            CallDirStr::Incoming => "incoming",
            CallDirStr::Outgoing => "outgoing",
            CallDirStr::Missed => "missed",
        }
    }
}

/// One row of the `calls` table.
#[derive(Debug, Clone, PartialEq, Eq, FromRow, Serialize, Deserialize)]
pub struct CallRow {
    /// Auto-increment primary key.
    pub id: i64,
    /// Owning device.
    pub device_id: uuid::Uuid,
    /// Phone number.
    pub phone_number: String,
    /// Resolved contact name.
    pub contact_name: Option<String>,
    /// State (ringing/offhook/idle).
    pub state: String,
    /// Epoch ms.
    pub started_at: i64,
    /// Epoch ms.
    pub ended_at: Option<i64>,
    /// Direction.
    pub direction: String,
    /// Duration in seconds.
    pub duration_secs: Option<i64>,
    /// SIM slot 0 or 1.
    pub sim_slot: Option<u8>,
}

// ============================================================================
// audit_log
// ============================================================================

/// One row of the `audit_log` table.
#[derive(Debug, Clone, PartialEq, Eq, FromRow, Serialize, Deserialize)]
pub struct AuditLogRow {
    /// Auto-increment primary key.
    pub id: i64,
    /// Unix epoch ms.
    pub timestamp: i64,
    /// Optional device id this event relates to.
    pub device_id: Option<uuid::Uuid>,
    /// Event type, e.g. `pair.success`, `ws.closed`, `device.unpair`.
    pub event: String,
    /// Optional JSON detail.
    pub detail: Option<String>,
}
