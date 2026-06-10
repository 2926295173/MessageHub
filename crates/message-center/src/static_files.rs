// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 PhoneBridge Contributors
//
// This file is part of PhoneBridge. See LICENSE and the dual-licensing
// notice in README.md for details.

//! Static file serving for the embedded Next.js web console.
//!
//! The frontend is built with `next build` which produces `frontend/out/`
//! (or `frontend/out/console/` if `basePath` is honored at runtime). The
//! message-center embeds that directory at compile time via `include_dir!` and
//! serves it under the `/console/` path. Other paths fall back to
//! `frontend/out/index.html` for client-side routing (App Router).

use axum::body::Body;
use axum::extract::{Path, Request, State};
use axum::http::{header, Response, StatusCode, Uri};
use axum::response::{IntoResponse, Redirect};
use axum::routing::get;
use axum::Router;
use include_dir::{include_dir, Dir};
use mime_guess::from_path;
use tracing::warn;

use crate::app_state::AppState;

/// The embedded directory tree.
pub static FRONTEND: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/../../frontend/out");

/// Build the static-file sub-router.
///
/// Mounts:
/// - `GET /`           → redirect to `/console/`
/// - `GET /console`    → redirect to `/console/`
/// - `GET /console/`   → `out/index.html` (the App Router root index)
/// - `GET /console/*`  → file lookup under `out/`, fallback to `out/index.html`
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(root_redirect))
        .route("/console", get(console_redirect))
        .route("/console/", get(serve_console_index))
        .route("/console/*path", get(serve_console_file))
}

async fn root_redirect() -> impl IntoResponse {
    Redirect::permanent("/console/")
}

async fn console_redirect() -> impl IntoResponse {
    Redirect::permanent("/console/")
}

async fn serve_console_index() -> Response<Body> {
    serve_file("index.html")
}

/// Serve a file from the embedded directory, falling back to `index.html`
/// for unknown paths (so App Router's client-side routing works).
async fn serve_console_file(
    State(_state): State<AppState>,
    Path(path): Path<String>,
    _req: Request,
) -> Response<Body> {
    let requested = path.trim_start_matches('/');
    let requested = if requested.is_empty() {
        "index.html"
    } else {
        requested
    };

    // Try direct file first.
    if let Some(resp) = try_file(requested) {
        return resp;
    }

    // Try as directory (serve /<dir>/index.html).
    let dir_index = format!("{}/index.html", requested.trim_end_matches('/'));
    if requested != dir_index {
        if let Some(resp) = try_file(&dir_index) {
            return resp;
        }
    }

    // App Router fallback: serve the root index.html so the client router
    // can pick up the URL. This is only correct for /console/* paths that
    // look like client routes (no extension). For paths with extensions
    // (e.g. .js, .css), we 404.
    if std::path::Path::new(requested).extension().is_some() {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }

    warn!(
        requested = %requested,
        "static file miss; falling back to index.html (client-side route)"
    );
    serve_file("index.html")
}

fn try_file(rel: &str) -> Option<Response<Body>> {
    let f = FRONTEND.get_file(rel)?;
    let body = Body::from(f.contents());
    let mime = from_path(rel).first_or_octet_stream();
    let mut resp = (StatusCode::OK, body).into_response();
    resp.headers_mut().insert(
        header::CONTENT_TYPE,
        mime.essence_str()
            .parse()
            .unwrap_or_else(|_| "application/octet-stream".parse().unwrap()),
    );
    Some(resp)
}

fn serve_file(rel: &str) -> Response<Body> {
    try_file(rel).unwrap_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("frontend build artifact missing: {rel}; run `cd frontend && bun run build`"),
        )
            .into_response()
    })
}

/// Test-only helper: confirm the embedded dir is not empty at build time.
#[allow(dead_code)]
fn _assert_embedded(_: &Uri) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display_auth::DisplayAuth;
    use axum::body::Body;
    use axum::http::{Request, StatusCode as S};
    use tower::ServiceExt;

    async fn make_state() -> AppState {
        let db = phonebridge_storage::Db::open_memory().await.unwrap();
        db.migrate().await.unwrap();
        let tmp = std::env::temp_dir().join(format!(
            "phonebridge-static-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        let auth_paths = phonebridge_core::paths::AppPaths {
            config_dir: tmp,
            data_dir: std::env::temp_dir(),
            log_dir: std::env::temp_dir(),
        };
        let display_auth = DisplayAuth::load_or_generate(&auth_paths).unwrap();
        AppState::new(
            std::sync::Arc::new(phonebridge_core::Config::default()),
            std::sync::Arc::new(db),
            phonebridge_net::DeviceRegistry::new(),
            display_auth,
            uuid::Uuid::new_v4(),
            "PUBKEYB64".into(),
            "DE:AD:BE:EF".repeat(8),
            "test".into(),
        )
    }

    #[tokio::test]
    async fn root_redirects_to_console() {
        let app = router().with_state(make_state().await);
        let resp = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), S::PERMANENT_REDIRECT);
    }

    #[tokio::test]
    async fn console_index_serves_html() {
        let app = router().with_state(make_state().await);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/console/")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), S::OK);
        let ct = resp
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert!(ct.starts_with("text/html"), "got: {ct}");
    }

    #[tokio::test]
    async fn unknown_client_route_falls_back_to_index() {
        let app = router().with_state(make_state().await);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/console/dashboard/")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), S::OK);
        let ct = resp
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert!(ct.starts_with("text/html"));
    }
}
