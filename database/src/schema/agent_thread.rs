//! One Codex agent session about one customer machine.
//!
//! The admin-agent broker owns every row: it creates one when a tech accepts the
//! AI-help offer, records the codex thread id so a restart can resume the thread,
//! and keeps `status` current so any Mastertech instance can list live sessions
//! without talking to codex.

use serde::{Deserialize, Serialize};

use super::{Datetime, RecordId, SurrealValue};
use crate::db;

pub const AGENT_THREAD_TABLE: &str = "agent_thread";

/// Statuses a thread moves through; `closed` and `failed` are terminal.
pub const AGENT_THREAD_OPEN_STATUSES: [&str; 5] =
    ["queued", "starting", "idle", "running", "waiting_approval"];

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, SurrealValue)]
pub struct AgentThread {
    pub id: RecordId,
    #[serde(default)]
    #[surreal(default)]
    pub status: String,
    #[serde(default)]
    #[surreal(default)]
    pub connection_string: String,
    #[serde(default)]
    #[surreal(default)]
    pub hostname: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub service_number: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub store: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub requested_by: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub assignee: Option<RecordId>,
    #[serde(default)]
    #[surreal(default)]
    pub assist_request: Option<RecordId>,
    #[serde(default)]
    #[surreal(default)]
    pub service_order: Option<RecordId>,
    #[serde(default)]
    #[surreal(default)]
    pub computer: Option<RecordId>,
    #[serde(default)]
    #[surreal(default)]
    pub customer: Option<RecordId>,
    #[serde(default)]
    #[surreal(default)]
    pub diagnostic_session: Option<RecordId>,
    #[serde(default)]
    #[surreal(default)]
    pub codex_thread_id: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub model: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub provider: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub driven_by: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub tool_path: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub title: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub error: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub broker_node: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub allow_box_shell: bool,
    #[serde(default)]
    #[surreal(default)]
    pub tokens_used: Option<i64>,
    #[serde(default)]
    #[surreal(default)]
    pub tokens_window: Option<i64>,
    #[serde(default)]
    #[surreal(default)]
    pub last_seq: Option<i64>,
    #[serde(default)]
    #[surreal(default)]
    pub created_at: Option<Datetime>,
    #[serde(default)]
    #[surreal(default)]
    pub updated_at: Option<Datetime>,
    #[serde(default)]
    #[surreal(default)]
    pub last_event_at: Option<Datetime>,
    #[serde(default)]
    #[surreal(default)]
    pub closed_at: Option<Datetime>,
}

/// Everything known about a session before codex has been asked to start it.
#[derive(Debug, Clone, Default)]
pub struct NewAgentThread {
    pub assist_request: Option<RecordId>,
    pub connection_string: String,
    pub hostname: Option<String>,
    pub service_number: Option<String>,
    pub store: Option<String>,
    /// Email of the tech who asked; resolved to `assignee` on insert.
    pub requested_by: Option<String>,
    pub service_order: Option<RecordId>,
    pub computer: Option<RecordId>,
    pub customer: Option<RecordId>,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub driven_by: Option<String>,
    pub tool_path: Option<String>,
    pub broker_node: Option<String>,
    pub title: Option<String>,
}

impl AgentThread {
    pub fn is_open(&self) -> bool {
        AGENT_THREAD_OPEN_STATUSES.contains(&self.status.as_str())
    }

    /// Service number when known, else the machine.
    pub fn label(&self) -> String {
        match (&self.title, &self.service_number) {
            (Some(t), _) if !t.trim().is_empty() => t.clone(),
            (_, Some(sn)) => format!("#{sn} {}", self.hostname.clone().unwrap_or_default()).trim().to_string(),
            _ => self.connection_string.clone(),
        }
    }

    /// Inserts a `starting` row; the assignee is the user whose email matches the requester.
    pub async fn create(new: &NewAgentThread) -> anyhow::Result<RecordId> {
        let mut res = db()
            .query(
                "LET $assignee = (SELECT VALUE id FROM user WHERE email = $requested_by LIMIT 1)[0]; \
                 CREATE agent_thread CONTENT { status: 'starting', assist_request: $assist_request, \
                 connection_string: $cs, hostname: $hostname, service_number: $sn, store: $store, \
                 requested_by: $requested_by, assignee: $assignee, service_order: $service_order, \
                 computer: $computer, customer: $customer, model: $model, provider: $provider, \
                 driven_by: $driven_by, tool_path: $tool_path, broker_node: $broker_node, title: $title, \
                 updated_at: time::now() } RETURN VALUE id",
            )
            .bind(("assist_request", new.assist_request.clone()))
            .bind(("cs", new.connection_string.clone()))
            .bind(("hostname", new.hostname.clone()))
            .bind(("sn", new.service_number.clone()))
            .bind(("store", new.store.clone()))
            .bind(("requested_by", new.requested_by.clone()))
            .bind(("service_order", new.service_order.clone()))
            .bind(("computer", new.computer.clone()))
            .bind(("customer", new.customer.clone()))
            .bind(("model", new.model.clone()))
            .bind(("provider", new.provider.clone()))
            .bind(("driven_by", new.driven_by.clone()))
            .bind(("tool_path", new.tool_path.clone()))
            .bind(("broker_node", new.broker_node.clone()))
            .bind(("title", new.title.clone()))
            .await?;
        let ids: Vec<RecordId> = res.take(1).unwrap_or_default();
        ids.into_iter().next().ok_or_else(|| anyhow::anyhow!("agent_thread was not created"))
    }

