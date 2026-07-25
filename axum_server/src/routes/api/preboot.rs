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
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::IntoResponse;
use axum::{Json, Router};
use serde::Serialize;
use tokio::sync::Mutex;

use crate::AppState;
use crate::routes::api::admin::{PeerAddr, now_rfc3339};

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
    /// Socket peer and User-Agent of the last firmware-originated request.
    origin: Origin,
    first_seen_at: Option<String>,
    last_seen_at: Option<String>,
}

/// Where a request came from, as observed at the socket and in the forwarding
/// headers. Populated for every advertisement and firmware heartbeat.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Origin {
    pub peer: Option<String>,
    pub forwarded_for: Option<String>,
    pub real_ip: Option<String>,
    pub user_agent: Option<String>,
    pub content_type: Option<String>,
}

impl Origin {
    fn from(peer: Option<SocketAddr>, headers: &HeaderMap) -> Self {
        let get = |name: &str| {
            headers
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        };
        Self {
            peer: peer.map(|p| p.to_string()),
            forwarded_for: get("x-forwarded-for"),
            real_ip: get("x-real-ip"),
            user_agent: get(header::USER_AGENT.as_str()),
            content_type: get(header::CONTENT_TYPE.as_str()),
        }
    }
}

/// A console advertising a direct-link TCP endpoint firmware can dial. Consoles
/// re-advertise periodically; entries older than [`CONSOLE_TTL_SECS`] are stale.
struct ConsoleEndpoint {
    addr: String,
    first_seen: Instant,
    last_seen: Instant,
    first_seen_at: String,
    last_seen_at: String,
    advert_count: u64,
    /// Gap between the two most recent advertisements.
    last_interval_secs: Option<u64>,
    origin: Origin,
    /// Advertisement body exactly as received.
    raw_body: String,
}

/// A POST to the console endpoint that did not yield a usable addr.
#[derive(Clone, Debug, Serialize)]
pub struct ConsoleReject {
    pub at: String,
    pub reason: String,
    pub raw_body: String,
    pub body_bytes: usize,
    pub origin: Origin,
}

/// Consoles live no longer than this between re-advertisements.
const CONSOLE_TTL_SECS: u64 = 120;
/// Rejected advertisements retained for the management console.
const MAX_CONSOLE_REJECTS: usize = 50;
/// Bytes of a rejected body retained.
const MAX_REJECT_BODY: usize = 2048;

#[derive(Default)]
pub struct PreBootRegistry {
    sessions: HashMap<String, PreBootSession>,
    /// Latest log snapshot per serial; survives session sweeps so a box that
    /// dropped off can still be post-mortemed via GET {serial}/logs.
    logs: HashMap<String, Vec<String>>,
    /// LAN endpoints advertised by admin consoles for the direct-link path,
    /// keyed by advertised addr. Firmware GETs these to dial a console.
    consoles: HashMap<String, ConsoleEndpoint>,
    /// Ring of advertisement POSTs that were refused, with their raw bodies.
    rejects: VecDeque<ConsoleReject>,
    /// Advertisements accepted since process start (survives TTL sweeps).
    advert_total: u64,
}

pub type SharedPreBoot = Arc<Mutex<PreBootRegistry>>;

pub fn new_registry() -> SharedPreBoot {
    Arc::new(Mutex::new(PreBootRegistry::default()))
}

