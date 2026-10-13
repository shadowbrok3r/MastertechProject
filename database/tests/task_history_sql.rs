//! Task writes that record history, run against in-memory SurrealDB with the real task tables.

use chrono::{Duration, Utc};
use database::CURRENT_USER_INFO;
use database::schema::assistant::{complete_assistant_task_on, reassign_tasks_on};
use database::schema::{
    Datetime, HistoryAuthor, LiveTaskPayload, Priority, RecordId, SYSTEM_AUTHOR, Status, TaskField,
    TaskHistory, TaskUpdate, User,
};
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, Mem};

const SCHEMA: &[&str] = &[
    include_str!("../schema/task.surql"),
    include_str!("../schema/task_history.surql"),
    "DEFINE TABLE notification TYPE ANY SCHEMALESS; DEFINE TABLE user TYPE ANY SCHEMALESS;",
];

async fn mem_db() -> Surreal<Db> {
    let db = Surreal::new::<Mem>(()).await.expect("in-memory SurrealDB");
    db.use_ns("test").use_db("test").await.expect("use ns/db");
    for ddl in SCHEMA {
        let ddl = ddl.replace("DEFINE FIELD ", "DEFINE FIELD OVERWRITE ");
        db.query(ddl)
            .await
            .expect("apply schema")
            .check()
            .expect("schema statements");
    }
    db
}

/// Creates `task:<key>` with every field a task row carries.
async fn create_task(db: &Surreal<Db>, key: &str, status: &str, assignee: &str) -> RecordId {
    let id = RecordId::new("task", key);
    db.query(
        "CREATE $id CONTENT { task_name: 'Clone drive', task_description: '', assignee: $assignee, \
         status: $status, priority: 'Normal', completed: false }",
    )
    .bind(("id", id.clone()))
    .bind(("assignee", RecordId::new("user", assignee)))
    .bind(("status", status.to_string()))
    .await
    .expect("create")
    .check()
    .expect("create statement");
    id
}

async fn task(db: &Surreal<Db>, id: &RecordId) -> LiveTaskPayload {
    let task: Option<LiveTaskPayload> = db.select(id.clone()).await.expect("select task");
    task.expect("the task exists")
}

/// History rows for `id`, oldest first.
async fn history(db: &Surreal<Db>, id: &RecordId) -> Vec<TaskHistory> {
    db.query("SELECT * FROM task_history WHERE task_id == $id ORDER BY created_at ASC")
        .bind(("id", id.clone()))
        .await
        .expect("select history")
        .take(0)
        .expect("history rows")
}

fn author() -> HistoryAuthor {
    HistoryAuthor {
        user: RecordId::new("user", "editor"),
        username: "editor".into(),
    }
}

#[tokio::test]
async fn a_status_change_records_one_row_for_its_author() {
    let db = mem_db().await;
    let id = create_task(&db, "a", "follow up", "tech").await;

    let status = Status::CustomStatus("REPO".into());
    let rows = TaskUpdate::status(&id, &status)
        .run(&db, Some(author()))
        .await
        .expect("update");
    assert_eq!(rows, 1);
    TaskUpdate::status(&id, &status)
        .run(&db, Some(author()))
        .await
        .expect("same status");

    let rows = history(&db, &id).await;
    assert_eq!(
        rows.len(),
        1,
        "an UPDATE that changes nothing records nothing"
    );
    let row = &rows[0];
    assert_eq!(
        (row.username.as_str(), &row.user),
        ("editor", &RecordId::new("user", "editor"))
    );
    assert!(row.from_writer);
    assert_eq!(row.diff["status"]["old"], "follow up");
    assert_eq!(row.diff["status"]["new"], "REPO");
    assert_eq!(row.diff.as_object().map(|m| m.len()), Some(1));
}

#[tokio::test]
async fn every_status_binds_and_sets_completion() {
    let db = mem_db().await;
    let id = create_task(&db, "a", "Todo", "tech").await;
    let cases = [
        (Status::Complete, true),
        (Status::InRepair, false),
        (Status::Complete, true),
        (Status::Todo, false),
        (Status::Qc, false),
        (Status::Sales, false),
        (Status::CustomStatus("REPO".into()), false),
    ];
    for (status, completed) in cases {
        TaskUpdate::status(&id, &status)
            .run(&db, Some(author()))
            .await
            .expect("update");
        let task = task(&db, &id).await;
        assert_eq!(task.status, status);
        assert_eq!(task.completed, completed, "{status:?}");
    }
    assert_eq!(history(&db, &id).await.len(), 7);
}

