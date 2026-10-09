//! Read-only lookups sized for spoken answers: one service order's whole status, one person's task list,
//! the orders PrestaShop took in over a day or range.

use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, Datelike, Days, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::business_calendar::{STORE_TZ, parse_store_local};
use super::prestashop::{Order, OrderState, OrderType, Prestashop};
use super::task_schedule::{local_label, weekday_from_name};
use super::{Datetime, RecordId, RecordIdExt, Store, SurrealValue};
use crate::db;

/// Longest free-text field a reply carries, in characters.
const TEXT_MAX_CHARS: usize = 320;
/// Orders offered back when a partial number matches several.
const MAX_MATCHES: usize = 5;
/// Fewest digits a partial service number must have.
const MIN_PARTIAL_DIGITS: usize = 4;
/// Most tasks one list returns.
pub const MAX_TASKS: u32 = 50;
/// Longest range one placed-orders call covers, in days.
pub const MAX_RANGE_DAYS: i64 = 31;
/// Most orders a placed-orders reply lists one by one; its counts cover every order.
const MAX_LISTED_ORDERS: usize = 60;
/// PrestaShop `limit` for a placed-orders range.
const PLACED_LIMIT: &str = "0,3000";
const PLACED_DISPLAY: &str = "[id,reference,id_order_type,id_store,current_state,date_add]";
/// Fewest digits a full service number has; shorter ones are never looked up by PrestaShop id.
const FULL_NUMBER_DIGITS: usize = 7;

const ORDER_BY_NUMBER_SQL: &str =
    "SELECT id, service_number, customer.name AS customer, created_at FROM service_order WHERE service_number == $sn";

const ORDERS_ENDING_SQL: &str = "SELECT id, service_number, customer.name AS customer, created_at FROM service_order \
     WHERE string::ends_with(service_number ?? '', $digits) ORDER BY created_at DESC LIMIT 6";

const ORDER_STATUS_SQL: &str = "
LET $sn = $order.service_number;
LET $computer = $order.computer;
SELECT service_number, tech, sales_rep, checkin_rep, ticket_total, checkin_notes,
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
    pub id: Option<RecordId>,
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

/// PrestaShop's side of an order: its kind and store, when it came in, and its state now.
#[derive(Debug, Clone, PartialEq)]
pub struct Placement {
    pub number: String,
    pub kind: &'static str,
    pub store: String,
    pub state: String,
    pub placed: Option<DateTime<Utc>>,
}

impl Placement {
    pub fn from_order(order: &Order) -> Self {
        Self {
            number: order.id.clone(),
            kind: order_kind(&order.id_order_type),
            store: store_code(&order.id_store),
            state: OrderState::try_name_from_id_str(&order.current_state)
                .map_or_else(|| format!("state {}", order.current_state), str::to_string),
            placed: parse_store_local(&order.date_add).ok(),
        }
    }
}

fn order_kind(id: &str) -> &'static str {
    match OrderType::try_from_id_str(id) {
        Some(OrderType::ServiceOrder) => "service",
        Some(OrderType::SalesOrder) => "sales",
        Some(OrderType::RepairOrder) => "repair",
        Some(OrderType::ReadyToRoll) => "ready_to_roll",
        Some(OrderType::Bsd) => "bsd",
        Some(OrderType::Rci) => "rci",
        Some(OrderType::Unknown) | None => "other",
    }
}

/// An order kind as `orders_placed` names it, or `None` for an unknown name.
pub fn parse_kind(raw: &str) -> Option<&'static str> {
    Some(match raw.trim().to_ascii_lowercase().replace([' ', '-'], "_").as_str() {
        "service" | "services" => "service",
        "sale" | "sales" => "sales",
        "repair" | "repairs" => "repair",
        "ready_to_roll" | "rtr" => "ready_to_roll",
        "bsd" => "bsd",
        "rci" => "rci",
        _ => return None,
    })
}

fn store_code(id: &str) -> String {
    Store::try_from_presta_store_id(id).map_or_else(|| format!("store {id}"), |s| s.as_str().to_string())
}

