use crate::db;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use super::{Datetime, RecordId, SurrealValue};
use surrealdb_types::{Kind, Value};

/// Lifecycle of an AI hands-on handoff task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AiTaskStatus {
    /// Checklist has unchecked items; the tech owns it.
    Open,
    /// Every item is checked; the requesting operator owns follow-up.
    AwaitingFollowup,
    /// Operator accepted the handback; terminal.
    Closed,
}

impl Default for AiTaskStatus {
    fn default() -> Self { Self::Open }
}

impl Serialize for AiTaskStatus {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for AiTaskStatus {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Ok(Self::from_str(&s))
    }
}

impl SurrealValue for AiTaskStatus {
    fn kind_of() -> Kind {
        Kind::String
    }

    fn into_value(self) -> Value {
        Value::String(self.as_str().to_string())
    }

    fn from_value(value: Value) -> Result<Self, surrealdb::Error>
    where
        Self: Sized,
    {
        match value {
            Value::String(s) => Ok(Self::from_str(&s)),
            other => Err(surrealdb::Error::validation(
                format!("AiTaskStatus expected string, got {other:?}"),
                None,
            )),
        }
    }
}

impl AiTaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::AwaitingFollowup => "awaiting_followup",
            Self::Closed => "closed",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "awaiting_followup" | "awaitingfollowup" | "awaiting followup" => Self::AwaitingFollowup,
            "closed" => Self::Closed,
            _ => Self::Open,
        }
    }
}

/// AI-authored hands-on handoff overlay pointing at an existing `task`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, SurrealValue)]
pub struct AiTask {
    pub id: RecordId,
    pub task_ref: RecordId,
    pub session_ref: RecordId,
    pub assignee: RecordId,
    pub requested_by: RecordId,
    pub title: String,
    pub customer_name: String,
    pub service_number: String,
    pub connection_string: Option<String>,
    pub status: AiTaskStatus,
    pub acknowledged_at: Option<Datetime>,
    pub review_acknowledged_at: Option<Datetime>,
    pub created_at: Datetime,
    pub completed_at: Option<Datetime>,
    pub closed_at: Option<Datetime>,
    /// Mirrored from the session by set_current_theory, so the card shows
    /// why the checklist exists without loading the session.
    #[serde(default)]
    #[surreal(default)]
    pub current_theory: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub theory_next_step: Option<String>,
}

impl Default for AiTask {
    fn default() -> Self {
        let now: Datetime = chrono::Utc::now().into();
        Self {
            id: super::random_record_id(super::AI_TASK_TABLE),
            task_ref: super::random_record_id(super::TASK_TABLE),
            session_ref: super::random_record_id(super::DIAGNOSTIC_SESSION_TABLE),
            assignee: super::random_record_id(super::USER_TABLE),
            requested_by: super::random_record_id(super::USER_TABLE),
            title: String::new(),
            customer_name: String::new(),
            service_number: String::new(),
            connection_string: None,
            status: AiTaskStatus::Open,
            acknowledged_at: None,
            review_acknowledged_at: None,
            created_at: now,
            completed_at: None,
            closed_at: None,
            current_theory: None,
            theory_next_step: None,
        }
    }
}

/// One checklist row belonging to an `ai_task`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, SurrealValue)]
pub struct AiTaskItem {
    pub id: RecordId,
    pub ai_task_ref: RecordId,
    pub text: String,
    pub position: i64,
    pub checked: bool,
    pub checked_by: Option<RecordId>,
    pub checked_at: Option<Datetime>,
    pub entry_ref: Option<RecordId>,
    pub created_at: Datetime,
    /// Set when the step's text is rewritten; absent on never-edited items.
    #[serde(default)]
    #[surreal(default)]
    pub updated_at: Option<Datetime>,
    /// Which hands-on category justifies this being a human step. Absent on
    /// items written before the gate existed.
    #[serde(default)]
    #[surreal(default)]
    pub reason: Option<String>,
}

/// A checklist step plus the hands-on category justifying it as human work.
#[derive(Debug, Clone)]
pub struct NewStep {
    pub text: String,
    pub reason: Option<String>,
}

