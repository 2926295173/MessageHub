// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 PhoneBridge Contributors
//
// This file is part of PhoneBridge. See LICENSE and the dual-licensing
// notice in README.md for details.

//! Adapter that implements [`phonebridge_net::WsSink`] on top of the
//! message-center’s storage + audit log + console bus + display bus.

#![forbid(unsafe_code)]
#![allow(missing_docs)]

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use chrono::Utc;
use serde::Serialize;
use serde_json::Value;
use tracing::{info, warn};
use uuid::Uuid;

use phonebridge_net::WsSink;
use phonebridge_proto::{
    CallHistory, CallIncoming, CallState, DeviceHello, DisplayEvent, Envelope, MessageType,
    NotificationDismissed, NotificationReceived, SmsListResult, SmsReceived, SmsSendResult, Unpair,
};
use phonebridge_storage::models::{CallRow, DeviceRow, NotificationRow, SmsDirection, SmsRow};
use phonebridge_storage::Db;

use crate::console_bus::ConsoleBus;
use crate::display_bus::DisplayBus;
use crate::noise_filter;

/// WsSink implementation: persists every event to the message-center’s
/// database, writes an audit log entry, and publishes to **both**
/// the console bus (web UI live-push) and the display bus
/// (desktop notification endpoint).
pub struct CenterSink {
    db: Arc<Db>,
    console_bus: ConsoleBus,
    display_bus: DisplayBus,
}

impl CenterSink {
    /// Construct a new sink bound to the given DB, console bus,
    /// and display bus. The two buses serve different consumers
    /// (web UI vs desktop notification) so the same event is
    /// published to both — the bus layer fan-outs independently.
    pub fn new(db: Arc<Db>, console_bus: ConsoleBus, display_bus: DisplayBus) -> Self {
        Self {
            db,
            console_bus,
            display_bus,
        }
    }

    async fn audit(&self, device_id: Option<Uuid>, event: &str, detail: Option<&str>) {
        let ts = Utc::now().timestamp_millis();
        if let Err(e) = self.db.insert_audit_log(ts, device_id, event, detail).await {
            warn!("audit log write failed: {e}");
        }
    }

    async fn touch_device(&self, device_id: Uuid) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        if let Err(e) = self.db.touch_device(device_id, now).await {
            warn!("touch_device failed: {e}");
        }
    }

    /// Publish a synthetic envelope to the console bus.
    fn publish_console(&self, env: &Envelope) {
        self.console_bus.publish(env);
    }

    /// Publish a [`DisplayEvent`] for the desktop notification
    /// endpoint. `kind` is the wire kind (e.g. `notification.received`),
    /// `envelope_id` is the original wire id, and `payload` is
    /// the full deserialized payload (serialized to JSON).
    ///
    /// The event is run through the [`noise_filter`] module
    /// before being published: a small set of known-noise kinds
    /// (heartbeats, pairing internals, info updates, unpair)
    /// and known-noise payload patterns (our own package, system
    /// packages, transient notification categories) are dropped.
    /// All other events pass through (default allow). See the
    /// `noise_filter` module doc for the full predicate list.
    fn publish_display(
        &self,
        kind: MessageType,
        envelope_id: Uuid,
        device_id: Uuid,
        payload: impl Serialize,
    ) {
        let payload_value: Value = serde_json::to_value(payload).unwrap_or_else(|e| {
            warn!(kind = %kind, error = %e, "display payload serialize failed");
            Value::Null
        });
        let event = DisplayEvent {
            kind: kind.as_str().to_string(),
            device_id,
            envelope_id,
            timestamp: Utc::now().timestamp_millis(),
            payload: payload_value,
            summary: Default::default(),
        };
        if let Some(reason) = noise_filter::should_filter(&event) {
            // Filtered. We log at `debug` because in normal
            // operation this happens many times per minute
            // (heartbeats, system noise); operators who want to
            // see the filter at work can turn the level up.
            tracing::debug!(
                kind = %event.kind,
                reason = %reason,
                "display event filtered by noise_filter"
            );
            return;
        }
        self.display_bus.publish(event);
    }
}

