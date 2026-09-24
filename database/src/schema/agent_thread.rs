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

/// Threads of the signed-in technician: assigned to them, or asked for with their email.
const SIGNED_IN_TECH_THREADS: &str =
    "$auth != NONE AND (assignee = $auth.id OR requested_by = $auth.email)";

/// A token count in thousands, or millions past a million.
fn compact_tokens(n: i64) -> String {
    match n {
        n if n >= 1_000_000 => format!("{:.1}M", n as f64 / 1_000_000.0),
        n if n >= 1_000 => format!("{}k", n / 1_000),
        n => n.to_string(),
    }
}

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

/// Thread-row fields written together; `None` leaves a field as it is.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentThreadState {
    pub status: Option<String>,
    pub error: Option<String>,
    pub last_seq: Option<i64>,
    pub tokens_used: Option<i64>,
    pub tokens_window: Option<i64>,
}

impl AgentThreadState {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Connection string of a technician's session with no machine in scope.
pub fn general_connection(email: &str) -> String {
    format!("general:{}", email.trim().to_lowercase())
}

/// A session with no machine in scope: records-only tools, no remote actions.
pub fn is_general(connection_string: &str) -> bool {
    connection_string.starts_with("general:")
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

    /// `context 45% · 58k/124k`, once both token counts are known.
    pub fn context_usage(&self) -> Option<String> {
        let used = self.tokens_used.filter(|u| *u >= 0)?;
        let window = self.tokens_window.filter(|w| *w > 0)?;
        Some(format!(
            "context {}% \u{00b7} {}/{}",
            used * 100 / window,
            compact_tokens(used),
            compact_tokens(window)
        ))
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
        let state = AgentThreadState {
            status: Some(status.to_string()),
            error: error.map(str::to_string),
            ..Default::default()
        };
        Self::save_state(id, &state).await
    }

    /// Writes the set fields of `state` in one update; `last_seq` never moves backwards.
    pub async fn save_state(id: &RecordId, state: &AgentThreadState) -> anyhow::Result<()> {
        let terminal = matches!(state.status.as_deref(), Some("closed" | "failed"));
        db().query(
            "UPDATE $id SET status = $status ?? status, \
             error = $error ?? error, \
             closed_at = IF $terminal THEN time::now() ELSE closed_at END, \
             last_seq = IF $seq != NONE THEN math::max([last_seq ?? 0, $seq]) ELSE last_seq END, \
             last_event_at = IF $seq != NONE THEN time::now() ELSE last_event_at END, \
             tokens_used = $used ?? tokens_used, tokens_window = $window ?? tokens_window, \
             updated_at = time::now()",
        )
        .bind(("id", id.clone()))
        .bind(("status", state.status.clone()))
        .bind(("error", state.error.as_ref().map(|e| e.chars().take(800).collect::<String>())))
        .bind(("terminal", terminal))
        .bind(("seq", state.last_seq))
        .bind(("used", state.tokens_used))
        .bind(("window", state.tokens_window))
        .await?
        .check()?;
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

    /// Deletes the transcript, turns and approvals of threads closed longer ago
    /// than `retention` (a duration such as `30d`); the thread rows stay, stamped `purged_at`.
    pub async fn purge_closed_before(retention: &str) -> anyhow::Result<usize> {
        let mut res = db()
            .query(
                "LET $old = (SELECT VALUE id FROM agent_thread WHERE status IN ['closed', 'failed'] \
                 AND purged_at = NONE AND closed_at != NONE \
                 AND closed_at < time::now() - type::duration($retention)); \
                 DELETE agent_event WHERE thread IN $old; \
                 DELETE agent_turn WHERE thread IN $old; \
                 DELETE agent_approval WHERE thread IN $old; \
                 UPDATE agent_thread SET purged_at = time::now() WHERE id IN $old; \
                 RETURN array::len($old);",
            )
            .bind(("retention", retention.to_string()))
            .await?;
        let purged: Option<i64> = res.take(5).unwrap_or_default();
        Ok(purged.unwrap_or(0).max(0) as usize)
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

    /// `LIVE SELECT` over the signed-in technician's threads.
    pub fn live_query_for_signed_in_tech() -> String {
        format!("LIVE SELECT * FROM agent_thread WHERE {SIGNED_IN_TECH_THREADS}")
    }

    /// The signed-in technician's threads that are open or changed in the last hour, newest first.
    pub async fn list_for_signed_in_tech(limit: usize) -> anyhow::Result<Vec<Self>> {
        let mut res = db()
            .query(format!(
                "SELECT * FROM agent_thread WHERE {SIGNED_IN_TECH_THREADS} \
                 AND (status NOT IN ['closed', 'failed'] OR updated_at > time::now() - 1h) \
                 ORDER BY updated_at DESC LIMIT $limit"
            ))
            .bind(("limit", limit))
            .await?;
        Ok(res.take(0)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn thread(used: Option<i64>, window: Option<i64>) -> AgentThread {
        AgentThread {
            id: RecordId::new(AGENT_THREAD_TABLE, "t"),
            status: "idle".to_string(),
            connection_string: String::new(),
            hostname: None,
            service_number: None,
            store: None,
            requested_by: None,
            assignee: None,
            assist_request: None,
            service_order: None,
            computer: None,
            customer: None,
            diagnostic_session: None,
            codex_thread_id: None,
            model: None,
            provider: None,
            driven_by: None,
            tool_path: None,
            title: None,
            error: None,
            broker_node: None,
            allow_box_shell: false,
            tokens_used: used,
            tokens_window: window,
            last_seq: None,
            created_at: None,
            updated_at: None,
            last_event_at: None,
            closed_at: None,
        }
    }

    #[test]
    fn context_usage_reads_percent_and_thousands() {
        assert_eq!(
            thread(Some(58_000), Some(124_518)).context_usage().as_deref(),
            Some("context 46% \u{00b7} 58k/124k")
        );
    }

    #[test]
    fn context_usage_keeps_small_and_million_counts_readable() {
        assert_eq!(
            thread(Some(640), Some(1_048_576)).context_usage().as_deref(),
            Some("context 0% \u{00b7} 640/1.0M")
        );
    }

    #[test]
    fn context_usage_needs_both_counts() {
        assert_eq!(thread(None, Some(124_518)).context_usage(), None);
        assert_eq!(thread(Some(58_000), None).context_usage(), None);
        assert_eq!(thread(Some(58_000), Some(0)).context_usage(), None);
    }
}

#[cfg(test)]
mod tests {
    use super::AgentThreadState;

    #[test]
    fn an_empty_state_writes_nothing() {
        assert!(AgentThreadState::default().is_empty());
        let seq_only = AgentThreadState { last_seq: Some(3), ..Default::default() };
        assert!(!seq_only.is_empty());
    }
}
