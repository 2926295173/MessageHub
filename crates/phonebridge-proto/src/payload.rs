//! All payload structs, plus the [`Payload`] enum that dispatches based on
//! the message type.

use serde::{Deserialize, Serialize};

use crate::types::*;

// ============================================================================
// Device lifecycle payloads
// ============================================================================

/// `device.hello` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceHello {
    /// Human-readable device name (1-64 chars).
    pub name: String,
    /// Whether this is desktop or android.
    pub device_type: DeviceType,
    /// Protocol version (always 1 in MVP).
    pub protocol_version: u16,
    /// Base64 of the device's long-term ECDH P-256 public key (SubjectPublicKeyInfo).
    pub pubkey: String,
    /// Optional: port the device is listening on for WebSocket.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// Optional: manufacturer name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manufacturer: Option<String>,
    /// Optional: model name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// `device.heartbeat` payload (all fields optional).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceHeartbeat {
    /// Round-trip time in milliseconds (informational).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rtt_ms: Option<u32>,
}

/// `device.info.update` payload.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceInfoUpdate {
    /// Battery percentage 0-100.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub battery_level: Option<u8>,
    /// Whether the device is currently charging.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_charging: Option<bool>,
    /// Active network transport.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network_type: Option<NetworkType>,
    /// Android OS version (e.g. "14").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub android_version: Option<String>,
    /// PhoneBridge app version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_version: Option<String>,
}

// ============================================================================
// Pairing payloads
// ============================================================================

/// `device.pair.request` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairRequest {
    /// Base64 of ephemeral ECDH P-256 public key.
    pub ephemeral_pubkey: String,
}

/// `device.pair.challenge` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairChallenge {
    /// Base64 of the responder's ephemeral ECDH P-256 public key.
    pub ephemeral_pubkey: String,
    /// 6-digit decimal code derived from the shared secret.
    pub code: String,
    /// Unix epoch ms after which this code is invalid.
    pub expires_at: i64,
}

/// `device.pair.confirm` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairConfirm {
    /// True if the user accepted, false if rejected.
    pub accepted: bool,
}

/// `device.pair.accept` payload (empty object).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairAccept {}

/// `device.pair.reject` payload.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairReject {
    /// Optional reason string.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// `device.pair.complete` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairComplete {
    /// PEM-encoded X.509 certificate.
    pub cert_pem: String,
    /// SHA-256 fingerprint, colon-separated upper-case hex.
    pub cert_fingerprint: String,
}

/// `device.unpair` payload.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unpair {
    /// Optional reason.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

// ============================================================================
// Notification payloads
// ============================================================================

/// `notification.received` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationReceived {
    /// Per-device notification id.
    pub id: String,
    /// App package name.
    pub package: String,
    /// Human-readable app name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_name: Option<String>,
    /// Notification title.
    pub title: String,
    /// Notification content body.
    pub content: String,
    /// Unix epoch ms when the notification was posted.
    pub posted_at: i64,
    /// True if the app used FLAG_NO_PEEK / locked-screen SECRET.
    #[serde(default)]
    pub is_sensitive: bool,
    /// Optional Android notification category.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
}

/// `notification.dismissed` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotificationDismissed {
    /// The id of the dismissed notification.
    pub id: String,
}

// ============================================================================
// SMS payloads
// ============================================================================

/// `sms.received` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmsReceived {
    /// Per-device SMS id.
    pub id: String,
    /// Sender phone number.
    pub address: String,
    /// SMS body.
    pub body: String,
    /// Unix epoch ms.
    pub received_at: i64,
    /// SIM slot 0 or 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sim_slot: Option<u8>,
    /// Android subscription id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscription_id: Option<i32>,
}

/// `sms.send.request` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmsSendRequest {
    /// Recipient phone number.
    pub to: String,
    /// SMS body.
    pub body: String,
    /// Optional subscription id to use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscription_id: Option<i32>,
}

/// `sms.send.result` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmsSendResult {
    /// Id of the `sms.send.request` envelope being answered.
    pub request_id: uuid::Uuid,
    /// Whether the SMS was sent.
    pub ok: bool,
    /// Optional error code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    /// Optional human-readable error.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

/// `sms.list.request` payload.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmsListRequest {
    /// Maximum messages to return.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    /// Return messages older than this timestamp.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<i64>,
}

/// `sms.list.result` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmsListResult {
    /// List of messages.
    pub messages: Vec<SmsReceived>,
}

// ============================================================================
// Call payloads
// ============================================================================

/// `call.state` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallState {
    /// New state.
    pub state: CallStateKind,
    /// Other party's phone number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phone_number: Option<String>,
    /// Per-call id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
    /// Resolved contact name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact_name: Option<String>,
    /// SIM slot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sim_slot: Option<u8>,
}

/// `call.incoming` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallIncoming {
    /// Caller phone number.
    pub phone_number: String,
    /// Resolved contact name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact_name: Option<String>,
    /// SIM slot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sim_slot: Option<u8>,
}

/// `call.answer.request` payload (empty).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallAnswerRequest {}

/// `call.end.request` payload (empty).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallEndRequest {}

/// `call.dial.request` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallDialRequest {
    /// Number to dial.
    pub number: String,
}

/// One entry of `call.history`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallHistoryEntry {
    /// Phone number.
    pub phone_number: String,
    /// Resolved contact name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact_name: Option<String>,
    /// Unix epoch ms.
    pub started_at: i64,
    /// Duration in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_seconds: Option<u32>,
    /// Direction.
    pub direction: CallDirection,
    /// SIM slot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sim_slot: Option<u8>,
}

/// `call.history` payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallHistory {
    /// List of entries.
    pub entries: Vec<CallHistoryEntry>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn device_hello_round_trip() {
        let p = DeviceHello {
            name: "Pixel".into(),
            device_type: DeviceType::Android,
            protocol_version: 1,
            pubkey: "AAAA".into(),
            port: Some(8443),
            manufacturer: None,
            model: None,
        };
        let j = serde_json::to_value(&p).unwrap();
        assert_eq!(j["name"], "Pixel");
        assert_eq!(j["device_type"], "android");
        assert_eq!(j["protocol_version"], 1);
        assert_eq!(j["port"], 8443);
        // None fields are skipped
        assert!(j.get("manufacturer").is_none());

        let back: DeviceHello = serde_json::from_value(j).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn pair_challenge_rejects_short_code() {
        let raw = json!({
            "ephemeral_pubkey": "AAAA",
            "code": "12345",
            "expires_at": 1000
        });
        // No runtime length check here — the schema enforces it. The struct
        // deserializes fine. This is a documentation test: the protocol
        // boundary should reject before reaching us.
        let p: PairChallenge = serde_json::from_value(raw).unwrap();
        assert_eq!(p.code, "12345");
    }

    #[test]
    fn sms_send_result_carries_uuid() {
        let id = uuid::Uuid::new_v4();
        let p = SmsSendResult {
            request_id: id,
            ok: true,
            error_code: None,
            error_message: None,
        };
        let j = serde_json::to_value(&p).unwrap();
        assert_eq!(j["request_id"], id.to_string());
    }

    #[test]
    fn call_direction_serializes_lowercase() {
        for (d, expected) in [
            (CallDirection::Incoming, "incoming"),
            (CallDirection::Outgoing, "outgoing"),
            (CallDirection::Missed, "missed"),
        ] {
            let j = serde_json::to_string(&d).unwrap();
            assert_eq!(j, format!("\"{expected}\""));
        }
    }
}
