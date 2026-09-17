//! A decision the codex agent is waiting on: permission for a machine-touching
//! tool, or a structured question for the technician.
//!
//! The broker creates a row and polls it; any signed-in Mastertech instance may
//! decide it. The decision is a single conditional UPDATE, so two techs clicking
//! at once produce one winner and one stale-click.

use serde::{Deserialize, Serialize};

use super::{Datetime, RecordId, SurrealValue};
use crate::db;

pub const AGENT_APPROVAL_TABLE: &str = "agent_approval";

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, SurrealValue)]
pub struct AgentApproval {
    pub id: RecordId,
    pub thread: RecordId,
    #[serde(default)]
    #[surreal(default)]
    pub kind: String,
    #[serde(default)]
    #[surreal(default)]
    pub method: String,
    #[serde(default)]
    #[surreal(default)]
    pub codex_request_id: String,
    #[serde(default)]
    #[surreal(default)]
    pub summary: String,
    #[serde(default)]
    #[surreal(default)]
    pub server: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub tool: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub arguments: Option<serde_json::Value>,
    #[serde(default)]
    #[surreal(default)]
    pub params: Option<serde_json::Value>,
    #[serde(default)]
    #[surreal(default)]
    pub questions: Option<serde_json::Value>,
    #[serde(default)]
    #[surreal(default)]
    pub answers: Option<serde_json::Value>,
    #[serde(default)]
    #[surreal(default)]
    pub response_sent: Option<serde_json::Value>,
    #[serde(default)]
    #[surreal(default)]
    pub status: String,
    #[serde(default)]
    #[surreal(default)]
    pub assignee: Option<RecordId>,
    #[serde(default)]
    #[surreal(default)]
    pub connection_string: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub store: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub requested_at: Option<Datetime>,
    #[serde(default)]
    #[surreal(default)]
    pub expires_at: Option<Datetime>,
    #[serde(default)]
    #[surreal(default)]
    pub decided_at: Option<Datetime>,
    #[serde(default)]
    #[surreal(default)]
    pub sent_to_codex_at: Option<Datetime>,
    #[serde(default)]
    #[surreal(default)]
    pub decided_by: Option<RecordId>,
    #[serde(default)]
    #[surreal(default)]
    pub deny_note: Option<String>,
}

/// What the broker knows when codex asks.
#[derive(Debug, Clone, Default)]
pub struct NewAgentApproval {
    pub thread: Option<RecordId>,
    pub kind: String,
    pub method: String,
    pub codex_request_id: String,
    pub summary: String,
    pub server: Option<String>,
    pub tool: Option<String>,
    pub arguments: Option<serde_json::Value>,
    pub params: Option<serde_json::Value>,
    pub questions: Option<serde_json::Value>,
    pub assignee: Option<RecordId>,
    pub connection_string: Option<String>,
    pub store: Option<String>,
    pub ttl_secs: u64,
}

/// Result of a decision attempt.
#[derive(Debug, Clone, PartialEq)]
pub enum AgentDecideOutcome {
    Recorded,
    Missing,
    /// Someone (or the clock) got there first; carries the status that held.
    AlreadyResolved(String),
}

impl AgentApproval {
    pub fn is_pending(&self) -> bool {
        self.status == "pending"
    }

    /// Seconds left before this request expires; 0 once it has lapsed or has no deadline.
    pub fn secs_remaining(&self) -> i64 {
        let Some(expires) = self.expires_at.as_ref() else { return 0 };
        (expires.timestamp() - Datetime::now().timestamp()).max(0)
    }

    pub async fn create(new: &NewAgentApproval) -> anyhow::Result<RecordId> {
        let thread = new.thread.clone().ok_or_else(|| anyhow::anyhow!("approval needs a thread"))?;
        let mut res = db()
            .query(
                "CREATE agent_approval CONTENT { thread: $thread, kind: $kind, method: $method, \
                 codex_request_id: $rid, summary: $summary, server: $server, tool: $tool, \
                 arguments: $arguments, params: $params, questions: $questions, assignee: $assignee, \
                 connection_string: $cs, store: $store, status: 'pending', \
                 expires_at: time::now() + type::duration($ttl) } RETURN VALUE id",
            )
            .bind(("thread", thread))
            .bind(("kind", new.kind.clone()))
            .bind(("method", new.method.clone()))
            .bind(("rid", new.codex_request_id.clone()))
            .bind(("summary", new.summary.chars().take(400).collect::<String>()))
            .bind(("server", new.server.clone()))
            .bind(("tool", new.tool.clone()))
            .bind(("arguments", new.arguments.clone()))
            .bind(("params", new.params.clone()))
            .bind(("questions", new.questions.clone()))
            .bind(("assignee", new.assignee.clone()))
            .bind(("cs", new.connection_string.clone()))
            .bind(("store", new.store.clone()))
            .bind(("ttl", format!("{}s", new.ttl_secs.max(1))))
            .await?;
        let ids: Vec<RecordId> = res.take(0).unwrap_or_default();
        ids.into_iter().next().ok_or_else(|| anyhow::anyhow!("agent_approval was not created"))
    }