/// Hard ceiling on items per AI task, counted across create + every append.
pub const MAX_ITEMS_PER_TASK: usize = 15;

/// Hard ceiling on one step's text; rationale belongs in a diagnostic entry.
pub const MAX_STEP_CHARS: usize = 200;

impl Default for AiTaskItem {
    fn default() -> Self {
        Self {
            id: super::random_record_id(super::AI_TASK_ITEM_TABLE),
            ai_task_ref: super::random_record_id(super::AI_TASK_TABLE),
            text: String::new(),
            position: 0,
            checked: false,
            checked_by: None,
            checked_at: None,
            entry_ref: None,
            created_at: chrono::Utc::now().into(),
            updated_at: None,
            reason: None,
        }
    }
}

impl AiTask {
    /// Create the checklist items first, then the parent last — the parent
    /// CREATE fires the tech-attention event, so items must already exist.
    pub async fn create_with_items(task: &Self, steps: &[NewStep]) -> anyhow::Result<(RecordId, Vec<RecordId>)> {
        let mut t = task.clone();
        t.id = super::random_record_id(super::AI_TASK_TABLE);
        t.created_at = chrono::Utc::now().into();
        t.status = AiTaskStatus::Open;

        let mut item_ids = Vec::with_capacity(steps.len());
        for (idx, step) in steps.iter().enumerate() {
            let item = AiTaskItem {
                ai_task_ref: t.id.clone(),
                text: step.text.clone(),
                reason: step.reason.clone(),
                position: idx as i64,
                ..Default::default()
            };
            let created: Option<AiTaskItem> = db().create(item.id.clone()).content(item.clone()).await?;
            item_ids.push(created.map(|c| c.id).unwrap_or(item.id));
        }

        let created: Option<Self> = db().create(t.id.clone()).content(t.clone()).await?;
        Ok((created.map(|c| c.id).unwrap_or(t.id), item_ids))
    }

    /// How many checklist items the task already carries, for the total cap.
    pub async fn item_count(id: &RecordId) -> anyhow::Result<usize> {
        let n: Option<i64> = db()
            .query("array::first((SELECT VALUE count() FROM ai_task_item WHERE ai_task_ref = $id GROUP ALL))")
            .bind(("id", id.clone()))
            .await?
            .take(0)?;
        Ok(n.unwrap_or(0).max(0) as usize)
    }

    /// Append steps and reopen; positions continue after the current max.
    pub async fn add_steps(id: &RecordId, steps: &[NewStep]) -> anyhow::Result<Vec<RecordId>> {
        let next: Option<i64> = db()
            .query("array::first((SELECT VALUE math::max(position) FROM ai_task_item WHERE ai_task_ref = $id GROUP ALL))")
            .bind(("id", id.clone()))
            .await?
            .take(0)?;
        let start = next.map(|n| n + 1).unwrap_or(0);

        let mut item_ids = Vec::with_capacity(steps.len());
        for (idx, step) in steps.iter().enumerate() {
            let item = AiTaskItem {
                ai_task_ref: id.clone(),
                text: step.text.clone(),
                reason: step.reason.clone(),
                position: start + idx as i64,
                ..Default::default()
            };
            let created: Option<AiTaskItem> = db().create(item.id.clone()).content(item.clone()).await?;
            item_ids.push(created.map(|c| c.id).unwrap_or(item.id));
        }

        db().query("UPDATE $id SET status = 'open', completed_at = NONE, acknowledged_at = NONE, review_acknowledged_at = NONE WHERE status != 'closed'")
            .bind(("id", id.clone()))
            .await?;
        Ok(item_ids)
    }

    pub async fn close(id: &RecordId) -> anyhow::Result<()> {
        db().query("UPDATE $id SET status = 'closed', closed_at = time::now()")
            .bind(("id", id.clone()))
            .await?;
        Ok(())
    }

