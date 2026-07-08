//! Pre-boot terminal relay.
//!
//! A UEFI firmware app can only dial out, so it POSTs its rendered TUI frames
//! here and GETs queued viewer input; the admin console does the mirror (GET
//! frame, POST input). Frames and events are the shared
//! [`tcp_protocol::preboot`] bincode payloads, opaque to the server. Sessions
//! are keyed by machine serial; only firmware-originated requests (frame,
//! alive, input drain, viewer check, log upload) create or refresh a session,
//! so the roster reflects real firmware connectivity, never viewer activity.

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
/// Sessions with no firmware traffic for this long are swept. Wide enough to
/// span the firmware presence heartbeat (~45s); the ~5s viewer-flag polls keep
/// a live box far fresher than this.
const SESSION_IDLE_SECS: u64 = 90;
/// A viewer frame-poll within this window marks the session "viewer waiting",
/// which tells the firmware to auto-start streaming.
const VIEWER_WAIT_SECS: u64 = 10;

#[derive(Default)]
pub struct PreBootSession {
    /// Latest rendered frame (bincode `PreBootFrame`), served to the viewer.
    frame: Option<Vec<u8>>,
    frame_seq: u64,
    /// When the latest frame arrived (distinguishes live streams from stale frames).
    frame_at: Option<Instant>,
    /// Queued input events (bincode `PreBootEvent`) awaiting the firmware GET.
    input: VecDeque<Vec<u8>>,
    /// Last firmware-originated request.
    last_seen: Option<Instant>,
    /// Last viewer frame-poll or input POST (drives the auto-stream flag).
    viewer_last_poll: Option<Instant>,
}

#[derive(Default)]
pub struct PreBootRegistry {
    sessions: HashMap<String, PreBootSession>,
    /// Latest log snapshot per serial; survives session sweeps so a box that
    /// dropped off can still be post-mortemed via GET {serial}/logs.
    logs: HashMap<String, Vec<String>>,
}

pub type SharedPreBoot = Arc<Mutex<PreBootRegistry>>;

pub fn new_registry() -> SharedPreBoot {
    Arc::new(Mutex::new(PreBootRegistry::default()))
}

/// A frame newer than this means the firmware is actively streaming.
const STREAMING_FRESH_SECS: u64 = 5;

/// Drop sessions whose firmware side has gone quiet (log snapshots are kept).
fn sweep(reg: &mut PreBootRegistry) {
    let now = Instant::now();
    reg.sessions.retain(|_, s| {
        s.last_seen.map(|t| now.duration_since(t).as_secs() < SESSION_IDLE_SECS).unwrap_or(true)
    });
}

/// Firmware → relay: store the latest rendered frame for `serial`.
async fn post_frame(
    State(app): State<AppState>,
    Path(serial): Path<String>,
    body: Bytes,
) -> impl IntoResponse {
    tracing::debug!(serial = %serial, bytes = body.len(), "qc.preboot frame in");
    let mut reg = app.preboot.lock().await;
    sweep(&mut reg);
    let s = reg.sessions.entry(serial).or_default();
    s.frame = Some(body.to_vec());
    s.frame_seq = s.frame_seq.wrapping_add(1);
    s.frame_at = Some(Instant::now());
    s.last_seen = Some(Instant::now());
    StatusCode::OK
}