impl PreBootSession {
    /// Stamp a firmware-originated request onto the session.
    fn touch(&mut self, origin: Origin) {
        self.last_seen = Some(Instant::now());
        let at = now_rfc3339();
        self.first_seen_at.get_or_insert_with(|| at.clone());
        self.last_seen_at = Some(at);
        self.origin = origin;
    }
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
    PeerAddr(peer): PeerAddr,
    Path(serial): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    tracing::debug!(serial = %serial, bytes = body.len(), "qc.preboot frame in");
    let origin = Origin::from(peer, &headers);
    let mut reg = app.preboot.lock().await;
    sweep(&mut reg);
    let s = reg.sessions.entry(serial).or_default();
    s.frame = Some(body.to_vec());
    s.frame_seq = s.frame_seq.wrapping_add(1);
    s.frame_at = Some(Instant::now());
    s.touch(origin);
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
async fn get_input(
    State(app): State<AppState>,
    PeerAddr(peer): PeerAddr,
    Path(serial): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let origin = Origin::from(peer, &headers);
    let mut reg = app.preboot.lock().await;
    let s = reg.sessions.entry(serial).or_default();
    s.touch(origin);
    let bodies: Vec<Vec<u8>> = s.input.drain(..).collect();
    (StatusCode::OK, encode_event_batch(&bodies))
}

/// Firmware → relay: is an admin viewer currently polling this serial? Drives
/// the firmware's auto-start/stop of TUI streaming.
async fn get_viewer(
    State(app): State<AppState>,
    PeerAddr(peer): PeerAddr,
    Path(serial): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let origin = Origin::from(peer, &headers);
    let mut reg = app.preboot.lock().await;
    let s = reg.sessions.entry(serial).or_default();
    s.touch(origin);
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
    PeerAddr(peer): PeerAddr,
    Path(serial): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let text = String::from_utf8_lossy(&body);
    let lines: Vec<String> = text.lines().map(str::to_string).collect();
    tracing::info!(serial = %serial, lines = lines.len(), "qc.preboot logs uploaded");
    tracing::debug!(serial = %serial, "qc.preboot logs:\n{text}");
    let origin = Origin::from(peer, &headers);
    let mut reg = app.preboot.lock().await;
    reg.sessions.entry(serial.clone()).or_default().touch(origin);
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
/// fingerprints and the staleness reaper doesn't retire a live box. While the
/// row is unlinked, each beat also tries to match a `computer` by OA3 key or
/// serial and links computer + customer.
async fn alive(
    State(app): State<AppState>,
    PeerAddr(peer): PeerAddr,
    Path(serial): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    tracing::info!(serial = %serial, "qc.preboot alive (presence heartbeat)");
    let origin = Origin::from(peer, &headers);
    // Presence in the in-memory roster (GET /preboot lists it whether or not it
    // is streaming a screen yet).
    {
        let mut reg = app.preboot.lock().await;
        sweep(&mut reg);
        reg.sessions.entry(serial.clone()).or_default().touch(origin);
    }
    // SDK upsert for the deterministic row id (the prod engine rejects
    // `type::thing`, and param-bound UPSERT targets no-op — verified live).
    use database::schema::{RecordId, SurrealValue};
    #[derive(SurrealValue)]
    struct AliveMerge {
        connected: bool,
        client_kind: String,
        connection_string: String,
    }
    let rid = RecordId::new("connected_client", format!("qc_{serial}"));
    let up: Result<Option<serde_json::Value>, _> = database::db()
        .upsert(rid)
        .merge(AliveMerge {
            connected: true,
            client_kind: "uefi".to_string(),
            connection_string: serial.clone(),
        })
        .await;
    if let Err(e) = up {
        tracing::warn!(serial = %serial, error = %e, "qc.preboot alive upsert failed");
    }
    // Bump last_update and, while unlinked, match a computer by OA3 key or any
    // serial field. No record ids in-query, so it parses on every engine.
    let q = "UPDATE connected_client SET last_update = time::now() \
             WHERE connection_string = $cs AND client_kind = 'uefi'; \
             LET $comp = (SELECT id, customer FROM computer \
                          WHERE oa3_key = $cs OR device_serial = $cs \
                             OR product_serial = $cs OR motherboard_serial = $cs \
                          LIMIT 1)[0]; \
             IF $comp != NONE { \
                 UPDATE connected_client SET computer = $comp.id, customer = $comp.customer \
                 WHERE connection_string = $cs AND client_kind = 'uefi' AND computer IS NONE; \
             };";
    match database::db()
        .query(q)
        .bind(("cs", serial.clone()))
        .await
        .and_then(|r| r.check())
    {
        Ok(_) => {}
        Err(e) => tracing::warn!(serial = %serial, error = %e, "qc.preboot alive link/bump failed"),
    }
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

/// Console → relay: advertise a direct-link LAN endpoint firmware can dial.
/// Body: `{"addr":"192.168.x.y:9209"}`. Idempotent on addr; refreshes the TTL.
/// Every POST — accepted or refused — is attributed to its socket peer and
/// User-Agent so the management console can say who is advertising.
async fn post_console(
    State(app): State<AppState>,
    PeerAddr(peer): PeerAddr,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let origin = Origin::from(peer, &headers);
    let raw_body = String::from_utf8_lossy(&body).to_string();
    let parsed = serde_json::from_slice::<serde_json::Value>(&body);
    let reason = match &parsed {
        Err(e) => Some(format!("body is not JSON: {e}")),
        Ok(v) => match v.get("addr") {
            None => Some("JSON has no `addr` field".to_string()),
            Some(a) if !a.is_string() => Some(format!("`addr` is {} not a string", kind_of(a))),
            Some(a) if a.as_str().unwrap_or("").trim().is_empty() => {
                Some("`addr` is empty".to_string())
            }
            Some(_) => None,
        },
    };

    let mut reg = app.preboot.lock().await;
    let now = Instant::now();
    reg.consoles.retain(|_, c| now.duration_since(c.last_seen).as_secs() < CONSOLE_TTL_SECS);

    if let Some(reason) = reason {
        tracing::warn!(
            peer = origin.peer.as_deref().unwrap_or("?"),
            user_agent = origin.user_agent.as_deref().unwrap_or("?"),
            bytes = body.len(),
            %reason,
            body = %truncate(&raw_body, MAX_REJECT_BODY),
            "qc.preboot console advertisement rejected"
        );
        if reg.rejects.len() >= MAX_CONSOLE_REJECTS {
            reg.rejects.pop_front();
        }
        reg.rejects.push_back(ConsoleReject {
            at: now_rfc3339(),
            reason,
            raw_body: truncate(&raw_body, MAX_REJECT_BODY),
            body_bytes: body.len(),
            origin,
        });
        return StatusCode::BAD_REQUEST;
    }

    let addr = parsed
        .ok()
        .and_then(|v| v.get("addr").and_then(|a| a.as_str()).map(str::to_string))
        .map(|a| a.trim().to_string())
        .unwrap_or_default();

    reg.advert_total += 1;
    let at = now_rfc3339();
    match reg.consoles.get_mut(&addr) {
        Some(c) => {
            c.last_interval_secs = Some(now.duration_since(c.last_seen).as_secs());
            c.last_seen = now;
            c.last_seen_at = at;
            c.advert_count += 1;
            c.origin = origin;
            c.raw_body = raw_body;
            tracing::debug!(
                %addr,
                peer = c.origin.peer.as_deref().unwrap_or("?"),
                user_agent = c.origin.user_agent.as_deref().unwrap_or("?"),
                count = c.advert_count,
                interval_secs = c.last_interval_secs.unwrap_or(0),
                "qc.preboot console re-advertised"
            );
        }
        None => {
            tracing::info!(
                %addr,
                peer = origin.peer.as_deref().unwrap_or("?"),
                forwarded_for = origin.forwarded_for.as_deref().unwrap_or("-"),
                user_agent = origin.user_agent.as_deref().unwrap_or("?"),
                body = %raw_body,
                "qc.preboot console advertised (new)"
            );
            reg.consoles.insert(
                addr.clone(),
                ConsoleEndpoint {
                    addr,
                    first_seen: now,
                    last_seen: now,
                    first_seen_at: at.clone(),
                    last_seen_at: at,
                    advert_count: 1,
                    last_interval_secs: None,
                    origin,
                    raw_body,
                },
            );
        }
    }
    StatusCode::OK
}

/// JSON type name, for the rejection reason.
fn kind_of(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "a bool",
        serde_json::Value::Number(_) => "a number",
        serde_json::Value::String(_) => "a string",
        serde_json::Value::Array(_) => "an array",
        serde_json::Value::Object(_) => "an object",
    }
}

/// Clip to `max` bytes on a char boundary, marking what was dropped.
fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… (+{} bytes)", &s[..end], s.len() - end)
}

/// Firmware → relay: list fresh console direct-link endpoints to dial.
async fn get_consoles(State(app): State<AppState>) -> impl IntoResponse {
    let mut reg = app.preboot.lock().await;
    let now = Instant::now();
    reg.consoles.retain(|_, c| now.duration_since(c.last_seen).as_secs() < CONSOLE_TTL_SECS);
    let consoles: Vec<serde_json::Value> = reg
        .consoles
        .values()
        .map(|c| {
            serde_json::json!({
                "addr": c.addr,
                "age_secs": now.duration_since(c.last_seen).as_secs(),
            })
        })
        .collect();
    Json(serde_json::json!({ "consoles": consoles }))
}

/// Console advertisement as reported to the management console.
#[derive(Serialize)]
pub struct ConsoleDetail {
    pub addr: String,
    pub age_secs: u64,
    pub alive_secs: u64,
    pub advert_count: u64,
    pub last_interval_secs: Option<u64>,
    pub first_seen_at: String,
    pub last_seen_at: String,
    pub raw_body: String,
    pub origin: Origin,
    /// `false` once the entry is past [`CONSOLE_TTL_SECS`] and due to be swept.
    pub fresh: bool,
}

/// Pre-boot session as reported to the management console.
#[derive(Serialize)]
pub struct SessionDetail {
    pub serial: String,
    pub idle_secs: u64,
    pub frame_seq: u64,
    pub has_frame: bool,
    pub frame_bytes: usize,
    pub streaming: bool,
    pub viewer: bool,
    pub queued_input: usize,
    pub log_lines: usize,
    pub first_seen_at: Option<String>,
    pub last_seen_at: Option<String>,
    pub origin: Origin,
}

#[derive(Serialize)]
pub struct PreBootDetail {
    pub now: String,
    pub session_idle_secs: u64,
    pub console_ttl_secs: u64,
    pub advert_total: u64,
    pub sessions: Vec<SessionDetail>,
    pub consoles: Vec<ConsoleDetail>,
    pub rejects: Vec<ConsoleReject>,
    /// Serials with a retained log snapshot but no live session.
    pub orphan_logs: Vec<String>,
}

/// Session / console / reject counts for the management console overview.
pub async fn admin_counts(reg: &SharedPreBoot) -> (usize, usize, usize) {
    let reg = reg.lock().await;
    (reg.sessions.len(), reg.consoles.len(), reg.rejects.len())
}

/// Full registry dump. Unlike [`list_sessions`] this does not sweep, so a
/// just-expired console is still visible with `fresh: false`.
pub async fn admin_snapshot(reg: &SharedPreBoot) -> PreBootDetail {
    let reg = reg.lock().await;
    let now = Instant::now();
    let mut sessions: Vec<SessionDetail> = reg
        .sessions
        .iter()
        .map(|(serial, s)| SessionDetail {
            serial: serial.clone(),
            idle_secs: s.last_seen.map(|t| now.duration_since(t).as_secs()).unwrap_or(u64::MAX),
            frame_seq: s.frame_seq,
            has_frame: s.frame.is_some(),
            frame_bytes: s.frame.as_ref().map(Vec::len).unwrap_or(0),
            streaming: s
                .frame_at
                .map(|t| now.duration_since(t).as_secs() < STREAMING_FRESH_SECS)
                .unwrap_or(false),
            viewer: s
                .viewer_last_poll
                .map(|t| now.duration_since(t).as_secs() < VIEWER_WAIT_SECS)
                .unwrap_or(false),
            queued_input: s.input.len(),
            log_lines: reg.logs.get(serial).map(Vec::len).unwrap_or(0),
            first_seen_at: s.first_seen_at.clone(),
            last_seen_at: s.last_seen_at.clone(),
            origin: s.origin.clone(),
        })
        .collect();
    sessions.sort_by(|a, b| a.serial.cmp(&b.serial));

    let mut consoles: Vec<ConsoleDetail> = reg
        .consoles
        .values()
        .map(|c| {
            let age = now.duration_since(c.last_seen).as_secs();
            ConsoleDetail {
                addr: c.addr.clone(),
                age_secs: age,
                alive_secs: now.duration_since(c.first_seen).as_secs(),
                advert_count: c.advert_count,
                last_interval_secs: c.last_interval_secs,
                first_seen_at: c.first_seen_at.clone(),
                last_seen_at: c.last_seen_at.clone(),
                raw_body: c.raw_body.clone(),
                origin: c.origin.clone(),
                fresh: age < CONSOLE_TTL_SECS,
            }
        })
        .collect();
    consoles.sort_by_key(|c| c.age_secs);

    let orphan_logs = reg
        .logs
        .keys()
        .filter(|s| !reg.sessions.contains_key(*s))
        .cloned()
        .collect();

    PreBootDetail {
        now: now_rfc3339(),
        session_idle_secs: SESSION_IDLE_SECS,
        console_ttl_secs: CONSOLE_TTL_SECS,
        advert_total: reg.advert_total,
        sessions,
        consoles,
        rejects: reg.rejects.iter().rev().cloned().collect(),
        orphan_logs,
    }
}

/// Drop one advertised console immediately. Returns `false` if it wasn't there.
pub async fn admin_evict_console(reg: &SharedPreBoot, addr: &str) -> bool {
    reg.lock().await.consoles.remove(addr).is_some()
}

pub fn preboot_routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/qc/preboot", axum::routing::get(list_sessions))
        .route(
            "/api/v1/qc/preboot/console",
            axum::routing::post(post_console).get(get_consoles),
        )
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

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers(ua: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(header::USER_AGENT, HeaderValue::from_str(ua).unwrap());
        h
    }

