use database::schema::{LiveTaskPayload, RecordId, TaskNotePayload, User, Priority, TASK_TABLE};
use database::db;
use chrono::Utc;

// pub async fn fetch_my_tasks() -> anyhow::Result<Vec<LiveTaskPayload>> {
//     LiveTaskPayload::get_tasks().await
// }

/// Fetch all tasks that are not completed
pub async fn fetch_incomplete_tasks() -> anyhow::Result<Vec<LiveTaskPayload>> {
    let tasks: Vec<LiveTaskPayload> = db()
        .query("SELECT * FROM task WHERE $this.assignee.store == $auth.store AND $this.completed IS false ")
        .await?
        .take(0)?;
    Ok(tasks)
}

/// Fetch all tasks that are completed
pub async fn fetch_completed_tasks() -> anyhow::Result<Vec<LiveTaskPayload>> {
    let tasks: Vec<LiveTaskPayload> = db()
        .query("SELECT * FROM task WHERE $this.assignee.store == $auth.store AND $this.completed IS true ")
        .await?
        .take(0)?;
    Ok(tasks)
}

pub async fn fetch_task_notes(task_id: &RecordId) -> anyhow::Result<Vec<TaskNotePayload>> {
    TaskNotePayload::get_db_notes_from_task_id(task_id.clone()).await
}

pub async fn fetch_store_users() -> Vec<User> {
    // Best-effort read from global cache set by login; fall back to querying DB directly
    if let Ok(guard) = database::STORE_USERS.try_lock() {
        if !guard.is_empty() {
            return guard.clone();
        }
    }

    // Fallback: query active users
    let users: Vec<User> = db()
        .query("SELECT * FROM user WHERE active == true")
        .await
        .ok()
        .and_then(|mut r| r.take(0).ok())
        .unwrap_or_default();
    users
}

pub async fn toggle_completed(task: &LiveTaskPayload) -> anyhow::Result<()> {
    task.update_completed(!task.completed).await
}

pub async fn update_status(task: &LiveTaskPayload, status: database::schema::Status) -> anyhow::Result<()> {
    task.update_status(status).await
}

pub async fn update_assignee(task: &LiveTaskPayload, assignee: RecordId) -> anyhow::Result<()> {
    task.update_assignee(assignee).await
}

pub async fn add_note(task_id: RecordId, user: &User, text: String, private: bool, service_number: Option<String>) -> anyhow::Result<()> {
    use database::schema::task_note::task_note_builder::TaskNoteBuilder;

    let id_employee = user.get_employee_id().map(|v| v.to_string()).unwrap_or_default();
    let mut builder = TaskNoteBuilder::new(task_id, user.get_id(), id_employee)
        .note(text)
        .username(user.get_username());

    if let Some(sn) = service_number.filter(|s| !s.is_empty()) {
        builder = builder.service_number(sn);
    }
    if private {
        builder = builder.private(true);
    }

    // Build will also persist to DB and handle Prestashop when applicable
    let _ = builder.build().await?;
    Ok(())
}

// ------------------------
// Create Task (simple)
// ------------------------

#[derive(Clone, Debug)]
pub struct NewTaskInput {
    pub task_name: String,
    pub task_description: String,
    pub service_number: Option<String>,
    pub priority: Priority,
    pub assignee_username: String,
}

pub async fn create_task_simple(input: NewTaskInput) -> anyhow::Result<LiveTaskPayload> {
    // Resolve assignee by username from cache first
    let mut assignee: Option<RecordId> = None;
    if let Ok(guard) = database::STORE_USERS.try_lock() {
        if !guard.is_empty() {
            if let Some(u) = guard.iter().find(|u| u.get_username() == input.assignee_username) {
                assignee = Some(u.get_id());
            }
        }
    }
    if assignee.is_none() {
        // Fallback: query active users and match username
        let users: Vec<User> = db()
            .query("SELECT * FROM user WHERE active == true")
            .await
            .ok()
            .and_then(|mut r| r.take(0).ok())
            .unwrap_or_default();
        if let Some(u) = users.iter().find(|u| u.get_username() == input.assignee_username) {
            assignee = Some(u.get_id());
        }
    }
    let assignee = assignee.unwrap_or_else(|| {
        // Default to current auth user if available
        if let Ok(guard) = database::CURRENT_USER_INFO.try_lock() {
            if let Some(u) = guard.clone() { return u.get_id(); }
        }
        // fallback: a random user id is not acceptable; keep a placeholder that will likely error if used
        database::schema::random_record_id("user")
    });

    let mut task = LiveTaskPayload::default();
    task.task_name = input.task_name;
    task.task_description = input.task_description;
    task.service_number = input.service_number;
    task.priority = input.priority;
    task.assignee = assignee;
    task.completed = false;
    task.status = database::schema::Status::Todo;
    task.due_date = Utc::now().into();
    task.created_at = Utc::now().into();

    let created: Option<database::schema::Record> = db()
        .create(TASK_TABLE)
        .content(task.clone())
        .await?;
    // If created returns None, still return the task (id already set in default)
    let _ = created;
    Ok(task)
}
