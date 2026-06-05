//! OpenAPI spec + minimal Swagger UI page.
//!
//! The full Swagger UI asset bundle is not embedded (it would require
//! either a build-time download or vendoring megabytes of JS/CSS). Instead
//! we serve:
//!
//! - `GET  /console/api-docs/openapi.json` — the raw OpenAPI 3.1 JSON.
//! - `GET  /console/api-docs/`            — a tiny HTML page that loads
//!                                         `swagger-ui-dist` from a CDN and
//!                                         points it at the JSON.
//!
//! For offline or air-gapped environments, users can point their own
//! Swagger UI (or Redoc) at the JSON URL.

#![forbid(unsafe_code)]
#![allow(missing_docs)]

use axum::Router;
use axum::http::header;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use utoipa::OpenApi;

use phonebridge_storage::models::{
    AuditLogRow, CallRow, DeviceRow, NotificationRow, PairingRow, SmsRow,
};

use crate::app_state::AppState;
use crate::rest::{
    AuditListResponse, CallCounts, CallsResponse, CertBody, DashboardBody, DeviceListResponse,
    HealthBody, NotificationCounts, NotificationsResponse, NotificationsStats, PackageCount,
    PairingsResponse, SmsConversationsResponse, SmsConversation, SmsCounts, SmsListResponse,
};

#[derive(OpenApi)]
#[openapi(
    info(
        title = "PhoneBridge Daemon API",
        version = "0.1.0",
        description = "REST API for the PhoneBridge desktop daemon. All endpoints live under /api/v1.",
        contact(name = "PhoneBridge", url = "https://github.com/anomalyco/phonebridge"),
        license(name = "GPL-3.0-or-later"),
    ),
    paths(
        crate::rest::health,
        crate::rest::get_cert,
        crate::rest::list_devices,
        crate::rest::get_device,
        crate::rest::remove_device,
        crate::rest::list_pairings,
        crate::rest::pair_start,
        crate::rest::pair_accept,
        crate::rest::pair_reject,
        crate::rest::list_notifications,
        crate::rest::notifications_stats,
        crate::rest::mark_notification_read,
        crate::rest::mark_all_notifications_read,
        crate::rest::list_sms,
        crate::rest::list_sms_conversations,
        crate::rest::send_sms,
        crate::rest::list_calls,
        crate::rest::dashboard,
        crate::rest::list_audit,
        crate::rest::dial,
    ),
    components(schemas(
        HealthBody, CertBody, DeviceRow, DeviceListResponse, PairingsResponse,
        NotificationsResponse, NotificationRow, NotificationsStats, PackageCount,
        SmsListResponse, SmsRow, SmsConversationsResponse, SmsConversation,
        CallsResponse, CallRow, DashboardBody, NotificationCounts, SmsCounts, CallCounts,
        AuditListResponse, AuditLogRow,
        crate::rest::PairStartRequest, crate::rest::PairAcceptRequest, crate::rest::PairRejectRequest,
        crate::rest::SendSmsRequest, crate::rest::DialRequest,
    )),
    tags(
        (name = "health", description = "Liveness / readiness probes"),
        (name = "identity", description = "This daemon's cert + identity"),
        (name = "devices", description = "Paired Android clients"),
        (name = "pairings", description = "In-flight + persisted pairing records"),
        (name = "notifications", description = "Notification sync from Android"),
        (name = "sms", description = "SMS messages and conversations"),
        (name = "calls", description = "Call log and outgoing dial"),
        (name = "audit", description = "Audit log of WS / pairing events"),
    ),
)]
pub struct ApiDoc;

/// Tiny HTML page that loads Swagger UI from a CDN.
const SWAGGER_HTML: &str = r##"<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <title>PhoneBridge API</title>
    <link rel="stylesheet" href="https://unpkg.com/swagger-ui-dist@5.17.14/swagger-ui.css" />
    <style>
      body { margin: 0; padding: 0; }
      .swagger-ui .topbar { display: none; }
    </style>
  </head>
  <body>
    <div id="swagger-ui"></div>
    <script src="https://unpkg.com/swagger-ui-dist@5.17.14/swagger-ui-bundle.js" crossorigin></script>
    <script>
      window.onload = () => {
        window.ui = SwaggerUIBundle({
          url: "/console/api-docs/openapi.json",
          dom_id: "#swagger-ui",
          deepLinking: true,
          presets: [SwaggerUIBundle.presets.apis],
        });
      };
    </script>
  </body>
</html>
"##;

/// Build the OpenAPI sub-router. Returns `Router<AppState>` for
/// compatibility with the rest of the app; the routes are stateless.
pub fn router() -> Router<AppState> {
    use utoipa::openapi::OpenApi as _;
    let doc = ApiDoc::openapi();

    Router::new()
        .route(
            "/console/api-docs/openapi.json",
            get(move || async move {
                let json = doc.to_pretty_json().unwrap_or_else(|_| "{}".to_string());
                (
                    [(header::CONTENT_TYPE, "application/json")],
                    json,
                )
                    .into_response()
            }),
        )
        .route(
            "/console/api-docs/",
            get(|| async { swagger_html().into_response() }),
        )
        .route(
            "/console/api-docs",
            get(|| async { swagger_html().into_response() }),
        )
}

fn swagger_html() -> Response {
    (
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        SWAGGER_HTML,
    )
        .into_response()
}