    pub async fn acknowledge(id: &RecordId, review: bool) -> anyhow::Result<()> {
        let sql = if review {
            "UPDATE $id SET review_acknowledged_at = time::now()"
        } else {
            "UPDATE $id SET acknowledged_at = time::now()"
        };
        db().query(sql).bind(("id", id.clone())).await?;
        Ok(())
    }

    pub async fn reassign(id: &RecordId, assignee: &RecordId) -> anyhow::Result<()> {
        db().query("UPDATE $id SET assignee = $assignee, acknowledged_at = NONE")
            .bind(("id", id.clone()))
            .bind(("assignee", assignee.clone()))
            .await?;
        Ok(())
    }

    /// Snapshot of every non-closed AI task (+items) visible to this store.
    pub async fn list_active_for_store() -> anyhow::Result<(Vec<Self>, Vec<AiTaskItem>)> {
        let mut res = db()
            .query("SELECT * FROM ai_task WHERE assignee.store == $auth.store AND status != 'closed'")
            .query("SELECT * FROM ai_task_item WHERE ai_task_ref.assignee.store == $auth.store AND ai_task_ref.status != 'closed'")
            .await?;
        let tasks: Vec<Self> = res.take(0)?;
        let items: Vec<AiTaskItem> = res.take(1)?;
        Ok((tasks, items))
    }

    pub async fn get_full(id: &RecordId) -> anyhow::Result<Option<(Self, Vec<AiTaskItem>)>> {
        let task: Option<Self> = db().select(id.clone()).await?;
        let Some(task) = task else { return Ok(None) };
        let items: Vec<AiTaskItem> = db()
            .query("SELECT * FROM ai_task_item WHERE ai_task_ref == $id ORDER BY position ASC")
            .bind(("id", id.clone()))
            .await?
            .take(0)?;
        Ok(Some((task, items)))
    }

    /// Newest non-closed AI task on a diagnostic session, if any.
    pub async fn get_open_for_session(session_ref: &RecordId) -> anyhow::Result<Option<Self>> {
        let tasks: Vec<Self> = db()
            .query("SELECT * FROM ai_task WHERE session_ref == $sid AND status != 'closed' ORDER BY created_at DESC LIMIT 1")
            .bind(("sid", session_ref.clone()))
            .await?
            .take(0)?;
        Ok(tasks.into_iter().next())
    }

    /// Oldest non-closed AI task on a service task, if any. Keyed on the task
    /// rather than the session so a later engagement appends instead of
    /// opening a rival checklist.
    pub async fn get_open_for_task(task_ref: &RecordId) -> anyhow::Result<Option<Self>> {
        let tasks: Vec<Self> = db()
            .query("SELECT * FROM ai_task WHERE task_ref == $tid AND status != 'closed' ORDER BY created_at ASC LIMIT 1")
            .bind(("tid", task_ref.clone()))
            .await?
            .take(0)?;
        Ok(tasks.into_iter().next())
    }

    /// The session's own open AI task, else the open one on its service task.
    /// The bool is true when it resolved via the task, so a caller can say the
    /// checklist belongs to an earlier engagement.
    pub async fn get_open_for_session_or_task(
        session_ref: &RecordId,
    ) -> anyhow::Result<Option<(Self, bool)>> {
        if let Some(t) = Self::get_open_for_session(session_ref).await? {
            return Ok(Some((t, false)));
        }
        let Some(task_ref) = Self::session_task(session_ref).await? else {
            return Ok(None);
        };
        Ok(Self::get_open_for_task(&task_ref).await?.map(|t| (t, true)))
    }

    /// Whether an escalation is backed by a checklist: one on the session, or
    /// an open one on its service task. A repeat engagement cannot create its
    /// own, so a session-only check would refuse a legitimate close.
    pub async fn handoff_exists(session_ref: &RecordId) -> anyhow::Result<bool> {
        if Self::any_for_session(session_ref).await? {
            return Ok(true);
        }
        let Some(task_ref) = Self::session_task(session_ref).await? else {
            return Ok(false);
        };
        Ok(Self::get_open_for_task(&task_ref).await?.is_some())
    }

