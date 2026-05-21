//! Fleet QC orchestrator schema.
//!
//! Three tables back the in-memory `FleetState` in `axum_server`:
//!
//! - [`FleetAgent`]  — one row per qc-app instance. Identified by the same
//!   `machine_id` the agent SHA-256s out of hostname + CPU brand. Survives
//!   restarts so the dashboard doesn't go dark every time axum_server
//!   redeploys.
//! - [`FleetEvent`]  — append-only audit log of register / heartbeat /
//!   report / command_issued / command_acked events. Cheap to query
//!   per-agent (the `agent_ref` link is indexed by the FETCH semantics
//!   of SurrealDB record refs).
//! - [`FleetCommand`] — pending + acknowledged commands. The agent polls
//!   `pending` rows, executes, then acks; the row sticks around so the
//!   admin UI has a queryable history.

use serde::{Deserialize, Serialize};

use super::{Datetime, RecordId, SurrealValue};

/// One row per known machine. `id = fleet_agent:<machine_id>` so upserts are
/// idempotent on a deterministic key. The `last_*` fields are denormalized
/// hot-path mirrors of the latest `fleet_event` — readers that just want
/// "is this agent online" don't pay for a join.
#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
pub struct FleetAgent {
    pub id: RecordId,
    pub machine_id: String,
    pub agent_version: String,
    pub registered_at: Datetime,
    pub last_heartbeat: Datetime,
    pub last_report_at: Option<Datetime>,
    /// Mean CPU% from the most recent heartbeat. 0..=100.
    pub cpu_avg_pct: f32,
    /// Optional hostname when the agent surfaces it. Helps humans on the
    /// admin side; `machine_id` is still the join key.
    pub hostname: Option<String>,
}

/// Audit log entry — append-only, never updated in place.
#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
pub struct FleetEvent {
    pub id: RecordId,
    pub agent_ref: RecordId,
    pub machine_id: String,
    pub kind: FleetEventKind,
    pub at: Datetime,
    /// Free-form structured payload (heartbeat snapshot, report excerpt, etc).
    /// Stored as JSON-equivalent so the admin UI can render without re-deriving
    /// row schemas every time we add a field.
    pub payload: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, SurrealValue)]
#[serde(rename_all = "snake_case")]
#[surreal(untagged)]
pub enum FleetEventKind {
    #[surreal(value = "register")]
    Register,
    #[surreal(value = "heartbeat")]
    Heartbeat,
    #[surreal(value = "report")]
    Report,
    #[surreal(value = "command_issued")]
    CommandIssued,
    #[surreal(value = "command_acked")]
    CommandAcked,
}

impl FleetEventKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Register => "register",
            Self::Heartbeat => "heartbeat",
            Self::Report => "report",
            Self::CommandIssued => "command_issued",
            Self::CommandAcked => "command_acked",
        }
    }
}

/// A command the admin pushed to an agent. The runtime queue lives in
/// `axum_server::FleetState`, but every command is mirrored here so we
/// can answer "what did we ask machine X to do this week".
#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
pub struct FleetCommand {
    pub id: RecordId,
    pub agent_ref: RecordId,
    pub machine_id: String,
    /// External (HTTP-visible) command id — what the agent quotes in its ack.
    pub external_id: String,
    pub kind: FleetCommandKind,
    pub status: FleetCommandStatus,
    pub issued_at: Datetime,
    pub acked_at: Option<Datetime>,
    /// Optional opaque payload for `Custom` commands.
    pub payload: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, SurrealValue)]
#[serde(rename_all = "snake_case")]
#[surreal(untagged)]
pub enum FleetCommandKind {
    /// Ask the agent to emit a full `QcReport` immediately.
    #[surreal(value = "send_report")]
    SendReport,
    /// Ask the agent to run a multi-stage stress scenario. Stage list is in `payload`.
    #[surreal(value = "run_stress_scenario")]
    RunStressScenario,
    /// Free-form command the operator stamped into `payload`.
    #[surreal(value = "custom")]
    Custom,
}

impl FleetCommandKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SendReport => "send_report",
            Self::RunStressScenario => "run_stress_scenario",
            Self::Custom => "custom",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, SurrealValue)]
#[serde(rename_all = "snake_case")]
#[surreal(untagged)]
pub enum FleetCommandStatus {
    #[surreal(value = "pending")]
    Pending,
    #[surreal(value = "acknowledged")]
    Acknowledged,
    /// Reserved for an explicit failure ack from the agent (not yet wired).
    #[surreal(value = "failed")]
    Failed,
}

impl FleetCommandStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Acknowledged => "acknowledged",
            Self::Failed => "failed",
        }
    }
}

/// Build the deterministic record id for an agent given its `machine_id`.
pub fn fleet_agent_record_id(machine_id: &str) -> RecordId {
    RecordId::new(super::FLEET_AGENT_TABLE, machine_id.to_string())
}