#[async_trait]
impl WsSink for CenterSink {
    async fn on_notification(
        &self,
        envelope_id: Uuid,
        device_id: Uuid,
        env: &NotificationReceived,
    ) {
        let row = NotificationRow {
            id: env.id.clone(),
            device_id,
            package_name: env.package.clone(),
            app_name: env.app_name.clone(),
            title: env.title.clone(),
            content: env.content.clone(),
            posted_at: env.posted_at,
            is_sensitive: env.is_sensitive,
            category: env.category.clone(),
            read: false,
        };
        if let Err(e) = self.db.insert_notification(&row).await {
            warn!(%device_id, "insert notification failed: {e}");
        }
        if let Ok(e) = Envelope::new(MessageType::NotificationReceived, device_id, env.clone()) {
            self.publish_console(&e);
        }
        self.publish_display(
            MessageType::NotificationReceived,
            envelope_id,
            device_id,
            env,
        );
    }

    async fn on_notification_dismissed(
        &self,
        envelope_id: Uuid,
        device_id: Uuid,
        env: &NotificationDismissed,
    ) {
        if let Err(e) = self.db.mark_notification_read(device_id, &env.id).await {
            warn!(%device_id, "mark notification read failed: {e}");
        }
        if let Ok(e) = Envelope::new(MessageType::NotificationDismissed, device_id, env.clone()) {
            self.publish_console(&e);
        }
        self.publish_display(
            MessageType::NotificationDismissed,
            envelope_id,
            device_id,
            env,
        );
    }

    async fn on_sms_received(&self, envelope_id: Uuid, device_id: Uuid, env: &SmsReceived) {
        let row = SmsRow {
            id: env.id.clone(),
            device_id,
            sim_slot: env.sim_slot,
            phone_number: env.address.clone(),
            body: env.body.clone(),
            direction: SmsDirection::In.as_str().to_string(),
            timestamp: env.received_at,
        };
        if let Err(e) = self.db.insert_sms(&row).await {
            warn!(%device_id, "insert sms failed: {e}");
        }
        if let Ok(e) = Envelope::new(MessageType::SmsReceived, device_id, env.clone()) {
            self.publish_console(&e);
        }
        self.publish_display(MessageType::SmsReceived, envelope_id, device_id, env);
    }

    async fn on_sms_send_result(&self, envelope_id: Uuid, device_id: Uuid, env: &SmsSendResult) {
        info!(%device_id, request_id = %env.request_id, ok = env.ok, "sms.send.result");
        // Forward to display too — the desktop endpoint uses
        // this to show a "Sent to 138…" / "Failed: …" toast.
        if let Ok(e) = Envelope::new(MessageType::SmsSendResult, device_id, env.clone()) {
            self.publish_console(&e);
        }
        self.publish_display(MessageType::SmsSendResult, envelope_id, device_id, env);
    }

    async fn on_call_state(&self, envelope_id: Uuid, device_id: Uuid, env: &CallState) {
        let row = CallRow {
            id: 0,
            device_id,
            phone_number: env.phone_number.clone().unwrap_or_default(),
            contact_name: env.contact_name.clone(),
            state: match env.state {
                phonebridge_proto::CallStateKind::Idle => "idle",
                phonebridge_proto::CallStateKind::Ringing => "ringing",
                phonebridge_proto::CallStateKind::Offhook => "offhook",
            }
            .to_string(),
            started_at: Utc::now().timestamp_millis(),
            ended_at: None,
            direction: "incoming".to_string(),
            duration_secs: None,
            sim_slot: env.sim_slot,
        };
        if let Err(e) = self.db.insert_call(&row).await {
            warn!(%device_id, "insert call state failed: {e}");
        }
        if let Ok(e) = Envelope::new(MessageType::CallState, device_id, env.clone()) {
            self.publish_console(&e);
        }
        self.publish_display(MessageType::CallState, envelope_id, device_id, env);
    }

