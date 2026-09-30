//! Read-only lookups sized for spoken answers: one service order's whole status, one person's task list.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::task_schedule::local_label;
use super::{Datetime, RecordId, SurrealValue};
use crate::db;

/// Longest free-text field a reply carries, in characters.
const TEXT_MAX_CHARS: usize = 320;
/// Orders offered back when a partial number matches several.
const MAX_MATCHES: usize = 5;
/// Fewest digits a partial service number must have.
const MIN_PARTIAL_DIGITS: usize = 4;
/// Most tasks one list returns.
pub const MAX_TASKS: u32 = 50;

const ORDER_BY_NUMBER_SQL: &str =
    "SELECT id, service_number, customer.name AS customer, created_at FROM service_order WHERE service_number == $sn";

const ORDERS_ENDING_SQL: &str = "SELECT id, service_number, customer.name AS customer, created_at FROM service_order \
     WHERE string::ends_with(service_number ?? '', $digits) ORDER BY created_at DESC LIMIT 6";

const ORDER_STATUS_SQL: &str = "
LET $sn = $order.service_number;
LET $computer = $order.computer;
SELECT service_number, created_at, tech, sales_rep, checkin_rep, ticket_total, checkin_notes,
    customer.name AS customer, customer.phone_number AS phone,
    computer.hostname AS hostname, computer.device_model AS model, computer.product_name AS product
    FROM $order;
SELECT task_name, status, completed, priority, due_date, assignee.name AS assignee, created_at FROM task
    WHERE service_ticket == $order OR ($sn != NONE AND service_number == $sn)
    ORDER BY created_at DESC LIMIT 3;
SELECT status, started_at, last_activity_at, ended_at, diagnosed_at, summary, current_theory,
    theory_next_step, theory_confidence FROM diagnostic_session
    WHERE service_order == $order OR ($computer != NONE AND computer_id == $computer)
    ORDER BY started_at DESC LIMIT 2;
SELECT title, status, created_at, completed_at, current_theory, theory_next_step,
    array::len((SELECT VALUE id FROM ai_task_item WHERE ai_task_ref == $parent.id AND checked == false)) AS items_open,
    array::len((SELECT VALUE id FROM ai_task_item WHERE ai_task_ref == $parent.id)) AS items
    FROM ai_task WHERE $sn != NONE AND service_number == $sn
    ORDER BY created_at DESC LIMIT 2;
";

const TASKS_FOR_SQL: &str = "
SELECT id, task_name, status, priority, service_number, due_date, completed, assigned_by.name AS assigned_by
    FROM task WHERE assignee == $who AND ($all OR completed == false)
    ORDER BY due_date ASC LIMIT $limit;
SELECT VALUE count() FROM task WHERE assignee == $who AND completed == false GROUP ALL;
";

/// What a spoken or partial service number resolves to.
#[derive(Debug, Clone, PartialEq)]
pub enum OrderLookup {
    Found(RecordId),
    Several(Vec<OrderMatch>),
    Missing,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, SurrealValue)]
pub struct OrderMatch {
    pub id: RecordId,
    #[serde(default)]
    #[surreal(default)]
    pub service_number: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub customer: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub created_at: Option<Datetime>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, SurrealValue)]
pub struct OrderHead {
    #[serde(default)]
    #[surreal(default)]
    pub service_number: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub created_at: Option<Datetime>,
    #[serde(default)]
    #[surreal(default)]
    pub tech: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub sales_rep: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub checkin_rep: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub ticket_total: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub checkin_notes: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub customer: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub phone: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub hostname: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub model: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub product: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, SurrealValue)]
pub struct OrderTask {
    #[serde(default)]
    #[surreal(default)]
    pub task_name: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub status: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub completed: Option<bool>,
    #[serde(default)]
    #[surreal(default)]
    pub priority: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub due_date: Option<Datetime>,
    #[serde(default)]
    #[surreal(default)]
    pub assignee: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub created_at: Option<Datetime>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, SurrealValue)]
pub struct OrderDiagnosis {
    #[serde(default)]
    #[surreal(default)]
    pub status: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub started_at: Option<Datetime>,
    #[serde(default)]
    #[surreal(default)]
    pub last_activity_at: Option<Datetime>,
    #[serde(default)]
    #[surreal(default)]
    pub ended_at: Option<Datetime>,
    #[serde(default)]
    #[surreal(default)]
    pub diagnosed_at: Option<Datetime>,
    #[serde(default)]
    #[surreal(default)]
    pub summary: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub current_theory: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub theory_next_step: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub theory_confidence: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, SurrealValue)]
