//! Root account administration: the account list, field edits, activity and open-task handoff.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::password_reset::{LATEST_CODE_SQL, ResetCodeStatus};
use super::{User, UserAuthorization, normalize_email, short_name};
use crate::db;
use crate::schema::{Datetime, RecordId, Status, Store, SurrealValue};

/// Columns the account list reads; never the password, keys or settings.
const ACCOUNT_FIELDS: &str = "id, name, email, active, authorization, store, id_store, id_prestashop, user_statuses, hidden_tabs";

/// One account as the User Admin tab shows and edits it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, SurrealValue)]
pub struct AdminAccount {
    pub id: RecordId,
    pub name: String,
    pub email: String,
    pub active: bool,
    pub authorization: String,
    pub store: String,
    pub id_store: Option<String>,
    pub id_prestashop: Option<u64>,
    pub user_statuses: Option<Vec<String>>,
    pub hidden_tabs: Option<Vec<String>>,
}

impl AdminAccount {
    /// The role, or `None` for a value this build does not know.
    pub fn role(&self) -> Option<UserAuthorization> {
        UserAuthorization::ALL
            .into_iter()
            .find(|role| role.as_str() == self.authorization)
    }

    /// The store, or `None` for an unknown store code.
    pub fn store_code(&self) -> Option<Store> {
        Store::from_code(&self.store)
    }

    pub fn username(&self) -> &str {
        short_name(&self.email)
    }

    pub fn statuses(&self) -> &[String] {
        self.user_statuses.as_deref().unwrap_or_default()
    }

    pub fn hidden(&self) -> &[String] {
        self.hidden_tabs.as_deref().unwrap_or_default()
    }

    /// Whether `query` matches the name, email or store, case-insensitively.
    pub fn matches(&self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        query.is_empty()
            || [&self.name, &self.email, &self.store]
                .iter()
                .any(|field| field.to_lowercase().contains(&query))
    }
}

/// Every account, active first and then by name; rows that fail to decode are logged and skipped.
pub async fn list_accounts() -> anyhow::Result<Vec<AdminAccount>> {
    let rows: Vec<surrealdb::types::Value> = db()
        .query(format!("SELECT {ACCOUNT_FIELDS} FROM user"))
        .await?
        .check()?
        .take(0)?;
    let mut accounts: Vec<AdminAccount> = rows
        .into_iter()
        .filter_map(|row| {
            AdminAccount::from_value(row)
                .inspect_err(|e| log::error!("Skipping an account row that failed to decode: {e}"))
                .ok()
        })
        .collect();
    accounts.sort_by_key(|a| (!a.active, a.name.to_lowercase()));
    Ok(accounts)
}

/// One field change made from the User Admin tab.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountChange {
    Name(String),
    Email(String),
    Active(bool),
    Role(UserAuthorization),
    Store(Store),
    PrestashopId(Option<u64>),
    Statuses(Vec<String>),
    HiddenTabs(Vec<String>),
}

