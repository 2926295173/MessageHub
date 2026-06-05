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
//! - `POST /notifications/mark-all-read`       — mark all read
//! - `GET  /sms`               — list SMS
//! - `GET  /sms/conversations` — group by phone number
//! - `POST /sms`               — send an SMS (forwards to android via WS)
//! - `GET  /calls`             — list calls
//! - `GET  /dashboard`         — aggregate counts
//! - `GET  /audit`             — recent audit log
//! - `POST /dial`              — place an outgoing call (forwards to android)
//!
//! OpenAPI: Swagger UI is served at `/console/api-docs` (in production
//! at the daemon's HTTPS port). The raw OpenAPI JSON is at
//! `/console/api-docs/openapi.json`.

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
use utoipa::{OpenApi, ToSchema};

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
        .route("/pair/accept", post(pair_accept))
        .route("/pair/reject", post(pair_reject))
        .route("/notifications", get(list_notifications))
        .route("/notifications/stats", get(notifications_stats))
        .route(
            "/notifications/:device_id/:id/read",
            post(mark_notification_read),
        )
        .route(
            "/notifications/mark-all-read",
            post(mark_all_notifications_read),
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

#[derive(Debug, Serialize, ToSchema)]
pub struct HealthBody {
    pub status: &'static str,
    pub version: &'static str,
    pub our_device_id: Uuid,
    pub our_fingerprint: String,
    pub paired_devices: i64,
    pub online_devices: i64,
}

#[utoipa::path(
    get,
    path = "/api/v1/health",
    tag = "health",
    responses(
        (status = 200, description = "Daemon is healthy", body = HealthBody),
    )
)]
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

#[derive(Debug, Serialize, ToSchema)]
pub struct CertBody {
    pub device_id: Uuid,
    pub name: String,
    pub fingerprint: String,
    pub public_key: String,
}

#[utoipa::path(
    get,
    path = "/api/v1/cert",
    tag = "identity",
    responses(
        (status = 200, description = "This daemon's cert + identity", body = CertBody),
    )
)]
async fn get_cert(State(state): State<AppState>) -> impl IntoResponse {
    let body = CertBody {
        device_id: state.our_device_id,
        name: state.our_name.read().clone(),
        fingerprint: state.our_fingerprint.read().clone(),
        public_key: state.our_public_key_b64.read().clone(),
    };
    (StatusCode::OK, Json(body))
}

// ============================================================================
// devices
// ============================================================================

#[derive(Debug, Serialize, ToSchema)]
pub struct DeviceListResponse {
    pub devices: Vec<DeviceRow>,
}

#[utoipa::path(
    get,
    path = "/api/v1/devices",
    tag = "devices",
    responses(
        (status = 200, description = "List paired + discovered devices", body = DeviceListResponse),
    )
)]
async fn list_devices(State(state): State<AppState>) -> impl IntoResponse {
    match state.db.list_devices().await {
        Ok(devs) => (StatusCode::OK, Json(DeviceListResponse { devices: devs })).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("db: {e}")).into_response(),
    }
}

#[utoipa::path(
    get,
    path = "/api/v1/devices/{id}",
    tag = "devices",
    params(
        ("id" = Uuid, Path, description = "Device id (UUIDv4)"),
    ),
    responses(
        (status = 200, description = "Device", body = DeviceRow),
        (status = 404, description = "Device not found"),
    )
)]
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

#[utoipa::path(
    delete,
    path = "/api/v1/devices/{id}",
    tag = "devices",
    params(
        ("id" = Uuid, Path, description = "Device id (UUIDv4)"),
    ),
    responses(
        (status = 204, description = "Device unpaired + removed"),
        (status = 500, description = "DB error"),
    )
)]
async fn remove_device(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> impl IntoResponse {
    state.pairing.remove(&id);
    state.registry.unregister(&id);
    state.pin_store.write().remove(&id);
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

#[derive(Debug, Serialize, ToSchema)]
pub struct PairingsResponse {
    pub in_flight: Vec<Uuid>,
    pub persisted: Vec<PairingRow>,
}

#[utoipa::path(
    get,
    path = "/api/v1/pairings",
    tag = "pairings",
    responses(
        (status = 200, description = "In-flight + persisted pairings", body = PairingsResponse),
    )
)]
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

#[derive(Debug, Deserialize, ToSchema)]
pub struct PairStartRequest {
    /// The device id to pair with (must be currently connected).
    pub device_id: Uuid,
}