    fn peer(port: u16) -> PeerAddr {
        PeerAddr(Some(SocketAddr::from(([192, 168, 22, 243], port))))
    }

    async fn advertise(app: &AppState, body: &str, ua: &str) -> StatusCode {
        post_console(
            State(app.clone()),
            peer(51000),
            headers(ua),
            Bytes::from(body.to_string()),
        )
        .await
        .into_response()
        .status()
    }

    #[tokio::test]
    async fn advertisement_records_its_origin_and_body() {
        let app = AppState::new();
        let body = r#"{"addr":"192.168.99.128:9209"}"#;
        assert_eq!(advertise(&app, body, "MasterTech/4.8.0").await, StatusCode::OK);

        let snap = admin_snapshot(&app.preboot).await;
        assert_eq!(snap.advert_total, 1);
        let c = &snap.consoles[0];
        assert_eq!(c.addr, "192.168.99.128:9209");
        assert_eq!(c.advert_count, 1);
        assert_eq!(c.last_interval_secs, None);
        assert_eq!(c.raw_body, body);
        assert_eq!(c.origin.peer.as_deref(), Some("192.168.22.243:51000"));
        assert_eq!(c.origin.user_agent.as_deref(), Some("MasterTech/4.8.0"));
        assert!(c.fresh);
    }

