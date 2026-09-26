//! Pure `sync` planning: user rows plus employee lookups in, a sectioned write plan out.

use std::collections::{HashMap, HashSet};
use std::fmt;

use database::schema::{EmployeeRecord, RecordId, RecordIdExt, Store, SurrealValue};
use serde::{Deserialize, Serialize};

/// One `user` row as sync reads it, with `store` as the raw string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, SurrealValue)]
pub struct UserRow {
    pub id: RecordId,
    pub email: String,
    pub name: String,
    pub active: bool,
    pub authorization: String,
    pub store: String,
    pub id_store: Option<String>,
    pub id_prestashop: Option<u64>,
}

/// `table:key` text of a record id.
pub fn rid(id: &RecordId) -> String {
    format!("{}:{}", id.table, id.key_string())
}

/// Trimmed, lowercased email used to match users, flags and employees.
pub fn email_key(email: &str) -> String {
    email.trim().to_lowercase()
}

/// Lowercased name with whitespace runs collapsed.
fn name_key(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn is_root(authorization: &str) -> bool {
    authorization.trim().eq_ignore_ascii_case("Root")
}

/// Employee fields the plan compares and reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Employee {
    pub id: u64,
    pub email: String,
    pub name: String,
    pub id_store: String,
    pub active: bool,
}

impl Employee {
    fn store(&self) -> Option<Store> {
        Store::try_from_presta_store_id(self.id_store.trim())
    }
}

impl From<&EmployeeRecord> for Employee {
    fn from(record: &EmployeeRecord) -> Self {
        Self {
            id: record.id,
            email: record.email.clone(),
            name: record.name(),
            id_store: record.id_store.trim().to_string(),
            active: record.active,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LookupMethod {
    Id,
    Email,
    /// Not looked up (`--skip`).
    Skipped,
}

/// A user with the result of its directory lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub user: UserRow,
    pub method: LookupMethod,
    pub employee: Option<Employee>,
}

/// An open task assigned to a user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenTask {
    pub id: RecordId,
    pub service_number: Option<String>,
}

/// `--reassign FROM=TO`, both normalized emails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reassign {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Default)]
pub struct SyncOptions {
    pub allow_war: bool,
    pub deactivate_missing: Vec<String>,
    pub skip: Vec<String>,
    pub reassign: Vec<Reassign>,
    pub orphan_tasks: Vec<String>,
    pub max_deactivations: Option<usize>,
    pub trust_id: Vec<String>,
}

/// A user as the plan reports them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserRef {
    pub id: RecordId,
    pub email: String,
    pub name: String,
    pub authorization: String,
    pub active: bool,
    pub store: String,
    pub id_store: Option<String>,
}

impl UserRef {
    pub fn is_root(&self) -> bool {
        is_root(&self.authorization)
    }
}