    pub async fn fetch(id: &RecordId) -> anyhow::Result<Option<Self>> {
        let mut res = db().query("SELECT * FROM $id").bind(("id", id.clone())).await?;
        let rows: Vec<Self> = res.take(0).unwrap_or_default();
        Ok(rows.into_iter().next())
    }

    /// Records a technician's decision if the row is still pending and unexpired.
    pub async fn decide(
        id: &RecordId,
        status: &str,
        decided_by: Option<RecordId>,
        deny_note: Option<String>,
        answers: Option<serde_json::Value>,
    ) -> anyhow::Result<AgentDecideOutcome> {
        let won: Option<Self> = db()
            .query(
                "UPDATE $id SET status = $status, decided_by = $by, deny_note = $note, \
                 answers = $answers ?? answers, decided_at = time::now() \
                 WHERE status = 'pending' AND (expires_at = NONE OR expires_at > time::now()) \
                 RETURN AFTER",
            )
            .bind(("id", id.clone()))
            .bind(("status", status.to_string()))
            .bind(("by", decided_by))
            .bind(("note", deny_note))
            .bind(("answers", answers))
            .await?
            .check()?
            .take(0)?;
        if won.is_some() {
            return Ok(AgentDecideOutcome::Recorded);
        }
        let Some(row) = Self::fetch(id).await? else {
            return Ok(AgentDecideOutcome::Missing);
        };
        // Still `pending` here means the expiry clause refused it.
        let held = if row.status == "pending" { "expired".to_string() } else { row.status };
        Ok(AgentDecideOutcome::AlreadyResolved(held))
    }

    /// The broker's own resolution (auto policy, expiry, or a reply it sent).
    pub async fn resolve_by_broker(
        id: &RecordId,
        status: &str,
        response: Option<serde_json::Value>,
    ) -> anyhow::Result<()> {
        db().query(
            "UPDATE $id SET status = IF status = 'pending' THEN $status ELSE status END, \
             response_sent = $response ?? response_sent, sent_to_codex_at = time::now(), \
             decided_at = decided_at ?? time::now()",
        )
        .bind(("id", id.clone()))
        .bind(("status", status.to_string()))
        .bind(("response", response))
        .await?;
        Ok(())
    }

    /// Flips pending rows past their deadline to `expired`.
    pub async fn expire_stale() -> anyhow::Result<()> {
        db().query(
            "UPDATE agent_approval SET status = 'expired', decided_at = time::now() \
             WHERE status = 'pending' AND expires_at != NONE AND expires_at < time::now()",
        )
        .await?;
        Ok(())
    }

    /// Fails every pending decision of a thread whose broker died before relaying it.
    pub async fn fail_pending_for_thread(thread: &RecordId, note: &str) -> anyhow::Result<usize> {
        let mut res = db()
            .query(
                "UPDATE agent_approval SET status = 'failed', deny_note = $note, decided_at = time::now() \
                 WHERE thread = $thread AND status = 'pending' RETURN VALUE id",
            )
            .bind(("thread", thread.clone()))
            .bind(("note", note.to_string()))
            .await?;
        let ids: Vec<RecordId> = res.take(0).unwrap_or_default();
        Ok(ids.len())
    }

    /// Every open decision, oldest first.
    pub async fn list_pending() -> anyhow::Result<Vec<Self>> {
        let mut res = db()
            .query("SELECT * FROM agent_approval WHERE status = 'pending' ORDER BY requested_at ASC")
            .await?;
        Ok(res.take(0).unwrap_or_default())
    }

    /// Open decisions of one thread, oldest first.
    pub async fn list_pending_for_thread(thread: &RecordId) -> anyhow::Result<Vec<Self>> {
        let mut res = db()
            .query(
                "SELECT * FROM agent_approval WHERE thread = $thread AND status = 'pending' \
                 ORDER BY requested_at ASC",
            )
            .bind(("thread", thread.clone()))
            .await?;
        Ok(res.take(0).unwrap_or_default())
    }

    /// Open decisions a technician may answer: their own, or their store's.
    pub async fn list_pending_for(user: &RecordId, store: Option<&str>) -> anyhow::Result<Vec<Self>> {
        let mut res = db()
            .query(
                "SELECT * FROM agent_approval WHERE status = 'pending' \
                 AND (assignee = $user OR ($store != NONE AND store = $store)) ORDER BY requested_at ASC",
            )
            .bind(("user", user.clone()))
            .bind(("store", store.map(str::to_string)))
            .await?;
        Ok(res.take(0).unwrap_or_default())
    }
}
