//! Tech-confirmed AI assistance queue.
//!
//! The bench client creates a row when a signed-in tech confirms; the headless
//! agent claims it and dispatches a zeroclaw session. Identity fields default
//! from `$auth` in the schema, so the recorded requester is the signed-in tech
//! rather than whatever the client sent.

use serde::{Deserialize, Serialize};

use super::{RecordId, SurrealValue};
use crate::db;

pub const ASSIST_REQUEST_TABLE: &str = "assist_request";

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, SurrealValue)]
pub struct AssistRequest {
    pub id: RecordId,
    #[serde(default)]
    #[surreal(default)]
    pub status: String,
    #[serde(default)]
    #[surreal(default)]
    pub trigger_source: String,
    /// Tech affirmed this machine is the one on that service number.
    #[serde(default)]
    #[surreal(default)]
    pub machine_confirmed: bool,
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
    pub service_order: Option<RecordId>,
    #[serde(default)]
    #[surreal(default)]
    pub computer: Option<RecordId>,
    #[serde(default)]
    #[surreal(default)]
    pub customer: Option<RecordId>,
    #[serde(default)]
    #[surreal(default)]
    pub requested_by: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub store: Option<String>,
    /// Free text from the tech; untrusted, quoted when composed into a prompt.
    #[serde(default)]
    #[surreal(default)]
    pub tech_note: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub agent: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub dispatch_error: Option<String>,
}

impl AssistRequest {
    pub async fn get(id: &RecordId) -> anyhow::Result<Option<Self>> {
        let mut res = db().query("SELECT * FROM $id").bind(("id", id.clone())).await?;
        let rows: Vec<Self> = res.take(0).unwrap_or_default();
        Ok(rows.into_iter().next())
    }

    /// Files a request from the chat rail; nobody confirmed the machine at the bench.
    pub async fn create_from_chat(
        connection_string: &str,
        requested_by: Option<&str>,
        store: Option<&str>,
        service_number: Option<&str>,
        tech_note: &str,
    ) -> anyhow::Result<RecordId> {
        let hostname = connection_string.split(':').next().filter(|h| !h.is_empty()).map(str::to_string);
        let mut res = db()
            .query(
                "CREATE assist_request CONTENT { connection_string: $cs, hostname: $host, \
                 requested_by: $by, store: $store, service_number: $sn, trigger_source: 'chat', \
                 machine_confirmed: false, status: 'pending', tech_note: $note } RETURN VALUE id",
            )
            .bind(("cs", connection_string.to_string()))
            .bind(("host", hostname))
            .bind(("by", requested_by.map(str::to_string)))
            .bind(("store", store.map(str::to_string)))
            .bind(("sn", service_number.map(str::to_string)))
            .bind(("note", tech_note.chars().take(500).collect::<String>()))
            .await?;
        let ids: Vec<RecordId> = res.take(0).unwrap_or_default();
        ids.into_iter().next().ok_or_else(|| anyhow::anyhow!("assist_request was not created"))
    }

    /// Records the codex session the broker opened for this request.
    pub async fn link_thread(id: &RecordId, thread: &RecordId) -> anyhow::Result<()> {
        db().query("UPDATE $id SET agent_thread = $thread")
            .bind(("id", id.clone()))
            .bind(("thread", thread.clone()))
            .await?;
        Ok(())
    }

    /// Claims a pending row; `false` means another dispatcher took it.
    pub async fn claim(id: &RecordId) -> anyhow::Result<bool> {
        let mut res = db()
            .query(
                "UPDATE $id SET status = 'dispatched', dispatched_at = time::now() \
                 WHERE status = 'pending' RETURN VALUE id",
            )
            .bind(("id", id.clone()))
            .await?;
        let claimed: Vec<RecordId> = res.take(0).unwrap_or_default();
        Ok(!claimed.is_empty())
    }

    pub async fn finish(id: &RecordId, status: &str, error: Option<String>) -> anyhow::Result<()> {
        db().query(
            "UPDATE $id SET status = $status, dispatch_error = $error, finished_at = time::now()",
        )
        .bind(("id", id.clone()))
        .bind(("status", status.to_string()))
        .bind(("error", error))
        .await?;
        Ok(())
    }

    /// Rows left pending while no dispatcher was listening.
    pub async fn pending() -> anyhow::Result<Vec<Self>> {
        let mut res = db()
            .query("SELECT * FROM assist_request WHERE status = 'pending' LIMIT 25")
            .await?;
        Ok(res.take(0).unwrap_or_default())
    }
}
