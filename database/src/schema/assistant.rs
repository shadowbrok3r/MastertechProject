//! People, the store rule and the writes behind the AI assistant tools: tasks, reminders and ticket briefs.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{Datetime, RecordId, SurrealValue};
use crate::db;

/// Origin stamped on tasks the assistant creates.
pub const ASSISTANT_ORIGIN: &str = "ai";
/// Longest assistant task title, in characters.
pub const TASK_TITLE_MAX_CHARS: usize = 120;
/// Longest assistant task description or notification text, in characters.
pub const TASK_TEXT_MAX_CHARS: usize = 1000;
/// Longest single line of a ticket brief, in characters.
pub const BRIEF_LINE_MAX_CHARS: usize = 220;
/// Longest whole ticket brief, in characters.
pub const BRIEF_TOTAL_MAX_CHARS: usize = 600;
/// `task_note.kind` of a ticket brief.
pub const BRIEF_NOTE_KIND: &str = "ai_brief";
/// `task_note.username` of a ticket brief.
pub const BRIEF_AUTHOR: &str = "AI brief";

pub const TYPE_REMINDER: &str = "Reminder";
pub const TYPE_PART_REQUEST: &str = "Part Request";
pub const TYPE_OVERDUE: &str = "Overdue";
pub const TYPE_MORNING_BRIEF: &str = "Morning Brief";

/// The user columns every assistant lookup reads.
const PERSON_FIELDS: &str = "id, name, email, store, authorization, active";

/// Creates an assistant task; the task CREATE event notifies the assignee.
pub const CREATE_ASSIGNED_TASK_SQL: &str = "CREATE task CONTENT { task_name: $name, \
     task_description: $description, assignee: $assignee, assigned_by: $assigned_by, due_date: $due, \
     priority: $priority, status: 'Todo', completed: false, origin: $origin, schedule: $schedule, \
     about_service: $about, part_request: $part } RETURN VALUE id";

/// A notification for `$user`, optionally from someone and about a task.
pub const CREATE_NOTIFICATION_SQL: &str = "CREATE notification CONTENT { user: $user, \
     notification_type: $kind, notification_description: $text, status: 'Unread', from_user: $from, \
     task: $task } RETURN VALUE id";

/// Returns snoozed notifications whose time has come to the unread list.
pub const UNSNOOZE_SQL: &str = "UPDATE notification SET status = 'Unread', snooze_until = NONE \
     WHERE status = 'Snoozed' AND snooze_until != NONE AND snooze_until <= time::now() RETURN VALUE id";

/// Hides `$id` until `$until`.
pub const SNOOZE_SQL: &str = "UPDATE $id SET status = 'Snoozed', snooze_until = $until";

/// Replaces the ticket's brief with a new private note.
pub const POST_BRIEF_SQL: &str = "DELETE task_note WHERE task_id = $task AND kind = 'ai_brief'; \
     CREATE task_note CONTENT { task_id: $task, note: $note, username: $author_name, user: $author, \
     service_number: $sn, private: true, kind: 'ai_brief' } RETURN VALUE id";

/// A private staff note on a ticket's task.
pub const PRIVATE_NOTE_SQL: &str = "CREATE task_note CONTENT { task_id: $task, note: $note, \
     username: $author_name, user: $author, service_number: $sn, private: true } RETURN VALUE id";

/// `task_note.username` of other assistant notes.
pub const ASSISTANT_NOTE_AUTHOR: &str = "AI assistant";

/// Writes a private note on `task`.
pub async fn post_private_note(
    task: &RecordId,
    service_number: &str,
    author: &RecordId,
    note: &str,
) -> anyhow::Result<RecordId> {
    let ids: Vec<RecordId> = db()
        .query(PRIVATE_NOTE_SQL)
        .bind(("task", task.clone()))
        .bind(("note", note.to_string()))
        .bind(("author_name", ASSISTANT_NOTE_AUTHOR))
        .bind(("author", author.clone()))
        .bind(("sn", service_number.to_string()))
        .await?
        .check()?
        .take(0)?;
    ids.into_iter().next().ok_or_else(|| anyhow::anyhow!("note create returned no id"))
}