    pub async fn get(id: &RecordId) -> anyhow::Result<Option<Self>> {
        let mut res = db().query("SELECT * FROM $id").bind(("id", id.clone())).await?;
        let rows: Vec<Self> = res.take(0).unwrap_or_default();
        Ok(rows.into_iter().next())
    }

    pub async fn set_codex_thread(id: &RecordId, codex_thread_id: &str) -> anyhow::Result<()> {
        db().query("UPDATE $id SET codex_thread_id = $t, updated_at = time::now()")
            .bind(("id", id.clone()))
            .bind(("t", codex_thread_id.to_string()))
            .await?;
        Ok(())
    }

    /// Moves the thread to `status`; terminal statuses also stamp `closed_at`.
    pub async fn set_status(id: &RecordId, status: &str, error: Option<&str>) -> anyhow::Result<()> {
        let terminal = matches!(status, "closed" | "failed");
        db().query(
            "UPDATE $id SET status = $status, error = $error ?? error, updated_at = time::now(), \
             closed_at = IF $terminal THEN time::now() ELSE closed_at END",
        )
        .bind(("id", id.clone()))
        .bind(("status", status.to_string()))
        .bind(("error", error.map(|e| e.chars().take(800).collect::<String>())))
        .bind(("terminal", terminal))
        .await?;
        Ok(())
    }

    pub async fn set_tokens(id: &RecordId, used: Option<i64>, window: Option<i64>) -> anyhow::Result<()> {
        db().query(
            "UPDATE $id SET tokens_used = $used ?? tokens_used, tokens_window = $window ?? tokens_window, \
             updated_at = time::now()",
        )
        .bind(("id", id.clone()))
        .bind(("used", used))
        .bind(("window", window))
        .await?;
        Ok(())
    }

    /// Records that a transcript row landed, keeping `last_seq` monotonic.
    pub async fn touch_event(id: &RecordId, seq: i64) -> anyhow::Result<()> {
        db().query(
            "UPDATE $id SET last_seq = math::max([last_seq ?? 0, $seq]), last_event_at = time::now(), \
             updated_at = time::now()",
        )
        .bind(("id", id.clone()))
        .bind(("seq", seq))
        .await?;
        Ok(())
    }

    pub async fn set_diagnostic_session(id: &RecordId, session: &RecordId) -> anyhow::Result<()> {
        db().query("UPDATE $id SET diagnostic_session = $ds, updated_at = time::now()")
            .bind(("id", id.clone()))
            .bind(("ds", session.clone()))
            .await?;
        Ok(())
    }

    /// Threads the broker should be attached to, oldest first.
    pub async fn open_threads() -> anyhow::Result<Vec<Self>> {
        let mut res = db()
            .query(
                "SELECT * FROM agent_thread WHERE status NOT IN ['closed', 'failed'] \
                 ORDER BY created_at ASC",
            )
            .await?;
        Ok(res.take(0).unwrap_or_default())
    }

    /// The live thread for a machine, if one exists.
    pub async fn active_for_connection(connection_string: &str) -> anyhow::Result<Option<Self>> {
        let mut res = db()
            .query(
                "SELECT * FROM agent_thread WHERE connection_string = $cs \
                 AND status NOT IN ['closed', 'failed'] ORDER BY created_at DESC LIMIT 1",
            )
            .bind(("cs", connection_string.to_string()))
            .await?;
        let rows: Vec<Self> = res.take(0).unwrap_or_default();
        Ok(rows.into_iter().next())
    }

    /// The newest thread for a machine regardless of status.
    pub async fn latest_for_connection(connection_string: &str) -> anyhow::Result<Option<Self>> {
        let mut res = db()
            .query(
                "SELECT * FROM agent_thread WHERE connection_string = $cs \
                 ORDER BY created_at DESC LIMIT 1",
            )
            .bind(("cs", connection_string.to_string()))
            .await?;
        let rows: Vec<Self> = res.take(0).unwrap_or_default();
        Ok(rows.into_iter().next())
    }

    /// Newest threads for a session list; `include_closed` widens past the live ones.
    pub async fn list_recent(limit: usize, include_closed: bool) -> anyhow::Result<Vec<Self>> {
        let sql = if include_closed {
            "SELECT * FROM agent_thread ORDER BY created_at DESC LIMIT $limit"
        } else {
            "SELECT * FROM agent_thread WHERE status NOT IN ['closed', 'failed'] \
             ORDER BY created_at DESC LIMIT $limit"
        };
        let mut res = db().query(sql).bind(("limit", limit)).await?;
        Ok(res.take(0).unwrap_or_default())
    }
}
