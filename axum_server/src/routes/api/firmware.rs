//! Firmware capsule hosting.
//!
//! The firmware app is never handed an operator-supplied capsule URL. A
//! `bios_update` command names a published `capsule_id`; `qc_fleet` rewrites
//! that to [`fetch_capsule`]'s path, and the bytes come from our own bucket
//! with their SHA-256 re-verified on the way out. Publishing is Root-only, so
//! the `firmware_capsule` table is the complete allow-list of flashable images.
//!
//! # Routes
//!
//! | Method | Path | Description |
//! |--------|------|-------------|
//! | GET  | `/api/v1/firmware/capsules` | List published capsules (Root) |
//! | POST | `/api/v1/firmware/capsules` | Publish a capsule (Root, multipart-free JSON+base64) |
//! | GET  | `/api/v1/firmware/capsules/{capsule_id}` | Serve capsule bytes to firmware |

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use base64::Engine;
use database::schema::firmware::FirmwareCapsule;
use serde::Deserialize;

use crate::AppState;

pub fn firmware_routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/firmware/capsules",
            axum::routing::get(list_capsules).post(publish_capsule),
        )
        .route(
            "/api/v1/firmware/capsules/{capsule_id}",
            axum::routing::get(fetch_capsule),
        )
}

#[derive(Deserialize)]
pub struct PublishCapsuleRequest {
    pub capsule_id: String,
    pub board_product: String,
    /// ESRT firmware-class GUID this capsule targets.
    pub fw_class: String,
    pub version: u32,
    #[serde(default)]
    pub lowest_supported: Option<u32>,
    #[serde(default)]
    pub notes: Option<String>,
    /// Base64 capsule bytes.
    pub data_base64: String,
    /// Optional expected SHA-256; rejected on mismatch.
    #[serde(default)]
    pub sha256: Option<String>,
}

async fn list_capsules(State(app): State<AppState>, headers: HeaderMap) -> Response {
    if let Err(r) = app.admin.require_root(&headers).await {
        return r;
    }
    match FirmwareCapsule::list().await {
        Ok(mut rows) => {
            rows.sort_by(|a, b| b.published_at.cmp(&a.published_at));
            Json(serde_json::json!({ "capsules": rows })).into_response()
        }
        Err(e) => internal(e),
    }
}

async fn publish_capsule(
    State(app): State<AppState>,
    headers: HeaderMap,
    Json(req): Json<PublishCapsuleRequest>,
) -> Response {
    let viewer = match app.admin.require_root(&headers).await {
        Ok(v) => v,
        Err(r) => return r,
    };
    let bytes = match base64::engine::general_purpose::STANDARD.decode(&req.data_base64) {
        Ok(b) => b,
        Err(e) => return bad_request(format!("data_base64 is not valid base64: {e}")),
    };
    if bytes.is_empty() {
        return bad_request("capsule is empty".into());
    }
    let digest = database::schema::firmware::sha256_hex(&bytes);
    if let Some(expected) = req.sha256.as_deref() {
        if !expected.eq_ignore_ascii_case(&digest) {
            return bad_request(format!("sha256 mismatch (expected {expected}, got {digest})"));
        }
    }

    let entry = FirmwareCapsule {
        capsule_id: req.capsule_id.clone(),
        board_product: req.board_product,
        fw_class: req.fw_class,
        version: req.version,
        lowest_supported: req.lowest_supported,
        notes: req.notes,
        published_by: viewer.email.clone(),
        ..Default::default()
    };
    match FirmwareCapsule::publish(entry, bytes).await {
        Ok(stored) => {
            tracing::info!(
                capsule_id = %stored.capsule_id,
                version = stored.version,
                bytes = stored.size_bytes,
                by = %viewer.email,
                "firmware.capsule_published",
            );
            (StatusCode::CREATED, Json(serde_json::json!({ "capsule": stored }))).into_response()
        }
        Err(e) => internal(e),
    }
}

/// Serve capsule bytes to a firmware agent. Unauthenticated like the rest of the
/// agent-facing surface, but harmless in isolation: an attacker who can reach it
/// only gets a signed vendor image the firmware would still have to accept, and
/// the flash itself is gated by the Root-only command rail.
async fn fetch_capsule(Path(capsule_id): Path<String>) -> Response {
    let capsule = match FirmwareCapsule::get(&capsule_id).await {
        Ok(Some(c)) => c,
        Ok(None) => {
            return not_found(format!("no published capsule '{capsule_id}'"));
        }
        Err(e) => return internal(e),
    };
    let bytes = match capsule.fetch_bytes().await {
        Ok(b) => b,
        Err(e) => return internal(e),
    };
    tracing::info!(
        capsule_id = %capsule_id,
        bytes = bytes.len(),
        version = capsule.version,
        "firmware.capsule_served",
    );
    (
        [
            (axum::http::header::CONTENT_TYPE, "application/octet-stream".to_string()),
            (axum::http::header::HeaderName::from_static("x-capsule-sha256"), capsule.sha256),
        ],
        bytes,
    )
        .into_response()
}

fn bad_request(msg: String) -> Response {
    (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": msg }))).into_response()
}

fn not_found(msg: String) -> Response {
    (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": msg }))).into_response()
}

fn internal<E: std::fmt::Display>(e: E) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "error": e.to_string() })),
    )
        .into_response()
}