/// The store-local date at `now`.
pub fn store_today(now: DateTime<Utc>) -> NaiveDate {
    now.with_timezone(&STORE_TZ).date_naive()
}

/// "today", "yesterday", a weekday name (its latest date up to today) or YYYY-MM-DD.
pub fn parse_day(raw: &str, today: NaiveDate) -> Option<NaiveDate> {
    let raw = raw.trim().to_ascii_lowercase();
    match raw.as_str() {
        "" | "today" => Some(today),
        "yesterday" => today.pred_opt(),
        _ => NaiveDate::parse_from_str(&raw, "%Y-%m-%d").ok().or_else(|| {
            let day = weekday_from_name(&raw)?;
            let back = (7 + today.weekday().num_days_from_monday() - day.num_days_from_monday()) % 7;
            today.checked_sub_days(Days::new(u64::from(back)))
        }),
    }
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
    let Some(head) = head.into_iter().next() else {
        return Ok(None);
    };
    let placed = match head.service_number.as_deref() {
        Some(sn) => placement(sn).await.inspect_err(|e| log::warn!("PrestaShop order {sn}: {e:#}")).ok(),
        None => None,
    };
    Ok(Some(order_status_json(&head, placed.as_ref(), &tasks, &diagnoses, &ai_tasks, now)))
}

/// PrestaShop's record of one order by its number.
pub async fn placement(number: &str) -> anyhow::Result<Placement> {
    let order: Order = Prestashop::default().request_subresources_by_id_wasm("orders", "order", number).await?;
    Ok(Placement::from_order(&order))
}

/// PrestaShop's record of an order MasterTech has no row for; only full service numbers are tried.
pub async fn placement_for_missing(raw: &str) -> Option<Placement> {
    let digits = service_digits(raw);
    if digits.len() < FULL_NUMBER_DIGITS {
        return None;
    }
    placement(&digits).await.inspect_err(|e| log::info!("PrestaShop order {digits}: {e:#}")).ok()
}

/// Orders PrestaShop took in from `from` through `through` (store-local days), oldest first.
pub async fn orders_placed(from: NaiveDate, through: NaiveDate) -> anyhow::Result<Vec<Placement>> {
    let range = format!("[{from} 00:00:00,{through} 23:59:59]");
    let mut api = Prestashop::default();
    api.display = PLACED_DISPLAY;
    let query = HashMap::from([
        ("filter[date_add]", range.as_str()),
        ("date", "1"),
        ("sort", "[date_add_ASC]"),
        ("limit", PLACED_LIMIT),
        ("output_format", "JSON"),
    ]);
    let rows: Vec<Order> = api.request_resources_checked("orders", query).await?;
    Ok(rows.iter().map(Placement::from_order).collect())
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

/// `kind`, `store`, `placed` and `order_state` from PrestaShop.
fn put_placement(out: &mut Map<String, Value>, p: &Placement) {
    put(out, "kind", Some(p.kind));
    put(out, "store", Some(p.store.clone()));
    put(out, "placed", p.placed.map(local_label));
    put(out, "order_state", Some(p.state.clone()));
}

/// Compact status JSON; store-local times, empty fields left out.
pub fn order_status_json(
    head: &OrderHead,
    placed: Option<&Placement>,
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
    if let Some(p) = placed {
        put_placement(&mut out, p);
    }
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
                Value::Object(o)
            })
            .collect(),
    )
}

/// Status of an order only PrestaShop has.
pub fn placement_json(p: &Placement) -> Value {
    let mut out = Map::new();
    put(&mut out, "service_number", Some(p.number.clone()));
    put_placement(&mut out, p);
    out.insert("in_mastertech".into(), Value::Bool(false));
    out.insert(
        "note".into(),
        json!("PrestaShop has this order but MasterTech has not loaded it yet, so it has no task or diagnosis"),
    );
    Value::Object(out)
}

fn day_label(d: NaiveDate) -> String {
    d.format("%a %b %-d").to_string()
}