#[tokio::test]
async fn a_field_batch_binds_every_field_and_records_one_row() {
    let db = mem_db().await;
    let id = create_task(&db, "a", "Todo", "tech").await;
    let due = Datetime::from(Utc::now() + Duration::days(2));
    let staged = LiveTaskPayload {
        assignee: RecordId::new("user", "other"),
        priority: Priority::Fire,
        due_date: due.clone(),
        status: Status::Qc,
        completed: true,
        ..task(&db, &id).await
    };
    let fields = [
        TaskField::Assignee,
        TaskField::Priority,
        TaskField::DueDate,
        TaskField::Status,
        TaskField::Completed,
    ];

    let update = TaskUpdate::fields(&staged, &fields).expect("fields to write");
    update.run(&db, Some(author())).await.expect("update");

    let written = task(&db, &id).await;
    assert_eq!(written.assignee, staged.assignee);
    assert_eq!(written.priority, Priority::Fire);
    assert_eq!(written.due_date, due);
    assert_eq!(written.status, Status::Qc);
    assert!(written.completed && written.completed_at.is_some());
    let rows = history(&db, &id).await;
    assert_eq!(rows.len(), 1);
    let diff = rows[0].diff.as_object().expect("diff object");
    for field in ["assignee", "priority", "due_date", "status", "completed"] {
        assert!(diff.contains_key(field), "{field} in {diff:?}");
    }
    assert_eq!(rows[0].diff["assignee"]["new"], "other");
}

#[tokio::test]
async fn a_bulk_completion_records_only_changed_tasks_and_keeps_existing_stamps() {
    let db = mem_db().await;
    let open = create_task(&db, "open", "Todo", "tech").await;
    let done = create_task(&db, "done", "Complete", "tech").await;
    let stamp = Datetime::from(Utc::now() - Duration::days(3));
    db.query("UPDATE $id SET completed = true, completed_at = $at")
        .bind(("id", done.clone()))
        .bind(("at", stamp.clone()))
        .await
        .expect("seed")
        .check()
        .expect("seed statement");

    let rows = TaskUpdate::completed_many(vec![open.clone(), done.clone()], true)
        .run(&db, Some(author()))
        .await
        .expect("bulk complete");

    assert_eq!(rows, 2);
    assert_eq!(history(&db, &open).await.len(), 1);
    assert!(history(&db, &done).await.is_empty());
    assert_eq!(task(&db, &done).await.completed_at, Some(stamp));
    assert!(task(&db, &open).await.completed_at.is_some());

    TaskUpdate::completed_many(vec![open.clone()], false)
        .run(&db, Some(author()))
        .await
        .expect("bulk reopen");
    let reopened = task(&db, &open).await;
    assert_eq!(
        (reopened.status, reopened.completed, reopened.completed_at),
        (Status::Todo, false, None)
    );
}

#[tokio::test]
async fn the_signed_in_user_authors_a_change_and_no_user_files_it_as_system() {
    let db = mem_db().await;
    let id = create_task(&db, "a", "Todo", "tech").await;
    let mut user = User::default();
    user.id = RecordId::new("user", "signed");
    user.set_email("signed.in@pclaptops.com");

    *CURRENT_USER_INFO.lock().expect("user lock") = Some(user);
    TaskUpdate::task_name(&id, "Renamed".into())
        .run(&db, None)
        .await
        .expect("rename");
    *CURRENT_USER_INFO.lock().expect("user lock") = None;
    TaskUpdate::due_now_many(vec![id.clone()])
        .run(&db, None)
        .await
        .expect("due now");

    let rows = history(&db, &id).await;
    assert_eq!(rows.len(), 2);
    assert_eq!(
        (rows[0].username.as_str(), &rows[0].user),
        ("signed.in", &RecordId::new("user", "signed"))
    );
    assert_eq!(
        (rows[1].username.as_str(), &rows[1].user),
        (SYSTEM_AUTHOR, &RecordId::new("user", "tech"))
    );
}

#[tokio::test]
async fn a_whole_row_replace_records_its_diff() {
    let db = mem_db().await;
    let id = create_task(&db, "a", "Todo", "tech").await;
    let replaced = LiveTaskPayload {
        task_name: "Dana Whitfield - 2000002".into(),
        service_number: Some("2000002".into()),
        ..task(&db, &id).await
    };

    TaskUpdate::replace(replaced)
        .run(&db, Some(author()))
        .await
        .expect("replace");

    let rows = history(&db, &id).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].diff["task_name"]["new"], "Dana Whitfield - 2000002");
    assert_eq!(rows[0].diff["service_number"]["new"], "2000002");
}