/// `POST /api/v1/pair/start` — initiate pairing with a connected device.
#[utoipa::path(
    post,
    path = "/api/v1/pair/start",
    tag = "pairings",
    request_body = PairStartRequest,
    responses(
        (status = 200, description = "device.pair.request sent"),
        (status = 409, description = "Device not connected"),
    )
)]
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
    // Persist the Initiator in the shared PairingMap so the WS handler
    // can drive it when the device sends back pair.challenge, etc.
    // We must also drop any prior Responder session that was inserted
    // on device.hello, otherwise the WS handler's get() would return
    // the Responder (which is no longer the active role).
    state.pairing.insert(
        device_id,
        phonebridge_net::DeviceSession::Unpaired(
            phonebridge_net::UnpairedSession::Initiator(initiator),
        ),
    );
    info!(%device_id, "pair_start: sent device.pair.request");

    (
        StatusCode::OK,
        Json(json!({
            "status": "pair.request sent",
            "device_id": device_id,
        })),
    )
        .into_response()
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct PairAcceptRequest {
    /// The device id whose pairing should be accepted.
    pub device_id: Uuid,
    /// The 6-digit code the user typed (visual confirmation only). Optional
    /// in MVP: the daemon currently does not re-derive the code (KDF
    /// verification is the next hardening step). Stored for audit.
    #[serde(default)]
    pub code: Option<String>,
}

