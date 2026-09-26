//! The `sync` write set, its guarded transaction and its inverse.

use std::collections::BTreeMap;
use std::fmt;

use database::schema::{RecordId, SurrealValue};
use serde::{Deserialize, Serialize};
use surrealdb::types::Value;

use crate::plan::{Plan, TaskAction};

/// The `active`, `store` and `id_store` values a guarded update expects.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RowState {
    pub active: bool,
    pub store: String,
    pub id_store: Option<String>,
}

/// Open-task handling recorded with a deactivation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TaskHandling {
    RequireNone,
    Reassign {
        to: RecordId,
        to_email: String,
        tasks: Vec<RecordId>,
    },
    Orphan {
        tasks: Vec<RecordId>,
    },
}

/// One row write with the values it replaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Write {
    Deactivate {
        id: RecordId,
        email: String,
        prior: RowState,
        tasks: TaskHandling,
    },
    MoveStore {
        id: RecordId,
        email: String,
        prior: RowState,
        store: String,
        id_store: String,
    },
}

/// Field change of a guarded user update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowSet {
    Active(bool),
    Store {
        store: String,
        id_store: Option<String>,
    },
}

/// A user update that applies only while the row still matches `expect`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowUpdate {
    pub id: RecordId,
    pub email: String,
    pub expect: RowState,
    pub set: RowSet,
}

impl Write {
    pub fn email(&self) -> &str {
        match self {
            Self::Deactivate { email, .. } | Self::MoveStore { email, .. } => email,
        }
    }

    /// The update `--apply` runs.
    pub fn forward(&self) -> RowUpdate {
        match self {
            Self::Deactivate {
                id, email, prior, ..
            } => RowUpdate {
                id: id.clone(),
                email: email.clone(),
                expect: prior.clone(),
                set: RowSet::Active(false),
            },
            Self::MoveStore {
                id,
                email,
                prior,
                store,
                id_store,
            } => RowUpdate {
                id: id.clone(),
                email: email.clone(),
                expect: prior.clone(),
                set: RowSet::Store {
                    store: store.clone(),
                    id_store: Some(id_store.clone()),
                },
            },
        }
    }

    /// The update `--revert` runs: expects the post-apply row and restores `prior`.
    pub fn inverse(&self) -> RowUpdate {
        match self {
            Self::Deactivate {
                id, email, prior, ..
            } => RowUpdate {
                id: id.clone(),
                email: email.clone(),
                expect: RowState {
                    active: false,
                    ..prior.clone()
                },
                set: RowSet::Active(prior.active),
            },
            Self::MoveStore {
                id,
                email,
                prior,
                store,
                id_store,
            } => RowUpdate {
                id: id.clone(),
                email: email.clone(),
                expect: RowState {
                    active: prior.active,
                    store: store.clone(),
                    id_store: Some(id_store.clone()),
                },
                set: RowSet::Store {
                    store: prior.store.clone(),
                    id_store: prior.id_store.clone(),
                },
            },
        }
    }
}

/// True when any update writes store 'WAR'.
pub fn sets_war<'a>(updates: impl IntoIterator<Item = &'a RowUpdate>) -> bool {
    updates.into_iter().any(|update| {
        matches!(&update.set, RowSet::Store { store, .. } if store.trim().eq_ignore_ascii_case("WAR"))
    })
}

/// The writes `--apply` performs for `plan`; held moves are excluded.
pub fn writes(plan: &Plan) -> Vec<Write> {
    let deactivations = plan.deactivate.iter().map(|d| {
        let task_ids = || d.open_tasks.iter().map(|task| task.id.clone()).collect();
        Write::Deactivate {
            id: d.user.id.clone(),
            email: d.user.email.clone(),
            prior: RowState {
                active: d.user.active,
                store: d.user.store.clone(),
                id_store: d.user.id_store.clone(),
            },
            tasks: match &d.tasks {
                TaskAction::Untouched => TaskHandling::RequireNone,
                TaskAction::Reassign { to, to_email } => TaskHandling::Reassign {
                    to: to.clone(),
                    to_email: to_email.clone(),
                    tasks: task_ids(),
                },
                TaskAction::Orphan => TaskHandling::Orphan { tasks: task_ids() },
            },
        }
    });
    let moves = plan.move_store.iter().map(|m| Write::MoveStore {
        id: m.user.id.clone(),
        email: m.user.email.clone(),
        prior: RowState {
            active: m.user.active,
            store: m.user.store.clone(),
            id_store: m.user.id_store.clone(),
        },
        store: m.store.as_str().to_string(),
        id_store: m.id_store.clone(),
    });
    deactivations.chain(moves).collect()
}