/// Clock time alone within one day, else the full store-time label.
fn placed_label(t: Option<DateTime<Utc>>, one_day: bool) -> Option<String> {
    t.map(|t| if one_day { t.with_timezone(&STORE_TZ).format("%H:%M").to_string() } else { local_label(t) })
}

/// Orders placed over a range in store time: counts by kind, store and day, and each order while they fit.
pub fn placed_json(from: NaiveDate, through: NaiveDate, orders: &[Placement]) -> Value {
    let one_day = from == through;
    let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
    let mut stores: BTreeMap<&str, usize> = BTreeMap::new();
    let mut days: BTreeMap<NaiveDate, usize> = BTreeMap::new();
    for o in orders {
        *kinds.entry(o.kind).or_default() += 1;
        *stores.entry(o.store.as_str()).or_default() += 1;
        if let Some(t) = o.placed {
            *days.entry(store_today(t)).or_default() += 1;
        }
    }
    let mut out = Map::new();
    if one_day {
        out.insert("day".into(), json!(day_label(from)));
    } else {
        out.insert("from".into(), json!(day_label(from)));
        out.insert("through".into(), json!(day_label(through)));
    }
    out.insert("total".into(), json!(orders.len()));
    out.insert("by_kind".into(), json!(kinds));
    out.insert("by_store".into(), json!(stores));
    if !one_day {
        let by_day = days.iter().map(|(d, n)| json!({ "day": day_label(*d), "orders": n })).collect();
        out.insert("by_day".into(), Value::Array(by_day));
    }
    put(&mut out, "first", orders.first().and_then(|o| placed_label(o.placed, one_day)));
    put(&mut out, "last", orders.last().and_then(|o| placed_label(o.placed, one_day)));
    if orders.len() <= MAX_LISTED_ORDERS {
        let list = orders
            .iter()
            .map(|o| {
                let mut m = Map::new();
                put(&mut m, "number", Some(o.number.clone()));
                put(&mut m, "kind", Some(o.kind));
                put(&mut m, "store", Some(o.store.clone()));
                put(&mut m, "placed", placed_label(o.placed, one_day));
                put(&mut m, "state", Some(o.state.clone()));
                Value::Object(m)
            })
            .collect();
        out.insert("orders".into(), Value::Array(list));
    } else {
        out.insert(
            "orders_omitted".into(),
            json!(format!("more than {MAX_LISTED_ORDERS}; narrow by store, kind or day to list them")),
        );
    }
    Value::Object(out)
}

