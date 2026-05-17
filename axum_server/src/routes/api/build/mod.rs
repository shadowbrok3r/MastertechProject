//! Public HTTP surface for the SurrealDB-backed `build_job` queue.
//!
//! Designed to live behind a domain (e.g. `axum.master-tech.app`) so
//! a no-Rust MCP host can dispatch compiles without speaking SurrealQL
//! directly. The actual coordination still lives in the DB — workers
//! subscribe via `LIVE SELECT * FROM build_job`, claim atomically, and
//! write results back. These routes are a thin wrapper around the
//! [`database::schema::BuildJob`] helpers.
//!
//! # Routes
//!
//! | Method | Path                                | Description |
//! |--------|-------------------------------------|-------------|
//! | POST   | `/api/build/jobs`                   | Create a pending job (returns `job_id`). |
//! | GET    | `/api/build/jobs/{job_id}`          | Poll a job's current state. `done` jobs return wasm bytes as base64. |
//! | GET    | `/api/build/workers`                | List `connected_client` rows with `client_kind = 'build_worker'`. Stale-pruned by `last_update`. |
//!
//! Errors map to `500` with the SurrealDB error string in the body so
//! the agent can debug schema/binding issues without a separate log
//! pull.

use axum::extract::Path;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};
use axum::{Router, routing};
use base64::Engine as _;
use database::schema::{
    BuildJob, ClientKind, ConnectedClient, RecordId, RecordIdExt, BUILD_JOB_TABLE,
};
use serde::{Deserialize, Serialize};

use crate::AppState;

// ── Request / response shapes ──────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateJobRequest {
    pub plugin_id: String,
    pub cargo_toml: String,
    pub lib_rs: String,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub profile: Option<String>,
    /// Optional `connected_client.id` (table:key form, e.g.
    /// `connected_client:build_worker_alpha`) pinning the job to one
    /// worker. Omitted → any worker advertising the target may claim.
    #[serde(default)]
    pub assigned_worker_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CreateJobResponse {
    pub job_id: String,
    pub plugin_id: String,
    pub target: String,
    pub profile: String,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct JobStatusResponse {
    pub job_id: String,
    pub plugin_id: String,
    pub target: String,
    pub profile: String,
    pub status: String,
    pub claimed_worker_id: Option<String>,
    pub duration_ms: u64,
    /// Base64-encoded wasm bytes (only present when `status = "done"`).
    /// Base64 avoids the need to negotiate a binary content type on the
    /// MCP side, and the payloads are small enough that the ~33%
    /// inflation is acceptable.
    pub wasm_base64: Option<String>,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Serialize)]
pub struct WorkerSummary {
    pub connection_string: String,
    pub friendly_name: Option<String>,
    pub local_ip: Option<String>,
    pub last_update_iso: Option<String>,
}

// ── Routing helper ─────────────────────────────────────────────────

pub fn build_routes() -> Router<AppState> {
    Router::new()
        .route("/api/build/jobs", routing::post(create_job))
        .route("/api/build/jobs/{job_id}", routing::get(get_job))
        .route("/api/build/workers", routing::get(list_workers))
}

// ── Handlers ───────────────────────────────────────────────────────

async fn create_job(
    Json(req): Json<CreateJobRequest>,
) -> Result<Json<CreateJobResponse>, (StatusCode, String)> {
    let target = req.target.clone().unwrap_or_else(|| "wasm32-wasip1".into());
    let profile = req.profile.clone().unwrap_or_else(|| "release".into());
    let assigned = req
        .assigned_worker_id
        .as_deref()
        .map(parse_record_id_loose);

    let row = BuildJob::create(
        &req.plugin_id,
        &req.cargo_toml,
        &req.lib_rs,
        &target,
        &profile,
        assigned,
    )
    .await
    .map_err(internal)?;
    Ok(Json(CreateJobResponse {
        job_id: record_id_to_string(&row.id),
        plugin_id: row.plugin_id,
        target: row.target,
        profile: row.profile,
        status: row.status,
    }))
}

async fn get_job(
    Path(job_id): Path<String>,
) -> Result<Json<JobStatusResponse>, (StatusCode, String)> {
    let rid = RecordId::new(BUILD_JOB_TABLE, job_id.as_str());
    let row = BuildJob::get(&rid).await.map_err(internal)?;
    let row = row.ok_or((StatusCode::NOT_FOUND, format!("no job '{}'", job_id)))?;
    let wasm_base64 = row.wasm_bytes.as_ref().map(|b| {
        // `Bytes` derefs to `bytes::Bytes`; `&b[..]` gets us a `&[u8]`
        // without depending on the unstable `as_slice` method.
        base64::engine::general_purpose::STANDARD.encode(&b[..])
    });
    Ok(Json(JobStatusResponse {
        job_id: record_id_to_string(&row.id),
        plugin_id: row.plugin_id,
        target: row.target,
        profile: row.profile,
        status: row.status,
        claimed_worker_id: row.claimed_worker_id.as_ref().map(record_id_to_string),
        duration_ms: row.duration_ms,
        wasm_base64,
        stdout: row.stdout,
        stderr: row.stderr,
    }))
}

async fn list_workers() -> Result<Json<Vec<WorkerSummary>>, (StatusCode, String)> {
    // Stale window matches the worker heartbeat (30s) × 3. The schema
    // event refreshes `last_update` on every UPDATE so this is the
    // cheapest liveness signal we have without a dedicated heartbeat
    // route.
    let mut response = database::DATABASE
        .query(
            "SELECT * FROM connected_client \
             WHERE client_kind = 'build_worker' \
               AND last_update > time::now() - 90s \
             ORDER BY last_update DESC",
        )
        .await
        .map_err(internal)?;
    let rows: Vec<ConnectedClient> = response.take(0).map_err(internal)?;
    let _ = ClientKind::BuildWorker; // touch enum so unused-import lint stays quiet
    Ok(Json(
        rows.into_iter()
            .map(|c| WorkerSummary {
                connection_string: c.connection_string,
                friendly_name: c.friendly_name,
                local_ip: c.local_ip,
                last_update_iso: c.last_update.map(|d| d.to_string()),
            })
            .collect(),
    ))
}

// ── Helpers ────────────────────────────────────────────────────────

fn internal<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

/// Accept either `table:key` or bare `key` (assumed `connected_client`).
fn parse_record_id_loose(s: &str) -> RecordId {
    if let Some((table, key)) = s.split_once(':') {
        RecordId::new(table, key)
    } else {
        RecordId::new("connected_client", s)
    }
}

fn record_id_to_string(rid: &RecordId) -> String {
    // SurrealDB 3.x `RecordId` doesn't implement `Display`; the
    // codebase uses `RecordIdExt::key_string()` to get the bare key
    // and we reattach the table name so external callers see the
    // canonical `table:key` form.
    format!("{}:{}", rid.table, rid.key_string())
}
