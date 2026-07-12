use crate::db;
use crossbeam::channel::Sender;
use serde::{Deserialize, Serialize};

use super::{Datetime, RecordId, SurrealValue};

/// Per-user read marker for a task's notes. One row per `(task, user)` pair —
/// enforced by the `task_note_read_unique` index in the SurrealDB schema.
///
/// The DDL for this table lives outside the Rust crate, alongside the other
/// table definitions. To apply manually:
///
/// ```surql
/// DEFINE TABLE task_note_read SCHEMALESS PERMISSIONS FULL;
/// DEFINE FIELD task     ON task_note_read TYPE record<task>;
/// DEFINE FIELD user     ON task_note_read TYPE record<user>;
/// DEFINE FIELD read_at  ON task_note_read TYPE datetime DEFAULT time::now();
/// DEFINE INDEX task_note_read_unique ON task_note_read FIELDS task, user UNIQUE;
/// ```
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, SurrealValue)]
pub struct TaskNoteRead {
    pub task: RecordId,
    pub read_at: Datetime,
}

impl TaskNoteRead {
    /// Fetch every `(task, read_at)` row owned by the currently authenticated
    /// user. Sent over `tx` so callers can stay sync.
    pub async fn fetch_all_for_user(
        tx: Sender<Vec<Self>>,
    ) -> anyhow::Result<(), anyhow::Error> {
        let rows: Vec<Self> = db()
            .query("SELECT task, read_at FROM task_note_read WHERE user == $auth.id")
            .await?
            .take(0)?;

        let _ = tx.try_send(rows);
        Ok(())
    }

    /// Upsert the read marker for `(task_id, $auth.id)`. Uses the unique index
    /// so no full-table scan is needed even as the table grows.
    pub async fn mark_read(task_id: RecordId) -> anyhow::Result<(), anyhow::Error> {
        db()
            .query(
                "UPSERT task_note_read \
                 SET task = $task, user = $auth.id, read_at = time::now() \
                 WHERE task = $task AND user = $auth.id",
            )
            .bind(("task", task_id))
            .await?;
        Ok(())
    }
}
