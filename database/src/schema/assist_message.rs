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

/// One agent conversation, as a thread list needs it.
#[derive(Debug, Clone, PartialEq)]
pub struct AssistThread {
    pub thread: String,
    pub tech: Option<String>,
    pub service_number: Option<String>,
    pub connection_string: Option<String>,
    /// Newest message in the thread.
    pub last_at: String,
    /// Newest message came from the tech, so the agent still owes a reply.
    pub awaiting_reply: bool,
    pub messages: usize,
}

impl AssistThread {
    /// Service number when known, else the machine, else the raw thread id.
    pub fn label(&self) -> String {
        match (&self.service_number, &self.connection_string) {
            (Some(sn), _) => format!("#{sn}"),
            (None, Some(cs)) => cs.clone(),
            (None, None) => self.thread.chars().take(8).collect(),
        }
    }
}

/// Folds newest-first rows into one entry per thread, then keeps only `tech`'s
/// threads when a scope is given. Identity fields are carried up from older
/// rows because agent replies leave them null.
fn group_threads(rows: &[serde_json::Value], tech: Option<&str>) -> Vec<AssistThread> {
    let mut out: Vec<AssistThread> = Vec::new();
    for row in rows {
        let Some(thread) = row.get("thread").and_then(|v| v.as_str()) else { continue };
        let field = |k: &str| {
            row.get(k).and_then(|v| v.as_str()).map(str::to_string).filter(|s| !s.is_empty())
        };
        match out.iter_mut().find(|t| t.thread == thread) {
            // Rows arrive newest first, so the first one seen sets the head.
            Some(existing) => {
                existing.messages += 1;
                existing.tech = existing.tech.take().or_else(|| field("tech"));
                existing.service_number =
                    existing.service_number.take().or_else(|| field("service_number"));
                existing.connection_string =
                    existing.connection_string.take().or_else(|| field("connection_string"));
            },
            None => out.push(AssistThread {
                thread: thread.to_string(),
                tech: field("tech"),
                service_number: field("service_number"),
                connection_string: field("connection_string"),
                last_at: field("created_at").unwrap_or_default(),
                awaiting_reply: field("direction").as_deref() == Some("in"),
                messages: 1,
            }),
        }
    }
    if let Some(me) = tech {
        out.retain(|t| t.tech.as_deref() == Some(me));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Shapes taken from a live `/sql` read: agent replies carry a null `tech`,
    /// which is what makes per-row scoping wrong.
    fn live_rows() -> Vec<serde_json::Value> {
        vec![
            json!({"thread": "27517d89", "tech": "logan.lees@pclaptops.com", "service_number": null,
                   "connection_string": "DESKTOP-EI5PV29:69cd8115b", "direction": "in",
                   "created_at": "2026-09-08T20:17:56Z"}),
            json!({"thread": "repair-diag", "tech": null, "service_number": null,
                   "connection_string": null, "direction": "out",
                   "created_at": "2026-08-19T23:41:18Z"}),
            json!({"thread": "1f0c3af0", "tech": null, "service_number": null,
                   "connection_string": null, "direction": "out",
                   "created_at": "2026-08-19T19:11:34Z"}),
            json!({"thread": "1f0c3af0", "tech": "logan.lees@pclaptops.com", "service_number": null,
                   "connection_string": "DESKTOP-EI5PV29:69cd8115b", "direction": "in",
                   "created_at": "2026-08-19T19:11:14Z"}),
            json!({"thread": "1f0c3af0", "tech": null, "service_number": null,
                   "connection_string": null, "direction": "out",
                   "created_at": "2026-08-19T19:08:06Z"}),
        ]
    }

    #[test]
    fn root_sees_every_thread_with_counts_and_identity() {
        let threads = group_threads(&live_rows(), None);
        assert_eq!(threads.len(), 3, "one entry per thread");

        let convo = threads.iter().find(|t| t.thread == "1f0c3af0").expect("thread present");
        assert_eq!(convo.messages, 3, "counts replies, not just the tech's own");
        // Newest row is an agent reply, so nothing is owed.
        assert!(!convo.awaiting_reply);
        // Newest row has a null tech; identity comes up from the older `in` row.
        assert_eq!(convo.tech.as_deref(), Some("logan.lees@pclaptops.com"));
        assert_eq!(convo.connection_string.as_deref(), Some("DESKTOP-EI5PV29:69cd8115b"));

        let waiting = threads.iter().find(|t| t.thread == "27517d89").expect("thread present");
        assert!(waiting.awaiting_reply, "newest row is from the tech");
    }

    #[test]
    fn a_tech_sees_only_their_own_threads() {
        let threads = group_threads(&live_rows(), Some("logan.lees@pclaptops.com"));
        let ids: Vec<&str> = threads.iter().map(|t| t.thread.as_str()).collect();
        assert!(ids.contains(&"1f0c3af0") && ids.contains(&"27517d89"));
        // Owned by nobody, so it belongs to no technician's list.
        assert!(!ids.contains(&"repair-diag"));
    }

    #[test]
    fn scoping_does_not_break_reply_state() {
        // The bug this guards: per-row scoping hid every `out` row, so a thread
        // the agent had already answered still read as awaiting a reply.
        let threads = group_threads(&live_rows(), Some("logan.lees@pclaptops.com"));
        let convo = threads.iter().find(|t| t.thread == "1f0c3af0").expect("thread present");
        assert!(!convo.awaiting_reply);
        assert_eq!(convo.messages, 3);
    }

    #[test]
    fn label_prefers_service_number_then_machine() {
        let mut t = AssistThread {
            thread: "abcdef123456".into(),
            tech: None,
            service_number: None,
            connection_string: None,
            last_at: String::new(),
            awaiting_reply: false,
            messages: 0,
        };
        assert_eq!(t.label(), "abcdef12");
        t.connection_string = Some("DESKTOP-X:1".into());
        assert_eq!(t.label(), "DESKTOP-X:1");
        t.service_number = Some("2151936".into());
        assert_eq!(t.label(), "#2151936");
    }
}

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

    /// Records an agent turn that produced no answer, flagged so the client can
    /// show it as a dead end rather than as a reply.
    pub async fn reply_empty(room: &str) -> anyhow::Result<()> {
        db().query(
            "CREATE assist_message CONTENT { thread: $room, room: $room, direction: 'out',              text: $text, status: 'delivered', error: 'no_visible_reply' }",
        )
        .bind(("room", room.to_string()))
        .bind((
            "text",
            "The agent finished without an answer. Rephrase and send it again."
                .to_string(),
        ))
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

    /// One conversation, summarised for a thread list.
    pub async fn thread_index(tech: Option<&str>, scan: usize) -> anyhow::Result<Vec<AssistThread>> {
        // Grouped in Rust rather than SurrealQL: 3.x forbids nested aggregates,
        // and the newest row per thread is needed for both `last_at` and
        // `awaiting_reply`, which one GROUP BY cannot give at once.
        // Scoped per thread, not per row: only `in` rows carry `tech`, so a
        // `WHERE tech = $tech` would hide every agent reply — undercounting the
        // thread and leaving `awaiting_reply` stuck true forever. Rows are read
        // whole and the grouped threads are filtered by owner below. `text` is
        // never selected, so no message body is read for another tech's thread.
        // `created_at` is projected so ORDER BY may name it.
        let mut res = db()
            .query(
                "SELECT thread, tech, service_number, connection_string, direction, created_at \
                 FROM assist_message ORDER BY created_at DESC LIMIT $scan",
            )
            .bind(("scan", scan))
            .await?;
        let rows: Vec<serde_json::Value> = res.take(0).unwrap_or_default();
        Ok(group_threads(&rows, tech))
    }

    /// Threads that already carry agent messages, so a restarted client keeps
    /// routing them to the agent instead of to the chat endpoint.
    pub async fn agent_thread_ids(limit: usize) -> anyhow::Result<Vec<String>> {
        let mut res = db()
            .query("SELECT VALUE thread FROM assist_message GROUP BY thread LIMIT $limit")
            .bind(("limit", limit))
            .await?;
        Ok(res.take(0).unwrap_or_default())
    }

    /// zeroclaw session key recorded for a room, if one is known.
    pub async fn session_key_for(room: &str) -> anyhow::Result<Option<String>> {
        let mut res = db()
            .query(
                "SELECT VALUE session_key FROM assist_message \
                 WHERE room = $room AND session_key != NONE LIMIT 1",
            )
            .bind(("room", room.to_string()))
            .await?;
        Ok(res.take::<Vec<String>>(0).unwrap_or_default().into_iter().next())
    }

    /// Stamps the session key on every message of a room.
    pub async fn set_session_key(room: &str, key: &str) -> anyhow::Result<()> {
        db().query("UPDATE assist_message SET session_key = $key WHERE room = $room")
            .bind(("room", room.to_string()))
            .bind(("key", key.to_string()))
            .await?;
        Ok(())
    }

    /// Points the diagnostic sessions this conversation produced at its agent
    /// transcript. Scoped to the machine and to sessions started while the
    /// conversation was live, so an unrelated older session is not claimed.
    /// Two round trips on purpose: a LET occupies a result slot, and mis-indexing
    /// it turns the UPDATE into a silent no-op.
    pub async fn link_diagnostic_sessions(room: &str, key: &str) -> anyhow::Result<usize> {
        let mut res = db()
            .query(
                "SELECT VALUE connection_string FROM assist_message                  WHERE room = $room AND connection_string != NONE LIMIT 1",
            )
            .bind(("room", room.to_string()))
            .await?;
        let Some(cs) = res.take::<Vec<String>>(0).unwrap_or_default().into_iter().next() else {
            return Ok(0);
        };
        let mut res = db()
            .query(
                "UPDATE diagnostic_session SET zeroclaw_session = $key                  WHERE connection_string = $cs AND zeroclaw_session = NONE                  AND started_at > time::now() - 3h RETURN VALUE id",
            )
            .bind(("cs", cs))
            .bind(("key", key.to_string()))
            .await?;
        Ok(res.take::<Vec<RecordId>>(0).unwrap_or_default().len())
    }

    /// Readable label for a conversation: the service number and machine it is about.
    pub async fn room_label(room: &str) -> anyhow::Result<Option<String>> {
        let mut res = db()
            .query(
                "SELECT service_number, connection_string FROM assist_message \
                 WHERE room = $room AND direction = 'in' LIMIT 1",
            )
            .bind(("room", room.to_string()))
            .await?;
        let rows: Vec<serde_json::Value> = res.take(0).unwrap_or_default();
        let Some(row) = rows.first() else { return Ok(None) };
        let sn = row.get("service_number").and_then(|v| v.as_str()).unwrap_or_default();
        let host = row
            .get("connection_string")
            .and_then(|v| v.as_str())
            .and_then(|cs| cs.split(':').next())
            .unwrap_or_default();
        let label = match (sn.is_empty(), host.is_empty()) {
            (true, true) => return Ok(None),
            (false, true) => format!("#{sn}"),
            (true, false) => host.to_string(),
            (false, false) => format!("#{sn} {host}"),
        };
        Ok(Some(label))
    }

    pub fn is_from_tech(&self) -> bool {
        self.direction == "in"
    }
}
