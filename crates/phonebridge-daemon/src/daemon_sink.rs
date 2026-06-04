//! Adapter that implements [`phonebridge_net::WsSink`] on top of the
//! daemon's storage + audit log + console bus.

#![forbid(unsafe_code)]
#![allow(missing_docs)]

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use chrono::Utc;
use tracing::{info, warn};
use uuid::Uuid;

use phonebridge_net::WsSink;
use phonebridge_proto::{
    CallHistory, CallIncoming, CallState, DeviceHello, Envelope, MessageType,
    NotificationDismissed, NotificationReceived, SmsListResult, SmsReceived, SmsSendResult, Unpair,
};
use phonebridge_storage::models::{
    CallRow, DeviceRow, NotificationRow, SmsRow, SmsDirection,
};
use phonebridge_storage::Db;

use crate::console_bus::ConsoleBus;

/// WsSink implementation: persists every event to the daemon's
/// database, writes an audit log entry, and publishes to the console
/// bus for live-push to the web UI.
pub struct DaemonSink {
    db: Arc<Db>,
    console_bus: ConsoleBus,
}

impl DaemonSink {
    /// Construct a new sink bound to the given DB and console bus.
    pub fn new(db: Arc<Db>, console_bus: ConsoleBus) -> Self {
        Self { db, console_bus }
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
}

#[async_trait]
impl WsSink for DaemonSink {
    async fn on_notification(&self, device_id: Uuid, env: &NotificationReceived) {
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
    }

    async fn on_notification_dismissed(&self, device_id: Uuid, env: &NotificationDismissed) {
        if let Err(e) = self.db.mark_notification_read(device_id, &env.id).await {
            warn!(%device_id, "mark notification read failed: {e}");
        }
    }

    async fn on_sms_received(&self, device_id: Uuid, env: &SmsReceived) {
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
    }

    async fn on_sms_send_result(&self, device_id: Uuid, env: &SmsSendResult) {
        info!(%device_id, request_id = %env.request_id, ok = env.ok, "sms.send.result");
    }

    async fn on_call_state(&self, device_id: Uuid, env: &CallState) {
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
    }

    async fn on_call_incoming(&self, device_id: Uuid, env: &CallIncoming) {
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
    }

    async fn on_call_history(&self, device_id: Uuid, env: &CallHistory) {
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

    async fn on_sms_list_result(&self, device_id: Uuid, env: &SmsListResult) {
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
    }

    async fn on_hello(&self, device_id: Uuid, env: &DeviceHello) {
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
        };
        if let Err(e) = self.db.upsert_device(&row).await {
            warn!(%device_id, "upsert device failed: {e}");
        }
        self.touch_device(device_id).await;
        self.audit(Some(device_id), "ws.connected", None).await;
        if let Ok(e) = Envelope::new(MessageType::DeviceHello, device_id, env.clone()) {
            self.publish_console(&e);
        }
    }

    async fn on_unpair(&self, device_id: Uuid, env: &Unpair) {
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
