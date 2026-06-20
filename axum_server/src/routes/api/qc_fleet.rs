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
use database::schema::fleet::{
    fleet_agent_record_id, FleetAgent, FleetCommand as DbFleetCommand,
    FleetCommandKind as DbFleetCommandKind, FleetCommandStatus as DbFleetCommandStatus,
    FleetEvent, FleetEventKind,
};
use database::schema::{
    random_record_id, Datetime as DbDatetime, FLEET_AGENT_TABLE, FLEET_COMMAND_TABLE,
    FLEET_EVENT_TABLE,
};
use database::DATABASE;
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

/// External tagging on purpose: lets the wire body be
/// `{"kind": "send_report"}` or `{"kind": {"custom": {"payload": ...}}}`.
/// Internal tagging would collide with the surrounding `IssueCommandRequest.kind`
/// field and force callers to double-nest.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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
    {
        let record = state
            .agents
            .entry(req.machine_id.clone())
            .or_insert_with(|| AgentRecord {
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
        record.last_heartbeat = now.clone();
    }
    state.audit(
        &req.machine_id,
        "register",
        Some(serde_json::json!({ "version": req.agent_version })),
    );

    let snapshot = state
        .agents
        .get(&req.machine_id)
        .cloned()
        .expect("agent inserted above");
    drop(state);

    tracing::info!(
        machine_id = %req.machine_id,
        version = %req.agent_version,
        "fleet.register",
    );
    mirror_agent_upsert(snapshot);
    mirror_event(
        req.machine_id.clone(),
        FleetEventKind::Register,
        Some(serde_json::json!({ "version": req.agent_version })),
    );

    (StatusCode::OK, Json(serde_json::json!({ "status": "registered" })))
}

/// `POST /api/v1/qc/heartbeat` — liveness ping.
pub async fn heartbeat(
    State(app): State<AppState>,
    Json(req): Json<HeartbeatRequest>,
) -> impl IntoResponse {
    let mut state = app.fleet.lock().await;
    let now = now_utc();
    let was_new = !state.agents.contains_key(&req.machine_id);
    if let Some(rec) = state.agents.get_mut(&req.machine_id) {
        rec.last_heartbeat = now.clone();
        rec.cpu_avg_pct = req.cpu_avg_pct;
        rec.agent_version = req.agent_version.clone();
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

    let snapshot = state.agents.get(&req.machine_id).cloned();
    drop(state);

    if was_new {
        tracing::info!(
            machine_id = %req.machine_id,
            cpu_pct = req.cpu_avg_pct,
            "fleet.heartbeat -> auto-registered on first beat",
        );
    } else {
        tracing::debug!(
            machine_id = %req.machine_id,
            cpu_pct = req.cpu_avg_pct,
            "fleet.heartbeat",
        );
    }
    if let Some(rec) = snapshot {
        mirror_agent_upsert(rec);
    }
    mirror_event(
        req.machine_id.clone(),
        FleetEventKind::Heartbeat,
        Some(serde_json::json!({ "cpu": req.cpu_avg_pct })),
    );

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

    {
        let rec = state
            .agents
            .entry(machine_id.clone())
            .or_insert_with(|| AgentRecord {
                machine_id: machine_id.clone(),
                agent_version: payload
                    .get("agent_version")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?")
                    .to_string(),
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
    }

    state.audit(&machine_id, "report", Some(payload.clone()));

    let snapshot = state
        .agents
        .get(&machine_id)
        .cloned()
        .expect("agent inserted above");
    drop(state);

    tracing::info!(
        machine_id = %machine_id,
        "fleet.report (bytes={})",
        serde_json::to_string(&payload).map(|s| s.len()).unwrap_or(0)
    );
    mirror_agent_upsert(snapshot);
    mirror_event(machine_id, FleetEventKind::Report, Some(payload));

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
        kind: req.kind.clone(),
        status: CommandStatus::Pending,
    };

    let detail = serde_json::to_value(&cmd).ok();
    match state.agents.get_mut(&machine_id) {
        Some(rec) => {
            rec.pending_commands.push_back(cmd.clone());
            state.audit(&machine_id, "command_issued", detail.clone());
            drop(state);

            tracing::info!(
                machine_id = %machine_id,
                command_id = %id,
                kind = ?req.kind,
                "fleet.command_issued",
            );
            mirror_command_issued(&machine_id, &cmd);
            mirror_event(machine_id, FleetEventKind::CommandIssued, detail);

            (StatusCode::OK, Json(serde_json::json!({ "command_id": id })))
        }
        None => {
            tracing::warn!(
                machine_id = %machine_id,
                "fleet.command_issued -> 404 (agent unknown)",
            );
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "agent not found" })),
            )
        }
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
        state.audit(
            &machine_id,
            "command_acked",
            Some(serde_json::json!({ "id": req.command_id })),
        );
        drop(state);
        tracing::info!(
            machine_id = %machine_id,
            command_id = %req.command_id,
            "fleet.command_acked",
        );
        mirror_command_acked(&req.command_id);
        mirror_event(
            machine_id,
            FleetEventKind::CommandAcked,
            Some(serde_json::json!({ "id": req.command_id })),
        );
        (StatusCode::OK, Json(serde_json::json!({ "status": "acked" })))
    } else {
        tracing::warn!(
            machine_id = %machine_id,
            command_id = %req.command_id,
            "fleet.command_acked -> 404 (command not found)",
        );
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "command not found" })),
        )
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
        .route("/api/v1/qc/fingerprint",             axum::routing::post(ingest_fingerprint))
        .route("/api/v1/qc/fingerprint/{serial}",    axum::routing::get(get_fingerprint))
}