/// SurrealQL text with its bound variables.
#[derive(Debug, Clone)]
pub struct Statement {
    pub sql: String,
    pub vars: BTreeMap<String, Value>,
}

struct Builder {
    lines: Vec<String>,
    vars: BTreeMap<String, Value>,
}

impl Builder {
    fn new() -> Self {
        Self {
            lines: vec!["BEGIN TRANSACTION;".into()],
            vars: BTreeMap::new(),
        }
    }

    fn bind(&mut self, name: String, value: impl SurrealValue) -> String {
        let reference = format!("${name}");
        self.vars.insert(name, value.into_value());
        reference
    }

    fn row(&mut self, i: usize, update: &RowUpdate, failure: &str) {
        let id = self.bind(format!("id{i}"), update.id.clone());
        let email = self.bind(format!("email{i}"), update.email.clone());
        let was_store = self.bind(format!("was_store{i}"), update.expect.store.clone());
        let was_shop = self.bind(format!("was_shop{i}"), update.expect.id_store.clone());
        let set = match &update.set {
            RowSet::Active(active) => format!("active = {active}"),
            RowSet::Store { store, id_store } => {
                let store = self.bind(format!("store{i}"), store.clone());
                let shop = self.bind(format!("shop{i}"), id_store.clone());
                format!("store = {store}, id_store = {shop}")
            }
        };
        self.lines.push(format!(
            "LET $row{i} = (UPDATE user SET {set} WHERE id = {id} AND active = {} AND store = {was_store} AND id_store = {was_shop} RETURN id);",
            update.expect.active
        ));
        self.lines.push(format!(
            "IF array::len($row{i}) != 1 {{ THROW string::concat('{failure}: ', {email}) }};"
        ));
    }

    fn finish(mut self) -> Statement {
        self.lines.push("COMMIT TRANSACTION;".into());
        Statement {
            sql: self.lines.join("\n"),
            vars: self.vars,
        }
    }
}

const STALE_USER: &str = "user changed since the report; rerun sync";
const STALE_TASKS: &str = "open tasks changed since the report; rerun sync";
const INACTIVE_TARGET: &str = "reassign target is not an active user";
const REVERT_STALE: &str = "user changed since the sync; nothing was reverted";

impl Statement {
    /// One all-or-nothing transaction of guarded forward writes.
    pub fn apply(writes: &[Write]) -> Self {
        let mut b = Builder::new();
        for (i, write) in writes.iter().enumerate() {
            b.row(i, &write.forward(), STALE_USER);
        }
        for (i, write) in writes.iter().enumerate() {
            let Write::Deactivate { tasks, .. } = write else {
                continue;
            };
            let id = format!("$id{i}");
            let email = format!("$email{i}");
            if let TaskHandling::Reassign {
                to,
                to_email,
                tasks,
            } = tasks
            {
                let to = b.bind(format!("to{i}"), to.clone());
                let to_email = b.bind(format!("to_email{i}"), to_email.clone());
                let list = b.bind(format!("tasks{i}"), tasks.clone());
                b.lines.push(format!(
                    "LET $target{i} = (SELECT VALUE id FROM user WHERE id = {to} AND active = true);"
                ));
                b.lines.push(format!(
                    "IF array::len($target{i}) != 1 {{ THROW string::concat('{INACTIVE_TARGET}: ', {to_email}) }};"
                ));
                b.lines.push(format!(
                    "LET $moved{i} = (UPDATE task SET assignee = {to} WHERE id IN {list} AND assignee = {id} AND completed = false RETURN id);"
                ));
                b.lines.push(format!(
                    "IF array::len($moved{i}) != array::len({list}) {{ THROW string::concat('{STALE_TASKS}: ', {email}) }};"
                ));
            }
            if !matches!(tasks, TaskHandling::Orphan { .. }) {
                b.lines.push(format!(
                    "LET $open{i} = (SELECT VALUE id FROM task WHERE assignee = {id} AND completed = false);"
                ));
                b.lines.push(format!(
                    "IF array::len($open{i}) != 0 {{ THROW string::concat('{STALE_TASKS}: ', {email}) }};"
                ));
            }
        }
        b.finish()
    }

