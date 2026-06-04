//! REST API surface.
//!
//! All endpoints are under `/api/v1`.
//!
//! - `GET  /health`            — health probe
//! - `GET  /cert`              — our cert PEM + fingerprint + device id
//! - `GET  /devices`           — list devices
//! - `GET  /devices/:id`       — one device
//! - `DELETE /devices/:id`     — unpair + remove
//! - `GET  /pairings`          — list in-flight + persisted pairings
//! - `POST /pair/start`        — find a connected device and send pair.request
//! - `GET  /notifications`     — list notifications
//! - `GET  /notifications/stats` — counts (total, unread, by package)
//! - `POST /notifications/:device_id/:id/read` — mark one read
//! - `GET  /sms`               — list SMS
//! - `GET  /sms/conversations` — group by phone number
//! - `POST /sms`               — send an SMS (forwards to android via WS)
//! - `GET  /calls`             — list calls
//! - `GET  /dashboard`         — aggregate counts
//! - `GET  /audit`             — recent audit log
//! - `POST /dial`              — place an outgoing call (forwards to android)

use axum::Json;
use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use chrono::Utc;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, warn};
use uuid::Uuid;

use phonebridge_net::DeviceRegistry;
use phonebridge_proto::{MessageType, SmsSendRequest, SmsSendResult};
use phonebridge_storage::models::{
    AuditLogRow, CallRow, DeviceRow, NotificationRow, PairingRow, SmsRow,
};

use crate::app_state::AppState;

/// Build the `/api/v1` sub-router.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .route("/cert", get(get_cert))
        .route("/devices", get(list_devices))
        .route("/devices/:id", get(get_device))
        .route("/devices/:id", delete(remove_device))
        .route("/pairings", get(list_pairings))
        .route("/pair/start", post(pair_start))
        .route("/notifications", get(list_notifications))
        .route("/notifications/stats", get(notifications_stats))
        .route(
            "/notifications/:device_id/:id/read",
            post(mark_notification_read),
        )
        .route("/sms", get(list_sms))
        .route("/sms/conversations", get(list_sms_conversations))
        .route("/sms", post(send_sms))
        .route("/calls", get(list_calls))
        .route("/dashboard", get(dashboard))
        .route("/audit", get(list_audit))
        .route("/dial", post(dial))
}

// ============================================================================
// health + cert
// ============================================================================

#[derive(Debug, Serialize)]
struct HealthBody {
    status: &'static str,
    version: &'static str,
    our_device_id: Uuid,
    our_fingerprint: String,
    paired_devices: i64,
    online_devices: i64,
}

async fn health(State(state): State<AppState>) -> impl IntoResponse {
    let paired = count_paired(&state).await.unwrap_or(0);
    let body = HealthBody {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        our_device_id: state.our_device_id,
        our_fingerprint: state.our_fingerprint.read().clone(),
        paired_devices: paired,
        online_devices: state.registry.connected_count() as i64,
    };
    (StatusCode::OK, Json(body))
}

async fn count_paired(state: &AppState) -> anyhow::Result<i64> {
    use sqlx::Row;
    let row = sqlx::query("SELECT COUNT(*) AS n FROM devices WHERE paired = 1")
        .fetch_one(state.db.pool())
        .await?;
    let n: i64 = row.try_get("n")?;
    Ok(n)
}

async fn get_cert(State(state): State<AppState>) -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({
            "device_id": state.our_device_id,
            "name": state.our_name.read().clone(),
            "fingerprint": state.our_fingerprint.read().clone(),
            "public_key": state.our_public_key_b64.read().clone(),
        })),
    )
}

// ============================================================================
// devices
// ============================================================================

#[derive(Debug, Serialize)]
struct DeviceListResponse {
    devices: Vec<DeviceRow>,
}

async fn list_devices(State(state): State<AppState>) -> impl IntoResponse {
    match state.db.list_devices().await {
        Ok(devs) => (StatusCode::OK, Json(DeviceListResponse { devices: devs })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("db: {e}")).into_response(),
    }
}

async fn get_device(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    match state.db.get_device(id).await {
        Ok(Some(d)) => (StatusCode::OK, Json(d)).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, "device not found").into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("db: {e}")).into_response(),
    }
}