/// `POST /api/v1/qc/fingerprint` — pre-OS hardware fingerprint from the
/// Mastertech UEFI agent (HTTP path). Delegates to [`store_fingerprint`].
pub async fn ingest_fingerprint(Json(payload): Json<serde_json::Value>) -> impl IntoResponse {
    let resp = store_fingerprint(payload, None).await;
    let ok = resp.get("status").and_then(|v| v.as_str()) == Some("stored");
    let code = if ok {
        StatusCode::OK
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };
    (code, Json(resp))
}

/// `GET /api/v1/qc/fingerprint/{serial}` — the stored pre-boot fingerprint for a serial.
pub async fn get_fingerprint(Path(serial): Path<String>) -> impl IntoResponse {
    use database::schema::qc_fingerprint::{fingerprint_record_id, HardwareFingerprint};
    let res: Result<Option<HardwareFingerprint>, _> =
        DATABASE.select(fingerprint_record_id(&serial)).await;
    match res {
        Ok(Some(fp)) => (StatusCode::OK, Json(serde_json::to_value(&fp).unwrap_or_default())),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "no fingerprint for serial", "serial": serial })),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        ),
    }
}

/// Persist a posted fingerprint: upsert `qc_fingerprint:<serial>`, project the
/// hardware fields into `computer:<serial>`, mark the box as a live
/// `connected_client` of kind `qc_agent`, and append a `fleet_event`. Shared by
/// the HTTP route and the plain-TCP QC listener. Returns the JSON response body.
/// `source_ip` is the dialing box's IP when known (TCP path).
/// SMBIOS strings vendors ship instead of a real serial number.
fn is_placeholder_serial(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() || t == "-" || t == "0" {
        return true;
    }
    let l = t.to_lowercase();
    l.starts_with("to be filled")
        || matches!(
            l.as_str(),
            "system serial number"
                | "chassis serial number"
                | "base board serial number"
                | "default string"
                | "unknown"
                | "none"
                | "not specified"
                | "not applicable"
                | "n/a"
                | "no serial"
                | "invalid"
                | "123456789"
                | "0123456789"
        )
}