/// `POST /api/v1/pair/accept` — accept an in-flight pairing and send
/// `device.pair.accept` to the device. The user has just typed the
/// 6-digit code on the desktop and confirmed it matches what Android
/// shows.
#[utoipa::path(
    post,
    path = "/api/v1/pair/accept",
    tag = "pairings",
    request_body = PairAcceptRequest,
    responses(
        (status = 200, description = "device.pair.accept sent"),
        (status = 409, description = "No in-flight pairing for this device"),
    )
)]
async fn pair_accept(
    State(state): State<AppState>,
    Json(req): Json<PairAcceptRequest>,
) -> impl IntoResponse {
    let device_id = req.device_id;
    // Pop the Initiator from the shared pairing map (clone so we can
    // re-insert it for the next stage, where the WS handler will read
    // it on device.pair.confirm and device.pair.complete).
    let session = state.pairing.get(&device_id);
    let mut initiator = match session {
        Some(phonebridge_net::DeviceSession::Unpaired(
            phonebridge_net::UnpairedSession::Initiator(i),
        )) => i,
        _ => {
            return (
                StatusCode::CONFLICT,
                Json(json!({
                    "error": "no in-flight initiator pairing for this device",
                    "device_id": device_id,
                })),
            )
                .into_response();
        }
    };

    // Optional: validate the 6-digit code shape (matches the protocol
    // boundary that ws_handler enforces). We do not recompute the
    // shared secret here; in MVP the user typing the code on the
    // desktop is the verification.
    if let Some(ref code) = req.code {
        if code.len() != 6 || !code.chars().all(|c| c.is_ascii_digit()) {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "code must be 6 digits",
                })),
            )
                .into_response();
        }
        info!(%device_id, code = %code, "pair_accept: user typed code");
    } else {
        info!(%device_id, "pair_accept: accepted without typed code (dev shortcut)");
    }

    let env = match initiator.build_accept_envelope(state.our_device_id) {
        Ok(e) => e,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("build accept: {e}"),
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
    // Re-insert the Initiator in its post-challenge state so the WS
    // handler can keep driving it on subsequent device.pair.confirm /
    // device.pair.complete.
    state.pairing.insert(
        device_id,
        phonebridge_net::DeviceSession::Unpaired(
            phonebridge_net::UnpairedSession::Initiator(initiator),
        ),
    );
    info!(%device_id, "pair_accept: sent device.pair.accept");

    (
        StatusCode::OK,
        Json(json!({
            "status": "pair.accept sent",
            "device_id": device_id,
        })),
    )
        .into_response()
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct PairRejectRequest {
    /// The device id whose pairing should be rejected.
    pub device_id: Uuid,
    /// Optional reason text.
    #[serde(default)]
    pub reason: Option<String>,
}

/// `POST /api/v1/pair/reject` — reject an in-flight pairing and send
/// `device.pair.reject` to the device.
#[utoipa::path(
    post,
    path = "/api/v1/pair/reject",
    tag = "pairings",
    request_body = PairRejectRequest,
    responses(
        (status = 200, description = "device.pair.reject sent"),
        (status = 409, description = "No in-flight pairing for this device"),
    )
)]
async fn pair_reject(
    State(state): State<AppState>,
    Json(req): Json<PairRejectRequest>,
) -> impl IntoResponse {
    let device_id = req.device_id;
    let reason = req.reason.as_deref().unwrap_or("rejected by user");
    let session = state.pairing.get(&device_id);
    let mut initiator = match session {
        Some(phonebridge_net::DeviceSession::Unpaired(
            phonebridge_net::UnpairedSession::Initiator(i),
        )) => i,
        _ => {
            return (
                StatusCode::CONFLICT,
                Json(json!({
                    "error": "no in-flight initiator pairing for this device",
                    "device_id": device_id,
                })),
            )
                .into_response();
        }
    };
    let env = match initiator.build_reject_envelope(state.our_device_id, reason) {
        Ok(e) => e,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("build reject: {e}"),
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
    // Drop the session.
    state.pairing.remove(&device_id);
    info!(%device_id, reason, "pair_reject: sent device.pair.reject");

    (
        StatusCode::OK,
        Json(json!({
            "status": "pair.reject sent",
            "device_id": device_id,
            "reason": reason,
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

#[derive(Debug, Serialize, ToSchema)]
pub struct NotificationsResponse {
    pub notifications: Vec<NotificationRow>,
}

#[utoipa::path(
    get,
    path = "/api/v1/notifications",
    tag = "notifications",
    params(
        ("device_id" = Option<Uuid>, Query, description = "Filter by device"),
        ("limit" = Option<i64>, Query, description = "Max rows (default 50, max 500)"),
        ("package" = Option<String>, Query, description = "Filter by app package"),
        ("unread_only" = Option<bool>, Query, description = "If true, only unread"),
    ),
    responses(
        (status = 200, description = "Notifications", body = NotificationsResponse),
    )
)]
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

#[derive(Debug, Serialize, ToSchema)]
pub struct NotificationsStats {
    pub total: i64,
    pub unread: i64,
    pub by_package: Vec<PackageCount>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct PackageCount {
    pub package: String,
    pub count: i64,
}

#[utoipa::path(
    get,
    path = "/api/v1/notifications/stats",
    tag = "notifications",
    params(
        ("device_id" = Option<Uuid>, Query, description = "Filter by device"),
    ),
    responses(
        (status = 200, description = "Aggregate counts", body = NotificationsStats),
    )
)]
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

#[utoipa::path(
    post,
    path = "/api/v1/notifications/{device_id}/{id}/read",
    tag = "notifications",
    params(
        ("device_id" = Uuid, Path, description = "Device id"),
        ("id" = String, Path, description = "Notification id (per-device)"),
    ),
    responses(
        (status = 204, description = "Marked read"),
    )
)]
async fn mark_notification_read(
    State(state): State<AppState>,
    Path((device_id, id)): Path<(Uuid, String)>,
) -> impl IntoResponse {
    if let Err(e) = state.db.mark_notification_read(device_id, &id).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, format!("db: {e}")).into_response();
    }
    (StatusCode::NO_CONTENT, "").into_response()
}

#[utoipa::path(
    post,
    path = "/api/v1/notifications/mark-all-read",
    tag = "notifications",
    params(
        ("device_id" = Option<Uuid>, Query, description = "If absent, mark all devices' notifications read"),
    ),
    responses(
        (status = 204, description = "All matching notifications marked read"),
    )
)]
async fn mark_all_notifications_read(
    State(state): State<AppState>,
    Query(q): Query<ListNotificationsQuery>,
) -> impl IntoResponse {
    let limit = 100_000i64;
    match state
        .db
        .list_notifications(q.device_id, limit, true, None)
        .await
    {
        Ok(rows) => {
            for n in rows {
                let _ = state.db.mark_notification_read(n.device_id, &n.id).await;
            }
            (StatusCode::NO_CONTENT, "").into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("db: {e}")).into_response(),
    }
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

#[derive(Debug, Serialize, ToSchema)]
pub struct SmsListResponse {
    pub messages: Vec<SmsRow>,
}

#[utoipa::path(
    get,
    path = "/api/v1/sms",
    tag = "sms",
    params(
        ("device_id" = Option<Uuid>, Query, description = "Filter by device"),
        ("phone_number" = Option<String>, Query, description = "Filter by phone number"),
        ("limit" = Option<i64>, Query, description = "Max rows (default 50)"),
    ),
    responses(
        (status = 200, description = "SMS messages", body = SmsListResponse),
    )
)]
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

