// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 PhoneBridge Contributors
//
// This file is part of PhoneBridge. See LICENSE and the dual-licensing
// notice in README.md for details.

//! The `Envelope` is the single wire format for all PhoneBridge messages.
//!
//! ```text
//! {
//!   "v": 1,
//!   "id": "uuid-v4",
//!   "ts": 1717000000000,
//!   "type": "device.hello",
//!   "device_id": "uuid-v4",
//!   "payload": { ... }
//! }
//! ```

use std::fmt;

use chrono::{DateTime, TimeZone, Utc};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

/// All message types defined in the protocol v1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MessageType {
    /// First message after WebSocket open.
    #[serde(rename = "device.hello")]
    DeviceHello,
    /// Liveness ping.
    #[serde(rename = "device.heartbeat")]
    DeviceHeartbeat,
    /// Device info (battery, network, version).
    #[serde(rename = "device.info.update")]
    DeviceInfoUpdate,
    /// Start pairing.
    #[serde(rename = "device.pair.request")]
    DevicePairRequest,
    /// Pairing challenge (ephemeral pubkey + 4-digit code).
    #[serde(rename = "device.pair.challenge")]
    DevicePairChallenge,
    /// User accepted/rejected the pairing code.
    #[serde(rename = "device.pair.confirm")]
    DevicePairConfirm,
    /// Pairing accepted, ready to send cert.
    #[serde(rename = "device.pair.accept")]
    DevicePairAccept,
    /// Pairing rejected.
    #[serde(rename = "device.pair.reject")]
    DevicePairReject,
    /// Pairing complete (sends cert PEM + fingerprint).
    #[serde(rename = "device.pair.complete")]
    DevicePairComplete,
    /// Unpair request.
    #[serde(rename = "device.unpair")]
    DeviceUnpair,
    /// New notification on the phone.
    #[serde(rename = "notification.received")]
    NotificationReceived,
    /// Notification dismissed on the phone.
    #[serde(rename = "notification.dismissed")]
    NotificationDismissed,
    /// Incoming SMS.
    #[serde(rename = "sms.received")]
    SmsReceived,
    /// Send an SMS.
    #[serde(rename = "sms.send.request")]
    SmsSendRequest,
    /// Result of an SMS send.
    #[serde(rename = "sms.send.result")]
    SmsSendResult,
    /// Request recent SMS history.
    #[serde(rename = "sms.list.request")]
    SmsListRequest,
    /// Response with SMS history.
    #[serde(rename = "sms.list.result")]
    SmsListResult,
    /// Phone state change.
    #[serde(rename = "call.state")]
    CallState,
    /// Incoming call started.
    #[serde(rename = "call.incoming")]
    CallIncoming,
    /// Answer the ringing call.
    #[serde(rename = "call.answer.request")]
    CallAnswerRequest,
    /// Hang up.
    #[serde(rename = "call.end.request")]
    CallEndRequest,
    /// Place an outgoing call.
    #[serde(rename = "call.dial.request")]
    CallDialRequest,
    /// Recent call log.
    #[serde(rename = "call.history")]
    CallHistory,
}

impl MessageType {
    /// The dotted wire string.
    pub fn as_str(&self) -> &'static str {
        match self {
            MessageType::DeviceHello => "device.hello",
            MessageType::DeviceHeartbeat => "device.heartbeat",
            MessageType::DeviceInfoUpdate => "device.info.update",
            MessageType::DevicePairRequest => "device.pair.request",
            MessageType::DevicePairChallenge => "device.pair.challenge",
            MessageType::DevicePairConfirm => "device.pair.confirm",
            MessageType::DevicePairAccept => "device.pair.accept",
            MessageType::DevicePairReject => "device.pair.reject",
            MessageType::DevicePairComplete => "device.pair.complete",
            MessageType::DeviceUnpair => "device.unpair",
            MessageType::NotificationReceived => "notification.received",
            MessageType::NotificationDismissed => "notification.dismissed",
            MessageType::SmsReceived => "sms.received",
            MessageType::SmsSendRequest => "sms.send.request",
            MessageType::SmsSendResult => "sms.send.result",
            MessageType::SmsListRequest => "sms.list.request",
            MessageType::SmsListResult => "sms.list.result",
            MessageType::CallState => "call.state",
            MessageType::CallIncoming => "call.incoming",
            MessageType::CallAnswerRequest => "call.answer.request",
            MessageType::CallEndRequest => "call.end.request",
            MessageType::CallDialRequest => "call.dial.request",
            MessageType::CallHistory => "call.history",
        }
    }
}