pub struct OrderAiTask {
    #[serde(default)]
    #[surreal(default)]
    pub title: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub status: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub created_at: Option<Datetime>,
    #[serde(default)]
    #[surreal(default)]
    pub completed_at: Option<Datetime>,
    #[serde(default)]
    #[surreal(default)]
    pub current_theory: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub theory_next_step: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub items_open: Option<i64>,
    #[serde(default)]
    #[surreal(default)]
    pub items: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, SurrealValue)]
pub struct TaskRow {
    #[serde(default)]
    #[surreal(default)]
    pub task_name: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub status: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub priority: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub service_number: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub due_date: Option<Datetime>,
    #[serde(default)]
    #[surreal(default)]
    pub completed: Option<bool>,
    #[serde(default)]
    #[surreal(default)]
    pub assigned_by: Option<String>,
}

/// The digits of a spoken service number: "SO-2155144" and "#2155144" both give "2155144".
pub fn service_digits(raw: &str) -> String {
    raw.chars().filter(char::is_ascii_digit).collect()
}

/// Resolves a service number exactly, then by its trailing digits when speech dropped the front.
pub async fn find_order(raw: &str) -> anyhow::Result<OrderLookup> {
    let digits = service_digits(raw);
    let typed = raw.trim().trim_start_matches('#').trim();
    for sn in [typed, digits.as_str()] {
        if sn.is_empty() {
            continue;
        }
        let rows: Vec<OrderMatch> =
            db().query(ORDER_BY_NUMBER_SQL).bind(("sn", sn.to_string())).await?.check()?.take(0)?;
        if let Some(order) = rows.into_iter().next() {
            return Ok(OrderLookup::Found(order.id));
        }
    }
    if digits.len() < MIN_PARTIAL_DIGITS {
        return Ok(OrderLookup::Missing);
    }
    let rows: Vec<OrderMatch> = db().query(ORDERS_ENDING_SQL).bind(("digits", digits)).await?.check()?.take(0)?;
    Ok(pick_match(rows))
}

fn pick_match(mut rows: Vec<OrderMatch>) -> OrderLookup {
    match rows.len() {
        0 => OrderLookup::Missing,
        1 => OrderLookup::Found(rows.remove(0).id),
        _ => {
            rows.truncate(MAX_MATCHES);
            OrderLookup::Several(rows)
        }
    }
}

/// The order's status as compact JSON, or `None` when the record is gone.
pub async fn order_status(order: &RecordId, now: DateTime<Utc>) -> anyhow::Result<Option<Value>> {
    let mut resp = db().query(ORDER_STATUS_SQL).bind(("order", order.clone())).await?.check()?;
    let head: Vec<OrderHead> = resp.take(2)?;
    let tasks: Vec<OrderTask> = resp.take(3)?;
    let diagnoses: Vec<OrderDiagnosis> = resp.take(4)?;
    let ai_tasks: Vec<OrderAiTask> = resp.take(5)?;
    Ok(head.into_iter().next().map(|head| order_status_json(&head, &tasks, &diagnoses, &ai_tasks, now)))
}

/// One person's tasks, oldest due first, and their full open count.
pub async fn tasks_for(who: &RecordId, include_completed: bool, limit: u32) -> anyhow::Result<(i64, Vec<TaskRow>)> {
    let mut resp = db()
        .query(TASKS_FOR_SQL)
        .bind(("who", who.clone()))
        .bind(("all", include_completed))
        .bind(("limit", i64::from(limit.clamp(1, MAX_TASKS))))
        .await?
        .check()?;
    let rows: Vec<TaskRow> = resp.take(0)?;
    let open: Vec<i64> = resp.take(1)?;
    Ok((open.first().copied().unwrap_or(0), rows))
}

fn clip(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() <= TEXT_MAX_CHARS {
        return text.to_string();
    }
    let mut out: String = text.chars().take(TEXT_MAX_CHARS - 1).collect();
    out.push('…');
    out
}

fn when(t: &Option<Datetime>) -> Option<String> {
    t.clone().map(|t| local_label(t.into_inner()))
}

fn text(t: &Option<String>) -> Option<String> {
    t.as_deref().map(clip).filter(|t| !t.is_empty())
}

fn put(map: &mut Map<String, Value>, key: &str, value: Option<impl Into<Value>>) {
    if let Some(v) = value {
        map.insert(key.to_string(), v.into());
    }
}