/// Store close today when at least two open hours remain, else close on the next open day.
pub fn part_due(now: DateTime<Utc>) -> DateTime<Utc> {
    use super::business_calendar::{CLOSE_HOUR, STORE_TZ, is_open_day};
    use chrono::{Datelike, Duration};
    let mut day = now.with_timezone(&STORE_TZ).date_naive();
    for _ in 0..8 {
        let close = day.and_hms_opt(CLOSE_HOUR, 0, 0).and_then(super::task_schedule::local_to_utc);
        if let Some(close) = close.filter(|c| is_open_day(day.weekday()) && *c - now >= Duration::hours(2)) {
            return close;
        }
        day += Duration::days(1);
    }
    now + Duration::hours(24)
}

/// Open assistant tasks past `$cutoff` that nobody has been told about.
pub const OVERDUE_ASSIGNED_SQL: &str = "SELECT id, task_name, assignee, assigned_by, due_date, \
     assignee.name AS assignee_name, assignee.store AS store FROM task \
     WHERE completed = false AND escalated_at = NONE AND (assigned_by != NONE OR schedule != NONE) \
     AND due_date < $cutoff LIMIT 50";

/// Marks `$id` as escalated.
pub const MARK_ESCALATED_SQL: &str = "UPDATE $id SET escalated_at = time::now()";

/// Completes an assistant task that has no service ticket of its own.
pub const COMPLETE_ASSISTANT_TASK_SQL: &str = "UPDATE $id SET completed = true, completed_at = time::now(), \
     status = 'Complete' WHERE service_ticket = NONE AND (assigned_by != NONE OR schedule != NONE) RETURN VALUE id";

/// An active user as the assistant tools see them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, SurrealValue)]
pub struct Person {
    pub id: RecordId,
    #[serde(default)]
    #[surreal(default)]
    pub name: String,
    #[serde(default)]
    #[surreal(default)]
    pub email: String,
    #[serde(default)]
    #[surreal(default)]
    pub store: String,
    #[serde(default)]
    #[surreal(default)]
    pub authorization: String,
    #[serde(default)]
    #[surreal(default)]
    pub active: bool,
}

impl Person {
    pub fn is_root(&self) -> bool {
        self.active && self.authorization == "Root"
    }

    pub fn is_manager(&self) -> bool {
        self.active && self.authorization == "Manager"
    }

    /// `Name (STORE)`.
    pub fn label(&self) -> String {
        format!("{} ({})", self.name, self.store)
    }

    pub fn first_name(&self) -> &str {
        self.name.split_whitespace().next().unwrap_or(&self.name)
    }

    pub async fn load(id: &RecordId) -> anyhow::Result<Option<Self>> {
        let rows: Vec<Self> =
            db().query(format!("SELECT {PERSON_FIELDS} FROM $id")).bind(("id", id.clone())).await?.check()?.take(0)?;
        Ok(rows.into_iter().next())
    }

    pub async fn active_all() -> anyhow::Result<Vec<Self>> {
        Ok(db().query(format!("SELECT {PERSON_FIELDS} FROM user WHERE active = true")).await?.check()?.take(0)?)
    }

    /// Active Managers of `store`, or active Root users when the store has none.
    pub async fn escalation_contacts(store: &str) -> anyhow::Result<Vec<Self>> {
        let people = Self::active_all().await?;
        Ok(escalation_contacts_from(&people, store))
    }
}

/// A store from its code (`RIV`, `ltn`, ...).
pub fn store_from_code(code: &str) -> Option<super::Store> {
    let code = code.trim();
    super::Store::VALUES.into_iter().find(|s| s.as_str().eq_ignore_ascii_case(code))
}

fn same_store(a: &str, b: &str) -> bool {
    !a.trim().is_empty() && a.trim().eq_ignore_ascii_case(b.trim())
}

