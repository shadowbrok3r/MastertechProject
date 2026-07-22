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
}

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
        }
    }
}

impl AiTask {
    /// Create the checklist items first, then the parent last — the parent
    /// CREATE fires the tech-attention event, so items must already exist.
    pub async fn create_with_items(task: &Self, steps: &[String]) -> anyhow::Result<(RecordId, Vec<RecordId>)> {
        let mut t = task.clone();
        t.id = super::random_record_id(super::AI_TASK_TABLE);
        t.created_at = chrono::Utc::now().into();
        t.status = AiTaskStatus::Open;

        let mut item_ids = Vec::with_capacity(steps.len());
        for (idx, text) in steps.iter().enumerate() {
            let item = AiTaskItem {
                ai_task_ref: t.id.clone(),
                text: text.clone(),
                position: idx as i64,
                ..Default::default()
            };
            let created: Option<AiTaskItem> = db().create(item.id.clone()).content(item.clone()).await?;
            item_ids.push(created.map(|c| c.id).unwrap_or(item.id));
        }

        let created: Option<Self> = db().create(t.id.clone()).content(t.clone()).await?;
        Ok((created.map(|c| c.id).unwrap_or(t.id), item_ids))
    }

    /// Append steps and reopen; positions continue after the current max.
    pub async fn add_steps(id: &RecordId, steps: &[String]) -> anyhow::Result<Vec<RecordId>> {
        let next: Option<i64> = db()
            .query("array::first((SELECT VALUE math::max(position) FROM ai_task_item WHERE ai_task_ref = $id GROUP ALL))")
            .bind(("id", id.clone()))
            .await?
            .take(0)?;
        let start = next.map(|n| n + 1).unwrap_or(0);

        let mut item_ids = Vec::with_capacity(steps.len());
        for (idx, text) in steps.iter().enumerate() {
            let item = AiTaskItem {
                ai_task_ref: id.clone(),
                text: text.clone(),
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
}
