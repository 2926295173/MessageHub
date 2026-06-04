//! PhoneBridge wire protocol types.
//!
//! Source of truth: `schema/protocol.schema.json`. This crate mirrors the
//! schema 1:1 so that both Rust (daemon) and Kotlin (Android client) can
//! derive their types from the same JSON definition.
//!
//! All messages are JSON, UTF-8, wrapped in an [`Envelope`]. Every type
//! round-trips through `serde_json` losslessly.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod envelope;
pub mod payload;
pub mod types;

pub use envelope::{Envelope, EnvelopeError, MessageType};
pub use payload::{
    CallAnswerRequest, CallDialRequest, CallEndRequest, CallHistory, CallHistoryEntry,
    CallIncoming, CallState, DeviceHello, DeviceHeartbeat, DeviceInfoUpdate,
    NotificationDismissed, NotificationReceived, PairAccept, PairChallenge, PairComplete,
    PairConfirm, PairReject, PairRequest, SmsListRequest, SmsListResult, SmsReceived,
    SmsSendRequest, SmsSendResult, Unpair,
};
pub use types::{CallDirection, CallStateKind, DeviceType, NetworkType, SimSlot};