/// A person's task list as compact JSON; `open` is the full open count, whatever the limit.
pub fn tasks_json(person: &str, open: i64, rows: &[TaskRow], now: DateTime<Utc>) -> Value {
    let tasks: Vec<Value> = rows
        .iter()
        .map(|t| {
            let mut m = Map::new();
            put(&mut m, "task_id", t.id.as_ref().map(|id| id.key_string()));
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

    fn ps_order(id: &str, kind: &str, store: &str, state: &str, date_add: &str) -> Order {
        Order {
            id: id.into(),
            id_order_type: kind.into(),
            id_store: store.into(),
            current_state: state.into(),
            date_add: date_add.into(),
            ..Default::default()
        }
    }

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
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
        let placed = Placement::from_order(&ps_order("2155144", "2", "8", "239", "2026-09-18 16:55:19"));
        let v = order_status_json(&head, Some(&placed), &tasks, &diagnoses, &[], now());
        assert_eq!(v["customer"], "Andrea Brandon");
        assert_eq!(v["computer"]["hostname"], "Owner-PC");
        assert_eq!(v["placed"], "Fri Sep 18 16:55");
        assert_eq!((v["kind"].as_str(), v["store"].as_str()), (Some("service"), Some("LTN")));
        assert_eq!(v["order_state"], "Accepted By Odoo");
        assert_eq!(v["checkin_notes"].as_str().unwrap().chars().count(), TEXT_MAX_CHARS);
        assert_eq!(v["service_tasks"][0]["overdue"], true);
        assert_eq!(v["diagnoses"][0]["theory"], "EXPO profile instability");
        assert!(v.get("phone").is_none() && v.get("ai_tasks").is_none());
    }

    #[test]
    fn task_list_flags_overdue_and_keeps_the_full_open_count() {
        let rows = [
            TaskRow {
                id: Some(RecordId::new("task", "abc123")),
                task_name: Some("Call Jennie".into()),
                due_date: at(2026, 9, 28, 1),
                ..Default::default()
            },
            TaskRow { task_name: Some("Order SSD".into()), due_date: at(2026, 10, 2, 1), ..Default::default() },
        ];
        let v = tasks_json("Logan Lees (WAR)", 16, &rows, now());
        assert_eq!(v["tasks"][0]["task_id"], "abc123");
        assert!(v["tasks"][1].get("task_id").is_none());
        assert_eq!(v["open"], 16);
        assert_eq!(v["shown"], 2);
        assert_eq!(v["overdue_shown"], 1);
        assert_eq!(v["tasks"][0]["overdue"], true);
        assert!(v["tasks"][1].get("overdue").is_none());
    }

    #[test]
    fn prestashop_times_read_as_store_time() {
        let p = Placement::from_order(&ps_order("2155684", "14", "3", "999", "2026-09-30 14:37:48"));
        assert_eq!(p.placed, Some(Utc.with_ymd_and_hms(2026, 9, 30, 20, 37, 48).unwrap()));
        assert_eq!((p.kind, p.store.as_str(), p.state.as_str()), ("rci", "store 3", "state 999"));
        assert_eq!(Placement::from_order(&ps_order("1", "2", "7", "29", "0000-00-00 00:00:00")).placed, None);
    }

    #[test]
    fn days_resolve_in_store_time() {
        let today = day(2026, 9, 30);
        assert_eq!(parse_day("Today", today), Some(today));
        assert_eq!(parse_day("yesterday", today), Some(day(2026, 9, 29)));
        assert_eq!(parse_day("monday", today), Some(day(2026, 9, 28)));
        assert_eq!(parse_day("wed", today), Some(today));
        assert_eq!(parse_day("Thursday", today), Some(day(2026, 9, 24)));
        assert_eq!(parse_day("2026-09-01", today), Some(day(2026, 9, 1)));
        assert_eq!(parse_day("someday", today), None);
        assert_eq!(store_today(Utc.with_ymd_and_hms(2026, 10, 1, 3, 0, 0).unwrap()), today);
        assert_eq!(parse_kind(" Ready to roll"), Some("ready_to_roll"));
        assert_eq!(parse_kind("sale"), Some("sales"));
        assert_eq!(parse_kind("widgets"), None);
    }

    #[test]
    fn placed_orders_count_by_kind_and_store() {
        let orders: Vec<Placement> = [
            ps_order("2155667", "2", "14", "36", "2026-09-30 10:09:14"),
            ps_order("2155670", "1", "8", "4", "2026-09-30 10:44:58"),
            ps_order("2155684", "2", "12", "29", "2026-09-30 14:37:48"),
        ]
        .iter()
        .map(Placement::from_order)
        .collect();
        let today = day(2026, 9, 30);
        let v = placed_json(today, today, &orders);
        assert_eq!(v["day"], "Wed Sep 30");
        assert_eq!(v["total"], 3);
        assert_eq!((v["by_kind"]["service"].as_u64(), v["by_kind"]["sales"].as_u64()), (Some(2), Some(1)));
        assert_eq!(v["by_store"]["ORE"], 1);
        assert_eq!((v["first"].as_str(), v["last"].as_str()), (Some("10:09"), Some("14:37")));
        assert_eq!(v["orders"][2]["state"], "Check-in Shelf");
        assert!(v.get("by_day").is_none());

        let week = placed_json(day(2026, 9, 28), today, &orders);
        assert_eq!(week["by_day"][0]["orders"], 3);
        assert_eq!(week["orders"][0]["placed"], "Wed Sep 30 10:09");
    }
}
