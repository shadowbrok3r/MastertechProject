//! `build_job` table — SurrealDB-backed work queue for remote
//! `plugin_builder` workers.
//!
//! Lifecycle handled by [`BuildJob::create`] / [`BuildJob::claim`] /
//! [`BuildJob::finish_success`] / [`BuildJob::finish_failure`]. The
//! atomic claim is the only operation that needs special care — see
//! the SurrealQL inside [`BuildJob::claim`] for the guarded UPDATE.

use serde::{Deserialize, Serialize};

use crate::DATABASE;

use super::{random_record_id, Datetime, RecordId, SurrealValue, BUILD_JOB_TABLE};

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, SurrealValue)]
pub struct BuildJob {
    pub id: RecordId,
    pub plugin_id: String,
    pub cargo_toml: String,
    pub lib_rs: String,
    pub target: String,
    pub profile: String,
    /// Lifecycle state. The schema asserts the allowed values; the
    /// Rust side keeps this as a plain string so a new state added on
    /// the wire doesn't require a code change.
    pub status: String,
    pub assigned_worker_id: Option<RecordId>,
    pub claimed_worker_id: Option<RecordId>,
    pub claimed_at: Option<Datetime>,
    /// Inline `.wasm` payload, populated on `status = 'done'`.
    /// SurrealDB-side this is the `bytes` type; serde_bytes lets us
    /// roundtrip without paying the base64 cost.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wasm_bytes: Option<surrealdb_types::Bytes>,
    #[serde(default)]
    pub stdout: String,
    #[serde(default)]
    pub stderr: String,
    #[serde(default)]
    pub duration_ms: u64,
    pub created_at: Datetime,
    pub updated_at: Datetime,
}

impl Default for BuildJob {
    fn default() -> Self {
        let now: Datetime = chrono::Utc::now().into();
        Self {
            id: random_record_id(BUILD_JOB_TABLE),
            plugin_id: String::new(),
            cargo_toml: String::new(),
            lib_rs: String::new(),
            target: "wasm32-wasip1".into(),
            profile: "release".into(),
            status: "pending".into(),
            assigned_worker_id: None,
            claimed_worker_id: None,
            claimed_at: None,
            wasm_bytes: None,
            stdout: String::new(),
            stderr: String::new(),
            duration_ms: 0,
            created_at: now.clone(),
            updated_at: now,
        }
    }
}

impl BuildJob {
    /// CREATE a fresh pending job. Caller supplies source; we generate
    /// the record id and timestamps. Returns the row as the DB sees it
    /// (so the caller has the authoritative `id` for status polling).
    pub async fn create(
        plugin_id: &str,
        cargo_toml: &str,
        lib_rs: &str,
        target: &str,
        profile: &str,
        assigned_worker_id: Option<RecordId>,
    ) -> anyhow::Result<Self> {
        let row = Self {
            plugin_id: plugin_id.to_string(),
            cargo_toml: cargo_toml.to_string(),
            lib_rs: lib_rs.to_string(),
            target: target.to_string(),
            profile: profile.to_string(),
            assigned_worker_id,
            ..Self::default()
        };
        let created: Option<Self> = DATABASE.create(row.id.clone()).content(row).await?;
        created.ok_or_else(|| anyhow::anyhow!("CREATE build_job returned None"))
    }

    pub async fn get(id: &RecordId) -> anyhow::Result<Option<Self>> {
        let row: Option<Self> = DATABASE.select(id.clone()).await?;
        Ok(row)
    }

    /// Atomically transition `pending → claimed` for this specific
    /// job, but only if it's still pending AND the claim is valid for
    /// `worker_id`. Returns `Ok(Some(job))` if we got it, `Ok(None)`
    /// if some other worker beat us (or the job is in a non-pending
    /// state). This is the only operation that must be race-safe:
    /// SurrealDB's per-record write lock guarantees a single winner.
    pub async fn claim(id: &RecordId, worker_id: &RecordId) -> anyhow::Result<Option<Self>> {
        let mut response = DATABASE
            .query(
                "UPDATE $id SET status = 'claimed', \
                                claimed_worker_id = $worker, \
                                claimed_at = time::now() \
                 WHERE status = 'pending' \
                   AND (assigned_worker_id == NONE OR assigned_worker_id == $worker) \
                 RETURN AFTER",
            )
            .bind(("id", id.clone()))
            .bind(("worker", worker_id.clone()))
            .await?;
        let claimed: Vec<Self> = response.take(0)?;
        Ok(claimed.into_iter().next())
    }

    pub async fn finish_success(
        id: &RecordId,
        wasm_bytes: Vec<u8>,
        stdout: String,
        stderr: String,
        duration_ms: u64,
    ) -> anyhow::Result<()> {
        let _: Option<Self> = DATABASE
            .query(
                "UPDATE $id SET status = 'done', \
                                wasm_bytes = $bytes, \
                                stdout = $stdout, \
                                stderr = $stderr, \
                                duration_ms = $dur \
                 RETURN AFTER",
            )
            .bind(("id", id.clone()))
            .bind(("bytes", surrealdb_types::Bytes::from(wasm_bytes)))
            .bind(("stdout", stdout))
            .bind(("stderr", stderr))
            .bind(("dur", duration_ms))
            .await?
            .take(0)?;
        Ok(())
    }

    pub async fn finish_failure(
        id: &RecordId,
        stdout: String,
        stderr: String,
        duration_ms: u64,
    ) -> anyhow::Result<()> {
        let _: Option<Self> = DATABASE
            .query(
                "UPDATE $id SET status = 'failed', \
                                stdout = $stdout, \
                                stderr = $stderr, \
                                duration_ms = $dur \
                 RETURN AFTER",
            )
            .bind(("id", id.clone()))
            .bind(("stdout", stdout))
            .bind(("stderr", stderr))
            .bind(("dur", duration_ms))
            .await?
            .take(0)?;
        Ok(())
    }

    /// Snapshot of currently-pending jobs that this worker is eligible
    /// to claim (either unassigned or pinned to it). Used by workers
    /// on startup to drain any jobs that arrived before its live
    /// subscription was active.
    pub async fn pending_for_worker(worker_id: &RecordId) -> anyhow::Result<Vec<Self>> {
        let mut response = DATABASE
            .query(
                "SELECT * FROM build_job \
                 WHERE status = 'pending' \
                   AND (assigned_worker_id == NONE OR assigned_worker_id == $worker) \
                 ORDER BY created_at ASC",
            )
            .bind(("worker", worker_id.clone()))
            .await?;
        let jobs: Vec<Self> = response.take(0)?;
        Ok(jobs)
    }
}