#[tokio::test]
async fn completing_an_assistant_task_records_the_completion() {
    let db = mem_db().await;
    let chore = create_task(&db, "chore", "Todo", "tech").await;
    db.query("UPDATE $id SET assigned_by = user:boss")
        .bind(("id", chore.clone()))
        .await
        .expect("assign")
        .check()
        .expect("assign statement");
    let ticket = create_task(&db, "ticket", "Todo", "tech").await;

    assert!(
        complete_assistant_task_on(&db, &chore, Some(author()))
            .await
            .expect("chore")
    );
    assert!(
        !complete_assistant_task_on(&db, &ticket, Some(author()))
            .await
            .expect("ticket")
    );

    let rows = history(&db, &chore).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].diff["status"]["new"], "Complete");
    assert_eq!(rows[0].diff["completed"]["new"], "true");
    assert!(history(&db, &ticket).await.is_empty());
}

#[tokio::test]
async fn a_reassignment_records_the_previous_assignee() {
    let db = mem_db().await;
    let id = create_task(&db, "a", "Todo", "aam998y60shwah3gr9hv").await;
    let to = RecordId::new("user", "jm9a7l3v32gsiccr7pgw");

    let moved = reassign_tasks_on(&db, &[id.clone()], &to, None, &author())
        .await
        .expect("reassign");
    let again = reassign_tasks_on(&db, &[id.clone()], &to, None, &author())
        .await
        .expect("reassign again");

    assert_eq!((moved, again.len()), (vec![id.clone()], 1));
    let rows = history(&db, &id).await;
    assert_eq!(
        rows.len(),
        1,
        "a move to the current assignee records nothing"
    );
    assert_eq!(rows[0].diff["assignee"]["old"], "aam998y60shwah3gr9hv");
    assert_eq!(rows[0].diff["assignee"]["new"], "jm9a7l3v32gsiccr7pgw");
}

#[tokio::test]
async fn the_database_stamps_saved_rows_and_legacy_rows_read_as_not_from_the_writer() {
    let db = mem_db().await;
    let task = create_task(&db, "a", "Todo", "tech").await;
    let skewed = TaskHistory {
        created_at: (Utc::now() - Duration::days(400)).into(),
        ..TaskHistory::new(
            task.clone(),
            RecordId::new("user", "editor"),
            "editor".into(),
            serde_json::json!({ "status": { "old": "Todo", "new": "QC" } }),
        )
    };
    skewed.save_on(&db).await.expect("save");
    db.query(
        "CREATE task_history CONTENT { task_id: $task, user: user:old, username: 'old', \
         diff: { status: { old: 'QC', new: 'Todo' } } }",
    )
    .bind(("task", task.clone()))
    .await
    .expect("legacy row")
    .check()
    .expect("legacy statement");

    let rows = history(&db, &task).await;
    assert_eq!(rows.len(), 2);
    let by = |name: &str| rows.iter().find(|row| row.username == name).expect("row");
    assert!(by("editor").from_writer);
    assert!(!by("old").from_writer);
    let stamped: chrono::DateTime<Utc> = by("editor").created_at.into();
    assert!(
        stamped > Utc::now() - Duration::minutes(1),
        "a client clock 400 days off does not reach created_at: {stamped}"
    );
}

#[tokio::test]
async fn usernames_resolve_every_user_a_diff_names_inactive_ones_included() {
    let db = mem_db().await;
    db.query(
        "CREATE user:gone CONTENT { email: 'gone.tech@pclaptops.com', active: false }; \
         CREATE user:here CONTENT { email: 'here.tech@pclaptops.com', active: true };",
    )
    .await
    .expect("users")
    .check()
    .expect("user statements");
    let row = TaskHistory::new(
        RecordId::new("task", "a"),
        RecordId::new("user", "editor"),
        "editor".into(),
        serde_json::json!({
            "assignee": { "old": "gone", "new": "user:here" },
            "status": { "old": "gone", "new": "QC" },
        }),
    );

    let names = TaskHistory::usernames_on(&db, &[row])
        .await
        .expect("usernames");

    assert_eq!(names.len(), 2);
    assert_eq!(names["gone"], "gone.tech");
    assert_eq!(names["here"], "here.tech");
}
