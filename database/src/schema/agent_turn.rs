//! A technician's instruction to a running agent_thread.
//!
//! Desktops queue rows; the admin-agent broker claims each one and forwards it
//! to codex, so a tech never needs a socket to the agent host.

use serde::{Deserialize, Serialize};

use super::{Datetime, RecordId, SurrealValue};
use crate::db;

pub const AGENT_TURN_TABLE: &str = "agent_turn";

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, SurrealValue)]
pub struct AgentTurn {
    pub id: RecordId,
    pub thread: RecordId,
    /// `start`, `steer`, `interrupt` or `close`.
    #[serde(default)]
    #[surreal(default)]
    pub kind: String,
    #[serde(default)]
    #[surreal(default)]
    pub text: String,
    #[serde(default)]
    #[surreal(default)]
    pub tech: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub status: String,
    #[serde(default)]
    #[surreal(default)]
    pub error: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub created_at: Option<Datetime>,
    #[serde(default)]
    #[surreal(default)]
    pub sent_at: Option<Datetime>,
}

impl AgentTurn {
    /// Queues an instruction; `tech` defaults from `$auth` in the schema.
    pub async fn ask(thread: &RecordId, kind: &str, text: &str) -> anyhow::Result<RecordId> {
        if kind == "start" && text.trim().is_empty() {
            anyhow::bail!("empty message");
        }
        let mut res = db()
            .query(
                "CREATE agent_turn CONTENT { thread: $thread, kind: $kind, text: $text, status: 'pending' } \
                 RETURN VALUE id",
            )
            .bind(("thread", thread.clone()))
            .bind(("kind", kind.to_string()))
            .bind(("text", text.trim().to_string()))
            .await?;
        let ids: Vec<RecordId> = res.take(0).unwrap_or_default();
        ids.into_iter().next().ok_or_else(|| anyhow::anyhow!("agent_turn was not created"))
    }

    /// Guarded claim so one broker owns a row even with several running.
    pub async fn claim(id: &RecordId) -> anyhow::Result<bool> {
        let mut res = db()
            .query(
                "UPDATE $id SET status = 'sent', sent_at = time::now() \
                 WHERE status = 'pending' RETURN VALUE id",
            )
            .bind(("id", id.clone()))
            .await?;
        let claimed: Vec<RecordId> = res.take(0).unwrap_or_default();
        Ok(!claimed.is_empty())
    }

    pub async fn mark_failed(id: &RecordId, error: &str) -> anyhow::Result<()> {
        db().query("UPDATE $id SET status = 'failed', error = $error")
            .bind(("id", id.clone()))
            .bind(("error", error.chars().take(400).collect::<String>()))
            .await?;
        Ok(())
    }

    /// Rows still waiting, oldest first; drains what a LIVE SELECT missed.
    pub async fn pending(limit: usize) -> anyhow::Result<Vec<Self>> {
        let mut res = db()
            .query("SELECT * FROM agent_turn WHERE status = 'pending' ORDER BY created_at ASC LIMIT $limit")
            .bind(("limit", limit))
            .await?;
        Ok(res.take(0).unwrap_or_default())
    }
}