/// Stable machine key for record ids. SMBIOS serials are preferred but
/// placeholder strings ("System Serial Number", "To be filled by O.E.M.", …)
/// would key every such box onto one record, so fall back through baseboard
/// serial → SMBIOS UUID → first MAC.
fn derive_machine_key(payload: &serde_json::Value) -> String {
    let s = |ptr: &str| {
        payload
            .pointer(ptr)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string()
    };
    for candidate in [s("/system/serial"), s("/baseboard/serial")] {
        if !is_placeholder_serial(&candidate) {
            return candidate;
        }
    }
    let uuid = s("/system/uuid").to_lowercase();
    let uuid_digits: String = uuid.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if !uuid_digits.is_empty()
        && !uuid_digits.chars().all(|c| c == '0')
        && !uuid_digits.chars().all(|c| c == 'f')
    {
        return format!("uuid-{uuid}");
    }
    if let Some(mac) = payload
        .pointer("/macs/0")
        .and_then(|v| v.as_str())
        .map(|m| m.replace([':', '-'], "").to_lowercase())
        .filter(|m| !m.is_empty() && !m.chars().all(|c| c == '0'))
    {
        return format!("mac-{mac}");
    }
    "unknown".to_string()
}

pub async fn store_fingerprint(
    payload: serde_json::Value,
    source_ip: Option<String>,
) -> serde_json::Value {
    use database::schema::qc_fingerprint::{fingerprint_record_id, HardwareFingerprint};

    let s = |ptr: &str| {
        payload
            .pointer(ptr)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string()
    };
    let serial = derive_machine_key(&payload);
    let cpu_cores = payload
        .pointer("/cpu/cores")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as u32;
    let ram_bytes = payload
        .pointer("/memory/total_bytes")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let dimm_count = payload
        .pointer("/memory/dimms")
        .and_then(|v| v.as_array())
        .map_or(0, |a| a.len()) as u32;
    let disk_count = payload
        .pointer("/storage")
        .and_then(|v| v.as_array())
        .map_or(0, |a| a.len()) as u32;
    let win11_ready = payload
        .get("win11_ready")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let rec = HardwareFingerprint {
        id: fingerprint_record_id(&serial),
        serial: serial.clone(),
        uuid: s("/system/uuid"),
        captured_at: now_db_datetime(),
        cpu_model: s("/cpu/model"),
        cpu_cores,
        ram_bytes,
        dimm_count,
        disk_count,
        win11_ready,
        raw: payload.clone(),
    };

    let res: Result<Option<HardwareFingerprint>, _> =
        DATABASE.upsert(fingerprint_record_id(&serial)).content(rec).await;

    // Project the hardware fields into a `computer` record keyed by serial.
    // OS/business fields are left blank for the Windows agent / order linkage.
    {
        use database::schema::computer::{ComputerData, DriveData};
        use database::schema::{COMPUTER_TABLE, RecordId};

        let gpu = payload
            .pointer("/gpu")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|g| {
                        let vid = g.get("vendor_id").and_then(|x| x.as_u64()).unwrap_or(0);
                        if vid == 0 {
                            return None;
                        }
                        let vn = g.get("vendor").and_then(|x| x.as_str()).unwrap_or("");
                        let did = g.get("device_id").and_then(|x| x.as_u64()).unwrap_or(0);
                        Some(format!("{vn} [{vid:04x}:{did:04x}]"))
                    })
                    .collect::<Vec<_>>()
                    .join("; ")
            })
            .unwrap_or_default();

        let ram = match ram_bytes / (1024 * 1024 * 1024) {
            0 => String::new(),
            g => format!("{g} GB"),
        };

        let drives: Vec<DriveData> = payload
            .pointer("/storage")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .map(|d| {
                        let gib = d.get("capacity_bytes").and_then(|x| x.as_u64()).unwrap_or(0)
                            / (1024 * 1024 * 1024);
                        DriveData {
                            drive_letter: String::new(),
                            drive_type: d
                                .get("drive_type")
                                .and_then(|x| x.as_str())
                                .unwrap_or("")
                                .to_string(),
                            total_size: if gib > 0 { gib.to_string() } else { String::new() },
                            space_left: String::new(),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        let comp = ComputerData {
            id: RecordId::new(COMPUTER_TABLE, serial.clone()),
            cpu: s("/cpu/model"),
            gpu,
            ram,
            drives,
            device_name: Some(s("/firmware/chassis")),
            device_mfg: Some(s("/system/manufacturer")),
            device_model: Some(s("/system/product")),
            device_serial: Some(serial.clone()),
            motherboard_name: s("/baseboard/product"),
            motherboard_serial: s("/baseboard/serial"),
            motherboard_asset_tag: s("/baseboard/asset_tag"),
            motherboard_vendor: s("/baseboard/manufacturer"),
            product_name: s("/system/product"),
            product_sku: s("/system/sku"),
            product_serial: s("/system/serial"),
            product_vendor: s("/system/manufacturer"),
            ..ComputerData::default()
        };
        let cid = comp.id.clone();
        let r: Result<Option<ComputerData>, _> = DATABASE.upsert(cid).content(comp).await;
        if let Err(e) = r {
            tracing::warn!(serial = %serial, error = %e, "qc.fingerprint computer upsert failed");
        }
    }

    // Mark the box as a live connected client (kind = qc_agent) so it shows up
    // in the inventory/dashboard for the duration of the QC session.
    {
        use database::schema::client::{ClientKind, ConnectedClient};
        use database::schema::{CONNECTED_CLIENT_TABLE, RecordId};

        let friendly = match s("/system/product") {
            p if p.is_empty() => format!("UEFI QC {serial}"),
            p => p,
        };
        let cc = ConnectedClient {
            id: RecordId::new(CONNECTED_CLIENT_TABLE, format!("qc_{serial}")),
            connection_string: serial.clone(),
            client_hash: serial.clone(),
            connected: true,
            last_update: Some(now_db_datetime()),
            friendly_name: Some(friendly),
            local_ip: source_ip,
            client_kind: ClientKind::QcAgent,
            ..Default::default()
        };
        let cid = cc.id.clone();
        let r: Result<Option<ConnectedClient>, _> = DATABASE.upsert(cid).content(cc).await;
        if let Err(e) = r {
            tracing::warn!(serial = %serial, error = %e, "qc.fingerprint connected_client upsert failed");
        }
    }

    // Append-only history alongside the upserted current row.
    mirror_event(serial.clone(), FleetEventKind::Report, Some(payload));

    match res {
        Ok(_) => {
            tracing::info!(serial = %serial, "qc.fingerprint stored");
            serde_json::json!({ "status": "stored", "serial": serial, "win11_ready": win11_ready })
        }
        Err(e) => {
            tracing::warn!(serial = %serial, error = %e, "qc.fingerprint upsert failed");
            serde_json::json!({ "status": "error", "error": e.to_string() })
        }
    }
}

// ── SurrealDB mirror helpers ──────────────────────────────────────────────────
//
// All mirror writes are `tokio::spawn`-ed so the HTTP response is never blocked
// on a DB round-trip. The in-memory `FleetState` is the source of truth on the
// request path; SurrealDB is the durable replica that the dashboard reads back
// when axum_server restarts.

/// Fire-and-forget upsert into `fleet_agent`. Idempotent on `machine_id` —
/// the row id is `fleet_agent:<machine_id>`.
fn mirror_agent_upsert(rec: AgentRecord) {
    tokio::spawn(async move {
        let id = fleet_agent_record_id(&rec.machine_id);
        let registered_at = parse_rfc3339(&rec.registered_at);
        let last_heartbeat = parse_rfc3339(&rec.last_heartbeat);
        let last_report_at = rec
            .last_report_at
            .as_deref()
            .map(parse_rfc3339);

        let agent = FleetAgent {
            id: id.clone(),
            machine_id: rec.machine_id.clone(),
            agent_version: rec.agent_version.clone(),
            registered_at,
            last_heartbeat,
            last_report_at,
            cpu_avg_pct: rec.cpu_avg_pct,
            hostname: None,
        };

        let res: Result<Option<FleetAgent>, _> = DATABASE.upsert(id).content(agent).await;
        if let Err(e) = res {
            tracing::warn!(
                machine_id = %rec.machine_id,
                error = %e,
                "fleet.mirror_agent_upsert failed (DB unavailable?)",
            );
        }
    });
}

/// Append a new `fleet_event` row.
fn mirror_event(machine_id: String, kind: FleetEventKind, payload: Option<serde_json::Value>) {
    tokio::spawn(async move {
        let event = FleetEvent {
            id: random_record_id(FLEET_EVENT_TABLE),
            agent_ref: fleet_agent_record_id(&machine_id),
            machine_id: machine_id.clone(),
            kind,
            at: now_db_datetime(),
            payload,
        };
        let res: Result<Option<FleetEvent>, _> =
            DATABASE.create(FLEET_EVENT_TABLE).content(event).await;
        if let Err(e) = res {
            tracing::warn!(
                machine_id = %machine_id,
                error = %e,
                "fleet.mirror_event failed",
            );
        }
    });
}

/// Create a `fleet_command` row in `pending` state.
fn mirror_command_issued(machine_id: &str, cmd: &FleetCommand) {
    let machine_id = machine_id.to_string();
    let external_id = cmd.id.clone();
    let issued_at = parse_rfc3339(&cmd.issued_at);
    let (kind, payload) = command_to_db(&cmd.kind);
    tokio::spawn(async move {
        let row = DbFleetCommand {
            id: random_record_id(FLEET_COMMAND_TABLE),
            agent_ref: fleet_agent_record_id(&machine_id),
            machine_id: machine_id.clone(),
            external_id,
            kind,
            status: DbFleetCommandStatus::Pending,
            issued_at,
            acked_at: None,
            payload,
        };
        let res: Result<Option<DbFleetCommand>, _> =
            DATABASE.create(FLEET_COMMAND_TABLE).content(row).await;
        if let Err(e) = res {
            tracing::warn!(
                machine_id = %machine_id,
                error = %e,
                "fleet.mirror_command_issued failed",
            );
        }
    });
}

/// Mark every `fleet_command` row with this `external_id` as acknowledged.
/// We update by query rather than by id because the DB row id is the random
/// `fleet_command:<uuid>` we generated on issue, not the short HTTP id the
/// agent quotes back.
fn mirror_command_acked(external_id: &str) {
    let external_id = external_id.to_string();
    tokio::spawn(async move {
        let q = format!(
            "UPDATE {table} SET status = 'acknowledged', acked_at = time::now() \
             WHERE external_id = $eid AND status = 'pending'",
            table = FLEET_COMMAND_TABLE
        );
        if let Err(e) = DATABASE.query(q).bind(("eid", external_id.clone())).await {
            tracing::warn!(
                external_id = %external_id,
                error = %e,
                "fleet.mirror_command_acked failed",
            );
        }
    });
}

fn command_to_db(kind: &FleetCommandKind) -> (DbFleetCommandKind, Option<serde_json::Value>) {
    match kind {
        FleetCommandKind::SendReport => (DbFleetCommandKind::SendReport, None),
        FleetCommandKind::Custom { payload } => {
            (DbFleetCommandKind::Custom, Some(payload.clone()))
        }
    }
}

fn parse_rfc3339(s: &str) -> DbDatetime {
    chrono::DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&chrono::Utc))
        .unwrap_or_else(|_| chrono::Utc::now())
        .into()
}

