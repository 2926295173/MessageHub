// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 PhoneBridge Contributors
//
// This file is part of PhoneBridge. See LICENSE and the dual-licensing
// notice in README.md for details.

//! PhoneBridge wire protocol types.
//!
//! Source of truth: `schema/protocol.schema.json`. This crate mirrors the
//! schema 1:1 so that both Rust (message-center) and Kotlin (Android client) can
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
    ActionResultEvent, CallAnswerRequest, CallDialRequest, CallEndRequest, CallHistory,
    CallHistoryEntry, CallIncoming, CallState, DeviceHeartbeat, DeviceHello, DeviceInfoUpdate,
    DisplayAction, DisplayEvent, NotificationDismissed, NotificationReceived, PairAccept,
    PairChallenge, PairComplete, PairConfirm, PairReject, PairRequest, SmsListRequest,
    SmsListResult, SmsReceived, SmsSendRequest, SmsSendResult, Unpair,
};
pub use types::{CallDirection, CallStateKind, DeviceType, NetworkType, SimSlot};