/// Compact status JSON; store-local times, empty fields left out.
pub fn order_status_json(
    head: &OrderHead,
    tasks: &[OrderTask],
    diagnoses: &[OrderDiagnosis],
    ai_tasks: &[OrderAiTask],
    now: DateTime<Utc>,
) -> Value {
    let mut out = Map::new();
    put(&mut out, "service_number", head.service_number.clone());
    put(&mut out, "customer", text(&head.customer));
    put(&mut out, "phone", text(&head.phone));
    let computer = [("hostname", &head.hostname), ("model", &head.model), ("product", &head.product)]
        .into_iter()
        .filter_map(|(k, v)| text(v).map(|v| (k.to_string(), Value::String(v))))
        .collect::<Map<_, _>>();
    if !computer.is_empty() {
        out.insert("computer".into(), Value::Object(computer));
    }
    put(&mut out, "checked_in", when(&head.created_at));
    put(&mut out, "checked_in_by", text(&head.checkin_rep));
    put(&mut out, "tech", text(&head.tech));
    put(&mut out, "sales_rep", text(&head.sales_rep));
    put(&mut out, "ticket_total", text(&head.ticket_total));
    put(&mut out, "checkin_notes", text(&head.checkin_notes));

    let tasks: Vec<Value> = tasks
        .iter()
        .map(|t| {
            let mut m = Map::new();
            put(&mut m, "name", text(&t.task_name));
            put(&mut m, "status", text(&t.status));
            put(&mut m, "done", t.completed);
            put(&mut m, "assignee", text(&t.assignee));
            put(&mut m, "priority", text(&t.priority));
            put(&mut m, "due", when(&t.due_date));
            let overdue = t.completed != Some(true) && t.due_date.clone().is_some_and(|d| d.into_inner() < now);
            if overdue {
                m.insert("overdue".into(), Value::Bool(true));
            }
            Value::Object(m)
        })
        .collect();
    if !tasks.is_empty() {
        out.insert("service_tasks".into(), Value::Array(tasks));
    }

    let diagnoses: Vec<Value> = diagnoses
        .iter()
        .map(|d| {
            let mut m = Map::new();
            put(&mut m, "status", text(&d.status));
            put(&mut m, "started", when(&d.started_at));
            put(&mut m, "last_activity", when(&d.last_activity_at));
            put(&mut m, "diagnosed", when(&d.diagnosed_at));
            put(&mut m, "ended", when(&d.ended_at));
            put(&mut m, "theory", text(&d.current_theory));
            put(&mut m, "next_step", text(&d.theory_next_step));
            put(&mut m, "confidence", text(&d.theory_confidence));
            put(&mut m, "summary", text(&d.summary));
            Value::Object(m)
        })
        .collect();
    if !diagnoses.is_empty() {
        out.insert("diagnoses".into(), Value::Array(diagnoses));
    }

    let ai_tasks: Vec<Value> = ai_tasks
        .iter()
        .map(|a| {
            let mut m = Map::new();
            put(&mut m, "title", text(&a.title));
            put(&mut m, "status", text(&a.status));
            put(&mut m, "created", when(&a.created_at));
            put(&mut m, "completed", when(&a.completed_at));
            put(&mut m, "steps_open", a.items_open);
            put(&mut m, "steps", a.items);
            put(&mut m, "theory", text(&a.current_theory));
            put(&mut m, "next_step", text(&a.theory_next_step));
            Value::Object(m)
        })
        .collect();
    if !ai_tasks.is_empty() {
        out.insert("ai_tasks".into(), Value::Array(ai_tasks));
    }
    Value::Object(out)
}

/// Candidate orders for a partial number, newest first.
pub fn matches_json(matches: &[OrderMatch]) -> Value {
    Value::Array(
        matches
            .iter()
            .map(|m| {
                let mut o = Map::new();
                put(&mut o, "service_number", m.service_number.clone());
                put(&mut o, "customer", text(&m.customer));
                put(&mut o, "checked_in", when(&m.created_at));
                Value::Object(o)
            })
            .collect(),
    )
}

