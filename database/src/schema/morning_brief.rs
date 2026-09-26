//! The store-open morning brief: each person's due work, callbacks, checklists and overnight crashes.

use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use super::assistant::{Person, TYPE_MORNING_BRIEF};
use super::business_calendar::{CLOSE_HOUR, STORE_TZ};
use super::task_schedule::local_to_utc;
use super::{Datetime, RecordId, SurrealValue};
use crate::db;

/// Items listed per section before the rest are counted.
const MAX_LISTED: usize = 3;

const OPEN_TASKS_SQL: &str = "SELECT id, task_name, status, due_date, assignee, assigned_by, \
     assigned_by.name AS assigned_by_name, schedule, part_request, about_service, service_number \
     FROM task WHERE completed = false";

const OPEN_CHECKLISTS_SQL: &str =
    "SELECT id, assignee, service_number, customer_name FROM ai_task WHERE status = 'open'";

const OPEN_STEPS_SQL: &str =
    "SELECT ai_task_ref, count() AS open FROM ai_task_item WHERE checked = false GROUP BY ai_task_ref";

const NEW_DUMPS_SQL: &str = "SELECT task_ref, count() AS n FROM crash_sighting \
     WHERE created_at > $since AND task_ref != NONE GROUP BY task_ref";

const SHELF_ALERTS_SQL: &str = "SELECT store, count() AS n FROM shelf_candidate \
     WHERE score >= 50 AND swept_at > $since GROUP BY store";

const BRIEF_OPT_OUTS_SQL: &str = "SELECT VALUE id FROM user WHERE active = true AND ai_profile.morning_brief = false";

const BRIEFED_SINCE_SQL: &str =
    "SELECT VALUE user FROM notification WHERE notification_type = $kind AND created_at >= $since";

#[derive(Clone, Debug, Serialize, Deserialize, SurrealValue)]
struct OpenTask {
    id: RecordId,
    #[serde(default)]
    #[surreal(default)]
    task_name: String,
    status: Option<String>,
    due_date: Option<Datetime>,
    assignee: Option<RecordId>,
    assigned_by: Option<RecordId>,
    assigned_by_name: Option<String>,
    schedule: Option<RecordId>,
    part_request: Option<super::assistant::PartRequest>,
    about_service: Option<String>,
    service_number: Option<String>,
}

impl OpenTask {
    fn is_assistant_task(&self) -> bool {
        self.assigned_by.is_some() || self.schedule.is_some()
    }

    fn status_is(&self, wanted: &str) -> bool {
        self.status.as_deref().is_some_and(|s| s.trim().eq_ignore_ascii_case(wanted))
    }

    fn label(&self) -> String {
        match (&self.assigned_by_name, self.is_assistant_task()) {
            (Some(from), true) => format!("{} (from {})", self.task_name, first_word(from)),
            _ => self.task_name.clone(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, SurrealValue)]
struct Checklist {
    id: RecordId,
    assignee: Option<RecordId>,
    service_number: Option<String>,
    customer_name: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, SurrealValue)]
struct StepCount {
    ai_task_ref: RecordId,
    open: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, SurrealValue)]
struct DumpCount {
    task_ref: RecordId,
    n: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, SurrealValue)]
struct StoreCount {
    store: Option<String>,
    n: i64,
}

fn first_word(s: &str) -> &str {
    s.split_whitespace().next().unwrap_or(s)
}

/// What one person's brief says; empty sections are left out.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BriefInput {
    pub first_name: String,
    pub due_today: Vec<String>,
    pub overdue: Vec<String>,
    pub parts_to_send: Vec<String>,
    pub follow_ups: Vec<String>,
    pub checklists: Vec<String>,
    pub new_dumps: Vec<String>,
    pub waiting_parts: usize,
    pub in_repair: usize,
    pub in_qc: usize,
    pub todo: usize,
    pub store_lines: Vec<String>,
}

fn listed(items: &[String]) -> String {
    let shown: Vec<&str> = items.iter().take(MAX_LISTED).map(String::as_str).collect();
    let rest = items.len().saturating_sub(MAX_LISTED);
    if rest > 0 { format!("{}; +{rest} more", shown.join("; ")) } else { shown.join("; ") }
}

/// The brief text, or `None` when there is nothing to say.
pub fn format_brief(input: &BriefInput) -> Option<String> {
    let mut lines = Vec::new();
    let sections = [
        ("Overdue", &input.overdue),
        ("Due today", &input.due_today),
        ("Parts to send", &input.parts_to_send),
        ("Callbacks owed", &input.follow_ups),
        ("AI checklists", &input.checklists),
        ("New crash dumps", &input.new_dumps),
    ];
    for (title, items) in sections {
        if !items.is_empty() {
            lines.push(format!("• {title} ({}): {}", items.len(), listed(items)));
        }
    }
    let mut counts = Vec::new();
    for (n, what) in [
        (input.in_repair, "in repair"),
        (input.in_qc, "in QC"),
        (input.todo, "to do"),
        (input.waiting_parts, "waiting on parts"),
    ] {
        if n > 0 {
            counts.push(format!("{n} {what}"));
        }
    }
    if !counts.is_empty() {
        lines.push(format!("• Your tickets: {}", counts.join(", ")));
    }
    for store in &input.store_lines {
        lines.push(format!("• {store}"));
    }
    if lines.is_empty() {
        return None;
    }
    Some(format!("Morning, {}.\n{}", input.first_name, lines.join("\n")))
}