impl fmt::Display for MessageType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// All messages on the wire are wrapped in an `Envelope`.
///
/// `v` is the protocol version (currently always `1`).
///
/// `payload` is a `serde_json::Value` (untyped) so the wire format stays
/// fully dynamic. Use [`Envelope::parse_payload`] to decode the payload into
/// a concrete type. The handler layer is responsible for choosing the
/// correct target type based on [`Envelope::message_type`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    /// Protocol version. Always `1` in MVP.
    pub v: u16,
    /// Per-message unique id (UUIDv4). Used for de-duplication.
    pub id: Uuid,
    /// Unix epoch milliseconds when the message was created.
    pub ts: i64,
    /// Dotted message type.
    #[serde(rename = "type")]
    pub message_type: MessageType,
    /// Stable id of the sending device.
    pub device_id: Uuid,
    /// Type-specific body. Stored as untyped JSON.
    pub payload: serde_json::Value,
}

impl Envelope {
    /// Construct an envelope with a fresh id and the current timestamp.
    pub fn new<P: Serialize>(
        message_type: MessageType,
        device_id: Uuid,
        payload: P,
    ) -> Result<Self, serde_json::Error> {
        Ok(Self {
            v: 1,
            id: Uuid::new_v4(),
            ts: Utc::now().timestamp_millis(),
            message_type,
            device_id,
            payload: serde_json::to_value(payload)?,
        })
    }

    /// Construct an envelope with a specific timestamp (for tests).
    pub fn with_ts<P: Serialize>(
        message_type: MessageType,
        device_id: Uuid,
        ts: i64,
        payload: P,
    ) -> Result<Self, serde_json::Error> {
        Ok(Self {
            v: 1,
            id: Uuid::new_v4(),
            ts,
            message_type,
            device_id,
            payload: serde_json::to_value(payload)?,
        })
    }

    /// Construct the timestamp as a `DateTime<Utc>`.
    pub fn timestamp(&self) -> DateTime<Utc> {
        Utc.timestamp_millis_opt(self.ts).single().unwrap_or_else(Utc::now)
    }

    /// Decode `payload` into a concrete type. The caller is responsible for
    /// choosing the right type based on `message_type`.
    pub fn parse_payload<T: DeserializeOwned>(&self) -> Result<T, serde_json::Error> {
        serde_json::from_value(self.payload.clone())
    }

    /// Serialize to a JSON string.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("Envelope serialization is infallible")
    }

    /// Serialize to a pretty JSON string.
    pub fn to_json_pretty(&self) -> String {
        serde_json::to_string_pretty(self).expect("Envelope serialization is infallible")
    }

    /// Parse from a JSON string.
    pub fn from_json(s: &str) -> Result<Self, EnvelopeError> {
        Ok(serde_json::from_str(s)?)
    }
}

/// Errors that can occur parsing an [`Envelope`].
#[derive(Debug, Error)]
pub enum EnvelopeError {
    /// JSON deserialization failed.
    #[error("invalid envelope JSON: {0}")]
    Json(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::payload::DeviceHello;
    use crate::types::DeviceType;
    use serde_json::json;

    #[test]
    fn envelope_round_trips() {
        let env = Envelope::new(
            MessageType::DeviceHello,
            Uuid::nil(),
            DeviceHello {
                name: "Pixel 8 Pro".into(),
                device_type: DeviceType::Android,
                protocol_version: 1,
                pubkey: "AAAA".into(),
                port: Some(8443),
                manufacturer: Some("Google".into()),
                model: Some("Pixel 8 Pro".into()),
                hardware_id: None,
            },
        )
        .unwrap();

        let s = env.to_json();
        let parsed = Envelope::from_json(&s).unwrap();
        assert_eq!(parsed.message_type, MessageType::DeviceHello);
        assert_eq!(parsed.v, 1);
        assert_eq!(parsed.device_id, Uuid::nil());

        // Decode the payload back into a typed struct.
        let hello: DeviceHello = parsed.parse_payload().unwrap();
        assert_eq!(hello.name, "Pixel 8 Pro");
        assert_eq!(hello.device_type, DeviceType::Android);
    }

    #[test]
    fn message_type_wire_string_matches_schema_enum() {
        for (mt, expected) in [
            (MessageType::DeviceHello, "device.hello"),
            (MessageType::DevicePairChallenge, "device.pair.challenge"),
            (MessageType::NotificationReceived, "notification.received"),
            (MessageType::SmsSendRequest, "sms.send.request"),
            (MessageType::CallState, "call.state"),
            (MessageType::CallDialRequest, "call.dial.request"),
            (MessageType::CallHistory, "call.history"),
        ] {
            assert_eq!(mt.as_str(), expected);
            let j = serde_json::to_string(&mt).unwrap();
            assert_eq!(j, format!("\"{expected}\""));
        }
    }

    #[test]
    fn unknown_message_type_rejected() {
        let raw = json!({
            "v": 1,
            "id": Uuid::new_v4(),
            "ts": 1,
            "type": "device.nope.this.does.not.exist",
            "device_id": Uuid::new_v4(),
            "payload": {}
        });
        let r: Result<Envelope, _> = serde_json::from_value(raw);
        assert!(r.is_err(), "unknown message type must be rejected");
    }
}
