//! Sink for ZeroClaw's `webhook-audit` hook.
//!
//! The hook POSTs one JSON body per matching tool call and sends no headers, so
//! the shared secret rides in the query string and is compared against
//! `ZEROCLAW_AUDIT_TOKEN`. Its SSRF guard requires `https` and rejects private
//! and loopback hosts, so this route only works behind the public domain.
//!
//! # Routes
//!
//! | Method | Path                            | Description |
//! |--------|---------------------------------|-------------|
//! | POST   | `/api/v1/audit/zeroclaw`        | Append one tool-call event to `zeroclaw_audit`. |
//!
//! Rows land in an append-only table (`update`/`delete` are `NONE`); MasterTech's
//! Agent Audit tab reads them.

use axum::extract::Query;
use axum::http::StatusCode;
use axum::response::Json;
use axum::{Router, routing};
use serde::{Deserialize, Serialize};

use crate::AppState;

/// Shared secret, absent when unset so the route refuses every request.
/// Trimmed so a newline-terminated Secret value still matches.
fn expected_token() -> Option<String> {
    std::env::var("ZEROCLAW_AUDIT_TOKEN")
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
}

#[derive(Debug, Deserialize)]
pub struct AuditAuth {
    #[serde(default)]
    token: String,
}

/// One `webhook-audit` POST body.
#[derive(Debug, Deserialize)]
pub struct AuditEvent {
    #[serde(default)]
    event: Option<String>,
    #[serde(default)]
    timestamp: Option<String>,
    #[serde(default)]
    tool: Option<String>,
    #[serde(default)]
    success: Option<bool>,
    #[serde(default)]
    duration_ms: Option<u64>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    args: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
pub struct AuditAck {
    stored: bool,
}

pub fn audit_routes() -> Router<AppState> {
    Router::new().route("/api/v1/audit/zeroclaw", routing::post(ingest))
}

fn internal(e: impl std::fmt::Display) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

async fn ingest(
    Query(auth): Query<AuditAuth>,
    Json(ev): Json<AuditEvent>,
) -> Result<Json<AuditAck>, (StatusCode, String)> {
    let Some(expected) = expected_token() else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "ZEROCLAW_AUDIT_TOKEN is not set".to_string(),
        ));
    };
    if auth.token.trim() != expected {
        return Err((StatusCode::UNAUTHORIZED, "bad token".to_string()));
    }

    // `event_ts` keeps the hook's own clock; `created_at` defaults server-side.
    database::db()
        .query(
            "CREATE zeroclaw_audit CONTENT {
                 event: $event,
                 event_ts: $event_ts,
                 tool: $tool,
                 success: $success,
                 duration_ms: $duration_ms,
                 error: $error,
                 args: $args
             }",
        )
        .bind(("event", ev.event.unwrap_or_else(|| "tool_call".to_string())))
        .bind(("event_ts", ev.timestamp))
        .bind(("tool", ev.tool))
        .bind(("success", ev.success.unwrap_or(false)))
        .bind(("duration_ms", ev.duration_ms.unwrap_or(0)))
        .bind(("error", ev.error))
        .bind(("args", ev.args))
        .await
        .map_err(internal)?;

    Ok(Json(AuditAck { stored: true }))
}