fn local_date(t: DateTime<Utc>) -> NaiveDate {
    t.with_timezone(&STORE_TZ).date_naive()
}

/// Store close on the open day before `now`, the window "overnight" covers.
pub fn previous_close(now: DateTime<Utc>) -> DateTime<Utc> {
    let mut day = local_date(now) - Duration::days(1);
    for _ in 0..7 {
        if super::business_calendar::is_open_day(chrono::Datelike::weekday(&day)) {
            break;
        }
        day -= Duration::days(1);
    }
    day.and_hms_opt(CLOSE_HOUR, 0, 0).and_then(local_to_utc).unwrap_or(now - Duration::hours(15))
}

/// Start of today, store-local, in UTC.
pub fn start_of_day(now: DateTime<Utc>) -> DateTime<Utc> {
    local_date(now).and_hms_opt(0, 0, 0).and_then(local_to_utc).unwrap_or(now - Duration::hours(12))
}

#[derive(Default)]
struct StoreTally {
    open: usize,
    follow_ups: usize,
    waiting_parts: usize,
    overdue: usize,
    shelf: i64,
}

fn store_line(store: &str, t: &StoreTally) -> String {
    let mut parts = vec![format!("{} open", t.open)];
    for (n, what) in
        [(t.follow_ups, "callbacks owed"), (t.waiting_parts, "waiting on parts"), (t.overdue, "overdue reminders")]
    {
        if n > 0 {
            parts.push(format!("{n} {what}"));
        }
    }
    if t.shelf > 0 {
        parts.push(format!("{} shelf alerts", t.shelf));
    }
    format!("{store}: {}", parts.join(", "))
}

/// Today's brief for each active person who has something to hear.
#[allow(clippy::mutable_key_type)]
pub async fn build_all(now: DateTime<Utc>) -> anyhow::Result<Vec<(Person, String)>> {
    let people = Person::active_all().await?;
    let since = previous_close(now);
    let today = local_date(now);
    let today_start = start_of_day(now);

    let tasks: Vec<OpenTask> = db().query(OPEN_TASKS_SQL).await?.check()?.take(0)?;
    let checklists: Vec<Checklist> = db().query(OPEN_CHECKLISTS_SQL).await?.check()?.take(0)?;
    let steps: Vec<StepCount> = db().query(OPEN_STEPS_SQL).await?.check()?.take(0)?;
    let dumps: Vec<DumpCount> =
        db().query(NEW_DUMPS_SQL).bind(("since", Datetime::from(since))).await?.check()?.take(0)?;
    let shelf: Vec<StoreCount> =
        db().query(SHELF_ALERTS_SQL).bind(("since", Datetime::from(since))).await?.check()?.take(0)?;
    let opted_out: HashSet<RecordId> =
        db().query(BRIEF_OPT_OUTS_SQL).await?.check()?.take::<Vec<RecordId>>(0)?.into_iter().collect();

    let open_steps: HashMap<RecordId, i64> = steps.into_iter().map(|s| (s.ai_task_ref, s.open)).collect();
    let dumps_by_task: HashMap<RecordId, i64> = dumps.into_iter().map(|d| (d.task_ref, d.n)).collect();
    let store_of: HashMap<RecordId, String> = people.iter().map(|p| (p.id.clone(), p.store.clone())).collect();

    let mut tallies: BTreeMap<String, StoreTally> = BTreeMap::new();
    for t in &tasks {
        let Some(store) = t.assignee.as_ref().and_then(|a| store_of.get(a)) else { continue };
        let tally = tallies.entry(store.clone()).or_default();
        if t.is_assistant_task() {
            if t.due_date.is_some_and(|d| d.into_inner() < today_start) {
                tally.overdue += 1;
            }
        } else {
            tally.open += 1;
            tally.follow_ups += usize::from(t.status_is("follow up"));
            tally.waiting_parts += usize::from(t.status_is("pending spo"));
        }
    }
    for s in shelf {
        if let Some(store) = s.store {
            tallies.entry(store).or_default().shelf += s.n;
        }
    }

    let mut out = Vec::new();
    for person in people {
        if opted_out.contains(&person.id) {
            continue;
        }
        let mine: Vec<&OpenTask> = tasks.iter().filter(|t| t.assignee.as_ref() == Some(&person.id)).collect();
        let mut input = BriefInput { first_name: person.first_name().to_string(), ..Default::default() };
        for t in &mine {
            if t.is_assistant_task() {
                let due = t.due_date.map(Datetime::into_inner);
                if let Some(part) = &t.part_request {
                    let sn = part.service_number.as_deref().map(|s| format!(" for {s}")).unwrap_or_default();
                    input.parts_to_send.push(format!("{}× {} to {}{sn}", part.quantity, part.part, part.to_store));
                } else if due.is_some_and(|d| d < today_start) {
                    input.overdue.push(t.label());
                } else if due.is_some_and(|d| local_date(d) == today) {
                    input.due_today.push(t.label());
                }
                continue;
            }
            if t.status_is("follow up") {
                input.follow_ups.push(t.task_name.clone());
            } else if t.status_is("pending spo") {
                input.waiting_parts += 1;
            } else if t.status_is("in repair") {
                input.in_repair += 1;
            } else if t.status_is("qc") {
                input.in_qc += 1;
            } else if t.status_is("todo") {
                input.todo += 1;
            }
            if let Some(n) = dumps_by_task.get(&t.id) {
                input.new_dumps.push(format!("{} ({n})", t.task_name));
            }
        }
        for c in checklists.iter().filter(|c| c.assignee.as_ref() == Some(&person.id)) {
            let open = open_steps.get(&c.id).copied().unwrap_or(0);
            if open > 0 {
                let label = match (c.customer_name.as_deref(), c.service_number.as_deref()) {
                    (Some(name), Some(sn)) => format!("{name} - {sn}"),
                    (Some(only), None) | (None, Some(only)) => only.to_string(),
                    (None, None) => "ticket".to_string(),
                };
                input.checklists.push(format!("{label} ({open} steps)"));
            }
        }
        if person.is_root() {
            input.store_lines = tallies.iter().map(|(store, t)| store_line(store, t)).collect();
        } else if let Some(t) = tallies.get(&person.store).filter(|_| person.is_manager()) {
            input.store_lines.push(store_line(&person.store, t));
        }
        if let Some(text) = format_brief(&input) {
            out.push((person, text));
        }
    }
    Ok(out)
}