    async fn on_call_incoming(&self, envelope_id: Uuid, device_id: Uuid, env: &CallIncoming) {
        let row = CallRow {
            id: 0,
            device_id,
            phone_number: env.phone_number.clone(),
            contact_name: env.contact_name.clone(),
            state: "ringing".to_string(),
            started_at: Utc::now().timestamp_millis(),
            ended_at: None,
            direction: "incoming".to_string(),
            duration_secs: None,
            sim_slot: env.sim_slot,
        };
        if let Err(e) = self.db.insert_call(&row).await {
            warn!(%device_id, "insert call incoming failed: {e}");
        }
        if let Ok(e) = Envelope::new(MessageType::CallIncoming, device_id, env.clone()) {
            self.publish_console(&e);
        }
        self.publish_display(MessageType::CallIncoming, envelope_id, device_id, env);
    }

    async fn on_call_history(&self, envelope_id: Uuid, device_id: Uuid, env: &CallHistory) {
        for entry in &env.entries {
            let row = CallRow {
                id: 0,
                device_id,
                phone_number: entry.phone_number.clone(),
                contact_name: entry.contact_name.clone(),
                state: "ended".to_string(),
                started_at: entry.started_at,
                ended_at: if entry.duration_seconds.is_some() {
                    Some(entry.started_at + entry.duration_seconds.unwrap_or(0) as i64 * 1000)
                } else {
                    None
                },
                direction: match entry.direction {
                    phonebridge_proto::CallDirection::Incoming => "incoming",
                    phonebridge_proto::CallDirection::Outgoing => "outgoing",
                    phonebridge_proto::CallDirection::Missed => "missed",
                }
                .to_string(),
                duration_secs: entry.duration_seconds.map(|d| d as i64),
                sim_slot: entry.sim_slot,
            };
            if let Err(e) = self.db.insert_call(&row).await {
                warn!(%device_id, "insert call history entry failed: {e}");
            }
        }
    }

    async fn on_sms_list_result(&self, envelope_id: Uuid, device_id: Uuid, env: &SmsListResult) {
        for m in &env.messages {
            let row = SmsRow {
                id: m.id.clone(),
                device_id,
                sim_slot: m.sim_slot,
                phone_number: m.address.clone(),
                body: m.body.clone(),
                direction: SmsDirection::In.as_str().to_string(),
                timestamp: m.received_at,
            };
            if let Err(e) = self.db.insert_sms(&row).await {
                warn!(%device_id, "insert sms (from list) failed: {e}");
            }
        }
        self.publish_display(MessageType::SmsListResult, envelope_id, device_id, env);
    }

    async fn on_hello(&self, envelope_id: Uuid, device_id: Uuid, env: &DeviceHello) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let row = DeviceRow {
            id: 0,
            name: env.name.clone(),
            device_id,
            public_key: env.pubkey.clone(),
            last_seen: now,
            paired: false,
            hardware_id: env.hardware_id.clone(),
        };
        if let Err(e) = self.db.upsert_device(&row).await {
            warn!(%device_id, "upsert device failed: {e}");
        }
        self.touch_device(device_id).await;
        self.audit(Some(device_id), "ws.connected", None).await;
        if let Ok(e) = Envelope::new(MessageType::DeviceHello, device_id, env.clone()) {
            self.publish_console(&e);
        }
        self.publish_display(MessageType::DeviceHello, envelope_id, device_id, env);
    }

    async fn on_unpair(&self, _envelope_id: Uuid, device_id: Uuid, env: &Unpair) {
        self.audit(
            Some(device_id),
            "device.unpair",
            Some(&env.reason.clone().unwrap_or_default()),
        )
        .await;
        if let Err(e) = self.db.remove_device(device_id).await {
            warn!(%device_id, "remove device failed: {e}");
        }
    }

    async fn on_disconnect(&self, device_id: Uuid) {
        self.touch_device(device_id).await;
        self.audit(Some(device_id), "ws.closed", None).await;
    }
}
