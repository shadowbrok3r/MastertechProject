//! In-memory SurrealDB carrying the repo's user and task schema.

use database::schema::RecordId;
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, Mem};

const USER_SCHEMA: &str = include_str!("../../../../database/schema/user.surql");
const TASK_SCHEMA: &str = include_str!("../../../../database/schema/task.surql");
const AUDIT_LOG_SCHEMA: &str = include_str!("../../../../database/schema/audit_log.surql");
const NOTIFICATION_SCHEMA: &str = include_str!("../../../../database/schema/notification.surql");
const USER_ACCESS_LEGACY: &str = include_str!("../../../../database/access/user_legacy.surql");
const JWT_KEY: &str = "user-admin-test-key-0123456789abcdef0123456789abcdef";

/// Applies the live `user` record access with the tables its clauses write.
pub async fn apply_legacy_access(db: &Surreal<Db>) {
    for (name, sql) in [
        ("audit_log", AUDIT_LOG_SCHEMA),
        ("notification", NOTIFICATION_SCHEMA),
    ] {
        db.query(sql)
            .await
            .unwrap_or_else(|e| panic!("{name} schema: {e}"))
            .check()
            .unwrap_or_else(|e| panic!("{name} schema statements: {e}"));
    }
    db.query(USER_ACCESS_LEGACY)
        .bind(("jwt_key", JWT_KEY))
        .await
        .expect("legacy access")
        .check()
        .expect("legacy access statement");
}

/// Root session on a fresh database with user.surql and task.surql applied.
pub async fn schema_db() -> Surreal<Db> {
    let db = Surreal::new::<Mem>(()).await.expect("in-memory SurrealDB");
    db.use_ns("test").use_db("test").await.expect("use ns/db");
    let overwrite = |sql: &str| sql.replace("DEFINE FIELD ", "DEFINE FIELD OVERWRITE ");
    for (name, sql) in [
        ("user", overwrite(USER_SCHEMA)),
        ("task", overwrite(TASK_SCHEMA)),
    ] {
        db.query(sql)
            .await
            .unwrap_or_else(|e| panic!("{name} schema: {e}"))
            .check()
            .unwrap_or_else(|e| panic!("{name} schema statements: {e}"));
    }
    db
}

/// Creates `user:<key>` with every required field; `None` leaves `id_store` NONE.
pub async fn insert_user(
    db: &Surreal<Db>,
    key: &str,
    store: &str,
    id_store: Option<&str>,
    active: bool,
) {
    let id = RecordId::new("user", key);
    db.query(
        "CREATE $id CONTENT { active: $active, authorization: 'User', email: $email, everest_initials: '', \
         name: $name, password: 'x', store: $store, id_store: $id_store ?? '', version: '', \
         user_settings: { ui_layout: { mastertech: {}, mtechserver: {} } } };
         IF $id_store == NONE { UPDATE $id SET id_store = NONE };",
    )
    .bind(("id", id))
    .bind(("active", active))
    .bind(("email", format!("{key}@pclaptops.com")))
    .bind(("name", format!("{key} person")))
    .bind(("store", store.to_string()))
    .bind(("id_store", id_store.map(str::to_string)))
    .await
    .expect("insert user")
    .check()
    .expect("insert user statement");
}

/// Creates `task:<key>` assigned to `user:<assignee>` with service number `SO-<key>`.
pub async fn insert_task(db: &Surreal<Db>, key: &str, assignee: &str, completed: bool) {
    db.query(
        "CREATE $id CONTENT { assignee: $assignee, completed: $completed, task_name: $name, service_number: $sn };",
    )
    .bind(("id", RecordId::new("task", key)))
    .bind(("assignee", RecordId::new("user", assignee)))
    .bind(("completed", completed))
    .bind(("name", format!("task {key}")))
    .bind(("sn", format!("SO-{key}")))
    .await
    .expect("insert task")
    .check()
    .expect("insert task statement");
}