async fn remove_device(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    state.pairing.remove(&id);
    state.registry.unregister(&id);
    state.pin_store.write().remove(&id);
    // Audit log + remove from DB.
    let _ = state
        .db
        .insert_audit_log(Utc::now().timestamp_millis(), Some(id), "device.unpair", None)
        .await;
    if let Err(e) = state.db.remove_device(id).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, format!("db: {e}")).into_response();
    }
    (StatusCode::NO_CONTENT, "").into_response()
}

// ============================================================================
// pairings
// ============================================================================

#[derive(Debug, Serialize)]
struct PairingsResponse {
    in_flight: Vec<Uuid>,
    persisted: Vec<PairingRow>,
}

async fn list_pairings(State(state): State<AppState>) -> impl IntoResponse {
    let in_flight: Vec<Uuid> = state
        .pairing
        .list_paired()
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    let persisted = match sqlx::query_as::<_, PairingRow>(
        "SELECT id, device_id, cert_pem, cert_fingerprint, paired_at FROM pairings ORDER BY paired_at DESC",
    )
    .fetch_all(state.db.pool())
    .await
    {
        Ok(rows) => rows,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("db: {e}"),
            )
                .into_response();
        }
    };
    (
        StatusCode::OK,
        Json(PairingsResponse {
            in_flight,
            persisted,
        }),
    )
        .into_response()
}

#[derive(Debug, Deserialize)]
struct PairStartRequest {
    /// The device id to pair with (must be currently connected).
    device_id: Uuid,
}

/// `POST /api/v1/pair/start` — initiate pairing with a connected device.
///
/// In the MVP flow, the android opens the WS first and sends `device.hello`.
/// The desktop's `ws_handler` registers a Responder state machine. When
/// the user clicks "Pair" in the web console, this endpoint finds the
/// device and sends `device.pair.request` over the WS to trigger the
/// pairing flow.
async fn pair_start(
    State(state): State<AppState>,
    Json(req): Json<PairStartRequest>,
) -> impl IntoResponse {
    let device_id = req.device_id;
    // The device must be connected. The Android sends hello; the
    // registry holds the channel.
    if !state.registry.connected_ids().contains(&device_id) {
        return (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "device not connected",
                "device_id": device_id,
                "connected": state.registry.connected_ids(),
            })),
        )
            .into_response();
    }

    // Replace any existing session for this device with a fresh
    // Initiator (desktop-driven) state machine.
    let mut initiator = match phonebridge_net::pairing::Initiator::start(
        device_id,
        state.our_name.read().clone(),
    ) {
        Ok(i) => i,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("initiator start: {e}"),
            )
                .into_response();
        }
    };
    let env = match initiator.build_request_envelope(state.our_device_id) {
        Ok(e) => e,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("build request: {e}"),
            )
                .into_response();
        }
    };
    if let Err(e) = state.registry.try_send(device_id, env).await {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": format!("send failed: {e}"),
                "device_id": device_id,
            })),
        )
            .into_response();
    }
    info!(%device_id, "pair_start: sent device.pair.request");

    // We don't keep the initiator state in the pairing map for M3 (the
    // desktop's own pair state isn't part of the WS handler's per-conn
    // map). The actual state transitions are done in the WS handler
    // when the responses come back.

    (
        StatusCode::OK,
        Json(json!({
            "status": "pair.request sent",
            "device_id": device_id,
        })),
    )
        .into_response()
}

// ============================================================================
// notifications
// ============================================================================

#[derive(Debug, Deserialize)]
struct ListNotificationsQuery {
    device_id: Option<Uuid>,
    limit: Option<i64>,
    package: Option<String>,
    unread_only: Option<bool>,
}

#[derive(Debug, Serialize)]
struct NotificationsResponse {
    notifications: Vec<NotificationRow>,
}

async fn list_notifications(
    State(state): State<AppState>,
    Query(q): Query<ListNotificationsQuery>,
) -> impl IntoResponse {
    let limit = q.limit.unwrap_or(50).clamp(1, 500);
    match state
        .db
        .list_notifications(q.device_id, limit, q.unread_only.unwrap_or(false), q.package.as_deref())
        .await
    {
        Ok(rows) => (StatusCode::OK, Json(NotificationsResponse { notifications: rows })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("db: {e}")).into_response(),
    }
}

#[derive(Debug, Serialize)]
struct NotificationsStats {
    total: i64,
    unread: i64,
    by_package: Vec<PackageCount>,
}