    /// One all-or-nothing transaction restoring every prior value; reassigned tasks still open move back.
    pub fn revert(writes: &[Write]) -> Self {
        let mut b = Builder::new();
        for (i, write) in writes.iter().enumerate().rev() {
            b.row(i, &write.inverse(), REVERT_STALE);
        }
        for (i, write) in writes.iter().enumerate().rev() {
            let Write::Deactivate {
                tasks: TaskHandling::Reassign { to, tasks, .. },
                ..
            } = write
            else {
                continue;
            };
            let to = b.bind(format!("to{i}"), to.clone());
            let list = b.bind(format!("tasks{i}"), tasks.clone());
            b.lines.push(format!(
                "UPDATE task SET assignee = $id{i} WHERE id IN {list} AND assignee = {to} AND completed = false;"
            ));
        }
        b.finish()
    }
}

/// The database a record was written against.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
    pub url: String,
    pub local: bool,
    pub ns: String,
    pub db: String,
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let scheme = if self.local { "ws" } else { "wss" };
        write!(f, "{scheme} {} ns={} db={}", self.url, self.ns, self.db)
    }
}

/// `out/sync_<utc>.json`: the plan and every touched row's prior values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncRecord {
    pub format: u32,
    pub created_at: String,
    /// Set once the sync transaction committed; `--revert` requires it.
    #[serde(default)]
    pub committed_at: Option<String>,
    /// Set once a revert of this record committed.
    #[serde(default)]
    pub reverted_at: Option<String>,
    pub target: Target,
    pub plan: Plan,
    pub writes: Vec<Write>,
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::plan::tests::{found, open, staff, user};
    use crate::plan::{Reassign, SyncOptions, plan};
    use crate::test_db;

    fn sample_plan() -> Plan {
        let jane = user("jane", "MUR", Some("10"), Some(1402));
        let bob = user("bob", "LTN", None, Some(1403));
        let matt = user("matt", "ORE", Some("12"), Some(12));
        let bstair = user("bstair", "RIV", Some("1"), Some(1));
        let tasks = HashMap::from([open(&jane, &["t1", "t2"]), open(&bob, &["t3"])]);
        let mut entries = staff();
        entries.extend([
            found(jane, "10", false),
            found(bob, "8", false),
            found(matt, "12", true),
            found(bstair, "1", true),
        ]);
        plan(
            &entries,
            &tasks,
            &SyncOptions {
                reassign: vec![Reassign {
                    from: "jane@pclaptops.com".into(),
                    to: "staff1@pclaptops.com".into(),
                }],
                orphan_tasks: vec!["bob@pclaptops.com".into()],
                ..SyncOptions::default()
            },
        )
    }

    #[test]
    fn writes_cover_deactivations_and_moves_but_not_held_war() {
        let plan = sample_plan();
        assert!(plan.refused.is_empty(), "{:?}", plan.refused);
        let writes = writes(&plan);
        let emails: Vec<&str> = writes.iter().map(Write::email).collect();
        assert_eq!(
            emails,
            vec![
                "bob@pclaptops.com",
                "jane@pclaptops.com",
                "matt@pclaptops.com"
            ]
        );
        assert!(!sets_war(
            writes.iter().map(Write::forward).collect::<Vec<_>>().iter()
        ));
        let Write::Deactivate { tasks, prior, .. } = &writes[1] else {
            panic!("jane is a deactivation");
        };
        assert_eq!(
            prior,
            &RowState {
                active: true,
                store: "MUR".into(),
                id_store: Some("10".into())
            }
        );
        assert_eq!(
            tasks,
            &TaskHandling::Reassign {
                to: RecordId::new("user", "staff1"),
                to_email: "staff1@pclaptops.com".into(),
                tasks: vec![RecordId::new("task", "t1"), RecordId::new("task", "t2")],
            }
        );
        assert!(
            matches!(&writes[0], Write::Deactivate { tasks: TaskHandling::Orphan { tasks }, .. } if tasks.len() == 1)
        );
    }

    #[test]
    fn apply_sql_guards_every_write_inside_one_transaction() {
        let writes = writes(&sample_plan());
        let stmt = Statement::apply(&writes);
        let lines: Vec<&str> = stmt.sql.lines().collect();
        assert_eq!(lines.first(), Some(&"BEGIN TRANSACTION;"));
        assert_eq!(lines.last(), Some(&"COMMIT TRANSACTION;"));
        assert_eq!(stmt.sql.matches("BEGIN TRANSACTION;").count(), 1);
        assert_eq!(stmt.sql.matches("COMMIT TRANSACTION;").count(), 1);

        for i in 0..writes.len() {
            let update = lines
                .iter()
                .find(|l| l.starts_with(&format!("LET $row{i} = (UPDATE user SET ")))
                .expect("guarded update");
            assert!(update.contains(&format!(
                "WHERE id = $id{i} AND active = true AND store = $was_store{i} AND id_store = $was_shop{i} RETURN id"
            )));
            assert!(lines.contains(&format!(
                "IF array::len($row{i}) != 1 {{ THROW string::concat('{STALE_USER}: ', $email{i}) }};"
            ).as_str()));
            for var in ["id", "email", "was_store", "was_shop"] {
                assert!(stmt.vars.contains_key(&format!("{var}{i}")), "{var}{i}");
            }
        }
        assert!(
            stmt.sql
                .contains("UPDATE user SET active = false WHERE id = $id0 ")
        );
        assert!(
            stmt.sql
                .contains("UPDATE user SET store = $store2, id_store = $shop2 WHERE id = $id2 ")
        );
        assert_eq!(stmt.vars["store2"], "SAN".to_string().into_value());
        assert_eq!(stmt.vars["shop2"], Some("12".to_string()).into_value());
        assert_eq!(stmt.vars["was_shop0"], None::<String>.into_value());
        assert_eq!(stmt.vars["id1"], RecordId::new("user", "jane").into_value());

        assert!(stmt.sql.contains(
            "LET $moved1 = (UPDATE task SET assignee = $to1 WHERE id IN $tasks1 AND assignee = $id1 AND completed = false RETURN id);"
        ));
        assert!(stmt.sql.contains("IF array::len($target1) != 1 "));
        assert!(stmt.sql.contains("IF array::len($open1) != 0 "));
        assert!(
            !stmt.sql.contains("$open0"),
            "orphaned tasks are not checked"
        );
        assert!(!stmt.sql.contains("$open2"), "moves do not check tasks");
        for var in ["to1", "to_email1", "tasks1"] {
            assert!(stmt.vars.contains_key(var), "{var}");
        }
        assert!(!stmt.sql.contains("DELETE") && !stmt.sql.contains("CREATE"));
        assert!(!stmt.sql.contains("email ="), "never writes email");
    }

    #[test]
    fn inverse_expects_the_forward_result_and_restores_the_prior_row() {
        for write in writes(&sample_plan()) {
            let forward = write.forward();
            let inverse = write.inverse();
            let after = match &forward.set {
                RowSet::Active(active) => RowState {
                    active: *active,
                    ..forward.expect.clone()
                },
                RowSet::Store { store, id_store } => RowState {
                    store: store.clone(),
                    id_store: id_store.clone(),
                    ..forward.expect.clone()
                },
            };
            assert_eq!(inverse.expect, after, "{}", write.email());
            let restored = match &inverse.set {
                RowSet::Active(active) => RowState {
                    active: *active,
                    ..inverse.expect.clone()
                },
                RowSet::Store { store, id_store } => RowState {
                    store: store.clone(),
                    id_store: id_store.clone(),
                    ..inverse.expect.clone()
                },
            };
            assert_eq!(restored, forward.expect, "{}", write.email());
        }
    }

    #[test]
    fn revert_sql_guards_post_apply_values_and_moves_open_tasks_back() {
        let writes = writes(&sample_plan());
        let stmt = Statement::revert(&writes);
        assert!(stmt.sql.starts_with("BEGIN TRANSACTION;\n"));
        assert!(stmt.sql.ends_with("\nCOMMIT TRANSACTION;"));
        assert!(stmt.sql.contains(
            "UPDATE user SET active = true WHERE id = $id1 AND active = false AND store = $was_store1"
        ));
        assert!(stmt.sql.contains(
            "UPDATE user SET store = $store2, id_store = $shop2 WHERE id = $id2 AND active = true AND store = $was_store2"
        ));
        assert_eq!(stmt.vars["was_store2"], "SAN".to_string().into_value());
        assert_eq!(stmt.vars["store2"], "ORE".to_string().into_value());
        assert!(stmt.sql.contains(
            "UPDATE task SET assignee = $id1 WHERE id IN $tasks1 AND assignee = $to1 AND completed = false;"
        ));
        assert!(!stmt.sql.contains("$tasks0"), "orphaned tasks never moved");
        assert!(stmt.sql.contains(REVERT_STALE));
    }

    #[test]
    fn reverting_a_move_off_war_sets_war() {
        let write = Write::MoveStore {
            id: RecordId::new("user", "w"),
            email: "w@pclaptops.com".into(),
            prior: RowState {
                active: true,
                store: "WAR".into(),
                id_store: Some("7".into()),
            },
            store: "RIV".into(),
            id_store: "7".into(),
        };
        assert!(!sets_war([&write.forward()]));
        assert!(sets_war([&write.inverse()]));
    }

    #[test]
    fn the_record_round_trips_through_json() {
        let plan = sample_plan();
        let record = SyncRecord {
            format: 1,
            created_at: "2026-09-26T10:00:00Z".into(),
            committed_at: Some("2026-09-26T10:00:01Z".into()),
            reverted_at: None,
            target: Target {
                url: "127.0.0.1:8000".into(),
                local: true,
                ns: "ns".into(),
                db: "db".into(),
            },
            writes: writes(&plan),
            plan,
        };
        let json = serde_json::to_string_pretty(&record).expect("serialize");
        let back: SyncRecord = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, record);
    }

    async fn state(
        db: &surrealdb::Surreal<surrealdb::engine::local::Db>,
    ) -> Vec<(String, bool, String, Option<String>)> {
        let rows: Vec<crate::plan::UserRow> = crate::source::read_users(db).await.expect("users");
        rows.into_iter()
            .map(|u| (u.email, u.active, u.store, u.id_store))
            .collect()
    }

    async fn assignees(
        db: &surrealdb::Surreal<surrealdb::engine::local::Db>,
    ) -> Vec<(String, String)> {
        let rows: Vec<serde_json::Value> = db
            .query("SELECT record::id(id) AS task, record::id(assignee) AS user FROM task ORDER BY task")
            .await
            .expect("query")
            .take(0)
            .expect("rows");
        rows.into_iter()
            .map(|r| {
                (
                    r["task"].as_str().unwrap_or_default().to_string(),
                    r["user"].as_str().unwrap_or_default().to_string(),
                )
            })
            .collect()
    }

    async fn seeded() -> surrealdb::Surreal<surrealdb::engine::local::Db> {
        let db = test_db::schema_db().await;
        for (key, store, id_store) in [
            ("staff1", "RIV", Some("7")),
            ("jane", "MUR", Some("10")),
            ("bob", "LTN", None),
            ("matt", "ORE", Some("12")),
        ] {
            test_db::insert_user(&db, key, store, id_store, true).await;
        }
        for (task, assignee, completed) in [
            ("t1", "jane", false),
            ("t2", "jane", false),
            ("t3", "bob", false),
            ("t9", "jane", true),
        ] {
            test_db::insert_task(&db, task, assignee, completed).await;
        }
        db
    }

    fn live_writes() -> Vec<Write> {
        let prior = |store: &str, id_store: Option<&str>| RowState {
            active: true,
            store: store.into(),
            id_store: id_store.map(str::to_string),
        };
        vec![
            Write::Deactivate {
                id: RecordId::new("user", "bob"),
                email: "bob@pclaptops.com".into(),
                prior: prior("LTN", None),
                tasks: TaskHandling::Orphan {
                    tasks: vec![RecordId::new("task", "t3")],
                },
            },
            Write::Deactivate {
                id: RecordId::new("user", "jane"),
                email: "jane@pclaptops.com".into(),
                prior: prior("MUR", Some("10")),
                tasks: TaskHandling::Reassign {
                    to: RecordId::new("user", "staff1"),
                    to_email: "staff1@pclaptops.com".into(),
                    tasks: vec![RecordId::new("task", "t1"), RecordId::new("task", "t2")],
                },
            },
            Write::MoveStore {
                id: RecordId::new("user", "matt"),
                email: "matt@pclaptops.com".into(),
                prior: prior("ORE", Some("12")),
                store: "SAN".into(),
                id_store: "12".into(),
            },
        ]
    }

    #[tokio::test]
    async fn apply_then_revert_round_trips_against_the_user_schema() {
        let db = seeded().await;
        let before = state(&db).await;
        let tasks_before = assignees(&db).await;

        crate::source::run(&db, Statement::apply(&live_writes()))
            .await
            .expect("apply");
        let after = state(&db).await;
        assert_eq!(
            after,
            vec![
                ("bob@pclaptops.com".into(), false, "LTN".into(), None),
                (
                    "jane@pclaptops.com".into(),
                    false,
                    "MUR".into(),
                    Some("10".into())
                ),
                (
                    "matt@pclaptops.com".into(),
                    true,
                    "SAN".into(),
                    Some("12".into())
                ),
                (
                    "staff1@pclaptops.com".into(),
                    true,
                    "RIV".into(),
                    Some("7".into())
                ),
            ]
        );
        assert_eq!(
            assignees(&db).await,
            vec![
                ("t1".into(), "staff1".into()),
                ("t2".into(), "staff1".into()),
                ("t3".into(), "bob".into()),
                ("t9".into(), "jane".into()),
            ]
        );

        crate::source::run(&db, Statement::revert(&live_writes()))
            .await
            .expect("revert");
        assert_eq!(state(&db).await, before);
        assert_eq!(assignees(&db).await, tasks_before);
    }

    #[tokio::test]
    async fn a_stale_row_aborts_the_whole_transaction() {
        let db = seeded().await;
        db.query("UPDATE user SET store = 'LTN' WHERE email = 'matt@pclaptops.com'")
            .await
            .expect("drift")
            .check()
            .expect("drift statement");
        let before = state(&db).await;
        let tasks_before = assignees(&db).await;

        let err = crate::source::run(&db, Statement::apply(&live_writes()))
            .await
            .expect_err("stale plan");
        assert!(
            err.to_string()
                .contains(&format!("{STALE_USER}: matt@pclaptops.com")),
            "{err:#}"
        );
        assert!(err.downcast_ref::<crate::source::Aborted>().is_some());
        assert_eq!(state(&db).await, before);
        assert_eq!(assignees(&db).await, tasks_before);
    }

    #[tokio::test]
    async fn a_new_open_task_aborts_the_deactivation() {
        let db = seeded().await;
        test_db::insert_task(&db, "t4", "jane", false).await;
        let err = crate::source::run(&db, Statement::apply(&live_writes()))
            .await
            .expect_err("new task");
        assert!(
            err.to_string()
                .contains(&format!("{STALE_TASKS}: jane@pclaptops.com")),
            "{err:#}"
        );
        assert!(state(&db).await.iter().all(|(_, active, _, _)| *active));
    }

    #[tokio::test]
    async fn an_inactive_reassign_target_aborts() {
        let db = seeded().await;
        db.query("UPDATE user SET active = false WHERE email = 'staff1@pclaptops.com'")
            .await
            .expect("deactivate target")
            .check()
            .expect("deactivate target statement");
        let err = crate::source::run(&db, Statement::apply(&live_writes()))
            .await
            .expect_err("inactive target");
        assert!(
            err.to_string()
                .contains(&format!("{INACTIVE_TARGET}: staff1@pclaptops.com")),
            "{err:#}"
        );
    }

    #[tokio::test]
    async fn revert_refuses_rows_changed_after_the_sync() {
        let db = seeded().await;
        crate::source::run(&db, Statement::apply(&live_writes()))
            .await
            .expect("apply");
        db.query("UPDATE user SET store = 'RIV' WHERE email = 'matt@pclaptops.com'")
            .await
            .expect("later edit")
            .check()
            .expect("later edit statement");
        let after = state(&db).await;
        let err = crate::source::run(&db, Statement::revert(&live_writes()))
            .await
            .expect_err("stale revert");
        assert!(
            err.to_string()
                .contains(&format!("{REVERT_STALE}: matt@pclaptops.com")),
            "{err:#}"
        );
        assert_eq!(state(&db).await, after);
    }
}