    async fn session_task(session_ref: &RecordId) -> anyhow::Result<Option<RecordId>> {
        let found: Option<RecordId> = db()
            .query("SELECT VALUE task_ref FROM $sid WHERE task_ref != NONE")
            .bind(("sid", session_ref.clone()))
            .await?
            .take(0)?;
        Ok(found)
    }

    /// True when any AI task (any status) was ever created for the session.
    pub async fn any_for_session(session_ref: &RecordId) -> anyhow::Result<bool> {
        let tasks: Vec<Self> = db()
            .query("SELECT * FROM ai_task WHERE session_ref == $sid LIMIT 1")
            .bind(("sid", session_ref.clone()))
            .await?
            .take(0)?;
        Ok(!tasks.is_empty())
    }

    pub async fn get_for_task(task_ref: &RecordId) -> anyhow::Result<Vec<Self>> {
        let tasks: Vec<Self> = db()
            .query("SELECT * FROM ai_task WHERE task_ref == $tid ORDER BY created_at DESC LIMIT 20")
            .bind(("tid", task_ref.clone()))
            .await?
            .take(0)?;
        Ok(tasks)
    }

    /// Transition an open task to awaiting_followup when every remaining item is
    /// checked. Item removal does not fire the `ai_task_item_checked` event, so
    /// removing the last unchecked item must complete the task explicitly.
    pub async fn reevaluate_completion(id: &RecordId) -> anyhow::Result<()> {
        let items: Vec<AiTaskItem> = db()
            .query("SELECT * FROM ai_task_item WHERE ai_task_ref == $id")
            .bind(("id", id.clone()))
            .await?
            .take(0)?;
        if !items.is_empty() && items.iter().all(|i| i.checked) {
            db().query(
                "UPDATE $id SET status = 'awaiting_followup', completed_at = time::now() \
                 WHERE status = 'open'",
            )
            .bind(("id", id.clone()))
            .await?;
        }
        Ok(())
    }
}

impl AiTaskItem {
    /// Toggle a checkbox; stamps checked_by/checked_at from the writer's auth.
    pub async fn set_checked(id: &RecordId, checked: bool) -> anyhow::Result<()> {
        let sql = if checked {
            "UPDATE $id SET checked = true, checked_by = $auth.id, checked_at = time::now()"
        } else {
            "UPDATE $id SET checked = false, checked_by = NONE, checked_at = NONE"
        };
        db().query(sql).bind(("id", id.clone())).await?;
        Ok(())
    }

    /// Fetch one checklist item by id.
    pub async fn get(id: &RecordId) -> anyhow::Result<Option<Self>> {
        Ok(db().select(id.clone()).await?)
    }

    /// Rewrite an item's step text, atomically re-asserting that it is unchecked
    /// and its task is open, so a tech checking the box or the task closing
    /// between the caller's read and this write can't slip through. Returns the
    /// updated row, or None when the guard now fails (item checked/closed/gone).
    pub async fn edit_text_if_editable(id: &RecordId, text: &str) -> anyhow::Result<Option<Self>> {
        let updated: Vec<Self> = db()
            .query(
                "UPDATE $id SET text = $text, updated_at = time::now() \
                 WHERE checked = false AND ai_task_ref.status != 'closed' RETURN AFTER",
            )
            .bind(("id", id.clone()))
            .bind(("text", text.to_string()))
            .await?
            .take(0)?;
        Ok(updated.into_iter().next())
    }

    /// Delete a checklist item, atomically re-asserting unchecked + task-open so
    /// a concurrently-checked item is never destroyed. Returns the removed row
    /// (BEFORE state, for its entry_ref), or None when the guard now fails.
    pub async fn remove_if_unchecked(id: &RecordId) -> anyhow::Result<Option<Self>> {
        let deleted: Vec<Self> = db()
            .query(
                "DELETE $id \
                 WHERE checked = false AND ai_task_ref.status != 'closed' RETURN BEFORE",
            )
            .bind(("id", id.clone()))
            .await?
            .take(0)?;
        Ok(deleted.into_iter().next())
    }
}