#[derive(Debug, Serialize)]
struct PackageCount {
    package: String,
    count: i64,
}

async fn notifications_stats(
    State(state): State<AppState>,
    Query(q): Query<ListNotificationsQuery>,
) -> impl IntoResponse {
    let total = state
        .db
        .list_notifications(q.device_id, 10000, false, None)
        .await
        .map(|v| v.len() as i64)
        .unwrap_or(0);
    let unread = state
        .db
        .count_unread_notifications(q.device_id)
        .await
        .unwrap_or(0);
    let by_package = state
        .db
        .count_notifications_by_package(q.device_id)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|(p, c)| PackageCount { package: p, count: c })
        .collect();
    (
        StatusCode::OK,
        Json(NotificationsStats {
            total,
            unread,
            by_package,
        }),
    )
        .into_response()
}

async fn mark_notification_read(
    State(state): State<AppState>,
    Path((device_id, id)): Path<(Uuid, String)>,
) -> impl IntoResponse {
    if let Err(e) = state.db.mark_notification_read(device_id, &id).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, format!("db: {e}")).into_response();
    }
    (StatusCode::NO_CONTENT, "").into_response()
}

// ============================================================================
// sms
// ============================================================================

#[derive(Debug, Deserialize)]
struct ListSmsQuery {
    device_id: Option<Uuid>,
    phone_number: Option<String>,
    limit: Option<i64>,
}

#[derive(Debug, Serialize)]
struct SmsListResponse {
    messages: Vec<SmsRow>,
}

async fn list_sms(
    State(state): State<AppState>,
    Query(q): Query<ListSmsQuery>,
) -> impl IntoResponse {
    let limit = q.limit.unwrap_or(50).clamp(1, 500);
    match state
        .db
        .list_sms(q.device_id, q.phone_number.as_deref(), limit)
        .await
    {
        Ok(rows) => (StatusCode::OK, Json(SmsListResponse { messages: rows })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("db: {e}")).into_response(),
    }
}

#[derive(Debug, Serialize)]
struct SmsConversationsResponse {
    conversations: Vec<SmsConversation>,
}

#[derive(Debug, Serialize)]
struct SmsConversation {
    address: String,
    last_timestamp: i64,
    count: i64,
}

async fn list_sms_conversations(
    State(state): State<AppState>,
    Query(q): Query<ListSmsQuery>,
) -> impl IntoResponse {
    match state.db.list_sms_conversations(q.device_id).await {
        Ok(rows) => {
            let conversations = rows
                .into_iter()
                .map(|(a, l, c)| SmsConversation {
                    address: a,
                    last_timestamp: l,
                    count: c,
                })
                .collect();
            (
                StatusCode::OK,
                Json(SmsConversationsResponse { conversations }),
            )
                .into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("db: {e}")).into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct SendSmsRequest {
    device_id: Uuid,
    to: String,
    body: String,
    #[serde(default)]
    subscription_id: Option<i32>,
}

/// In-memory map of pending SMS sends: `envelope_id` → list of
/// waiters. The WS handler's `on_sms_send_result` notifies the waiter.
type SmsWaiters = Arc<Mutex<HashMap<Uuid, Vec<oneshot::Sender<Result<SmsSendResult, String>>>>>>;
#[allow(dead_code)]
fn _sms_waiters_marker(_w: &SmsWaiters) {}
use tokio::sync::oneshot;

/// `POST /sms` — send an SMS via the device, return the result.
async fn send_sms(
    State(state): State<AppState>,
    Json(req): Json<SendSmsRequest>,
) -> impl IntoResponse {
    if !state.registry.connected_ids().contains(&req.device_id) {
        return (
            StatusCode::CONFLICT,
            Json(json!({"error": "device not connected"})),
        )
            .into_response();
    }
    let request_id = Uuid::new_v4();
    let env = match Envelope::new(
        MessageType::SmsSendRequest,
        state.our_device_id,
        SmsSendRequest {
            to: req.to.clone(),
            body: req.body.clone(),
            subscription_id: req.subscription_id,
        },
    ) {
        Ok(e) => e,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("build envelope: {e}"),
            )
                .into_response();
        }
    };
    // Mutate the envelope id to the request_id so we can match the result.
    let mut env = env;
    env.id = request_id;
    if let Err(e) = state.registry.try_send(req.device_id, env).await {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": format!("send: {e}")})),
        )
            .into_response();
    }

    // Persist the outgoing SMS as a row in `out` direction immediately.
    let row = SmsRow {
        id: request_id.to_string(),
        device_id: req.device_id,
        sim_slot: None,
        phone_number: req.to.clone(),
        body: req.body.clone(),
        direction: "out".into(),
        timestamp: Utc::now().timestamp_millis(),
    };
    let _ = state.db.insert_sms(&row).await;

    // For MVP we don't await the `sms.send.result` — the android may
    // respond asynchronously and we don't have a per-request waiter
    // map wired through. The result is persisted when it arrives.
    (
        StatusCode::ACCEPTED,
        Json(json!({
            "status": "queued",
            "request_id": request_id,
            "device_id": req.device_id,
            "to": req.to,
        })),
    )
        .into_response()
}

