//! Fleet QC orchestrator routes.
//!
//! Agents (`qc-app` instances running on warehouse machines) call these
//! endpoints to register themselves, send heartbeats, push QC reports, and
//! poll for pending commands.  Admin tooling reads the fleet state through
//! the same endpoints (read-only GETs).
//!
//! All data is held in-memory in `FleetState` for simplicity.  For
//! production, back `agents` and `audit_log` with SurrealDB using the same
//! pattern as other routes.
//!
//! # Routes
//!
//! | Method | Path | Description |
//! |--------|------|-------------|
//! | POST | `/api/v1/qc/register` | Agent self-registers on startup |
//! | POST | `/api/v1/qc/heartbeat` | Liveness ping (every 30 s) |
//! | POST | `/api/v1/qc/report` | Full `QcReport` upload |
//! | GET  | `/api/v1/qc/agents` | List all known agents |
//! | GET  | `/api/v1/qc/agents/{machine_id}` | Single agent details |
//! | POST | `/api/v1/qc/agents/{machine_id}/command` | Enqueue a command |
//! | GET  | `/api/v1/qc/agents/{machine_id}/commands` | Agent polls pending commands |
//! | POST | `/api/v1/qc/agents/{machine_id}/ack` | Acknowledge a command |
//! | GET  | `/api/v1/qc/audit` | Append-only audit log (latest 500 entries) |

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::SystemTime;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::AppState;

// ── Data types ────────────────────────────────────────────────────────────────

/// A command that an admin can push to an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FleetCommand {
    pub id: String,
    pub issued_at: String,
    pub kind: FleetCommandKind,
    pub status: CommandStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum FleetCommandKind {
    /// Send an immediate QC report.
    SendReport,
    /// Custom free-form command (future expansion).
    Custom { payload: serde_json::Value },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CommandStatus {
    Pending,
    Acknowledged,
}

/// Per-agent runtime record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRecord {
    pub machine_id: String,
    pub agent_version: String,
    pub registered_at: String,
    pub last_heartbeat: String,
    pub last_report_at: Option<String>,
    pub cpu_avg_pct: f32,
    /// Pending commands the agent has not yet acknowledged.
    pub pending_commands: VecDeque<FleetCommand>,
    /// The last full report payload, stored as raw JSON.
    pub last_report: Option<serde_json::Value>,
}

/// Append-only audit log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub timestamp: String,
    pub machine_id: String,
    pub event: String,
    pub detail: Option<serde_json::Value>,
}

/// Shared fleet state (in-memory; swap for Surreal persistence later).
#[derive(Default)]
pub struct FleetState {
    pub agents: HashMap<String, AgentRecord>,
    /// Rolling audit log; capped at `AUDIT_CAP` entries.
    pub audit_log: VecDeque<AuditEntry>,
}

const AUDIT_CAP: usize = 500;

impl FleetState {
    fn audit(&mut self, machine_id: &str, event: &str, detail: Option<serde_json::Value>) {
        if self.audit_log.len() >= AUDIT_CAP {
            self.audit_log.pop_front();
        }
        self.audit_log.push_back(AuditEntry {
            timestamp: now_utc(),
            machine_id: machine_id.to_string(),
            event: event.to_string(),
            detail,
        });
    }
}

pub type SharedFleetState = Arc<Mutex<FleetState>>;

// ── Request / response types ──────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct RegisterRequest {
    pub machine_id: String,
    pub agent_version: String,
}

#[derive(Deserialize)]
pub struct HeartbeatRequest {
    pub machine_id: String,
    pub agent_version: String,
    pub cpu_avg_pct: f32,
}

#[derive(Deserialize)]
pub struct IssueCommandRequest {
    pub kind: FleetCommandKind,
}

#[derive(Deserialize)]
pub struct AckRequest {
    pub command_id: String,
}

// ── Route handlers ────────────────────────────────────────────────────────────