/// A person's task list as compact JSON; `open` is the full open count, whatever the limit.
pub fn tasks_json(person: &str, open: i64, rows: &[TaskRow], now: DateTime<Utc>) -> Value {
    let tasks: Vec<Value> = rows
        .iter()
        .map(|t| {
            let mut m = Map::new();
            put(&mut m, "name", text(&t.task_name));
            put(&mut m, "status", text(&t.status));
            put(&mut m, "priority", text(&t.priority));
            put(&mut m, "service_number", text(&t.service_number));
            put(&mut m, "due", when(&t.due_date));
            put(&mut m, "assigned_by", text(&t.assigned_by));
            if t.completed == Some(true) {
                m.insert("done".into(), Value::Bool(true));
            } else if t.due_date.clone().is_some_and(|d| d.into_inner() < now) {
                m.insert("overdue".into(), Value::Bool(true));
            }
            Value::Object(m)
        })
        .collect();
    let overdue = tasks.iter().filter(|t| t.get("overdue").is_some()).count();
    json!({ "assignee": person, "open": open, "overdue_shown": overdue, "shown": tasks.len(), "tasks": tasks })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(y: i32, mo: u32, d: u32, h: u32) -> Option<Datetime> {
        Some(Datetime::from(Utc.with_ymd_and_hms(y, mo, d, h, 0, 0).unwrap()))
    }

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 30, 20, 0, 0).unwrap()
    }

    fn order(sn: &str) -> OrderMatch {
        OrderMatch {
            id: RecordId::new("service_order", sn),
            service_number: Some(sn.to_string()),
            customer: Some("Andrea Brandon".into()),
            created_at: at(2026, 9, 23, 17),
        }
    }

    #[test]
    fn digits_come_out_of_spoken_numbers() {
        assert_eq!(service_digits("SO-2155144"), "2155144");
        assert_eq!(service_digits("#21 55 144"), "2155144");
        assert_eq!(service_digits("to 155144"), "155144");
    }

    #[test]
    fn one_partial_match_resolves_and_several_are_offered() {
        assert_eq!(pick_match(vec![]), OrderLookup::Missing);
        assert_eq!(pick_match(vec![order("2155144")]), OrderLookup::Found(RecordId::new("service_order", "2155144")));
        let many: Vec<OrderMatch> = (0..8).map(|i| order(&format!("21{i}5144"))).collect();
        match pick_match(many) {
            OrderLookup::Several(rows) => assert_eq!(rows.len(), MAX_MATCHES),
            other => panic!("expected several, got {other:?}"),
        }
    }

    #[test]
    fn status_json_uses_store_time_and_drops_empty_fields() {
        let head = OrderHead {
            service_number: Some("2155144".into()),
            created_at: at(2026, 9, 23, 17),
            customer: Some("Andrea Brandon".into()),
            hostname: Some("Owner-PC".into()),
            checkin_notes: Some("x".repeat(1000)),
            ..Default::default()
        };
        let tasks = [OrderTask {
            task_name: Some("Back-office tuneup".into()),
            status: Some("In Progress".into()),
            completed: Some(false),
            due_date: at(2026, 9, 29, 1),
            assignee: Some("Logan Lees".into()),
            ..Default::default()
        }];
        let diagnoses = [OrderDiagnosis {
            status: Some("escalated".into()),
            current_theory: Some("EXPO profile instability".into()),
            ..Default::default()
        }];
        let v = order_status_json(&head, &tasks, &diagnoses, &[], now());
        assert_eq!(v["customer"], "Andrea Brandon");
        assert_eq!(v["computer"]["hostname"], "Owner-PC");
        assert!(v["checked_in"].as_str().unwrap().starts_with("Wed Sep 23 11:00"), "{}", v["checked_in"]);
        assert_eq!(v["checkin_notes"].as_str().unwrap().chars().count(), TEXT_MAX_CHARS);
        assert_eq!(v["service_tasks"][0]["overdue"], true);
        assert_eq!(v["diagnoses"][0]["theory"], "EXPO profile instability");
        assert!(v.get("phone").is_none() && v.get("ai_tasks").is_none());
    }

    #[test]
    fn task_list_flags_overdue_and_keeps_the_full_open_count() {
        let rows = [
            TaskRow { task_name: Some("Call Jennie".into()), due_date: at(2026, 9, 28, 1), ..Default::default() },
            TaskRow { task_name: Some("Order SSD".into()), due_date: at(2026, 10, 2, 1), ..Default::default() },
        ];
        let v = tasks_json("Logan Lees (WAR)", 16, &rows, now());
        assert_eq!(v["open"], 16);
        assert_eq!(v["shown"], 2);
        assert_eq!(v["overdue_shown"], 1);
        assert_eq!(v["tasks"][0]["overdue"], true);
        assert!(v["tasks"][1].get("overdue").is_none());
    }
}