impl AccountChange {
    /// Field name shown in errors and logged in `admin_action`.
    pub fn field(&self) -> &'static str {
        match self {
            Self::Name(_) => "name",
            Self::Email(_) => "email",
            Self::Active(_) => "active",
            Self::Role(_) => "authorization",
            Self::Store(_) => "store",
            Self::PrestashopId(_) => "id_prestashop",
            Self::Statuses(_) => "user_statuses",
            Self::HiddenTabs(_) => "hidden_tabs",
        }
    }

    /// The column assignments this change makes; `None` writes NONE.
    fn assignments(&self) -> Vec<(&'static str, Option<Value>)> {
        match self {
            Self::Name(name) => vec![("name", Some(json!(name)))],
            Self::Email(email) => vec![("email", Some(json!(email)))],
            Self::Active(active) => vec![("active", Some(json!(active)))],
            Self::Role(role) => vec![("authorization", Some(json!(role.as_str())))],
            Self::Store(store) => vec![
                ("store", Some(json!(store.as_str()))),
                ("id_store", Some(json!(store.into_store_id().to_string()))),
            ],
            Self::PrestashopId(id) => vec![("id_prestashop", id.map(|id| json!(id)))],
            Self::Statuses(statuses) => vec![("user_statuses", Some(json!(statuses)))],
            Self::HiddenTabs(tabs) => vec![("hidden_tabs", Some(json!(tabs)))],
        }
    }

    /// Whether `account` already holds this change's value.
    fn holds(&self, account: &AdminAccount) -> bool {
        match self {
            Self::Name(name) => &account.name == name,
            Self::Email(email) => &account.email == email,
            Self::Active(active) => account.active == *active,
            Self::Role(role) => account.authorization == role.as_str(),
            Self::Store(store) => {
                account.store == store.as_str()
                    && account.id_store.as_deref()
                        == Some(store.into_store_id().to_string().as_str())
            }
            Self::PrestashopId(id) => account.id_prestashop == *id,
            Self::Statuses(statuses) => account.statuses() == statuses.as_slice(),
            Self::HiddenTabs(tabs) => account.hidden() == tabs.as_slice(),
        }
    }

    /// The old and new value this change makes on `before`, for the log.
    fn log_entry(&self, before: &AdminAccount) -> Value {
        let old = match self {
            Self::Name(_) => json!(before.name),
            Self::Email(_) => json!(before.email),
            Self::Active(_) => json!(before.active),
            Self::Role(_) => json!(before.authorization),
            Self::Store(_) => json!(before.store),
            Self::PrestashopId(_) => json!(before.id_prestashop),
            Self::Statuses(_) => json!(before.statuses()),
            Self::HiddenTabs(_) => json!(before.hidden()),
        };
        let new = self
            .assignments()
            .into_iter()
            .next()
            .and_then(|(_, value)| value)
            .unwrap_or(Value::Null);
        json!({ "old": old, "new": new })
    }

    /// The change with its value trimmed and checked; `Err` is user-facing.
    fn normalized(self) -> Result<Self, String> {
        Ok(match self {
            Self::Name(name) => {
                let name = name.trim().to_string();
                if name.is_empty() {
                    return Err("Enter a name.".to_string());
                }
                Self::Name(name)
            }
            Self::Email(email) => Self::Email(
                normalize_email(&email)
                    .ok_or_else(|| format!("{:?} is not a valid email.", email.trim()))?,
            ),
            Self::Store(Store::Unknown) => return Err("Pick a store.".to_string()),
            Self::PrestashopId(Some(0)) => Self::PrestashopId(None),
            Self::Statuses(statuses) => Self::Statuses(clean_statuses(statuses)),
            Self::HiddenTabs(tabs) => Self::HiddenTabs(clean_tabs(tabs)),
            other => other,
        })
    }
}

/// Custom statuses trimmed, without blanks, built-in names or case-insensitive repeats.
pub fn clean_statuses(statuses: Vec<String>) -> Vec<String> {
    let built_in: Vec<String> = Status::VALUES
        .iter()
        .map(|s| s.as_str().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    let mut seen: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for status in statuses {
        let status = status.trim().to_string();
        let key = status.to_lowercase();
        if status.is_empty() || built_in.contains(&key) || seen.contains(&key) {
            continue;
        }
        seen.push(key);
        out.push(status);
    }
    out
}

/// Tab slugs trimmed, sorted and deduplicated.
pub fn clean_tabs(tabs: Vec<String>) -> Vec<String> {
    let mut tabs: Vec<String> = tabs
        .into_iter()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();
    tabs.sort();
    tabs.dedup();
    tabs
}

/// The changes that turn `before` into `draft`, in display order.
pub fn diff_accounts(before: &AdminAccount, draft: &AdminAccount) -> Vec<AccountChange> {
    let mut changes = Vec::new();
    if draft.name.trim() != before.name {
        changes.push(AccountChange::Name(draft.name.clone()));
    }
    if draft.email.trim().to_lowercase() != before.email {
        changes.push(AccountChange::Email(draft.email.clone()));
    }
    if draft.active != before.active {
        changes.push(AccountChange::Active(draft.active));
    }
    if draft.authorization != before.authorization
        && let Some(role) = draft.role()
    {
        changes.push(AccountChange::Role(role));
    }
    if draft.store != before.store
        && let Some(store) = draft.store_code()
    {
        changes.push(AccountChange::Store(store));
    }
    if draft.id_prestashop != before.id_prestashop {
        changes.push(AccountChange::PrestashopId(draft.id_prestashop));
    }
    let statuses = clean_statuses(draft.statuses().to_vec());
    if statuses != before.statuses() {
        changes.push(AccountChange::Statuses(statuses));
    }
    let tabs = clean_tabs(draft.hidden().to_vec());
    if tabs != clean_tabs(before.hidden().to_vec()) {
        changes.push(AccountChange::HiddenTabs(tabs));
    }
    changes
}

/// Why an account edit failed; `Display` is user-facing.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AdminError {
    #[error("{0}")]
    Invalid(String),
    #[error("The database refused the change.")]
    Refused,
    #[error("{0}")]
    Failed(String),
}