/// Managers of `store` from `people`, else its Roots, else every Root.
pub fn escalation_contacts_from(people: &[Person], store: &str) -> Vec<Person> {
    let managers: Vec<Person> =
        people.iter().filter(|p| p.is_manager() && same_store(&p.store, store)).cloned().collect();
    if !managers.is_empty() {
        return managers;
    }
    people.iter().filter(|p| p.is_root()).cloned().collect()
}

/// Ok when `actor` may assign work to `target`: themselves, their own store, or anyone for Root.
pub fn may_assign(actor: &Person, target: &Person) -> Result<(), String> {
    if !actor.active {
        return Err(format!("{} is not an active user", actor.name));
    }
    if !target.active {
        return Err(format!("{} is not an active user", target.name));
    }
    if actor.id == target.id || actor.is_root() || same_store(&actor.store, &target.store) {
        return Ok(());
    }
    Err(format!(
        "{} can only assign to people at {}; {} is at {}. A Root user can assign across stores.",
        actor.name, actor.store, target.name, target.store
    ))
}

fn normalize(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// Resolves `query` (email, username, full or partial name, or "me") to one active person.
pub fn match_person(query: &str, actor: &Person, people: &[Person]) -> Result<Person, String> {
    let q = normalize(query);
    if q.is_empty() || matches!(q.as_str(), "me" | "myself" | "self" | "i") {
        return Ok(actor.clone());
    }
    let active: Vec<&Person> = people.iter().filter(|p| p.active).collect();
    let email = if q.contains('@') { q.clone() } else { format!("{q}@pclaptops.com") };
    if let Some(p) = active.iter().find(|p| p.email.eq_ignore_ascii_case(&email)) {
        return Ok((*p).clone());
    }
    if let Some(p) = active.iter().find(|p| normalize(&p.name) == q) {
        return Ok((*p).clone());
    }
    let tokens: Vec<&str> = q.split(' ').collect();
    let hits: Vec<&Person> = active
        .iter()
        .copied()
        .filter(|p| {
            let name = normalize(&p.name);
            let parts: Vec<&str> = name.split(' ').collect();
            tokens.iter().all(|t| parts.iter().any(|part| part.starts_with(t)))
        })
        .collect();
    match hits.as_slice() {
        [one] => Ok((*one).clone()),
        [] => Err(format!("No active user matches '{}'.", query.trim())),
        many => {
            let local: Vec<&&Person> = many.iter().filter(|p| same_store(&p.store, &actor.store)).collect();
            if let [one] = local.as_slice() {
                return Ok((**one).clone());
            }
            let names: Vec<String> = many.iter().map(|p| p.label()).collect();
            Err(format!("'{}' matches {}; say which.", query.trim(), names.join(", ")))
        }
    }
}

/// Open task counts per assignee.
pub const OPEN_TASK_COUNTS_SQL: &str =
    "SELECT assignee, count() AS n FROM task WHERE completed = false AND assignee != NONE GROUP BY assignee";

#[derive(Clone, Debug, Serialize, Deserialize, SurrealValue)]
struct LoadRow {
    assignee: RecordId,
    n: i64,
}

/// Open tasks per assignee.
#[allow(clippy::mutable_key_type)]
pub async fn open_task_counts() -> anyhow::Result<std::collections::HashMap<RecordId, i64>> {
    let rows: Vec<LoadRow> = db().query(OPEN_TASK_COUNTS_SQL).await?.check()?.take(0)?;
    Ok(rows.into_iter().map(|r| (r.assignee, r.n)).collect())
}

/// The active person at `store` with the fewest open tasks, Root users only when nobody else works there.
#[allow(clippy::mutable_key_type)]
pub fn pick_sender(people: &[Person], store: &str, load: &std::collections::HashMap<RecordId, i64>) -> Option<Person> {
    let at_store: Vec<&Person> = people.iter().filter(|p| p.active && same_store(&p.store, store)).collect();
    let staff: Vec<&Person> = at_store.iter().copied().filter(|p| !p.is_root()).collect();
    let pool = if staff.is_empty() { at_store } else { staff };
    pool.into_iter().min_by_key(|p| (load.get(&p.id).copied().unwrap_or(0), p.name.clone())).cloned()
}

/// Loads the active roster and resolves `query` against it.
pub async fn resolve_person(query: &str, actor: &Person) -> anyhow::Result<Result<Person, String>> {
    let people = Person::active_all().await?;
    Ok(match_person(query, actor, &people))
}

/// `raw` collapsed to one line; `Err` when blank or longer than `max` characters.
pub fn clean_line(field: &str, raw: &str, max: usize) -> Result<String, String> {
    let line = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.is_empty() {
        return Err(format!("`{field}` is empty"));
    }
    let len = line.chars().count();
    if len > max {
        return Err(format!("`{field}` is {len} characters; keep it under {max}"));
    }
    Ok(line)
}

/// A store-to-store part move attached to a task.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, SurrealValue)]
pub struct PartRequest {
    pub part: String,
    pub quantity: i64,
    pub from_store: String,
    pub to_store: String,
    pub service_number: Option<String>,
    pub product_code: Option<String>,
    pub product_id: Option<i64>,
}

/// A task the assistant files for someone.
#[derive(Clone, Debug)]
pub struct AssignedTask {
    pub name: String,
    pub description: String,
    pub assignee: RecordId,
    pub assigned_by: Option<RecordId>,
    pub due: DateTime<Utc>,
    pub priority: String,
    pub schedule: Option<RecordId>,
    pub about_service: Option<String>,
    pub part_request: Option<PartRequest>,
}

impl AssignedTask {
    pub async fn create(&self) -> anyhow::Result<RecordId> {
        let ids: Vec<RecordId> = db()
            .query(CREATE_ASSIGNED_TASK_SQL)
            .bind(("name", self.name.clone()))
            .bind(("description", self.description.clone()))
            .bind(("assignee", self.assignee.clone()))
            .bind(("assigned_by", self.assigned_by.clone()))
            .bind(("due", Datetime::from(self.due)))
            .bind(("priority", self.priority.clone()))
            .bind(("origin", ASSISTANT_ORIGIN))
            .bind(("schedule", self.schedule.clone()))
            .bind(("about", self.about_service.clone()))
            .bind(("part", self.part_request.clone()))
            .await?
            .check()?
            .take(0)?;
        ids.into_iter().next().ok_or_else(|| anyhow::anyhow!("task create returned no id"))
    }
}

/// Writes a notification row.
pub async fn notify(
    user: &RecordId,
    kind: &str,
    text: &str,
    from: Option<&RecordId>,
    task: Option<&RecordId>,
) -> anyhow::Result<RecordId> {
    let ids: Vec<RecordId> = db()
        .query(CREATE_NOTIFICATION_SQL)
        .bind(("user", user.clone()))
        .bind(("kind", kind.to_string()))
        .bind(("text", text.to_string()))
        .bind(("from", from.cloned()))
        .bind(("task", task.cloned()))
        .await?
        .check()?
        .take(0)?;
    ids.into_iter().next().ok_or_else(|| anyhow::anyhow!("notification create returned no id"))
}

/// Hides a notification until `until`.
pub async fn snooze_notification(id: &RecordId, until: DateTime<Utc>) -> anyhow::Result<()> {
    db().query(SNOOZE_SQL).bind(("id", id.clone())).bind(("until", Datetime::from(until))).await?.check()?;
    Ok(())
}

/// Returns due snoozed notifications to unread; the count moved.
pub async fn unsnooze_due() -> anyhow::Result<usize> {
    let ids: Vec<RecordId> = db().query(UNSNOOZE_SQL).await?.check()?.take(0)?;
    Ok(ids.len())
}

/// Completes an assistant task; false when it is a ticket's own task or not an assistant task.
pub async fn complete_assistant_task(id: &RecordId) -> anyhow::Result<bool> {
    let ids: Vec<RecordId> =
        db().query(COMPLETE_ASSISTANT_TASK_SQL).bind(("id", id.clone())).await?.check()?.take(0)?;
    Ok(!ids.is_empty())
}

/// A ticket brief for staff: what is happening, what was found, what to tell the customer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TicketBrief {
    pub now: String,
    pub found: String,
    pub tell_customer: String,
}