    #[tokio::test]
    async fn re_advertising_the_same_addr_counts_instead_of_duplicating() {
        let app = AppState::new();
        let body = r#"{"addr":"192.168.99.128:9209"}"#;
        advertise(&app, body, "MasterTech/4.8.0").await;
        advertise(&app, body, "MasterTech/4.8.0").await;

        let snap = admin_snapshot(&app.preboot).await;
        assert_eq!(snap.consoles.len(), 1);
        assert_eq!(snap.consoles[0].advert_count, 2);
        assert!(snap.consoles[0].last_interval_secs.is_some());
        assert_eq!(snap.advert_total, 2);
    }

    #[tokio::test]
    async fn malformed_advertisements_land_in_the_reject_ring() {
        let app = AppState::new();
        for body in ["not json", r#"{"address":"1.2.3.4:9209"}"#, r#"{"addr":9209}"#, r#"{"addr":"  "}"#] {
            assert_eq!(advertise(&app, body, "curl/8").await, StatusCode::BAD_REQUEST);
        }

        let snap = admin_snapshot(&app.preboot).await;
        assert!(snap.consoles.is_empty());
        assert_eq!(snap.rejects.len(), 4);
        // Newest first.
        assert_eq!(snap.rejects[0].reason, "`addr` is empty");
        assert_eq!(snap.rejects[1].reason, "`addr` is a number not a string");
        assert_eq!(snap.rejects[2].reason, "JSON has no `addr` field");
        assert!(snap.rejects[3].reason.starts_with("body is not JSON"));
        assert_eq!(snap.rejects[3].raw_body, "not json");
        assert_eq!(snap.rejects[0].origin.user_agent.as_deref(), Some("curl/8"));
    }