fn now_db_datetime() -> DbDatetime {
    chrono::Utc::now().into()
}

/// Read every `fleet_agent` row back into the in-memory `FleetState`.
/// Called once on axum_server startup so a server restart doesn't black-hole
/// the admin dashboard until every agent re-registers.
pub async fn hydrate_from_db(state: &SharedFleetState) -> anyhow::Result<usize> {
    let mut rows: Vec<FleetAgent> = DATABASE.select(FLEET_AGENT_TABLE).await?;
    let count = rows.len();
    let mut guard = state.lock().await;
    for r in rows.drain(..) {
        let agent = AgentRecord {
            machine_id: r.machine_id.clone(),
            agent_version: r.agent_version,
            registered_at: db_datetime_to_iso(&r.registered_at),
            last_heartbeat: db_datetime_to_iso(&r.last_heartbeat),
            last_report_at: r.last_report_at.as_ref().map(db_datetime_to_iso),
            cpu_avg_pct: r.cpu_avg_pct,
            pending_commands: VecDeque::new(),
            last_report: None,
        };
        guard.agents.insert(r.machine_id, agent);
    }
    tracing::info!(loaded = count, "fleet.hydrate_from_db");
    Ok(count)
}

fn db_datetime_to_iso(dt: &DbDatetime) -> String {
    let chrono_dt: chrono::DateTime<chrono::Utc> = (*dt).into();
    chrono_dt.format("%Y-%m-%dT%H:%M:%SZ").to_string()
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