impl From<surrealdb::Error> for AdminError {
    fn from(err: surrealdb::Error) -> Self {
        Self::Failed(err.to_string())
    }
}

/// The row after an edit, and the fields the database silently kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountUpdate {
    pub account: AdminAccount,
    pub refused: Vec<&'static str>,
}

/// Writes `changes` to `before`'s row and logs the ones that stuck.
pub async fn apply_account_changes(
    before: &AdminAccount,
    changes: Vec<AccountChange>,
) -> Result<AccountUpdate, AdminError> {
    let changes = changes
        .into_iter()
        .map(AccountChange::normalized)
        .collect::<Result<Vec<_>, String>>()
        .map_err(AdminError::Invalid)?;
    if changes.is_empty() {
        return Ok(AccountUpdate {
            account: before.clone(),
            refused: Vec::new(),
        });
    }

    let assignments: Vec<(&'static str, Option<Value>)> = changes
        .iter()
        .flat_map(AccountChange::assignments)
        .collect();
    let set = assignments
        .iter()
        .enumerate()
        .map(|(i, (column, value))| match value {
            Some(_) => format!("{column} = $v{i}"),
            None => format!("{column} = NONE"),
        })
        .collect::<Vec<_>>()
        .join(", ");
    let dbh = db();
    let mut query = dbh
        .query(format!("UPDATE $id SET {set} RETURN {ACCOUNT_FIELDS}"))
        .bind(("id", before.id.clone()));
    for (i, (_, value)) in assignments.into_iter().enumerate() {
        if let Some(value) = value {
            query = query.bind((format!("v{i}"), value));
        }
    }
    let rows: Vec<AdminAccount> = query.await?.check()?.take(0)?;
    let account = rows.into_iter().next().ok_or(AdminError::Refused)?;

    let (applied, refused): (Vec<&AccountChange>, Vec<&AccountChange>) =
        changes.iter().partition(|change| change.holds(&account));
    if !applied.is_empty() {
        let detail: Map<String, Value> = applied
            .iter()
            .map(|change| (change.field().to_string(), change.log_entry(before)))
            .collect();
        log_action(&before.id, "update", Value::Object(detail)).await;
    }
    Ok(AccountUpdate {
        account,
        refused: refused.iter().map(|change| change.field()).collect(),
    })
}

/// Appends an `admin_action` row; a failure is logged, never raised.
pub async fn log_action(target: &RecordId, action: &str, detail: Value) {
    let result = db()
        .query("CREATE admin_action CONTENT { target: $target, action: $action, detail: $detail } RETURN NONE")
        .bind(("target", target.clone()))
        .bind(("action", action.to_string()))
        .bind(("detail", detail))
        .await
        .and_then(|response| response.check());
    if let Err(e) = result {
        log::warn!("admin_action {action} for {target:?} was not logged: {e}");
    }
}

/// A machine an account signed in on.
#[derive(Debug, Clone, PartialEq, Deserialize, SurrealValue)]
pub struct AccountDevice {
    pub connection_string: Option<String>,
    pub friendly_name: Option<String>,
    pub client_version: Option<String>,
    pub last_update: Option<Datetime>,
    pub connected: Option<bool>,
}

/// One `admin_action` row as the tab lists it.
#[derive(Debug, Clone, PartialEq, Deserialize, SurrealValue)]
pub struct AdminActionRow {
    pub created_at: Datetime,
    pub action: String,
    pub detail: Value,
    pub actor_name: Option<String>,
}

/// Sign-ins, machines, open tasks, the newest reset code and admin changes for one account.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AccountActivity {
    pub open_tasks: usize,
    pub last_sign_in: Option<Datetime>,
    pub failed_sign_ins_week: usize,
    pub devices: Vec<AccountDevice>,
    pub reset_code: Option<ResetCodeStatus>,
    pub history: Vec<AdminActionRow>,
}