impl TicketBrief {
    /// Trims each line and enforces the length caps.
    pub fn new(now: &str, found: &str, tell_customer: &str) -> Result<Self, String> {
        let brief = Self {
            now: clean_line("now", now, BRIEF_LINE_MAX_CHARS)?,
            found: clean_line("found", found, BRIEF_LINE_MAX_CHARS)?,
            tell_customer: clean_line("tell_customer", tell_customer, BRIEF_LINE_MAX_CHARS)?,
        };
        let total = brief.render().chars().count();
        if total > BRIEF_TOTAL_MAX_CHARS {
            return Err(format!("the brief is {total} characters; keep it under {BRIEF_TOTAL_MAX_CHARS}"));
        }
        Ok(brief)
    }

    pub fn render(&self) -> String {
        format!("Now: {}\nFound: {}\nTell the customer: {}", self.now, self.found, self.tell_customer)
    }

    /// Replaces the ticket's previous brief with this one as a private note.
    pub async fn post(&self, task: &RecordId, service_number: &str, author: &RecordId) -> anyhow::Result<RecordId> {
        let mut response = db()
            .query(POST_BRIEF_SQL)
            .bind(("task", task.clone()))
            .bind(("note", self.render()))
            .bind(("author_name", BRIEF_AUTHOR))
            .bind(("author", author.clone()))
            .bind(("sn", service_number.to_string()))
            .await?
            .check()?;
        let ids: Vec<RecordId> = response.take(1)?;
        ids.into_iter().next().ok_or_else(|| anyhow::anyhow!("brief note create returned no id"))
    }
}

