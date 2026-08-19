//! Two-way transcript between a technician in MasterTech and a zeroclaw agent.
//!
//! Bench clients only ever talk to MasterTech, so the conversation crosses to the
//! agent host through this table: the client writes `in` rows, the admin-agent
//! forwards them to the zeroclaw channel and writes the replies back as `out`.
//!
//! `room` is the MasterTech thread id rather than the service number, because a
//! reply from the channel carries only the room and has to route back to exactly
//! one chat thread. The service number rides in its own field.

use serde::{Deserialize, Serialize};

use super::{Datetime, RecordId, SurrealValue};
use crate::db;

pub const ASSIST_MESSAGE_TABLE: &str = "assist_message";

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, SurrealValue)]
pub struct AssistMessage {
    pub id: RecordId,
    #[serde(default)]
    #[surreal(default)]
    pub thread: String,
    #[serde(default)]
    #[surreal(default)]
    pub room: String,
    /// "in" from the tech, "out" from the agent.
    #[serde(default)]
    #[surreal(default)]
    pub direction: String,
    #[serde(default)]
    #[surreal(default)]
    pub text: String,
    #[serde(default)]
    #[surreal(default)]
    pub status: String,
    #[serde(default)]
    #[surreal(default)]
    pub tech: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub service_number: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub connection_string: Option<String>,
    /// zeroclaw's own session key, once resolved.
    #[serde(default)]
    #[surreal(default)]
    pub session_key: Option<String>,
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

/// What the client knows about the machine a conversation is about.
#[derive(Debug, Clone, Default)]
pub struct AssistContext {
    pub tech: Option<String>,
    pub service_number: Option<String>,
    pub connection_string: Option<String>,
}

impl AssistMessage {
    /// Queues a technician's message for the agent. The room is the thread, so a
    /// reply can only land in the conversation it belongs to.
    pub async fn ask(thread: &str, text: &str, ctx: &AssistContext) -> anyhow::Result<RecordId> {
        let text = text.trim();
        if text.is_empty() {
            anyhow::bail!("empty message");
        }
        let mut res = db()
            .query(
                "CREATE assist_message CONTENT { thread: $thread, room: $thread, direction: 'in', \
                 text: $text, status: 'pending', tech: $tech, service_number: $sn, \
                 connection_string: $cs } RETURN VALUE id",
            )
            .bind(("thread", thread.to_string()))
            .bind(("text", text.to_string()))
            .bind(("tech", ctx.tech.clone()))
            .bind(("sn", ctx.service_number.clone()))
            .bind(("cs", ctx.connection_string.clone()))
            .await?;
        let ids: Vec<RecordId> = res.take(0).unwrap_or_default();
        ids.into_iter().next().ok_or_else(|| anyhow::anyhow!("message was not created"))
    }

    /// Guarded claim so one dispatcher owns a row even with several running.
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

    /// Records the agent's reply so the client's live query renders it.
    pub async fn reply(room: &str, text: &str, session_key: Option<&str>) -> anyhow::Result<()> {
        db().query(
            "CREATE assist_message CONTENT { thread: $room, room: $room, direction: 'out', \
             text: $text, status: 'delivered', session_key: $key }",
        )
        .bind(("room", room.to_string()))
        .bind(("text", text.to_string()))
        .bind(("key", session_key.map(str::to_string)))
        .await?;
        Ok(())
    }

    /// Inbound rows still waiting, oldest first; drains what LIVE missed.
    pub async fn pending_inbound(limit: usize) -> anyhow::Result<Vec<Self>> {
        let mut res = db()
            .query(
                "SELECT * FROM assist_message WHERE direction = 'in' AND status = 'pending' \
                 ORDER BY created_at ASC LIMIT $limit",
            )
            .bind(("limit", limit))
            .await?;
        Ok(res.take(0).unwrap_or_default())
    }

    /// One conversation, oldest first.
    pub async fn thread_history(thread: &str, limit: usize) -> anyhow::Result<Vec<Self>> {
        let mut res = db()
            .query(
                "SELECT * FROM assist_message WHERE thread = $thread \
                 ORDER BY created_at ASC LIMIT $limit",
            )
            .bind(("thread", thread.to_string()))
            .bind(("limit", limit))
            .await?;
        Ok(res.take(0).unwrap_or_default())
    }

    pub fn is_from_tech(&self) -> bool {
        self.direction == "in"
    }
}