/// Statement `index` of `response`, or the default with a log line when that statement failed.
fn take_or_default<T>(response: &mut surrealdb::IndexedResults, index: usize, what: &str) -> T
where
    T: Default + SurrealValue,
    usize: surrealdb::opt::QueryResult<T>,
{
    response.take(index).unwrap_or_else(|e| {
        log::warn!("Account activity: {what} failed to load: {e}");
        T::default()
    })
}

/// Loads [`AccountActivity`] for `account` in one round trip; a failed part comes back empty.
pub async fn account_activity(account: &AdminAccount) -> anyhow::Result<AccountActivity> {
    let mut response = db()
        .query(
            "RETURN array::len((SELECT VALUE id FROM task WHERE assignee = $id AND completed = false));",
        )
        .query(
            "RETURN (SELECT time FROM audit_log WHERE signin_attempt.email = $email \
             AND signin_attempt.success = true ORDER BY time DESC LIMIT 1)[0].time;",
        )
        .query(
            "RETURN array::len((SELECT VALUE time FROM audit_log WHERE signin_attempt.email = $email \
             AND signin_attempt.success = false AND time > time::now() - 7d));",
        )
        .query(
            "SELECT connection_string, friendly_name, client_version, last_update, connected \
             FROM connected_client WHERE assigned_user = $id ORDER BY last_update DESC LIMIT 8;",
        )
        .query(LATEST_CODE_SQL)
        .query(
            "SELECT created_at, action, detail, actor.name AS actor_name FROM admin_action \
             WHERE target = $id ORDER BY created_at DESC LIMIT 25;",
        )
        .bind(("id", account.id.clone()))
        .bind(("email", account.email.clone()))
        .await?;
    let open_tasks: Option<i64> = take_or_default(&mut response, 0, "open tasks");
    let last_sign_in: Option<Datetime> = take_or_default(&mut response, 1, "last sign-in");
    let failed: Option<i64> = take_or_default(&mut response, 2, "failed sign-ins");
    let devices: Vec<AccountDevice> = take_or_default(&mut response, 3, "devices");
    let codes: Vec<ResetCodeStatus> = take_or_default(&mut response, 4, "reset code");
    let history: Vec<AdminActionRow> = take_or_default(&mut response, 5, "admin history");
    Ok(AccountActivity {
        open_tasks: open_tasks.unwrap_or_default().max(0) as usize,
        last_sign_in,
        failed_sign_ins_week: failed.unwrap_or_default().max(0) as usize,
        devices,
        reset_code: codes.into_iter().next(),
        history,
    })
}

/// One statement, so the move, its history rows and the log entry commit together.
const REASSIGN_SQL: &str = "RETURN {
    LET $moved = (UPDATE task SET assignee = $to WHERE assignee = $from AND completed = false RETURN id);
    FOR $task IN $moved {
        CREATE task_history CONTENT { task_id: $task.id, user: $actor, username: $username,
            diff: { assignee: { old: <string> record::id($from), new: <string> record::id($to) } },
            from_writer: true };
    };
    CREATE admin_action CONTENT { target: $from, action: 'reassign_tasks',
        detail: { to: $to, count: array::len($moved) } } RETURN NONE;
    array::len($moved)
};";

