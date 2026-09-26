//! Reads users, open tasks and employee records; runs a built statement.

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result};
use database::schema::{EmployeeDirectory, RecordId, SurrealValue};
use serde::Deserialize;
use surrealdb::types::QueryError;
use surrealdb::{Connection, Surreal};

use crate::apply::Statement;
use crate::plan::{Employee, Entry, LookupMethod, OpenTask, UserRow, email_key, rid};

const USERS_SQL: &str = "SELECT id, email, name, active, authorization, store, id_store, id_prestashop FROM user ORDER BY email;";
const OPEN_TASKS_SQL: &str =
    "SELECT id, assignee, service_number FROM task WHERE completed == false AND assignee IN $ids;";

pub async fn read_users<C: Connection>(db: &Surreal<C>) -> Result<Vec<UserRow>> {
    let mut response = db
        .query(USERS_SQL)
        .await
        .context("read users")?
        .check()
        .context("read users")?;
    response.take(0).context("decode user rows")
}

#[derive(Debug, Deserialize, SurrealValue)]
struct OpenTaskRow {
    id: RecordId,
    assignee: RecordId,
    service_number: Option<String>,
}

/// Open tasks (`completed == false`) keyed by the assignee's `table:key`.
pub async fn read_open_tasks<C: Connection>(
    db: &Surreal<C>,
    ids: Vec<RecordId>,
) -> Result<HashMap<String, Vec<OpenTask>>> {
    let mut response = db
        .query(OPEN_TASKS_SQL)
        .bind(("ids", ids))
        .await
        .context("read open tasks")?
        .check()
        .context("read open tasks")?;
    let rows: Vec<OpenTaskRow> = response.take(0).context("decode open tasks")?;
    let mut by_user: HashMap<String, Vec<OpenTask>> = HashMap::new();
    for row in rows {
        by_user
            .entry(rid(&row.assignee))
            .or_default()
            .push(OpenTask {
                id: row.id,
                service_number: row.service_number,
            });
    }
    for tasks in by_user.values_mut() {
        tasks.sort_by_key(|task| task.service_number.clone());
    }
    Ok(by_user)
}

/// Looks every user not in `skip` up by `id_prestashop`, else by email; the first directory error aborts.
pub async fn look_up<D: EmployeeDirectory>(
    dir: &D,
    users: Vec<UserRow>,
    skip: &[String],
) -> Result<Vec<Entry>> {
    let skip: HashSet<String> = skip.iter().map(|email| email_key(email)).collect();
    let mut entries = Vec::with_capacity(users.len());
    for user in users {
        let key = email_key(&user.email);
        if skip.contains(&key) {
            entries.push(Entry {
                user,
                method: LookupMethod::Skipped,
                employee: None,
            });
            continue;
        }
        let (method, result) = match user.id_prestashop {
            Some(id) => (LookupMethod::Id, dir.by_id(id).await),
            None => (LookupMethod::Email, dir.by_email(&key).await),
        };
        let employee = result.with_context(|| {
            format!(
                "employee lookup for {} failed; nothing was planned (pass --skip {key} to leave this user out)",
                user.email
            )
        })?;
        entries.push(Entry {
            employee: employee.as_ref().map(Employee::from),
            user,
            method,
        });
    }
    Ok(entries)
}

/// A statement error the database answered with; the transaction wrote nothing.
#[derive(Debug)]
pub struct Aborted(pub String);

impl std::fmt::Display for Aborted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "transaction aborted, nothing was written: {}", self.0)
    }
}

impl std::error::Error for Aborted {}

