//! REST API surface (M1: minimal `/health`).
//!
//! M3 will add:
//! - `GET  /devices`
//! - `POST /pair/start`
//! - `POST /pair/cancel`
//! - `DELETE /devices/:id`
//! - `GET  /notifications`
//! - `GET  /sms`
//! - `POST /sms`
//! - `GET  /calls`

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use serde::Serialize;

use crate::app_state::AppState;

/// Build the `/api/v1` sub-router.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/health", get(health))
}

#[derive(Debug, Serialize)]
struct HealthBody {
    status: &'static str,
    version: &'static str,
    /// Number of devices currently paired.
    paired_devices: i64,
    /// Number of devices currently online (have an open WS).
    online_devices: i64,
}

async fn health(State(state): State<AppState>) -> impl IntoResponse {
    // Best-effort: count devices. We don't fail the health endpoint if the
    // query fails, just return zeros.
    let paired = count_paired(&state).await.unwrap_or(0);
    let body = HealthBody {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
        paired_devices: paired,
        online_devices: 0,
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
        )
    }

    #[tokio::test]
    async fn health_endpoint_returns_ok() {
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
        assert!(j["version"].is_string());
    }
}