/// Moves `from`'s open tasks to `to` with a history row each and logs it; returns how many moved.
pub async fn reassign_open_tasks(
    from: &RecordId,
    to: &RecordId,
    actor: &User,
) -> anyhow::Result<usize> {
    let moved: Option<i64> = db()
        .query(REASSIGN_SQL)
        .bind(("from", from.clone()))
        .bind(("to", to.clone()))
        .bind(("actor", actor.get_id()))
        .bind(("username", actor.get_username().to_string()))
        .await?
        .check()?
        .take(0)?;
    Ok(moved.unwrap_or_default().max(0) as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account() -> AdminAccount {
        AdminAccount {
            id: RecordId::new("user", "a"),
            name: "Amy Adams".into(),
            email: "amy.adams@pclaptops.com".into(),
            active: true,
            authorization: "User".into(),
            store: "RIV".into(),
            id_store: Some("7".into()),
            id_prestashop: Some(12),
            user_statuses: Some(vec!["Waiting".into()]),
            hidden_tabs: None,
        }
    }

    #[test]
    fn an_unchanged_draft_has_no_changes() {
        let before = account();
        assert!(diff_accounts(&before, &before.clone()).is_empty());
    }

    #[test]
    fn every_edited_field_becomes_a_change() {
        let before = account();
        let mut draft = before.clone();
        draft.name = "Amy B".into();
        draft.email = "Amy.B@pclaptops.com".into();
        draft.active = false;
        draft.authorization = "Manager".into();
        draft.store = "LTN".into();
        draft.id_prestashop = None;
        draft.user_statuses = Some(vec!["Waiting".into(), "Parts".into()]);
        draft.hidden_tabs = Some(vec!["terminal".into()]);
        let fields: Vec<&str> = diff_accounts(&before, &draft)
            .iter()
            .map(AccountChange::field)
            .collect();
        assert_eq!(
            fields,
            [
                "name",
                "email",
                "active",
                "authorization",
                "store",
                "id_prestashop",
                "user_statuses",
                "hidden_tabs"
            ]
        );
    }

    #[test]
    fn unknown_roles_and_stores_are_not_written() {
        let before = account();
        let mut draft = before.clone();
        draft.authorization = "Admin".into();
        draft.store = "XYZ".into();
        assert!(diff_accounts(&before, &draft).is_empty());
    }

    #[test]
    fn whitespace_and_case_only_edits_are_not_changes() {
        let before = account();
        let mut draft = before.clone();
        draft.name = " Amy Adams ".into();
        draft.email = "AMY.ADAMS@pclaptops.com".into();
        draft.user_statuses = Some(vec![" Waiting ".into(), "waiting".into()]);
        assert!(diff_accounts(&before, &draft).is_empty());
    }

    #[test]
    fn statuses_drop_blanks_built_ins_and_repeats() {
        let cleaned = clean_statuses(vec![
            " Parts ".into(),
            "".into(),
            "todo".into(),
            "In Repair".into(),
            "parts".into(),
            "Waiting on customer".into(),
        ]);
        assert_eq!(cleaned, ["Parts", "Waiting on customer"]);
    }

    #[test]
    fn tabs_are_sorted_and_deduplicated() {
        assert_eq!(
            clean_tabs(vec![
                "terminal".into(),
                " ai ".into(),
                "terminal".into(),
                "".into()
            ]),
            ["ai", "terminal"]
        );
    }

    #[test]
    fn a_store_change_writes_both_columns() {
        let change = AccountChange::Store(Store::SAN);
        let columns = change.assignments();
        assert_eq!(
            columns,
            vec![
                ("store", Some(json!("SAN"))),
                ("id_store", Some(json!("12")))
            ]
        );
        let mut moved = account();
        moved.store = "SAN".into();
        assert!(!change.holds(&moved), "id_store still says RIV");
        moved.id_store = Some("12".into());
        assert!(change.holds(&moved));
    }

    #[test]
    fn normalizing_checks_names_emails_and_stores() {
        assert_eq!(
            AccountChange::Email(" Bob ".into()).normalized(),
            Ok(AccountChange::Email("bob@pclaptops.com".into()))
        );
        assert!(AccountChange::Email("a b".into()).normalized().is_err());
        assert!(AccountChange::Name("  ".into()).normalized().is_err());
        assert!(AccountChange::Store(Store::Unknown).normalized().is_err());
        assert_eq!(
            AccountChange::PrestashopId(Some(0)).normalized(),
            Ok(AccountChange::PrestashopId(None))
        );
    }

    #[test]
    fn clearing_the_prestashop_id_writes_none() {
        assert_eq!(
            AccountChange::PrestashopId(None).assignments(),
            vec![("id_prestashop", None)]
        );
    }

    #[test]
    fn the_log_records_old_and_new_values() {
        let before = account();
        assert_eq!(
            AccountChange::Store(Store::LTN).log_entry(&before),
            json!({ "old": "RIV", "new": "LTN" })
        );
        assert_eq!(
            AccountChange::Active(false).log_entry(&before),
            json!({ "old": true, "new": false })
        );
    }

    #[test]
    fn search_matches_name_email_and_store() {
        let a = account();
        assert!(a.matches(""));
        assert!(a.matches("amy"));
        assert!(a.matches("PCLAPTOPS"));
        assert!(a.matches("riv"));
        assert!(!a.matches("ltn"));
    }

    #[test]
    fn known_roles_and_stores_resolve() {
        let mut a = account();
        assert_eq!(a.role(), Some(UserAuthorization::User));
        assert_eq!(a.store_code(), Some(Store::RIV));
        a.authorization = "Admin".into();
        a.store = "nowhere".into();
        assert_eq!(a.role(), None);
        assert_eq!(a.store_code(), None);
    }
}