/// Viewer → relay: fetch the latest frame for `serial` (204 if none yet, 404 if
/// the firmware side isn't connected). Never creates a session — the roster
/// must reflect firmware connectivity only.
async fn get_frame(State(app): State<AppState>, Path(serial): Path<String>) -> impl IntoResponse {
    let mut reg = app.preboot.lock().await;
    match reg.sessions.get_mut(&serial) {
        Some(s) => {
            s.viewer_last_poll = Some(Instant::now());
            match s.frame.clone() {
                Some(bytes) => (StatusCode::OK, bytes).into_response(),
                None => StatusCode::NO_CONTENT.into_response(),
            }
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Viewer → relay: queue one input event for `serial`. 404 when the firmware
/// side isn't connected (never creates phantom sessions).
async fn post_input(
    State(app): State<AppState>,
    Path(serial): Path<String>,
    body: Bytes,
) -> impl IntoResponse {
    let mut reg = app.preboot.lock().await;
    let Some(s) = reg.sessions.get_mut(&serial) else {
        tracing::info!(serial = %serial, "qc.preboot input for unconnected serial");
        return (StatusCode::NOT_FOUND, "no such pre-boot session").into_response();
    };
    tracing::info!(serial = %serial, bytes = body.len(), "qc.preboot input queued");
    if s.input.len() >= MAX_INPUT_QUEUE {
        s.input.pop_front();
    }
    s.input.push_back(body.to_vec());
    s.viewer_last_poll = Some(Instant::now());
    StatusCode::OK.into_response()
}

/// Firmware → relay: drain queued input as a `[u32 count][(u32 len)(body)]*` batch.
async fn get_input(State(app): State<AppState>, Path(serial): Path<String>) -> impl IntoResponse {
    let mut reg = app.preboot.lock().await;
    let s = reg.sessions.entry(serial).or_default();
    s.last_seen = Some(Instant::now());
    let bodies: Vec<Vec<u8>> = s.input.drain(..).collect();
    (StatusCode::OK, encode_event_batch(&bodies))
}

/// Firmware → relay: is an admin viewer currently polling this serial? Drives
/// the firmware's auto-start/stop of TUI streaming.
async fn get_viewer(State(app): State<AppState>, Path(serial): Path<String>) -> impl IntoResponse {
    let mut reg = app.preboot.lock().await;
    let s = reg.sessions.entry(serial).or_default();
    s.last_seen = Some(Instant::now());
    let waiting = s
        .viewer_last_poll
        .map(|t| t.elapsed().as_secs() < VIEWER_WAIT_SECS)
        .unwrap_or(false);
    Json(serde_json::json!({ "viewer": waiting }))
}

/// Firmware → relay: replace the stored log snapshot for `serial` (the firmware
/// sends its whole in-memory ring each upload).
async fn post_logs(
    State(app): State<AppState>,
    Path(serial): Path<String>,
    body: Bytes,
) -> impl IntoResponse {
    let text = String::from_utf8_lossy(&body);
    let lines: Vec<String> = text.lines().map(str::to_string).collect();
    tracing::info!(serial = %serial, lines = lines.len(), "qc.preboot logs uploaded");
    tracing::debug!(serial = %serial, "qc.preboot logs:\n{text}");
    let mut reg = app.preboot.lock().await;
    reg.sessions.entry(serial.clone()).or_default().last_seen = Some(Instant::now());
    reg.logs.insert(serial, lines);
    StatusCode::OK
}

/// Admin: latest uploaded firmware logs as text/plain (curl-friendly).
async fn get_logs(State(app): State<AppState>, Path(serial): Path<String>) -> impl IntoResponse {
    let reg = app.preboot.lock().await;
    match reg.logs.get(&serial) {
        Some(lines) if !lines.is_empty() => (StatusCode::OK, lines.join("\n")).into_response(),
        _ => (StatusCode::NOT_FOUND, "no logs uploaded for this serial\n").into_response(),
    }
}

/// Firmware → relay: presence heartbeat. Bumps (or creates) the
/// connected_client:qc_<serial> row so it stays "connected" between
/// fingerprints and the staleness reaper doesn't retire a live box.
async fn alive(State(app): State<AppState>, Path(serial): Path<String>) -> impl IntoResponse {
    tracing::info!(serial = %serial, "qc.preboot alive (presence heartbeat)");
    // Presence in the in-memory roster (GET /preboot lists it whether or not it
    // is streaming a screen yet).
    {
        let mut reg = app.preboot.lock().await;
        sweep(&mut reg);
        reg.sessions.entry(serial.clone()).or_default().last_seen = Some(Instant::now());
    }
    let id = format!("qc_{serial}");
    let q = "UPSERT type::thing('connected_client', $id) MERGE \
             { connected: true, last_update: time::now(), \
               client_kind: 'qc_agent', connection_string: $cs }";
    let _ = database::DATABASE.query(q).bind(("id", id)).bind(("cs", serial)).await;
    StatusCode::OK
}

/// Admin: list active pre-boot sessions for the viewer picker.
async fn list_sessions(State(app): State<AppState>) -> impl IntoResponse {
    let mut reg = app.preboot.lock().await;
    sweep(&mut reg);
    let now = Instant::now();
    let sessions: Vec<serde_json::Value> = reg
        .sessions
        .iter()
        .map(|(serial, s)| {
            let idle = s.last_seen.map(|t| now.duration_since(t).as_secs()).unwrap_or(u64::MAX);
            let viewer = s
                .viewer_last_poll
                .map(|t| now.duration_since(t).as_secs() < VIEWER_WAIT_SECS)
                .unwrap_or(false);
            let streaming = s
                .frame_at
                .map(|t| now.duration_since(t).as_secs() < STREAMING_FRESH_SECS)
                .unwrap_or(false);
            serde_json::json!({
                "serial": serial,
                "idle_secs": idle,
                "frame_seq": s.frame_seq,
                "has_frame": s.frame.is_some(),
                "streaming": streaming,
                "viewer": viewer,
                "log_lines": reg.logs.get(serial).map(Vec::len).unwrap_or(0),
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
        .route("/api/v1/qc/preboot/{serial}/viewer", axum::routing::get(get_viewer))
        .route(
            "/api/v1/qc/preboot/{serial}/logs",
            axum::routing::post(post_logs).get(get_logs),
        )
        .route("/api/v1/qc/preboot/{serial}/alive", axum::routing::post(alive))
}
