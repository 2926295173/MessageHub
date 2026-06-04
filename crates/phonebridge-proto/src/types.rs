//! Wire-level types shared by all payloads: enums and small structs that
//! don't deserve their own module.

use serde::{Deserialize, Serialize};

/// Stable device type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeviceType {
    /// Desktop daemon.
    Desktop,
    /// Android client.
    Android,
}

/// Sim slot indicator (0-based, dual-SIM support).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(into = "u8", from = "u8")]
pub enum SimSlot {
    /// No SIM / unknown.
    None,
    /// Slot 1.
    Slot0,
    /// Slot 2.
    Slot1,
}

impl From<SimSlot> for u8 {
    fn from(s: SimSlot) -> u8 {
        match s {
            SimSlot::None => 255,
            SimSlot::Slot0 => 0,
            SimSlot::Slot1 => 1,
        }
    }
}

impl From<u8> for SimSlot {
    fn from(v: u8) -> SimSlot {
        match v {
            0 => SimSlot::Slot0,
            1 => SimSlot::Slot1,
            _ => SimSlot::None,
        }
    }
}

/// Active network transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NetworkType {
    /// 802.11 family.
    Wifi,
    /// Mobile data.
    Cellular,
    /// Wired Ethernet.
    Ethernet,
    /// Disconnected.
    None,
}

/// Phone call state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CallStateKind {
    /// No active call, phone idle.
    Idle,
    /// Incoming call ringing.
    Ringing,
    /// Off-hook (active or dialing).
    Offhook,
}

/// SMS / call direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CallDirection {
    /// Incoming call.
    Incoming,
    /// Outgoing call.
    Outgoing,
    /// Missed call.
    Missed,
}
