//! Pre-boot terminal relay.
//!
//! A UEFI firmware app can only dial out, so it POSTs its rendered TUI frames
//! here and GETs queued viewer input; the admin console does the mirror (GET
//! frame, POST input). Frames and events are the shared
//! [`tcp_protocol::preboot`] bincode payloads, opaque to the server. Sessions
//! are keyed by machine serial and evicted when idle.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Instant;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::{Json, Router};
use tokio::sync::Mutex;

use crate::AppState;

/// Pack queued event bodies as `[u32 LE count][(u32 LE len)(body)]*` — matches
/// `tcp_protocol::preboot::split_event_batch` on the firmware side. Inlined so
/// the server needs no dependency on the wire crate (payloads stay opaque).
fn encode_event_batch(bodies: &[Vec<u8>]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(bodies.len() as u32).to_le_bytes());
    for b in bodies {
        out.extend_from_slice(&(b.len() as u32).to_le_bytes());
        out.extend_from_slice(b);
    }
    out
}

/// Cap on buffered input events per session (drops oldest on overflow).
const MAX_INPUT_QUEUE: usize = 256;
/// Sessions idle longer than this are evicted on the next frame POST. Wide
/// enough to span the firmware presence heartbeat (~45s) so a heartbeating
/// box stays in the roster between frames.
const SESSION_IDLE_SECS: u64 = 90;

#[derive(Default)]
pub struct PreBootSession {
    /// Latest rendered frame (bincode `PreBootFrame`), served to the viewer.
    frame: Option<Vec<u8>>,
    frame_seq: u64,
    /// Queued input events (bincode `PreBootEvent`) awaiting the firmware GET.
    input: VecDeque<Vec<u8>>,
    last_seen: Option<Instant>,
}

pub type SharedPreBoot = Arc<Mutex<HashMap<String, PreBootSession>>>;

pub fn new_registry() -> SharedPreBoot {
    Arc::new(Mutex::new(HashMap::new()))
}

/// Firmware → relay: store the latest rendered frame for `serial`.
async fn post_frame(
    State(app): State<AppState>,
    Path(serial): Path<String>,
    body: Bytes,
) -> impl IntoResponse {
    let mut reg = app.preboot.lock().await;
    let now = Instant::now();
    reg.retain(|_, s| {
        s.last_seen.map(|t| now.duration_since(t).as_secs() < SESSION_IDLE_SECS).unwrap_or(true)
    });
    let s = reg.entry(serial).or_default();
    s.frame = Some(body.to_vec());
    s.frame_seq = s.frame_seq.wrapping_add(1);
    s.last_seen = Some(now);
    StatusCode::OK
}

/// Viewer → relay: fetch the latest frame for `serial` (204 if none yet).
async fn get_frame(State(app): State<AppState>, Path(serial): Path<String>) -> impl IntoResponse {
    let reg = app.preboot.lock().await;
    match reg.get(&serial).and_then(|s| s.frame.clone()) {
        Some(bytes) => (StatusCode::OK, bytes).into_response(),
        None => StatusCode::NO_CONTENT.into_response(),
    }
}

/// Viewer → relay: queue one input event for `serial`.
async fn post_input(
    State(app): State<AppState>,
    Path(serial): Path<String>,
    body: Bytes,
) -> impl IntoResponse {
    let mut reg = app.preboot.lock().await;
    let s = reg.entry(serial).or_default();
    if s.input.len() >= MAX_INPUT_QUEUE {
        s.input.pop_front();
    }
    s.input.push_back(body.to_vec());
    s.last_seen = Some(Instant::now());
    StatusCode::OK
}

/// Firmware → relay: drain queued input as a `[u32 count][(u32 len)(body)]*` batch.
async fn get_input(State(app): State<AppState>, Path(serial): Path<String>) -> impl IntoResponse {
    let mut reg = app.preboot.lock().await;
    let bodies: Vec<Vec<u8>> = match reg.get_mut(&serial) {
        Some(s) => {
            s.last_seen = Some(Instant::now());
            s.input.drain(..).collect()
        }
        None => Vec::new(),
    };
    (StatusCode::OK, encode_event_batch(&bodies))
}

/// Firmware → relay: presence heartbeat. Bumps (or creates) the
/// connected_client:qc_<serial> row so it stays "connected" between
/// fingerprints and the staleness reaper doesn't retire a live box.
async fn alive(State(app): State<AppState>, Path(serial): Path<String>) -> impl IntoResponse {
    // Presence in the in-memory roster (GET /preboot lists it whether or not it
    // is streaming a screen yet).
    app.preboot.lock().await.entry(serial.clone()).or_default().last_seen = Some(Instant::now());
    let id = format!("qc_{serial}");
    let q = "UPSERT type::thing('connected_client', $id) MERGE \
             { connected: true, last_update: time::now(), \
               client_kind: 'qc_agent', connection_string: $cs }";
    let _ = database::DATABASE.query(q).bind(("id", id)).bind(("cs", serial)).await;
    StatusCode::OK
}

/// Admin: list active pre-boot sessions for the viewer picker.
async fn list_sessions(State(app): State<AppState>) -> impl IntoResponse {
    let reg = app.preboot.lock().await;
    let now = Instant::now();
    let sessions: Vec<serde_json::Value> = reg
        .iter()
        .map(|(serial, s)| {
            let idle = s.last_seen.map(|t| now.duration_since(t).as_secs()).unwrap_or(u64::MAX);
            serde_json::json!({
                "serial": serial,
                "idle_secs": idle,
                "frame_seq": s.frame_seq,
                "has_frame": s.frame.is_some(),
            })
        })
        .collect();
    Json(serde_json::json!({ "sessions": sessions }))
}

pub fn preboot_routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/qc/preboot", axum::routing::get(list_sessions))
        .route(
            "/api/v1/qc/preboot/{serial}/frame",
            axum::routing::post(post_frame).get(get_frame),
        )
        .route(
            "/api/v1/qc/preboot/{serial}/input",
            axum::routing::post(post_input).get(get_input),
        )
        .route("/api/v1/qc/preboot/{serial}/alive", axum::routing::post(alive))
}
