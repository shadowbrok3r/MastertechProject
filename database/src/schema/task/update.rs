use std::sync::PoisonError;

use serde::{Deserialize, Serialize};
use surrealdb::{Connection, Surreal, types::Value};

use crate::{
    CURRENT_USER_INFO, db,
    schema::{
        Datetime, LiveTaskPayload, Priority, RecordId, Status, SurrealValue, TaskField, TaskHistory,
    },
};

/// Projection that makes a task UPDATE return each row's before and after state.
pub const RETURN_CHANGE: &str = "RETURN $before AS before, $after AS after";

/// Username on history rows that no signed-in user made.
pub const SYSTEM_AUTHOR: &str = "System";

/// One task row as an UPDATE found it and left it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, SurrealValue)]
pub struct TaskChange {
    #[serde(default)]
    #[surreal(default)]
    pub before: Option<LiveTaskPayload>,
    #[serde(default)]
    #[surreal(default)]
    pub after: Option<LiveTaskPayload>,
}

/// The user a task_history row is attributed to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryAuthor {
    pub user: RecordId,
    pub username: String,
}

impl HistoryAuthor {
    /// The signed-in user; blocks on the user lock.
    pub fn signed_in() -> Option<Self> {
        let user = CURRENT_USER_INFO
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()?;
        Some(Self {
            user: user.get_id(),
            username: user.get_username().to_string(),
        })
    }

    /// [`SYSTEM_AUTHOR`], filed under `user`.
    pub fn system(user: RecordId) -> Self {
        Self {
            user,
            username: SYSTEM_AUTHOR.to_string(),
        }
    }
}

impl TaskChange {
    /// The history row for this change; `None` when no diffed field changed.
    pub fn history(&self, author: &HistoryAuthor) -> Option<TaskHistory> {
        let (before, after) = (self.before.as_ref()?, self.after.as_ref()?);
        if !before.has_changes_from(after) {
            return None;
        }
        Some(TaskHistory::new(
            after.id.clone(),
            author.user.clone(),
            author.username.clone(),
            before.diff_to_json(after),
        ))
    }
}

/// Saves a history row per changed task for `author`, else the signed-in user, else [`SYSTEM_AUTHOR`].
pub(crate) async fn record_task_changes<C: Connection>(
    db: &Surreal<C>,
    changes: &[TaskChange],
    author: Option<HistoryAuthor>,
) {
    let author = author.or_else(HistoryAuthor::signed_in);
    for change in changes {
        let Some(after) = &change.after else {
            continue;
        };
        let author = author
            .clone()
            .unwrap_or_else(|| HistoryAuthor::system(after.assignee.clone()));
        let Some(history) = change.history(&author) else {
            continue;
        };
        if let Err(e) = history.save_on(db).await {
            log::error!("Error saving task history: {e:?}");
        }
    }
}

/// Decodes each returned row; a row that fails is logged and left out.
fn decode_changes(rows: Vec<Value>) -> Vec<TaskChange> {
    rows.into_iter()
        .filter_map(|row| {
            TaskChange::from_value(row)
                .inspect_err(|e| {
                    log::warn!("task change row did not decode; no history recorded for it: {e}")
                })
                .ok()
        })
        .collect()
}

/// A task UPDATE that returns [`TaskChange`] rows and records their history.
#[derive(Debug, Clone)]
pub struct TaskUpdate {
    sql: String,
    vars: Vec<(&'static str, Value)>,
}

impl TaskUpdate {
    /// `sql`, which must project [`RETURN_CHANGE`].
    pub(crate) fn new(sql: impl Into<String>) -> Self {
        Self {
            sql: sql.into(),
            vars: Vec::new(),
        }
    }

    /// `UPDATE <target> SET <set>` projecting [`RETURN_CHANGE`].
    fn set(target: &str, set: &str) -> Self {
        Self::new(format!("UPDATE {target} SET {set} {RETURN_CHANGE}"))
    }

    pub(crate) fn bind(mut self, name: &'static str, value: impl SurrealValue) -> Self {
        self.vars.push((name, value.into_value()));
        self
    }

    /// The statement this update runs.
    pub fn sql(&self) -> &str {
        &self.sql
    }

    /// Runs on `db`, records history for `author` or the signed-in user, and returns the rows updated.
    pub async fn run<C: Connection>(
        self,
        db: &Surreal<C>,
        author: Option<HistoryAuthor>,
    ) -> anyhow::Result<usize> {
        let mut query = db.query(self.sql);
        for var in self.vars {
            query = query.bind(var);
        }
        let rows: Vec<Value> = query.await?.check()?.take(0)?;
        let count = rows.len();
        record_task_changes(db, &decode_changes(rows), author).await;
        Ok(count)
    }