/// An open assistant task past its due time.
#[derive(Clone, Debug, Serialize, Deserialize, SurrealValue)]
pub struct OverdueTask {
    pub id: RecordId,
    #[serde(default)]
    #[surreal(default)]
    pub task_name: String,
    pub assignee: RecordId,
    pub assigned_by: Option<RecordId>,
    pub due_date: Option<Datetime>,
    #[serde(default)]
    #[surreal(default)]
    pub assignee_name: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub store: Option<String>,
}

impl OverdueTask {
    /// Open assistant tasks due before `cutoff` that were never escalated.
    pub async fn unescalated(cutoff: DateTime<Utc>) -> anyhow::Result<Vec<Self>> {
        Ok(db().query(OVERDUE_ASSIGNED_SQL).bind(("cutoff", Datetime::from(cutoff))).await?.check()?.take(0)?)
    }

    pub async fn mark_escalated(&self) -> anyhow::Result<()> {
        db().query(MARK_ESCALATED_SQL).bind(("id", self.id.clone())).await?.check()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn person(key: &str, name: &str, store: &str, auth: &str) -> Person {
        Person {
            id: RecordId::new("user", key),
            name: name.into(),
            email: format!("{key}@pclaptops.com"),
            store: store.into(),
            authorization: auth.into(),
            active: true,
        }
    }

    fn roster() -> Vec<Person> {
        vec![
            person("sam.jones", "Sam Jones", "RIV", "User"),
            person("sam.lee", "Sam Lee", "MUR", "User"),
            person("ana", "Ana Ortiz", "RIV", "Manager"),
            person("logan", "Logan Lees", "RIV", "Root"),
            person("kim", "Kim Park", "LTN", "User"),
        ]
    }

    #[test]
    fn same_store_and_root_may_assign() {
        let people = roster();
        let (sam, kim, logan, ana) = (&people[0], &people[4], &people[3], &people[2]);
        assert!(may_assign(ana, sam).is_ok());
        assert!(may_assign(sam, sam).is_ok());
        assert!(may_assign(logan, kim).is_ok());
        assert!(may_assign(sam, kim).unwrap_err().contains("only assign to people at RIV"));
        assert!(may_assign(ana, kim).is_err());
    }

    #[test]
    fn inactive_people_cannot_assign_or_be_assigned() {
        let people = roster();
        let mut gone = people[0].clone();
        gone.active = false;
        assert!(may_assign(&gone, &people[2]).is_err());
        assert!(may_assign(&people[2], &gone).is_err());
    }

    #[test]
    fn names_resolve_with_store_preference() {
        let people = roster();
        let ana = &people[2];
        assert_eq!(match_person("me", ana, &people).unwrap().name, "Ana Ortiz");
        assert_eq!(match_person("kim@pclaptops.com", ana, &people).unwrap().name, "Kim Park");
        assert_eq!(match_person("kim", ana, &people).unwrap().name, "Kim Park");
        assert_eq!(match_person("sam lee", ana, &people).unwrap().name, "Sam Lee");
        // Two Sams: the actor's store breaks the tie.
        assert_eq!(match_person("Sam", ana, &people).unwrap().name, "Sam Jones");
        let kim = &people[4];
        let err = match_person("sam", kim, &people).unwrap_err();
        assert!(err.contains("Sam Jones (RIV)") && err.contains("Sam Lee (MUR)"));
        assert!(match_person("nobody", ana, &people).is_err());
    }

    #[test]
    fn escalation_prefers_store_managers() {
        let people = roster();
        let riv: Vec<String> = escalation_contacts_from(&people, "RIV").into_iter().map(|p| p.name).collect();
        assert_eq!(riv, vec!["Ana Ortiz"]);
        let ltn: Vec<String> = escalation_contacts_from(&people, "LTN").into_iter().map(|p| p.name).collect();
        assert_eq!(ltn, vec!["Logan Lees"]);
    }

    #[test]
    #[allow(clippy::mutable_key_type)]
    fn sender_is_least_loaded_non_root() {
        let people = roster();
        let mut load = std::collections::HashMap::new();
        load.insert(RecordId::new("user", "sam.jones"), 4);
        load.insert(RecordId::new("user", "ana"), 1);
        assert_eq!(pick_sender(&people, "RIV", &load).unwrap().name, "Ana Ortiz");
        assert_eq!(pick_sender(&people, "LTN", &load).unwrap().name, "Kim Park");
        assert!(pick_sender(&people, "ORE", &load).is_none());
    }

    #[test]
    fn brief_enforces_short_lines() {
        let brief = TicketBrief::new(
            "  In repair,\n awaiting SSD ",
            "Failing NVMe (SMART 5 reallocated).",
            "SSD replacement, data is safe.",
        )
        .unwrap();
        assert_eq!(
            brief.render(),
            "Now: In repair, awaiting SSD\nFound: Failing NVMe (SMART 5 reallocated).\nTell the customer: SSD replacement, data is safe."
        );
        assert!(TicketBrief::new("", "x", "y").is_err());
        let long = "word ".repeat(60);
        assert!(TicketBrief::new(&long, "x", "y").unwrap_err().contains("under 220"));
    }

    #[test]
    fn part_due_is_today_or_next_open_day() {
        let utc = |s: &str| chrono::DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc);
        // Fri 11:00 local: due Fri 19:00.
        assert_eq!(part_due(utc("2026-09-25T17:00:00Z")), utc("2026-09-26T01:00:00Z"));
        // Sat 18:00 local: under two hours left, Sunday closed, due Mon 19:00.
        assert_eq!(part_due(utc("2026-09-27T00:00:00Z")), utc("2026-09-29T01:00:00Z"));
    }

    #[test]
    fn part_request_round_trips() {
        let req = PartRequest {
            part: "1TB NVMe".into(),
            quantity: 1,
            from_store: "SAN".into(),
            to_store: "RIV".into(),
            service_number: Some("2155144".into()),
            product_code: Some("SSD-1TB".into()),
            product_id: Some(42),
        };
        let back = PartRequest::from_value(req.clone().into_value()).unwrap();
        assert_eq!(back, req);
    }
}