// ============================================================================
// calls
// ============================================================================

#[derive(Debug, Deserialize)]
struct ListCallsQuery {
    device_id: Option<Uuid>,
    limit: Option<i64>,
}

#[derive(Debug, Serialize)]
struct CallsResponse {
    calls: Vec<CallRow>,
}

async fn list_calls(
    State(state): State<AppState>,
    Query(q): Query<ListCallsQuery>,
) -> impl IntoResponse {
    let limit = q.limit.unwrap_or(50).clamp(1, 500);
    match state.db.list_calls(q.device_id, limit).await {
        Ok(rows) => (StatusCode::OK, Json(CallsResponse { calls: rows })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("db: {e}")).into_response(),
    }
}

#[derive(Debug, Deserialize)]
struct DialRequest {
    device_id: Uuid,
    number: String,
}

async fn dial(
    State(state): State<AppState>,
    Json(req): Json<DialRequest>,
) -> impl IntoResponse {
    if !state.registry.connected_ids().contains(&req.device_id) {
        return (
            StatusCode::CONFLICT,
            Json(json!({"error": "device not connected"})),
        )
            .into_response();
    }
    let env = match Envelope::new(
        MessageType::CallDialRequest,
        state.our_device_id,
        phonebridge_proto::CallDialRequest {
            number: req.number.clone(),
        },
    ) {
        Ok(e) => e,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("build envelope: {e}"),
            )
                .into_response();
        }
    };
    if let Err(e) = state.registry.try_send(req.device_id, env).await {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": format!("send: {e}")})),
        )
            .into_response();
    }
    let _ = state
        .db
        .insert_audit_log(
            Utc::now().timestamp_millis(),
            Some(req.device_id),
            "call.dial",
            Some(&format!("number={}", req.number)),
        )
        .await;
    (
        StatusCode::ACCEPTED,
        Json(json!({
            "status": "dialing",
            "device_id": req.device_id,
            "number": req.number,
        })),
    )
        .into_response()
}

// ============================================================================
// dashboard + audit
// ============================================================================