    pub fn service_number(id: &RecordId, service_number: String) -> Self {
        Self::set("$id", "service_number = $service_number")
            .bind("id", id.clone())
            .bind("service_number", service_number)
    }

    /// Completes, or reopens as In Repair, stamping or clearing `completed_at`.
    pub fn completed(id: &RecordId, completed: bool) -> Self {
        let status = if completed {
            Status::Complete
        } else {
            Status::InRepair
        };
        Self::set(
            "$id",
            "completed = $completed, status = $status, \
             completed_at = IF $completed THEN time::now() ELSE NONE END",
        )
        .bind("id", id.clone())
        .bind("completed", completed)
        .bind("status", status.as_str().to_string())
    }

    /// Completes, or reopens as Todo, every task in `ids`; a task already complete keeps its stamp.
    pub fn completed_many(ids: Vec<RecordId>, completed: bool) -> Self {
        let status = if completed {
            Status::Complete
        } else {
            Status::Todo
        };
        Self::set(
            "$ids",
            "completed_at = IF !$completed THEN NONE ELSE IF completed THEN completed_at ?? time::now() \
             ELSE time::now() END, completed = $completed, status = $status",
        )
        .bind("ids", ids)
        .bind("completed", completed)
        .bind("status", status.as_str().to_string())
    }

    /// Sets every task in `ids` due now.
    pub fn due_now_many(ids: Vec<RecordId>) -> Self {
        Self::set("$ids", "due_date = time::now()").bind("ids", ids)
    }

    /// Writes the `fields` of `task` in one UPDATE; `None` when `fields` is empty.
    pub fn fields(task: &LiveTaskPayload, fields: &[TaskField]) -> Option<Self> {
        if fields.is_empty() {
            return None;
        }
        let assignments: Vec<&str> = fields
            .iter()
            .map(|field| match field {
                TaskField::Assignee => "assignee = $assignee",
                TaskField::Priority => "priority = $priority",
                TaskField::DueDate => "due_date = $due_date",
                TaskField::Status => "status = $status",
                TaskField::Completed => {
                    "completed = $completed, completed_at = IF $completed THEN time::now() ELSE NONE END"
                }
            })
            .collect();
        let update = Self::set("$id", &assignments.join(", ")).bind("id", task.id.clone());
        Some(fields.iter().fold(update, |update, field| match field {
            TaskField::Assignee => update.bind("assignee", task.assignee.clone()),
            TaskField::Priority => update.bind("priority", task.priority.clone()),
            TaskField::DueDate => update.bind("due_date", task.due_date),
            TaskField::Status => update.bind("status", task.status.as_str().to_string()),
            TaskField::Completed => update.bind("completed", task.completed),
        }))
    }

    pub fn due_date(id: &RecordId, due_date: Datetime) -> Self {
        Self::set("$id", "due_date = $due_date")
            .bind("id", id.clone())
            .bind("due_date", due_date)
    }

    /// Reassigns and resets the status to Todo.
    pub fn assignee(id: &RecordId, assignee: RecordId) -> Self {
        Self::set("$id", "assignee = $assignee, status = 'Todo'")
            .bind("id", id.clone())
            .bind("assignee", assignee)
    }

    pub fn task_name(id: &RecordId, name: String) -> Self {
        Self::set("$id", "task_name = $name")
            .bind("id", id.clone())
            .bind("name", name)
    }

    /// Sets the status; Todo and In Repair reopen, Complete completes.
    pub fn status(id: &RecordId, status: &Status) -> Self {
        let set = match status {
            Status::Todo | Status::InRepair => "status = $status, completed = false",
            Status::Complete => "status = $status, completed = true",
            _ => "status = $status",
        };
        Self::set("$id", set)
            .bind("id", id.clone())
            .bind("status", status.as_str().to_string())
    }

    pub fn priority(id: &RecordId, priority: Priority) -> Self {
        Self::set("$id", "priority = $priority")
            .bind("id", id.clone())
            .bind("priority", priority)
    }

    pub fn description(id: &RecordId, description: String) -> Self {
        Self::set("$id", "task_description = $description")
            .bind("id", id.clone())
            .bind("description", description)
    }

    /// Replaces the whole row with `task`.
    pub fn replace(task: LiveTaskPayload) -> Self {
        Self::new(format!("UPDATE $id CONTENT $data {RETURN_CHANGE}"))
            .bind("id", task.id.clone())
            .bind("data", task)
    }
}

impl LiveTaskPayload {
    pub async fn update_service_number(
        &self,
        service_number: String,
    ) -> anyhow::Result<(), anyhow::Error> {
        TaskUpdate::service_number(&self.id, service_number)
            .run(db().as_ref(), None)
            .await?;
        Ok(())
    }

