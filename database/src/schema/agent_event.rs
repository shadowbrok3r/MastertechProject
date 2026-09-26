//! Transcript rows of an agent_thread.
//!
//! One row per codex item, addressed by `<thread key>:<item id>` so streaming
//! text upserts onto the same row and a LIVE SELECT on the desktop sees it grow.
//! Markers (turn boundaries, errors, approvals) get generated ids of their own.

use serde::{Deserialize, Serialize};

use super::{Datetime, RecordId, RecordIdExt, SurrealValue};
use crate::db;

pub const AGENT_EVENT_TABLE: &str = "agent_event";

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, SurrealValue)]
pub struct AgentEvent {
    pub id: RecordId,
    pub thread: RecordId,
    #[serde(default)]
    #[surreal(default)]
    pub seq: i64,
    #[serde(default)]
    #[surreal(default)]
    pub turn_id: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub item_id: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub kind: String,
    #[serde(default)]
    #[surreal(default)]
    pub text: String,
    #[serde(default)]
    #[surreal(default)]
    pub done: bool,
    #[serde(default)]
    #[surreal(default)]
    pub item: Option<serde_json::Value>,
    #[serde(default)]
    #[surreal(default)]
    pub created_at: Option<Datetime>,
    #[serde(default)]
    #[surreal(default)]
    pub updated_at: Option<Datetime>,
}

/// Record key shared by every write about one codex item.
pub fn event_key(thread: &RecordId, item_id: &str) -> String {
    format!("{}:{}", thread.key_string(), item_id)
}

impl AgentEvent {
    /// Writes the current text of a streaming item; `seq` sticks from the first write.
    pub async fn upsert_text(
        thread: &RecordId,
        item_id: &str,
        seq: i64,
        turn_id: Option<&str>,
        kind: &str,
        text: &str,
        done: bool,
    ) -> anyhow::Result<()> {
        db().query(
            "UPSERT type::record('agent_event', $key) SET thread = $thread, seq = seq ?? $seq, \
             item_id = $item_id, turn_id = $turn_id ?? turn_id, kind = $kind, text = $text, \
             done = $done, updated_at = time::now()",
        )
        .bind(("key", event_key(thread, item_id)))
        .bind(("thread", thread.clone()))
        .bind(("seq", seq))
        .bind(("item_id", item_id.to_string()))
        .bind(("turn_id", turn_id.map(str::to_string)))
        .bind(("kind", kind.to_string()))
        .bind(("text", text.to_string()))
        .bind(("done", done))
        .await?;
        Ok(())
    }

    /// Marks an item complete with its authoritative payload.
    pub async fn complete(
        thread: &RecordId,
        item_id: &str,
        seq: i64,
        turn_id: Option<&str>,
        kind: &str,
        text: &str,
        item: serde_json::Value,
    ) -> anyhow::Result<()> {
        db().query(
            "UPSERT type::record('agent_event', $key) SET thread = $thread, seq = seq ?? $seq, \
             item_id = $item_id, turn_id = $turn_id ?? turn_id, kind = $kind, text = $text, \
             done = true, item = $item, updated_at = time::now()",
        )
        .bind(("key", event_key(thread, item_id)))
        .bind(("thread", thread.clone()))
        .bind(("seq", seq))
        .bind(("item_id", item_id.to_string()))
        .bind(("turn_id", turn_id.map(str::to_string)))
        .bind(("kind", kind.to_string()))
        .bind(("text", text.to_string()))
        .bind(("item", item))
        .await?;
        Ok(())
    }

    /// A standalone row with no codex item behind it (turn boundaries, errors, approvals).
    pub async fn marker(
        thread: &RecordId,
        seq: i64,
        turn_id: Option<&str>,
        kind: &str,
        text: &str,
        item: Option<serde_json::Value>,
    ) -> anyhow::Result<()> {
        db().query(
            "CREATE agent_event CONTENT { thread: $thread, seq: $seq, turn_id: $turn_id, kind: $kind, \
             text: $text, done: true, item: $item, updated_at: time::now() }",
        )
        .bind(("thread", thread.clone()))
        .bind(("seq", seq))
        .bind(("turn_id", turn_id.map(str::to_string)))
        .bind(("kind", kind.to_string()))
        .bind(("text", text.to_string()))
        .bind(("item", item))
        .await?;
        Ok(())
    }

    /// Rows past `after_seq`, rows still streaming, and rows written in the last 15 seconds.
    pub async fn since(thread: &RecordId, after_seq: i64, limit: usize) -> anyhow::Result<Vec<Self>> {
        let mut res = db()
            .query(
                "SELECT * FROM agent_event WHERE thread = $thread \
                 AND (seq > $after OR done = false OR updated_at > time::now() - 15s) \
                 ORDER BY seq ASC LIMIT $limit",
            )
            .bind(("thread", thread.clone()))
            .bind(("after", after_seq))
            .bind(("limit", limit))
            .await?;
        Ok(res.take(0).unwrap_or_default())
    }

    /// The newest row of `kind` on a thread.
    pub async fn latest_of_kind(thread: &RecordId, kind: &str) -> anyhow::Result<Option<Self>> {
        let mut res = db()
            .query(
                "SELECT * FROM agent_event WHERE thread = $thread AND kind = $kind \
                 ORDER BY seq DESC LIMIT 1",
            )
            .bind(("thread", thread.clone()))
            .bind(("kind", kind.to_string()))
            .await?;
        let rows: Vec<Self> = res.take(0)?;
        Ok(rows.into_iter().next())
    }

    /// Codex item ids already recorded for a thread.
    pub async fn item_ids(thread: &RecordId) -> anyhow::Result<Vec<String>> {
        let mut res = db()
            .query("SELECT VALUE item_id FROM agent_event WHERE thread = $thread AND item_id != NONE")
            .bind(("thread", thread.clone()))
            .await?;
        Ok(res.take(0).unwrap_or_default())
    }

    /// The newest `limit` rows of one thread, oldest first.
    pub async fn recent(thread: &RecordId, limit: usize) -> anyhow::Result<Vec<Self>> {
        let mut res = db()
            .query("SELECT * FROM agent_event WHERE thread = $thread ORDER BY seq DESC LIMIT $limit")
            .bind(("thread", thread.clone()))
            .bind(("limit", limit))
            .await?;
        let mut rows: Vec<Self> = res.take(0).unwrap_or_default();
        rows.reverse();
        Ok(rows)
    }

    /// Rows of one thread in order, starting after `after_seq`.
    pub async fn history(thread: &RecordId, after_seq: i64, limit: usize) -> anyhow::Result<Vec<Self>> {
        let mut res = db()
            .query(
                "SELECT * FROM agent_event WHERE thread = $thread AND seq > $after \
                 ORDER BY seq ASC LIMIT $limit",
            )
            .bind(("thread", thread.clone()))
            .bind(("after", after_seq))
            .bind(("limit", limit))
            .await?;
        Ok(res.take(0).unwrap_or_default())
    }
}