    #[tokio::test]
    async fn eviction_removes_one_endpoint() {
        let app = AppState::new();
        advertise(&app, r#"{"addr":"a:1"}"#, "ua").await;
        advertise(&app, r#"{"addr":"b:2"}"#, "ua").await;

        assert!(admin_evict_console(&app.preboot, "a:1").await);
        assert!(!admin_evict_console(&app.preboot, "a:1").await);
        let snap = admin_snapshot(&app.preboot).await;
        assert_eq!(snap.consoles.len(), 1);
        assert_eq!(snap.consoles[0].addr, "b:2");
    }

    #[tokio::test]
    async fn firmware_heartbeat_attributes_the_session() {
        let app = AppState::new();
        let origin = Origin::from(peer(4000).0, &headers("uefi-agent/1"));
        app.preboot.lock().await.sessions.entry("SN123".into()).or_default().touch(origin);

        let snap = admin_snapshot(&app.preboot).await;
        assert_eq!(snap.sessions.len(), 1);
        assert_eq!(snap.sessions[0].serial, "SN123");
        assert_eq!(snap.sessions[0].origin.user_agent.as_deref(), Some("uefi-agent/1"));
        assert!(snap.sessions[0].first_seen_at.is_some());
    }

    #[test]
    fn truncate_marks_what_it_dropped() {
        assert_eq!(truncate("abc", 10), "abc");
        assert_eq!(truncate("abcdef", 3), "abc… (+3 bytes)");
        // Multi-byte input must not split a char.
        assert!(truncate("ééééé", 3).starts_with("é"));
    }
}