    pub async fn update_completed(&self, completed: bool) -> anyhow::Result<(), anyhow::Error> {
        TaskUpdate::completed(&self.id, completed)
            .run(db().as_ref(), None)
            .await?;
        Ok(())
    }

    /// Completes, or reopens as Todo, every task in `ids` in one UPDATE.
    pub async fn update_completed_many(
        ids: Vec<RecordId>,
        completed: bool,
    ) -> anyhow::Result<(), anyhow::Error> {
        TaskUpdate::completed_many(ids, completed)
            .run(db().as_ref(), None)
            .await?;
        Ok(())
    }

    /// Sets every task in `ids` due now in one UPDATE.
    pub async fn update_due_now_many(ids: Vec<RecordId>) -> anyhow::Result<(), anyhow::Error> {
        TaskUpdate::due_now_many(ids)
            .run(db().as_ref(), None)
            .await?;
        Ok(())
    }

    /// Writes every field in `fields` in a single UPDATE so a batch of staged
    /// edits lands as one task_history entry and fires one notification.
    /// Values come from `self`; `fields` selects which are written.
    pub async fn update_fields(&self, fields: &[TaskField]) -> anyhow::Result<(), anyhow::Error> {
        if let Some(update) = TaskUpdate::fields(self, fields) {
            update.run(db().as_ref(), None).await?;
        }
        Ok(())
    }

    pub async fn update_due_date(&self) -> anyhow::Result<(), anyhow::Error> {
        TaskUpdate::due_date(&self.id, self.due_date)
            .run(db().as_ref(), None)
            .await?;
        Ok(())
    }

    pub async fn update_assignee(&self, assignee: RecordId) -> anyhow::Result<(), anyhow::Error> {
        log::debug!("assignee: {assignee:?}");
        TaskUpdate::assignee(&self.id, assignee)
            .run(db().as_ref(), None)
            .await?;
        Ok(())
    }

    pub async fn update_task_name(&self, name: String) -> anyhow::Result<(), anyhow::Error> {
        TaskUpdate::task_name(&self.id, name)
            .run(db().as_ref(), None)
            .await?;
        Ok(())
    }

    pub async fn update_status(&self, status: Status) -> anyhow::Result<(), anyhow::Error> {
        TaskUpdate::status(&self.id, &status)
            .run(db().as_ref(), None)
            .await?;
        Ok(())
    }

    pub async fn update_priority(
        &self,
        priority: Option<Priority>,
    ) -> anyhow::Result<(), anyhow::Error> {
        TaskUpdate::priority(&self.id, priority.unwrap_or_default())
            .run(db().as_ref(), None)
            .await?;
        Ok(())
    }

    pub async fn update_task_description(&self) -> anyhow::Result<(), anyhow::Error> {
        TaskUpdate::description(&self.id, self.task_description.clone())
            .run(db().as_ref(), None)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn author() -> HistoryAuthor {
        HistoryAuthor {
            user: RecordId::new("user", "editor"),
            username: "editor".into(),
        }
    }

    #[test]
    fn a_changed_row_yields_one_history_attributed_to_the_author() {
        let before = LiveTaskPayload {
            status: Status::CustomStatus("follow up".into()),
            ..Default::default()
        };
        let after = LiveTaskPayload {
            status: Status::CustomStatus("REPO".into()),
            ..before.clone()
        };
        let change = TaskChange {
            before: Some(before.clone()),
            after: Some(after),
        };

        let history = change
            .history(&author())
            .expect("a status change records history");
        assert_eq!(history.task_id, before.id);
        assert_eq!(history.user, RecordId::new("user", "editor"));
        assert_eq!(history.username, "editor");
        assert!(history.from_writer);
        assert_eq!(history.diff["status"]["old"], "follow up");
        assert_eq!(history.diff["status"]["new"], "REPO");
        assert_eq!(history.diff.as_object().map(|m| m.len()), Some(1));
    }

    #[test]
    fn an_unchanged_or_missing_row_yields_no_history() {
        let task = LiveTaskPayload::default();
        let same = TaskChange {
            before: Some(task.clone()),
            after: Some(task.clone()),
        };
        assert!(same.history(&author()).is_none());

        let missing = TaskChange {
            before: None,
            after: Some(task),
        };
        assert!(missing.history(&author()).is_none());
    }

    #[test]
    fn a_completed_at_stamp_alone_is_not_a_change() {
        let before = LiveTaskPayload::default();
        let after = LiveTaskPayload {
            completed_at: Some(chrono::Utc::now().into()),
            ..before.clone()
        };
        let change = TaskChange {
            before: Some(before),
            after: Some(after),
        };
        assert!(change.history(&author()).is_none());
    }

    #[test]
    fn an_empty_field_batch_builds_no_update() {
        assert!(TaskUpdate::fields(&LiveTaskPayload::default(), &[]).is_none());
    }
}