impl From<&UserRow> for UserRef {
    fn from(user: &UserRow) -> Self {
        Self {
            id: user.id.clone(),
            email: user.email.clone(),
            name: user.name.clone(),
            authorization: user.authorization.clone(),
            active: user.active,
            store: user.store.clone(),
            id_store: user.id_store.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DeactivateReason {
    PrestashopInactive { employee_id: u64 },
    NamedMissing,
}

/// What happens to a deactivated user's open tasks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TaskAction {
    Untouched,
    Reassign { to: RecordId, to_email: String },
    Orphan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Deactivation {
    pub user: UserRef,
    pub reason: DeactivateReason,
    pub open_tasks: Vec<OpenTask>,
    pub tasks: TaskAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreMove {
    pub user: UserRef,
    pub method: LookupMethod,
    pub employee_id: u64,
    pub store: Store,
    pub id_store: String,
    pub open_tasks: Vec<OpenTask>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Unmapped {
    pub user: UserRef,
    pub employee_id: u64,
    pub id_store: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotFound {
    pub user: UserRef,
    pub method: LookupMethod,
}

/// A user with the employee record it matched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Found {
    pub user: UserRef,
    pub employee: Employee,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DuplicateName {
    pub name: String,
    pub emails: Vec<String>,
}

/// Findings that are printed and never written.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub inactive_but_employed: Vec<Found>,
    pub email_drift: Vec<Found>,
    pub name_drift: Vec<Found>,
    pub invalid_store: Vec<UserRef>,
    pub duplicate_names: Vec<DuplicateName>,
    pub unlinked_matches: Vec<Found>,
}

/// A reason `--apply` is blocked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Refusal {
    LastActiveRoot {
        emails: Vec<String>,
    },
    TooManyDeactivations {
        planned: usize,
        max: usize,
    },
    NoSuchUser {
        flag: String,
        email: String,
    },
    NotMissing {
        email: String,
    },
    SkippedAndNamed {
        email: String,
    },
    OpenTasks {
        email: String,
        count: usize,
    },
    NotDeactivated {
        flag: String,
        email: String,
    },
    ReassignTarget {
        from: String,
        to: String,
        problem: String,
    },
    ConflictingTaskFlags {
        email: String,
    },
    IdEmailMismatch {
        email: String,
        employee_id: u64,
        employee_email: String,
        withheld: String,
    },
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LastActiveRoot { emails } => write!(
                f,
                "deactivating {} would leave no active Root user",
                emails.join(", ")
            ),
            Self::TooManyDeactivations { planned, max } => write!(
                f,
                "{planned} deactivations exceed the limit of {max}; pass --max-deactivations to raise it"
            ),
            Self::NoSuchUser { flag, email } => write!(f, "{flag} {email} matches no user"),
            Self::NotMissing { email } => write!(
                f,
                "--deactivate-missing {email} is not an active user missing from the directory"
            ),
            Self::SkippedAndNamed { email } => {
                write!(
                    f,
                    "{email} is named by both --skip and --deactivate-missing"
                )
            }
            Self::OpenTasks { email, count } => write!(
                f,
                "{email} has {count} open task(s); pass --reassign {email}=<active user> or --orphan-tasks {email}"
            ),
            Self::NotDeactivated { flag, email } => {
                write!(f, "{flag} {email} is not planned for deactivation")
            }
            Self::ReassignTarget { from, to, problem } => {
                write!(f, "--reassign {from}={to}: {to} {problem}")
            }
            Self::ConflictingTaskFlags { email } => write!(
                f,
                "{email} has more than one --reassign/--orphan-tasks instruction"
            ),
            Self::IdEmailMismatch {
                email,
                employee_id,
                employee_email,
                withheld,
            } => write!(
                f,
                "{email} is linked to ps#{employee_id}, whose email is {employee_email}; the {withheld} is withheld. Fix the link, or pass --skip {email} or --trust-id {email}"
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub users: usize,
    pub active_users: usize,
    pub by_id: usize,
    pub by_email: usize,
    pub allow_war: bool,
    pub max_deactivations: usize,
    pub deactivate: Vec<Deactivation>,
    pub move_store: Vec<StoreMove>,
    pub held_war: Vec<StoreMove>,
    pub unmapped: Vec<Unmapped>,
    pub not_found: Vec<NotFound>,
    pub skipped: Vec<UserRef>,
    pub report: Report,
    pub refused: Vec<Refusal>,
}

impl Plan {
    /// True when the plan writes nothing.
    pub fn is_empty(&self) -> bool {
        self.deactivate.is_empty() && self.move_store.is_empty()
    }
}

/// Unique normalized emails in first-seen order.
fn unique_keys(emails: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    emails
        .iter()
        .map(|email| email_key(email))
        .filter(|key| seen.insert(key.clone()))
        .collect()
}

/// Refusal for a write that rests on a record whose email differs from the user's.
fn mismatch(email: &str, employee: &Employee, withheld: String) -> Refusal {
    Refusal::IdEmailMismatch {
        email: email.to_string(),
        employee_id: employee.id,
        employee_email: employee.email.clone(),
        withheld,
    }
}

/// Builds the sync plan; performs no I/O.
pub fn plan(
    entries: &[Entry],
    open_tasks: &HashMap<String, Vec<OpenTask>>,
    opts: &SyncOptions,
) -> Plan {
    let users: HashMap<String, &UserRow> = entries
        .iter()
        .map(|entry| (email_key(&entry.user.email), &entry.user))
        .collect();
    let skip = unique_keys(&opts.skip);
    let named = unique_keys(&opts.deactivate_missing);
    let orphan = unique_keys(&opts.orphan_tasks);
    let trust = unique_keys(&opts.trust_id);
    let skip_set: HashSet<&str> = skip.iter().map(String::as_str).collect();
    let trust_set: HashSet<&str> = trust.iter().map(String::as_str).collect();
    let tasks_of = |id: &RecordId| open_tasks.get(&rid(id)).cloned().unwrap_or_default();

    let mut refused = Vec::new();
    for (flag, emails) in [("--skip", &skip), ("--trust-id", &trust)] {
        for email in emails {
            if !users.contains_key(email) {
                refused.push(Refusal::NoSuchUser {
                    flag: flag.into(),
                    email: email.clone(),
                });
            }
        }
    }

    let mut report = Report::default();
    let mut candidates: Vec<(UserRef, DeactivateReason)> = Vec::new();
    let mut move_store = Vec::new();
    let mut held_war = Vec::new();
    let mut unmapped = Vec::new();
    let mut not_found = Vec::new();
    let mut skipped = Vec::new();

    for entry in entries {
        let user = &entry.user;
        let key = email_key(&user.email);
        let user_ref = UserRef::from(user);
        if Store::from_code(&user.store).is_none() {
            report.invalid_store.push(user_ref.clone());
        }
        if let Some(employee) = &entry.employee {
            let found = || Found {
                user: user_ref.clone(),
                employee: employee.clone(),
            };
            if email_key(&employee.email) != key {
                report.email_drift.push(found());
            }
            if name_key(&employee.name) != name_key(&user.name) {
                report.name_drift.push(found());
            }
            if user.id_prestashop.is_none() {
                report.unlinked_matches.push(found());
            }
            if !user.active && employee.active {
                report.inactive_but_employed.push(found());
            }
        }
        if !user.active {
            continue;
        }
        if skip_set.contains(key.as_str()) || entry.method == LookupMethod::Skipped {
            skipped.push(user_ref);
            continue;
        }
        let Some(employee) = &entry.employee else {
            not_found.push(NotFound {
                user: user_ref,
                method: entry.method,
            });
            continue;
        };
        let untrusted = email_key(&employee.email) != key && !trust_set.contains(key.as_str());
        if !employee.active {
            if untrusted {
                refused.push(mismatch(&key, employee, "deactivation".into()));
                continue;
            }
            candidates.push((
                user_ref,
                DeactivateReason::PrestashopInactive {
                    employee_id: employee.id,
                },
            ));
            continue;
        }
        let Some(store) = employee.store() else {
            unmapped.push(Unmapped {
                user: user_ref,
                employee_id: employee.id,
                id_store: employee.id_store.clone(),
            });
            continue;
        };
        let unchanged = user.store == store.as_str()
            && user.id_store.as_deref() == Some(employee.id_store.as_str());
        if unchanged {
            continue;
        }
        if untrusted {
            let withheld = format!("store move to {}/{}", store.as_str(), employee.id_store);
            refused.push(mismatch(&key, employee, withheld));
            continue;
        }
        let planned = StoreMove {
            open_tasks: tasks_of(&user.id),
            user: user_ref,
            method: entry.method,
            employee_id: employee.id,
            store,
            id_store: employee.id_store.clone(),
        };
        if store == Store::WAR && !opts.allow_war {
            held_war.push(planned);
        } else {
            move_store.push(planned);
        }
    }

    for email in &named {
        if skip_set.contains(email.as_str()) {
            refused.push(Refusal::SkippedAndNamed {
                email: email.clone(),
            });
            continue;
        }
        match not_found
            .iter()
            .position(|missing: &NotFound| email_key(&missing.user.email) == *email)
        {
            Some(i) => {
                let missing = not_found.remove(i);
                candidates.push((missing.user, DeactivateReason::NamedMissing));
            }
            None if users.contains_key(email) => refused.push(Refusal::NotMissing {
                email: email.clone(),
            }),
            None => refused.push(Refusal::NoSuchUser {
                flag: "--deactivate-missing".into(),
                email: email.clone(),
            }),
        }
    }

    let deactivating: HashSet<String> = candidates
        .iter()
        .map(|(user, _)| email_key(&user.email))
        .collect();

    for reassign in &opts.reassign {
        let from = email_key(&reassign.from);
        let to = email_key(&reassign.to);
        if !users.contains_key(&from) {
            refused.push(Refusal::NoSuchUser {
                flag: "--reassign".into(),
                email: from.clone(),
            });
        } else if !deactivating.contains(&from) {
            refused.push(Refusal::NotDeactivated {
                flag: "--reassign".into(),
                email: from.clone(),
            });
        }
        let problem = match users.get(&to) {
            _ if from == to => Some("is the user being deactivated"),
            None => Some("matches no user"),
            Some(target) if !target.active => Some("is inactive"),
            Some(_) if deactivating.contains(&to) => Some("is being deactivated"),
            Some(_) => None,
        };
        if let Some(problem) = problem {
            refused.push(Refusal::ReassignTarget {
                from,
                to,
                problem: problem.into(),
            });
        }
    }
    for email in &orphan {
        if !users.contains_key(email) {
            refused.push(Refusal::NoSuchUser {
                flag: "--orphan-tasks".into(),
                email: email.clone(),
            });
        } else if !deactivating.contains(email) {
            refused.push(Refusal::NotDeactivated {
                flag: "--orphan-tasks".into(),
                email: email.clone(),
            });
        }
    }

    let mut deactivate = Vec::with_capacity(candidates.len());
    for (user, reason) in candidates {
        let key = email_key(&user.email);
        let targets: Vec<&Reassign> = opts
            .reassign
            .iter()
            .filter(|reassign| email_key(&reassign.from) == key)
            .collect();
        let orphaned = orphan.contains(&key);
        let open = tasks_of(&user.id);
        let tasks = match (targets.as_slice(), orphaned) {
            ([], false) => {
                if !open.is_empty() {
                    refused.push(Refusal::OpenTasks {
                        email: key.clone(),
                        count: open.len(),
                    });
                }
                TaskAction::Untouched
            }
            ([], true) => TaskAction::Orphan,
            ([reassign], false) => match users.get(&email_key(&reassign.to)) {
                Some(target) => TaskAction::Reassign {
                    to: target.id.clone(),
                    to_email: target.email.clone(),
                },
                None => TaskAction::Untouched,
            },
            _ => {
                refused.push(Refusal::ConflictingTaskFlags { email: key.clone() });
                TaskAction::Untouched
            }
        };
        deactivate.push(Deactivation {
            user,
            reason,
            open_tasks: open,
            tasks,
        });
    }
    deactivate.sort_by_key(|d| email_key(&d.user.email));

    let active_users = entries.iter().filter(|entry| entry.user.active).count();
    let max_deactivations = opts.max_deactivations.unwrap_or(active_users / 5);
    if deactivate.len() > max_deactivations {
        refused.push(Refusal::TooManyDeactivations {
            planned: deactivate.len(),
            max: max_deactivations,
        });
    }
    let active_roots = entries
        .iter()
        .filter(|entry| entry.user.active && is_root(&entry.user.authorization))
        .count();
    let roots_leaving: Vec<String> = deactivate
        .iter()
        .filter(|d| d.user.is_root())
        .map(|d| d.user.email.clone())
        .collect();
    if !roots_leaving.is_empty() && roots_leaving.len() >= active_roots {
        refused.push(Refusal::LastActiveRoot {
            emails: roots_leaving,
        });
    }

    let mut by_name: HashMap<String, Vec<String>> = HashMap::new();
    for entry in entries {
        let key = name_key(&entry.user.name);
        if !key.is_empty() {
            by_name
                .entry(key)
                .or_default()
                .push(entry.user.email.clone());
        }
    }
    let mut duplicate_names: Vec<DuplicateName> = by_name
        .into_iter()
        .filter(|(_, emails)| emails.len() > 1)
        .map(|(name, mut emails)| {
            emails.sort();
            DuplicateName { name, emails }
        })
        .collect();
    duplicate_names.sort_by(|a, b| a.name.cmp(&b.name));
    report.duplicate_names = duplicate_names;

    Plan {
        users: entries.len(),
        active_users,
        by_id: entries
            .iter()
            .filter(|entry| entry.method == LookupMethod::Id)
            .count(),
        by_email: entries
            .iter()
            .filter(|entry| entry.method == LookupMethod::Email)
            .count(),
        allow_war: opts.allow_war,
        max_deactivations,
        deactivate,
        move_store,
        held_war,
        unmapped,
        not_found,
        skipped,
        report,
        refused,
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn user(key: &str, store: &str, id_store: Option<&str>, ps: Option<u64>) -> UserRow {
        UserRow {
            id: RecordId::new("user", key),
            email: format!("{key}@pclaptops.com"),
            name: format!("{key} person"),
            active: true,
            authorization: "User".into(),
            store: store.into(),
            id_store: id_store.map(str::to_string),
            id_prestashop: ps,
        }
    }

    pub fn employee(user: &UserRow, id_store: &str, active: bool) -> Employee {
        Employee {
            id: user.id_prestashop.unwrap_or(9000),
            email: email_key(&user.email),
            name: user.name.clone(),
            id_store: id_store.into(),
            active,
        }
    }

    pub fn found(user: UserRow, id_store: &str, active: bool) -> Entry {
        let method = if user.id_prestashop.is_some() {
            LookupMethod::Id
        } else {
            LookupMethod::Email
        };
        Entry {
            employee: Some(employee(&user, id_store, active)),
            user,
            method,
        }
    }

    pub fn missing(user: UserRow) -> Entry {
        let method = if user.id_prestashop.is_some() {
            LookupMethod::Id
        } else {
            LookupMethod::Email
        };
        Entry {
            user,
            method,
            employee: None,
        }
    }

    pub fn open(user: &UserRow, keys: &[&str]) -> (String, Vec<OpenTask>) {
        let tasks = keys
            .iter()
            .map(|key| OpenTask {
                id: RecordId::new("task", *key),
                service_number: Some(format!("SO-{key}")),
            })
            .collect();
        (rid(&user.id), tasks)
    }

    /// Ten active, in-sync RIV users.
    pub fn staff() -> Vec<Entry> {
        (0..10)
            .map(|i| {
                found(
                    user(&format!("staff{i}"), "RIV", Some("7"), Some(100 + i)),
                    "7",
                    true,
                )
            })
            .collect()
    }

    fn with(extra: Vec<Entry>) -> Vec<Entry> {
        let mut entries = staff();
        entries.extend(extra);
        entries
    }

    fn opts() -> SyncOptions {
        SyncOptions::default()
    }

    fn emails<T>(items: &[T], email: impl Fn(&T) -> &str) -> Vec<String> {
        items.iter().map(|item| email(item).to_string()).collect()
    }

    #[test]
    fn in_sync_users_produce_an_empty_plan() {
        let plan = plan(&staff(), &HashMap::new(), &opts());
        assert!(plan.is_empty());
        assert!(plan.refused.is_empty());
        assert_eq!((plan.users, plan.active_users), (10, 10));
        assert_eq!((plan.by_id, plan.by_email), (10, 0));
        assert_eq!(plan.max_deactivations, 2);
    }

    #[test]
    fn a_prestashop_inactive_active_user_is_deactivated() {
        let jane = user("jane", "MUR", Some("10"), Some(1402));
        let plan = plan(
            &with(vec![found(jane, "10", false)]),
            &HashMap::new(),
            &opts(),
        );
        assert_eq!(plan.deactivate.len(), 1);
        let d = &plan.deactivate[0];
        assert_eq!(d.user.email, "jane@pclaptops.com");
        assert_eq!(
            d.reason,
            DeactivateReason::PrestashopInactive { employee_id: 1402 }
        );
        assert_eq!(d.tasks, TaskAction::Untouched);
        assert!(plan.refused.is_empty());
    }

    #[test]
    fn an_inactive_user_active_in_prestashop_is_reported_and_never_reactivated() {
        let mut blake = user("blake", "RIV", Some("10"), Some(55));
        blake.active = false;
        let plan = plan(
            &with(vec![found(blake, "10", true)]),
            &HashMap::new(),
            &opts(),
        );
        assert!(plan.is_empty());
        assert!(plan.held_war.is_empty() && plan.unmapped.is_empty());
        assert_eq!(
            emails(&plan.report.inactive_but_employed, |f| &f.user.email),
            vec!["blake@pclaptops.com"]
        );
    }

    #[test]
    fn inactive_rows_get_no_writes() {
        let mut gone = user("gone", "ORE", Some("14"), Some(77));
        gone.active = false;
        let mut quit = user("quit", "RIV", Some("7"), Some(78));
        quit.active = false;
        let mut lost = user("lost", "RIV", None, None);
        lost.active = false;
        let entries = with(vec![
            found(gone, "12", false),
            found(quit, "7", false),
            missing(lost),
        ]);
        let plan = plan(&entries, &HashMap::new(), &opts());
        assert!(plan.is_empty());
        assert!(plan.held_war.is_empty() && plan.unmapped.is_empty() && plan.not_found.is_empty());
        assert_eq!((plan.users, plan.active_users), (13, 10));
    }

    #[test]
    fn a_mapped_store_change_moves_both_fields() {
        let matt = user("matt", "ORE", Some("12"), Some(12));
        let plan = plan(
            &with(vec![found(matt, "12", true)]),
            &HashMap::new(),
            &opts(),
        );
        assert_eq!(plan.move_store.len(), 1);
        let m = &plan.move_store[0];
        assert_eq!((m.store, m.id_store.as_str()), (Store::SAN, "12"));
        assert_eq!(m.user.store, "ORE");
    }

    #[test]
    fn a_differing_id_store_alone_is_a_move() {
        let ann = user("ann", "RIV", None, Some(31));
        let plan = plan(&with(vec![found(ann, "7", true)]), &HashMap::new(), &opts());
        assert_eq!(plan.move_store.len(), 1);
        assert_eq!(plan.move_store[0].store, Store::RIV);
        assert_eq!(plan.move_store[0].id_store, "7");
    }

    #[test]
    fn an_unmapped_store_id_is_reported_without_a_write() {
        let carson = user("carson", "MUR", Some("11"), Some(11));
        let plan = plan(
            &with(vec![found(carson, "11", true)]),
            &HashMap::new(),
            &opts(),
        );
        assert!(plan.is_empty());
        assert_eq!(plan.unmapped.len(), 1);
        assert_eq!(plan.unmapped[0].id_store, "11");
    }

    #[test]
    fn a_warehouse_move_is_held_without_allow_war() {
        let bstair = user("bstair", "RIV", Some("1"), Some(1));
        let entries = with(vec![found(bstair, "1", true)]);
        let held = plan(&entries, &HashMap::new(), &opts());
        assert!(held.is_empty());
        assert_eq!(held.held_war.len(), 1);
        assert_eq!(held.held_war[0].store, Store::WAR);

        let allowed = plan(
            &entries,
            &HashMap::new(),
            &SyncOptions {
                allow_war: true,
                ..opts()
            },
        );
        assert!(allowed.held_war.is_empty());
        assert_eq!(allowed.move_store.len(), 1);
        assert_eq!(allowed.move_store[0].store, Store::WAR);
    }

    #[test]
    fn missing_users_are_not_deactivated_unless_named() {
        let johan = user("johan", "RIV", None, None);
        let entries = with(vec![missing(johan)]);
        let unnamed = plan(&entries, &HashMap::new(), &opts());
        assert!(unnamed.is_empty());
        assert_eq!(unnamed.not_found.len(), 1);
        assert_eq!(unnamed.not_found[0].method, LookupMethod::Email);

        let named = plan(
            &entries,
            &HashMap::new(),
            &SyncOptions {
                deactivate_missing: vec![" Johan@PCLaptops.com ".into()],
                ..opts()
            },
        );
        assert!(named.not_found.is_empty());
        assert_eq!(named.deactivate.len(), 1);
        assert_eq!(named.deactivate[0].reason, DeactivateReason::NamedMissing);
        assert!(named.refused.is_empty());
    }

    #[test]
    fn a_named_missing_email_that_is_not_missing_is_refused() {
        let found_user = user("present", "RIV", Some("7"), Some(5));
        let entries = with(vec![found(found_user, "7", true)]);
        let plan = plan(
            &entries,
            &HashMap::new(),
            &SyncOptions {
                deactivate_missing: vec![
                    "present@pclaptops.com".into(),
                    "nobody@pclaptops.com".into(),
                ],
                ..opts()
            },
        );
        assert!(plan.deactivate.is_empty());
        assert_eq!(
            plan.refused,
            vec![
                Refusal::NotMissing {
                    email: "present@pclaptops.com".into()
                },
                Refusal::NoSuchUser {
                    flag: "--deactivate-missing".into(),
                    email: "nobody@pclaptops.com".into()
                },
            ]
        );
    }

    #[test]
    fn skip_excludes_a_user_from_every_write() {
        let jane = user("jane", "MUR", Some("10"), Some(1402));
        let matt = user("matt", "ORE", Some("12"), Some(12));
        let entries = with(vec![found(jane, "10", false), found(matt, "12", true)]);
        let plan = plan(
            &entries,
            &HashMap::new(),
            &SyncOptions {
                skip: vec!["jane@pclaptops.com".into(), "MATT@pclaptops.com".into()],
                ..opts()
            },
        );
        assert!(plan.is_empty());
        assert_eq!(
            emails(&plan.skipped, |u| &u.email),
            vec!["jane@pclaptops.com", "matt@pclaptops.com"]
        );
        assert!(plan.refused.is_empty());
    }

    #[test]
    fn a_skip_email_matching_no_user_is_refused() {
        let plan = plan(
            &staff(),
            &HashMap::new(),
            &SyncOptions {
                skip: vec!["ghost@pclaptops.com".into()],
                ..opts()
            },
        );
        assert_eq!(
            plan.refused,
            vec![Refusal::NoSuchUser {
                flag: "--skip".into(),
                email: "ghost@pclaptops.com".into()
            }]
        );
    }

    #[test]
    fn skipping_and_naming_the_same_user_is_refused() {
        let johan = user("johan", "RIV", None, None);
        let plan = plan(
            &with(vec![missing(johan)]),
            &HashMap::new(),
            &SyncOptions {
                skip: vec!["johan@pclaptops.com".into()],
                deactivate_missing: vec!["johan@pclaptops.com".into()],
                ..opts()
            },
        );
        assert!(plan.deactivate.is_empty());
        assert_eq!(
            plan.refused,
            vec![Refusal::SkippedAndNamed {
                email: "johan@pclaptops.com".into()
            }]
        );
    }

    fn relinked(user: UserRow, id_store: &str, active: bool) -> Entry {
        let mut entry = found(user, id_store, active);
        if let Some(e) = entry.employee.as_mut() {
            e.email = "someone.else@pclaptops.com".into();
        }
        entry
    }

    fn mismatch_refusal(email: &str, employee_id: u64, withheld: &str) -> Refusal {
        Refusal::IdEmailMismatch {
            email: email.into(),
            employee_id,
            employee_email: "someone.else@pclaptops.com".into(),
            withheld: withheld.into(),
        }
    }

    #[test]
    fn a_deactivation_from_a_record_with_another_email_is_refused() {
        let jane = user("jane", "MUR", Some("10"), Some(1402));
        let entries = with(vec![relinked(jane, "10", false)]);
        let held = plan(&entries, &HashMap::new(), &opts());
        assert!(held.deactivate.is_empty());
        assert_eq!(
            held.refused,
            vec![mismatch_refusal("jane@pclaptops.com", 1402, "deactivation")]
        );
        assert_eq!(
            emails(&held.report.email_drift, |f| &f.user.email),
            vec!["jane@pclaptops.com"]
        );

        let trusted = plan(
            &entries,
            &HashMap::new(),
            &SyncOptions {
                trust_id: vec!["Jane@PCLaptops.com".into()],
                ..opts()
            },
        );
        assert_eq!(trusted.deactivate.len(), 1);
        assert!(trusted.refused.is_empty());

        let skipped = plan(
            &entries,
            &HashMap::new(),
            &SyncOptions {
                skip: vec!["jane@pclaptops.com".into()],
                ..opts()
            },
        );
        assert!(skipped.is_empty() && skipped.refused.is_empty());
    }

    #[test]
    fn a_store_move_from_a_record_with_another_email_is_refused() {
        let matt = user("matt", "ORE", Some("12"), Some(12));
        let bstair = user("bstair", "RIV", Some("1"), Some(1));
        let entries = with(vec![
            relinked(matt, "12", true),
            relinked(bstair, "1", true),
        ]);
        let held = plan(&entries, &HashMap::new(), &opts());
        assert!(held.is_empty() && held.held_war.is_empty());
        assert_eq!(
            held.refused,
            vec![
                mismatch_refusal("matt@pclaptops.com", 12, "store move to SAN/12"),
                mismatch_refusal("bstair@pclaptops.com", 1, "store move to WAR/1"),
            ]
        );

        let trusted = plan(
            &entries,
            &HashMap::new(),
            &SyncOptions {
                trust_id: vec!["matt@pclaptops.com".into()],
                ..opts()
            },
        );
        assert_eq!(
            emails(&trusted.move_store, |m| &m.user.email),
            vec!["matt@pclaptops.com"]
        );
        assert_eq!(
            trusted.refused,
            vec![mismatch_refusal(
                "bstair@pclaptops.com",
                1,
                "store move to WAR/1"
            )]
        );
    }

    #[test]
    fn a_mismatched_record_without_a_write_is_only_reported() {
        let ann = user("ann", "RIV", Some("7"), Some(31));
        let plan = plan(
            &with(vec![relinked(ann, "7", true)]),
            &HashMap::new(),
            &opts(),
        );
        assert!(plan.is_empty() && plan.refused.is_empty());
        assert_eq!(plan.report.email_drift.len(), 1);
    }

    #[test]
    fn a_trust_id_email_matching_no_user_is_refused() {
        let plan = plan(
            &staff(),
            &HashMap::new(),
            &SyncOptions {
                trust_id: vec!["ghost@pclaptops.com".into()],
                ..opts()
            },
        );
        assert_eq!(
            plan.refused,
            vec![Refusal::NoSuchUser {
                flag: "--trust-id".into(),
                email: "ghost@pclaptops.com".into()
            }]
        );
    }

    #[test]
    fn users_not_looked_up_are_skipped() {
        let mut jane = missing(user("jane", "MUR", Some("10"), Some(1402)));
        jane.method = LookupMethod::Skipped;
        let mut idle = missing(user("idle", "RIV", None, None));
        idle.method = LookupMethod::Skipped;
        idle.user.active = false;
        let plan = plan(&with(vec![jane, idle]), &HashMap::new(), &opts());
        assert!(plan.is_empty() && plan.not_found.is_empty() && plan.refused.is_empty());
        assert_eq!(
            emails(&plan.skipped, |u| &u.email),
            vec!["jane@pclaptops.com"]
        );
        assert_eq!((plan.by_id, plan.by_email), (10, 0));
    }

    #[test]
    fn deactivating_the_last_active_root_is_refused() {
        let mut root = user("root", "RIV", Some("7"), Some(1));
        root.authorization = "Root".into();
        let plan = plan(
            &with(vec![found(root, "7", false)]),
            &HashMap::new(),
            &opts(),
        );
        assert_eq!(plan.deactivate.len(), 1);
        assert_eq!(
            plan.refused,
            vec![Refusal::LastActiveRoot {
                emails: vec!["root@pclaptops.com".into()]
            }]
        );
    }

    #[test]
    fn deactivating_one_of_two_roots_is_allowed() {
        let mut root = user("root", "RIV", Some("7"), Some(1));
        root.authorization = "Root".into();
        let mut other = user("other", "RIV", Some("7"), Some(2));
        other.authorization = "Root".into();
        let plan = plan(
            &with(vec![found(root, "7", false), found(other, "7", true)]),
            &HashMap::new(),
            &opts(),
        );
        assert_eq!(plan.deactivate.len(), 1);
        assert!(plan.refused.is_empty());
    }

    #[test]
    fn exceeding_the_deactivation_cap_is_refused() {
        let extra = (0..3)
            .map(|i| {
                found(
                    user(&format!("gone{i}"), "RIV", Some("7"), Some(500 + i)),
                    "7",
                    false,
                )
            })
            .collect();
        let entries = with(extra);
        let capped = plan(&entries, &HashMap::new(), &opts());
        assert_eq!(capped.max_deactivations, 2);
        assert_eq!(
            capped.refused,
            vec![Refusal::TooManyDeactivations { planned: 3, max: 2 }]
        );

        let raised = plan(
            &entries,
            &HashMap::new(),
            &SyncOptions {
                max_deactivations: Some(3),
                ..opts()
            },
        );
        assert!(raised.refused.is_empty());
    }

    #[test]
    fn open_tasks_block_a_deactivation_without_instructions() {
        let jane = user("jane", "MUR", Some("10"), Some(1402));
        let tasks = HashMap::from([open(&jane, &["t1", "t2"])]);
        let plan = plan(&with(vec![found(jane, "10", false)]), &tasks, &opts());
        assert_eq!(plan.deactivate[0].open_tasks.len(), 2);
        assert_eq!(
            plan.refused,
            vec![Refusal::OpenTasks {
                email: "jane@pclaptops.com".into(),
                count: 2
            }]
        );
    }

    #[test]
    fn reassign_names_an_active_target() {
        let jane = user("jane", "MUR", Some("10"), Some(1402));
        let tasks = HashMap::from([open(&jane, &["t1"])]);
        let plan = plan(
            &with(vec![found(jane, "10", false)]),
            &tasks,
            &SyncOptions {
                reassign: vec![Reassign {
                    from: "jane@pclaptops.com".into(),
                    to: "Staff1@pclaptops.com".into(),
                }],
                ..opts()
            },
        );
        assert!(plan.refused.is_empty());
        assert_eq!(
            plan.deactivate[0].tasks,
            TaskAction::Reassign {
                to: RecordId::new("user", "staff1"),
                to_email: "staff1@pclaptops.com".into()
            }
        );
    }

    #[test]
    fn orphan_tasks_allows_the_deactivation() {
        let jane = user("jane", "MUR", Some("10"), Some(1402));
        let tasks = HashMap::from([open(&jane, &["t1"])]);
        let plan = plan(
            &with(vec![found(jane, "10", false)]),
            &tasks,
            &SyncOptions {
                orphan_tasks: vec!["jane@pclaptops.com".into()],
                ..opts()
            },
        );
        assert!(plan.refused.is_empty());
        assert_eq!(plan.deactivate[0].tasks, TaskAction::Orphan);
    }

    #[test]
    fn bad_reassign_targets_are_refused() {
        let jane = user("jane", "MUR", Some("10"), Some(1402));
        let bob = user("bob", "RIV", Some("7"), Some(1403));
        let mut idle = user("idle", "RIV", Some("7"), Some(1404));
        idle.active = false;
        let tasks = HashMap::from([open(&jane, &["t1"]), open(&bob, &["t2"])]);
        let entries = with(vec![
            found(jane, "10", false),
            found(bob, "7", false),
            found(idle, "7", false),
        ]);
        let reassign = |from: &str, to: &str| Reassign {
            from: format!("{from}@pclaptops.com"),
            to: format!("{to}@pclaptops.com"),
        };
        let case = |r: Reassign| {
            plan(
                &entries,
                &tasks,
                &SyncOptions {
                    reassign: vec![r],
                    orphan_tasks: vec!["bob@pclaptops.com".into()],
                    ..opts()
                },
            )
            .refused
        };
        let target = |to: &str, problem: &str| Refusal::ReassignTarget {
            from: "jane@pclaptops.com".into(),
            to: format!("{to}@pclaptops.com"),
            problem: problem.into(),
        };
        assert_eq!(
            case(reassign("jane", "idle")),
            vec![target("idle", "is inactive")]
        );
        assert_eq!(
            case(reassign("jane", "bob")),
            vec![target("bob", "is being deactivated")]
        );
        assert_eq!(
            case(reassign("jane", "ghost")),
            vec![target("ghost", "matches no user")]
        );
        assert_eq!(
            case(reassign("jane", "jane")),
            vec![target("jane", "is the user being deactivated")]
        );
    }

    #[test]
    fn task_flags_for_users_not_being_deactivated_are_refused() {
        let plan = plan(
            &staff(),
            &HashMap::new(),
            &SyncOptions {
                reassign: vec![Reassign {
                    from: "staff0@pclaptops.com".into(),
                    to: "staff1@pclaptops.com".into(),
                }],
                orphan_tasks: vec!["staff2@pclaptops.com".into(), "ghost@pclaptops.com".into()],
                ..opts()
            },
        );
        assert_eq!(
            plan.refused,
            vec![
                Refusal::NotDeactivated {
                    flag: "--reassign".into(),
                    email: "staff0@pclaptops.com".into()
                },
                Refusal::NotDeactivated {
                    flag: "--orphan-tasks".into(),
                    email: "staff2@pclaptops.com".into()
                },
                Refusal::NoSuchUser {
                    flag: "--orphan-tasks".into(),
                    email: "ghost@pclaptops.com".into()
                },
            ]
        );
    }

    #[test]
    fn reassign_plus_orphan_for_one_user_is_refused() {
        let jane = user("jane", "MUR", Some("10"), Some(1402));
        let tasks = HashMap::from([open(&jane, &["t1"])]);
        let plan = plan(
            &with(vec![found(jane, "10", false)]),
            &tasks,
            &SyncOptions {
                reassign: vec![Reassign {
                    from: "jane@pclaptops.com".into(),
                    to: "staff1@pclaptops.com".into(),
                }],
                orphan_tasks: vec!["jane@pclaptops.com".into()],
                ..opts()
            },
        );
        assert_eq!(
            plan.refused,
            vec![Refusal::ConflictingTaskFlags {
                email: "jane@pclaptops.com".into()
            }]
        );
    }

    #[test]
    fn report_only_findings_are_collected() {
        let mut drifted = user("ethan.thomas", "RIV", Some("7"), Some(40));
        drifted.name = "blake thomas".into();
        let mut renamed = user("old.address", "RIV", Some("7"), Some(41));
        renamed.name = "Old Address".into();
        let mut zz = user("zz_livetest", "ZZTest", None, None);
        zz.active = false;
        let dakota = user("dakota", "RIV", Some("7"), None);
        let mut j1 = user("johanmena", "RIV", None, None);
        j1.name = "Johan Mena".into();
        let mut j2 = user("joha.mena", "RIV", None, None);
        j2.name = " johan  mena ".into();

        let mut ethan = found(drifted, "7", true);
        if let Some(e) = ethan.employee.as_mut() {
            e.name = "Ethan Thomas".into();
        }
        let mut old = found(renamed, "7", true);
        if let Some(e) = old.employee.as_mut() {
            e.email = "new.address@xidax.com".into();
        }
        let entries = with(vec![
            ethan,
            old,
            missing(zz),
            found(dakota, "7", true),
            missing(j1),
            missing(j2),
        ]);
        let plan = plan(&entries, &HashMap::new(), &opts());

        assert!(plan.is_empty());
        let r = &plan.report;
        assert_eq!(
            emails(&r.name_drift, |f| &f.user.email),
            vec!["ethan.thomas@pclaptops.com"]
        );
        assert_eq!(
            emails(&r.email_drift, |f| &f.user.email),
            vec!["old.address@pclaptops.com"]
        );
        assert_eq!(
            emails(&r.invalid_store, |u| &u.email),
            vec!["zz_livetest@pclaptops.com"]
        );
        assert_eq!(
            emails(&r.unlinked_matches, |f| &f.user.email),
            vec!["dakota@pclaptops.com"]
        );
        assert_eq!(
            r.duplicate_names,
            vec![DuplicateName {
                name: "johan mena".into(),
                emails: vec![
                    "joha.mena@pclaptops.com".into(),
                    "johanmena@pclaptops.com".into()
                ],
            }]
        );
        assert_eq!(
            emails(&plan.not_found, |n| &n.user.email),
            vec!["johanmena@pclaptops.com", "joha.mena@pclaptops.com"]
        );
    }

    #[test]
    fn the_plan_round_trips_through_json() {
        let jane = user("jane", "MUR", Some("10"), Some(1402));
        let bstair = user("bstair", "RIV", Some("1"), Some(1));
        let tasks = HashMap::from([open(&jane, &["t1"])]);
        let plan = plan(
            &with(vec![found(jane, "10", false), found(bstair, "1", true)]),
            &tasks,
            &SyncOptions {
                orphan_tasks: vec!["jane@pclaptops.com".into()],
                ..opts()
            },
        );
        let json = serde_json::to_string(&plan).expect("serialize");
        let back: Plan = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, plan);
        assert!(json.contains("\"WAR\""));
    }
}