#[derive(Debug, Deserialize)]
struct DashboardQuery {
    device_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
struct DashboardBody {
    paired_devices: i64,
    online_devices: i64,
    notifications: NotificationCounts,
    sms: SmsCounts,
    calls: CallCounts,
}

#[derive(Debug, Serialize)]
struct NotificationCounts {
    total: i64,
    unread: i64,
}

#[derive(Debug, Serialize)]
struct SmsCounts {
    total: i64,
    conversations: i64,
}

#[derive(Debug, Serialize)]
struct CallCounts {
    total: i64,
    missed: i64,
    ringing: i64,
}

async fn dashboard(
    State(state): State<AppState>,
    Query(q): Query<DashboardQuery>,
) -> impl IntoResponse {
    let paired = count_paired(&state).await.unwrap_or(0);
    let notif_total = state
        .db
        .list_notifications(q.device_id, 100000, false, None)
        .await
        .map(|v| v.len() as i64)
        .unwrap_or(0);
    let notif_unread = state
        .db
        .count_unread_notifications(q.device_id)
        .await
        .unwrap_or(0);
    let sms_total = state
        .db
        .list_sms(q.device_id, None, 100000)
        .await
        .map(|v| v.len() as i64)
        .unwrap_or(0);
    let sms_convos = state
        .db
        .list_sms_conversations(q.device_id)
        .await
        .map(|v| v.len() as i64)
        .unwrap_or(0);
    let calls = state.db.list_calls(q.device_id, 100000).await.unwrap_or_default();
    let call_total = calls.len() as i64;
    let call_missed = calls
        .iter()
        .filter(|c| c.direction == "missed")
        .count() as i64;
    let call_ringing = calls
        .iter()
        .filter(|c| c.state == "ringing")
        .count() as i64;

    let body = DashboardBody {
        paired_devices: paired,
        online_devices: state.registry.connected_count() as i64,
        notifications: NotificationCounts { total: notif_total, unread: notif_unread },
        sms: SmsCounts { total: sms_total, conversations: sms_convos },
        calls: CallCounts {
            total: call_total,
            missed: call_missed,
            ringing: call_ringing,
        },
    };
    (StatusCode::OK, Json(body))
        .into_response()
        .into_response()
}

#[derive(Debug, Deserialize)]
struct AuditQuery {
    limit: Option<i64>,
}

async fn list_audit(
    State(state): State<AppState>,
    Query(q): Query<AuditQuery>,
) -> impl IntoResponse {
    let limit = q.limit.unwrap_or(100).clamp(1, 1000);
    match state.db.list_audit_log(limit).await {
        Ok(rows) => (StatusCode::OK, Json(json!({"entries": rows}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("db: {e}")).into_response(),
    }
}

// ============================================================================
// Use Envelope (avoids unused-import warning)
// ============================================================================
use phonebridge_proto::Envelope;

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode as S};
    use tower::ServiceExt;

    async fn make_state() -> AppState {
        let db = phonebridge_storage::Db::open_memory().await.unwrap();
        db.migrate().await.unwrap();
        AppState::new(
            std::sync::Arc::new(phonebridge_core::Config::default()),
            std::sync::Arc::new(db),
            phonebridge_net::DeviceRegistry::new(),
            Uuid::new_v4(),
            "PUBKEYB64".into(),
            "DE:AD:BE:EF".repeat(8),
            "test".into(),
        )
    }

    #[tokio::test]
    async fn health_endpoint_includes_identity() {
        let state = make_state().await;
        let app = router().with_state(state);
        let resp = app
            .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), S::OK);
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let j: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(j["status"], "ok");
        assert!(j["our_fingerprint"].as_str().unwrap().contains(':'));
    }

    #[tokio::test]
    async fn dashboard_returns_zero_counts() {
        let state = make_state().await;
        let app = router().with_state(state);
        let resp = app
            .oneshot(Request::builder().uri("/dashboard").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), S::OK);
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let j: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(j["paired_devices"], 0);
        assert_eq!(j["online_devices"], 0);
        assert_eq!(j["notifications"]["unread"], 0);
        assert_eq!(j["sms"]["conversations"], 0);
        assert_eq!(j["calls"]["total"], 0);
    }

    #[tokio::test]
    async fn list_notifications_empty() {
        let state = make_state().await;
        let app = router().with_state(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/notifications?limit=10")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), S::OK);
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let j: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(j["notifications"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn list_sms_empty() {
        let state = make_state().await;
        let app = router().with_state(state);
        let resp = app
            .oneshot(Request::builder().uri("/sms").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), S::OK);
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let j: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(j["messages"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn list_calls_empty() {
        let state = make_state().await;
        let app = router().with_state(state);
        let resp = app
            .oneshot(Request::builder().uri("/calls").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), S::OK);
    }

    #[tokio::test]
    async fn list_audit_empty() {
        let state = make_state().await;
        let app = router().with_state(state);
        let resp = app
            .oneshot(Request::builder().uri("/audit").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), S::OK);
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let j: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(j["entries"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn dial_to_unconnected_device_returns_409() {
        let state = make_state().await;
        let app = router().with_state(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/dial")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&serde_json::json!({
                            "device_id": Uuid::new_v4(),
                            "number": "+1234567890"
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), S::CONFLICT);
    }

    #[tokio::test]
    async fn send_sms_to_unconnected_device_returns_409() {
        let state = make_state().await;
        let app = router().with_state(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/sms")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&serde_json::json!({
                            "device_id": Uuid::new_v4(),
                            "to": "+1234567890",
                            "body": "hello"
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), S::CONFLICT);
    }
}