/// People who already have a brief from today.
#[allow(clippy::mutable_key_type)]
pub async fn briefed_today(now: DateTime<Utc>) -> anyhow::Result<HashSet<RecordId>> {
    let ids: Vec<RecordId> = db()
        .query(BRIEFED_SINCE_SQL)
        .bind(("kind", TYPE_MORNING_BRIEF))
        .bind(("since", Datetime::from(start_of_day(now))))
        .await?
        .check()?
        .take(0)?;
    Ok(ids.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn empty_brief_is_skipped() {
        let input = BriefInput { first_name: "Sam".into(), ..Default::default() };
        assert_eq!(format_brief(&input), None);
    }

    #[test]
    fn brief_lists_three_then_counts_the_rest() {
        let input = BriefInput {
            first_name: "Sam".into(),
            due_today: vec!["Count paste (from Logan)".into()],
            follow_ups: (1..=5).map(|i| format!("Customer {i} - 215500{i}")).collect(),
            in_repair: 2,
            waiting_parts: 1,
            ..Default::default()
        };
        let text = format_brief(&input).unwrap();
        assert_eq!(
            text,
            "Morning, Sam.\n\
             • Due today (1): Count paste (from Logan)\n\
             • Callbacks owed (5): Customer 1 - 2155001; Customer 2 - 2155002; Customer 3 - 2155003; +2 more\n\
             • Your tickets: 2 in repair, 1 waiting on parts"
        );
    }

    #[test]
    fn store_lines_follow_personal_sections() {
        let input = BriefInput {
            first_name: "Ana".into(),
            store_lines: vec!["RIV: 14 open, 3 callbacks owed".into()],
            ..Default::default()
        };
        assert_eq!(format_brief(&input).unwrap(), "Morning, Ana.\n• RIV: 14 open, 3 callbacks owed");
    }

    #[test]
    fn overnight_window_starts_at_the_last_open_close() {
        // Monday 10:00 local looks back to Saturday 19:00 local.
        let monday = utc("2026-09-28T16:00:00Z");
        assert_eq!(previous_close(monday), utc("2026-09-27T01:00:00Z"));
        // Tuesday 10:00 local looks back to Monday 19:00 local.
        assert_eq!(previous_close(utc("2026-09-29T16:00:00Z")), utc("2026-09-29T01:00:00Z"));
        assert_eq!(start_of_day(monday), utc("2026-09-28T06:00:00Z"));
    }

    #[test]
    fn store_line_skips_zero_counts() {
        let t = StoreTally { open: 5, follow_ups: 2, shelf: 1, ..Default::default() };
        assert_eq!(store_line("LTN", &t), "LTN: 5 open, 2 callbacks owed, 1 shelf alerts");
    }
}