/// `POST /api/v1/qc/register` — agent self-registers on startup.
pub async fn register(
    State(app): State<AppState>,
    Json(req): Json<RegisterRequest>,
) -> impl IntoResponse {
    let mut state = app.fleet.lock().await;
    let now = now_utc();
    let record = state.agents.entry(req.machine_id.clone()).or_insert_with(|| AgentRecord {
        machine_id: req.machine_id.clone(),
        agent_version: req.agent_version.clone(),
        registered_at: now.clone(),
        last_heartbeat: now.clone(),
        last_report_at: None,
        cpu_avg_pct: 0.0,
        pending_commands: VecDeque::new(),
        last_report: None,
    });
    record.agent_version = req.agent_version.clone();
    record.last_heartbeat = now;
    state.audit(&req.machine_id, "register", Some(serde_json::json!({ "version": req.agent_version })));
    (StatusCode::OK, Json(serde_json::json!({ "status": "registered" })))
}

/// `POST /api/v1/qc/heartbeat` — liveness ping.
pub async fn heartbeat(
    State(app): State<AppState>,
    Json(req): Json<HeartbeatRequest>,
) -> impl IntoResponse {
    let mut state = app.fleet.lock().await;
    let now = now_utc();
    if let Some(rec) = state.agents.get_mut(&req.machine_id) {
        rec.last_heartbeat = now.clone();
        rec.cpu_avg_pct = req.cpu_avg_pct;
        rec.agent_version = req.agent_version;
    } else {
        // Auto-register on first heartbeat so agents that restart cleanly
        // appear in the fleet list immediately.
        state.agents.insert(req.machine_id.clone(), AgentRecord {
            machine_id: req.machine_id.clone(),
            agent_version: req.agent_version.clone(),
            registered_at: now.clone(),
            last_heartbeat: now.clone(),
            last_report_at: None,
            cpu_avg_pct: req.cpu_avg_pct,
            pending_commands: VecDeque::new(),
            last_report: None,
        });
    }
    state.audit(&req.machine_id, "heartbeat", Some(serde_json::json!({ "cpu": req.cpu_avg_pct })));
    (StatusCode::OK, Json(serde_json::json!({ "status": "ok" })))
}

/// `POST /api/v1/qc/report` — full `QcReport` upload.
pub async fn ingest_report(
    State(app): State<AppState>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    let machine_id = payload
        .get("machine_id")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let mut state = app.fleet.lock().await;
    let now = now_utc();

    let rec = state.agents.entry(machine_id.clone()).or_insert_with(|| AgentRecord {
        machine_id: machine_id.clone(),
        agent_version: payload.get("agent_version")
            .and_then(|v| v.as_str()).unwrap_or("?").to_string(),
        registered_at: now.clone(),
        last_heartbeat: now.clone(),
        last_report_at: None,
        cpu_avg_pct: 0.0,
        pending_commands: VecDeque::new(),
        last_report: None,
    });
    rec.last_report = Some(payload.clone());
    rec.last_report_at = Some(now.clone());
    rec.last_heartbeat = now;

    state.audit(&machine_id, "report", Some(payload));
    (StatusCode::OK, Json(serde_json::json!({ "status": "received" })))
}

/// `GET /api/v1/qc/agents` — list all known agents (without pending command queues).
pub async fn list_agents(
    State(app): State<AppState>,
) -> impl IntoResponse {
    let state = app.fleet.lock().await;
    let summary: Vec<serde_json::Value> = state.agents.values().map(|rec| {
        serde_json::json!({
            "machine_id": rec.machine_id,
            "agent_version": rec.agent_version,
            "registered_at": rec.registered_at,
            "last_heartbeat": rec.last_heartbeat,
            "last_report_at": rec.last_report_at,
            "cpu_avg_pct": rec.cpu_avg_pct,
            "pending_commands": rec.pending_commands.len(),
        })
    }).collect();
    Json(summary)
}

/// `GET /api/v1/qc/agents/:machine_id` — full agent record including last report.
pub async fn get_agent(
    State(app): State<AppState>,
    Path(machine_id): Path<String>,
) -> impl IntoResponse {
    let state = app.fleet.lock().await;
    match state.agents.get(&machine_id) {
        Some(rec) => (StatusCode::OK, Json(serde_json::to_value(rec).unwrap_or_default())),
        None => (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "agent not found" }))),
    }
}