/// Runs `stmt`; a failed transaction is an [`Aborted`] carrying its THROW text.
pub async fn run<C: Connection>(db: &Surreal<C>, stmt: Statement) -> Result<()> {
    let mut response = db
        .query(stmt.sql)
        .bind(stmt.vars)
        .await
        .context("no answer from the database; check whether the transaction committed")?;
    let mut errors: Vec<(usize, surrealdb::Error)> = response.take_errors().into_iter().collect();
    if errors.is_empty() {
        return Ok(());
    }
    errors.sort_by_key(|(index, _)| *index);
    let not_executed =
        |e: &surrealdb::Error| matches!(e.query_details(), Some(QueryError::NotExecuted));
    let chosen = errors
        .iter()
        .find(|(_, e)| e.is_thrown())
        .or_else(|| errors.iter().find(|(_, e)| !not_executed(e)))
        .or_else(|| errors.first())
        .map(|(_, e)| e.message().to_string())
        .unwrap_or_default();
    Err(Aborted(chosen).into())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use database::schema::{EmployeeDirectory, EmployeeRecord, RecordId};

    use super::*;
    use crate::plan::tests::user;
    use crate::test_db;

    struct FakeDirectory {
        by_id: HashMap<u64, EmployeeRecord>,
        by_email: HashMap<String, EmployeeRecord>,
        fail_on: Option<String>,
    }

    impl EmployeeDirectory for FakeDirectory {
        async fn by_email(&self, email: &str) -> anyhow::Result<Option<EmployeeRecord>> {
            if self.fail_on.as_deref() == Some(email) {
                anyhow::bail!("HTTP 503");
            }
            Ok(self.by_email.get(email).cloned())
        }

        async fn by_id(&self, id: u64) -> anyhow::Result<Option<EmployeeRecord>> {
            Ok(self.by_id.get(&id).cloned())
        }
    }

    fn record(id: u64, email: &str, active: bool) -> EmployeeRecord {
        EmployeeRecord {
            id,
            email: email.into(),
            first_name: "First".into(),
            last_name: "Last".into(),
            id_store: "7".into(),
            active,
        }
    }

    fn directory(fail_on: Option<&str>) -> FakeDirectory {
        FakeDirectory {
            by_id: HashMap::from([(5, record(5, "linked@pclaptops.com", false))]),
            by_email: HashMap::from([(
                "dakota@pclaptops.com".to_string(),
                record(77, "dakota@pclaptops.com", true),
            )]),
            fail_on: fail_on.map(str::to_string),
        }
    }

    #[tokio::test]
    async fn lookups_use_the_id_when_set_and_the_email_otherwise() {
        let mut dakota = user("dakota", "RIV", Some("7"), None);
        dakota.email = " Dakota@PCLaptops.com ".into();
        let users = vec![
            user("linked", "RIV", Some("7"), Some(5)),
            dakota,
            user("nobody", "RIV", Some("7"), Some(6)),
        ];
        let entries = look_up(&directory(None), users, &[])
            .await
            .expect("lookups");
        let summary: Vec<(LookupMethod, Option<u64>)> = entries
            .iter()
            .map(|e| (e.method, e.employee.as_ref().map(|emp| emp.id)))
            .collect();
        assert_eq!(
            summary,
            vec![
                (LookupMethod::Id, Some(5)),
                (LookupMethod::Email, Some(77)),
                (LookupMethod::Id, None),
            ]
        );
        assert_eq!(
            entries[0].employee.as_ref().map(|e| e.name.as_str()),
            Some("First Last")
        );
    }

    #[tokio::test]
    async fn a_directory_error_aborts_before_planning() {
        let users = vec![
            user("linked", "RIV", Some("7"), Some(5)),
            user("dakota", "RIV", Some("7"), None),
        ];
        let err = look_up(&directory(Some("dakota@pclaptops.com")), users, &[])
            .await
            .expect_err("directory outage");
        assert!(err.to_string().contains("dakota@pclaptops.com"), "{err:#}");
        assert!(
            err.to_string().contains("--skip dakota@pclaptops.com"),
            "{err:#}"
        );
    }

    #[tokio::test]
    async fn skipped_users_are_not_looked_up() {
        let mut idle = user("idle", "RIV", None, None);
        idle.active = false;
        idle.email = "Idle@PCLaptops.com".into();
        let users = vec![
            user("linked", "RIV", Some("7"), Some(5)),
            user("dakota", "RIV", Some("7"), None),
            idle,
        ];
        let skip = [
            "dakota@pclaptops.com".to_string(),
            "idle@pclaptops.com".to_string(),
        ];
        let entries = look_up(&directory(Some("dakota@pclaptops.com")), users, &skip)
            .await
            .expect("the failing record is skipped");
        let methods: Vec<LookupMethod> = entries.iter().map(|e| e.method).collect();
        assert_eq!(
            methods,
            vec![
                LookupMethod::Id,
                LookupMethod::Skipped,
                LookupMethod::Skipped
            ]
        );
        assert!(entries[1].employee.is_none() && entries[2].employee.is_none());
    }

    #[tokio::test]
    async fn users_and_open_tasks_decode_from_the_schema() {
        let db = test_db::schema_db().await;
        test_db::insert_user(&db, "jane", "MUR", Some("10"), true).await;
        test_db::insert_user(&db, "zz_livetest", "ZZTest", None, false).await;
        test_db::insert_task(&db, "t1", "jane", false).await;
        test_db::insert_task(&db, "t2", "jane", true).await;
        test_db::insert_task(&db, "t3", "zz_livetest", false).await;

        let users = read_users(&db).await.expect("users");
        let summary: Vec<(&str, bool, &str, Option<&str>)> = users
            .iter()
            .map(|u| {
                (
                    u.email.as_str(),
                    u.active,
                    u.store.as_str(),
                    u.id_store.as_deref(),
                )
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                ("jane@pclaptops.com", true, "MUR", Some("10")),
                ("zz_livetest@pclaptops.com", false, "ZZTest", None),
            ]
        );

        let jane = RecordId::new("user", "jane");
        let open = read_open_tasks(&db, vec![jane]).await.expect("tasks");
        assert_eq!(open.len(), 1);
        assert_eq!(
            open["user:jane"]
                .iter()
                .map(|t| t.id.clone())
                .collect::<Vec<_>>(),
            vec![RecordId::new("task", "t1")]
        );
        assert_eq!(
            open["user:jane"][0].service_number.as_deref(),
            Some("SO-t1")
        );
    }
}