#[derive(Debug, Serialize, ToSchema)]
pub struct SmsConversationsResponse {
    pub conversations: Vec<SmsConversation>,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SmsConversation {
    pub address: String,
    pub last_timestamp: i64,
    pub count: i64,
}

#[utoipa::path(
    get,
    path = "/api/v1/sms/conversations",
    tag = "sms",
    params(
        ("device_id" = Option<Uuid>, Query, description = "Filter by device"),
    ),
    responses(
        (status = 200, description = "Grouped by phone number", body = SmsConversationsResponse),
    )
)]
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

#[derive(Debug, Deserialize, ToSchema)]
pub struct SendSmsRequest {
    pub device_id: Uuid,
    pub to: String,
    pub body: String,
    #[serde(default)]
    pub subscription_id: Option<i32>,
}

/// In-memory map of pending SMS sends: `envelope_id` → list of
/// waiters. The WS handler's `on_sms_send_result` notifies the waiter.
type SmsWaiters = Arc<Mutex<HashMap<Uuid, Vec<oneshot::Sender<Result<SmsSendResult, String>>>>>>;
#[allow(dead_code)]
fn _sms_waiters_marker(_w: &SmsWaiters) {}
use tokio::sync::oneshot;

/// `POST /sms` — send an SMS via the device, return the result.
#[utoipa::path(
    post,
    path = "/api/v1/sms",
    tag = "sms",
    request_body = SendSmsRequest,
    responses(
        (status = 202, description = "SMS queued; request_id returned"),
        (status = 409, description = "Device not connected"),
    )
)]
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

#[derive(Debug, Serialize, ToSchema)]
pub struct CallsResponse {
    pub calls: Vec<CallRow>,
}

#[utoipa::path(
    get,
    path = "/api/v1/calls",
    tag = "calls",
    params(
        ("device_id" = Option<Uuid>, Query, description = "Filter by device"),
        ("limit" = Option<i64>, Query, description = "Max rows (default 50)"),
    ),
    responses(
        (status = 200, description = "Call log entries", body = CallsResponse),
    )
)]
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

#[derive(Debug, Deserialize, ToSchema)]
pub struct DialRequest {
    pub device_id: Uuid,
    pub number: String,
}

#[utoipa::path(
    post,
    path = "/api/v1/dial",
    tag = "calls",
    request_body = DialRequest,
    responses(
        (status = 202, description = "Dialing"),
        (status = 409, description = "Device not connected"),
    )
)]
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

#[derive(Debug, Serialize, ToSchema)]
pub struct DashboardBody {
    pub paired_devices: i64,
    pub online_devices: i64,
    pub notifications: NotificationCounts,
    pub sms: SmsCounts,
    pub calls: CallCounts,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct NotificationCounts {
    pub total: i64,
    pub unread: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct SmsCounts {
    pub total: i64,
    pub conversations: i64,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct CallCounts {
    pub total: i64,
    pub missed: i64,
    pub ringing: i64,
}

#[utoipa::path(
    get,
    path = "/api/v1/dashboard",
    tag = "health",
    params(
        ("device_id" = Option<Uuid>, Query, description = "Filter by device"),
    ),
    responses(
        (status = 200, description = "Aggregate counts", body = DashboardBody),
    )
)]
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

#[derive(Debug, Serialize, ToSchema)]
pub struct AuditListResponse {
    pub entries: Vec<AuditLogRow>,
}

#[utoipa::path(
    get,
    path = "/api/v1/audit",
    tag = "audit",
    params(
        ("limit" = Option<i64>, Query, description = "Max entries (default 100, max 1000)"),
    ),
    responses(
        (status = 200, description = "Audit log entries", body = AuditListResponse),
    )
)]
async fn list_audit(
    State(state): State<AppState>,
    Query(q): Query<AuditQuery>,
) -> impl IntoResponse {
    let limit = q.limit.unwrap_or(100).clamp(1, 1000);
    match state.db.list_audit_log(limit).await {
        Ok(rows) => (StatusCode::OK, Json(AuditListResponse { entries: rows })).into_response(),
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
