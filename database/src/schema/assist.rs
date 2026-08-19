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
