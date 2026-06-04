//! REST API surface.
//!
//! Endpoints (all under `/api/v1`):
//! - `GET  /health`            — health probe (paired/online counts)
//! - `GET  /devices`           — list devices (paired + unpaired)
//! - `GET  /devices/:id`       — one device
//! - `DELETE /devices/:id`     — unpair + remove a device
//! - `GET  /pairings`          — list in-flight + completed pairings
//! - `POST /pair/start`        — (no-op stub in M2; real trigger in M3+)
//! - `GET  /cert`              — our cert PEM + fingerprint + device id

use axum::Json;
use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use serde::Serialize;
use serde_json::json;
use uuid::Uuid;

use phonebridge_proto::MessageType;
use phonebridge_storage::models::{DeviceRow, PairingRow};

use crate::app_state::AppState;

/// Build the `/api/v1` sub-router.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
        .route("/devices", get(list_devices))
        .route("/devices/:id", get(get_device))
        .route("/devices/:id", delete(remove_device))
        .route("/pairings", get(list_pairings))
        .route("/pair/start", post(pair_start_stub))
        .route("/cert", get(get_cert))
}

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
        online_devices: count_online(&state),
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

fn count_online(state: &AppState) -> i64 {
    state.pairing.list_paired().len() as i64
}

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
    state.pin_store.write().remove(&id);
    if let Err(e) = state.db.remove_device(id).await {
        return (StatusCode::INTERNAL_SERVER_ERROR, format!("db: {e}")).into_response();
    }
    (StatusCode::NO_CONTENT, "").into_response()
}

#[derive(Debug, Serialize)]
struct PairingsResponse {
    /// In-flight pairing sessions (currently on the WS).
    in_flight: Vec<Uuid>,
    /// Persisted pairings (from the DB).
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

/// M2 stub. The actual pairing flow is initiated over the WebSocket by the
/// Android client (which sees the desktop via mDNS and opens a WS). The
/// desktop's "click pair" UX in the web console will be implemented in M3.
async fn pair_start_stub(State(state): State<AppState>) -> impl IntoResponse {
    // In MVP, we don't have a way to push a `device.pair.request` to the
    // Android side from the desktop. The reverse flow (Android → desktop)
    // is the supported path: Android opens the WS, sends `device.hello`,
    // then waits for the desktop user to click "Pair" in the web console
    // (M3). For now, return a JSON body explaining the current model.
    (
        StatusCode::OK,
        Json(json!({
            "status": "noop",
            "message": "Pairing is initiated by the Android client in MVP. \
                        The desktop listens on /ws and mDNS advertises _phonebridge._tcp. \
                        Open the PhoneBridge Android app, discover this desktop, and tap Pair.",
            "our_device_id": state.our_device_id,
            "service_type": state.config.discovery.service_type,
        })),
    )
        .into_response()
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

// Re-export MessageType for any other module that might want it
#[allow(dead_code)]
const _MSG_TYPE: MessageType = MessageType::DeviceHeartbeat;

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
    async fn cert_endpoint_returns_our_identity() {
        let state = make_state().await;
        let app = router().with_state(state);
        let resp = app
            .oneshot(Request::builder().uri("/cert").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), S::OK);
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let j: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(j["public_key"], "PUBKEYB64");
    }

    #[tokio::test]
    async fn pair_start_stub_returns_message() {
        let state = make_state().await;
        let app = router().with_state(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/pair/start")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), S::OK);
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let j: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(j["status"], "noop");
        assert!(j["message"].as_str().unwrap().contains("MVP"));
    }
}