/// `POST /api/v1/qc/agents/:machine_id/command` — enqueue a command.
pub async fn issue_command(
    State(app): State<AppState>,
    Path(machine_id): Path<String>,
    Json(req): Json<IssueCommandRequest>,
) -> impl IntoResponse {
    let mut state = app.fleet.lock().await;
    let now = now_utc();
    let id = format!("{}-{}", &machine_id[..machine_id.len().min(8)], epoch_secs());

    let cmd = FleetCommand {
        id: id.clone(),
        issued_at: now.clone(),
        kind: req.kind,
        status: CommandStatus::Pending,
    };

    let detail = serde_json::to_value(&cmd).ok();
    match state.agents.get_mut(&machine_id) {
        Some(rec) => {
            rec.pending_commands.push_back(cmd);
            state.audit(&machine_id, "command_issued", detail);
            (StatusCode::OK, Json(serde_json::json!({ "command_id": id })))
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "agent not found" })),
        ),
    }
}

/// `GET /api/v1/qc/agents/:machine_id/commands` — agent polls pending commands.
pub async fn poll_commands(
    State(app): State<AppState>,
    Path(machine_id): Path<String>,
) -> impl IntoResponse {
    let state = app.fleet.lock().await;
    match state.agents.get(&machine_id) {
        Some(rec) => {
            let pending: Vec<&FleetCommand> = rec
                .pending_commands
                .iter()
                .filter(|c| c.status == CommandStatus::Pending)
                .collect();
            (StatusCode::OK, Json(serde_json::to_value(pending).unwrap_or_default()))
        }
        None => (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "agent not found" }))),
    }
}

/// `POST /api/v1/qc/agents/:machine_id/ack` — acknowledge a command.
pub async fn ack_command(
    State(app): State<AppState>,
    Path(machine_id): Path<String>,
    Json(req): Json<AckRequest>,
) -> impl IntoResponse {
    let mut state = app.fleet.lock().await;
    let acked = if let Some(rec) = state.agents.get_mut(&machine_id) {
        let mut found = false;
        for cmd in rec.pending_commands.iter_mut() {
            if cmd.id == req.command_id {
                cmd.status = CommandStatus::Acknowledged;
                found = true;
                break;
            }
        }
        // Prune acknowledged commands to keep queue tidy.
        rec.pending_commands.retain(|c| c.status == CommandStatus::Pending);
        found
    } else {
        false
    };

    if acked {
        state.audit(&machine_id, "command_acked", Some(serde_json::json!({ "id": req.command_id })));
        (StatusCode::OK, Json(serde_json::json!({ "status": "acked" })))
    } else {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "command not found" })))
    }
}

/// `GET /api/v1/qc/audit` — recent audit log entries (latest 500).
pub async fn audit_log(
    State(app): State<AppState>,
) -> impl IntoResponse {
    let state = app.fleet.lock().await;
    let entries: Vec<&AuditEntry> = state.audit_log.iter().rev().take(500).collect();
    Json(serde_json::to_value(entries).unwrap_or_default())
}

// ── Router builder ────────────────────────────────────────────────────────────

/// Build the `/api/v1/qc` sub-router.
///
/// The returned `Router<AppState>` must be merged **before** the parent
/// calls `.with_state(state)`, which injects `AppState` into every handler.
pub fn qc_fleet_routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/qc/register",               axum::routing::post(register))
        .route("/api/v1/qc/heartbeat",               axum::routing::post(heartbeat))
        .route("/api/v1/qc/report",                  axum::routing::post(ingest_report))
        .route("/api/v1/qc/agents",                  axum::routing::get(list_agents))
        .route("/api/v1/qc/agents/{machine_id}",     axum::routing::get(get_agent))
        .route("/api/v1/qc/agents/{machine_id}/command",  axum::routing::post(issue_command))
        .route("/api/v1/qc/agents/{machine_id}/commands", axum::routing::get(poll_commands))
        .route("/api/v1/qc/agents/{machine_id}/ack",      axum::routing::post(ack_command))
        .route("/api/v1/qc/audit",                   axum::routing::get(audit_log))
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn now_utc() -> String {
    let secs = SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let (y, mo, d, h, mi, s) = epoch_to_parts(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

fn epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn epoch_to_parts(mut secs: u64) -> (u64, u64, u64, u64, u64, u64) {
    let s = secs % 60; secs /= 60;
    let mi = secs % 60; secs /= 60;
    let h = secs % 24; secs /= 24;
    let z = secs + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    (y, mo, d, h, mi, s)
}
