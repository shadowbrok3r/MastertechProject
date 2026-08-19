//! Post-service outcome classification: a diagnostic counts as fixed unless
//! the same computer comes back as a new service order inside the window.
//!
//! Orders within the same-visit grace band around the session are paperwork,
//! not returns; the first order strictly after the grace is the comeback.
//! Windows that have not elapsed yet report indeterminate, never fixed.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use super::business_calendar::{add_business_days, open_days_between};
use crate::db;

/// Open days around a session inside which a same-computer order is the same
/// visit. Business days, not calendar: a Sunday inside the window is not a day
/// anyone could have touched the paperwork.
pub const GRACE_DAYS: i64 = 3;

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct OutcomeBucket {
    pub window_days: u32,
    pub confirmed_fixed: usize,
    pub comeback: usize,
    pub indeterminate: usize,
    /// comeback / (confirmed_fixed + comeback); 0.0 on an empty denominator.
    pub comeback_rate: f64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ComebackRef {
    pub service_number: String,
    pub order_created_unix: i64,
    /// Calendar days - windows measure customer time, not shop time.
    pub days_after_end: i64,
    /// Same gap counted in open days (Sundays excluded).
    #[serde(default)]
    pub business_days_after_end: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SessionOutcomeRow {
    pub session_id: String,
    pub hostname: String,
    pub status: String,
    #[serde(default)]
    pub started_at_unix: Option<i64>,
    #[serde(default)]
    pub ended_at_unix: Option<i64>,
    pub diagnosed: bool,
    /// Computer record key ("hostname:hash"); NONE rows are indeterminate.
    #[serde(default)]
    pub computer: Option<String>,
    /// Service numbers treated as the same visit.
    pub own_orders: Vec<String>,
    /// First same-computer order strictly after the grace band.
    #[serde(default)]
    pub comeback: Option<ComebackRef>,
    /// (window_days, "confirmed_fixed" | "comeback" | "indeterminate").
    pub per_window: Vec<(u32, String)>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct OutcomeRollup {
    pub computed_at_unix: i64,
    pub windows: Vec<u32>,
    pub overall: Vec<OutcomeBucket>,
    pub resolved: Vec<OutcomeBucket>,
    pub escalated: Vec<OutcomeBucket>,
    pub sessions_considered: usize,
    pub excluded_internal: usize,
    pub no_computer: usize,
    pub no_ended_at: usize,
    #[serde(default)]
    pub sessions: Option<Vec<SessionOutcomeRow>>,
}

#[derive(Debug, Clone)]
struct SessionLite {
    id: String,
    hostname: String,
    status: String,
    computer: Option<String>,
    linked_order: Option<String>,
    started_at: Option<DateTime<Utc>>,
    ended_at: Option<DateTime<Utc>>,
    diagnosed: bool,
}

#[derive(Debug, Clone)]
struct OrderLite {
    id: String,
    service_number: String,
    created_at: DateTime<Utc>,
}

/// Record ids and plain strings both reduce to a comparable string form.
fn id_string(v: Option<&serde_json::Value>) -> Option<String> {
    match v? {
        serde_json::Value::Null => None,
        serde_json::Value::String(s) => Some(s.clone()),
        other => Some(other.to_string()),
    }
}

fn text(v: &serde_json::Value, key: &str) -> String {
    v.get(key).and_then(|x| x.as_str()).unwrap_or_default().to_string()
}

fn datetime(v: Option<&serde_json::Value>) -> Option<DateTime<Utc>> {
    v?.as_str()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.with_timezone(&Utc))
}

fn session_lite(v: &serde_json::Value) -> SessionLite {
    SessionLite {
        id: id_string(v.get("id")).unwrap_or_default(),
        hostname: text(v, "hostname"),
        status: text(v, "status"),
        computer: id_string(v.get("computer_id")),
        linked_order: id_string(v.get("service_order")),
        started_at: datetime(v.get("started_at")),
        ended_at: datetime(v.get("ended_at")),
        diagnosed: v.get("diagnosed_at").is_some_and(|d| !d.is_null()),
    }
}

fn order_lite(v: &serde_json::Value) -> Option<(String, OrderLite)> {
    let computer = id_string(v.get("computer"))?;
    let created_at = datetime(v.get("created_at"))?;
    Some((
        computer,
        OrderLite {
            id: id_string(v.get("id")).unwrap_or_default(),
            service_number: text(v, "service_number"),
            created_at,
        },
    ))
}

/// Orders must be sorted by `created_at` ascending.
fn classify(s: &SessionLite, orders: &[OrderLite], windows: &[u32], now: DateTime<Utc>) -> SessionOutcomeRow {
    let mut row = SessionOutcomeRow {
        session_id: s.id.clone(),
        hostname: s.hostname.clone(),
        status: s.status.clone(),
        started_at_unix: s.started_at.map(|d| d.timestamp()),
        ended_at_unix: s.ended_at.map(|d| d.timestamp()),
        diagnosed: s.diagnosed,
        computer: s.computer.clone(),
        own_orders: Vec::new(),
        comeback: None,
        per_window: Vec::new(),
    };

    let Some(ended) = s.ended_at else {
        row.per_window = windows.iter().map(|w| (*w, "indeterminate".to_string())).collect();
        return row;
    };
    if s.computer.is_none() {
        row.per_window = windows.iter().map(|w| (*w, "indeterminate".to_string())).collect();
        return row;
    }

    let grace_start = add_business_days(s.started_at.unwrap_or(ended), -GRACE_DAYS);
    let grace_end = add_business_days(ended, GRACE_DAYS);
    for o in orders {
        let linked = s.linked_order.as_deref() == Some(o.id.as_str());
        if linked || (o.created_at >= grace_start && o.created_at <= grace_end) {
            row.own_orders.push(o.service_number.clone());
        } else if o.created_at > grace_end && row.comeback.is_none() {
            row.comeback = Some(ComebackRef {
                service_number: o.service_number.clone(),
                order_created_unix: o.created_at.timestamp(),
                days_after_end: (o.created_at - ended).num_days(),
                business_days_after_end: open_days_between(ended, o.created_at),
            });
        }
    }

    row.per_window = windows
        .iter()
        .map(|&w| {
            let verdict = match &row.comeback {
                Some(c) if c.days_after_end <= i64::from(w) => "comeback",
                _ if now < ended + Duration::days(i64::from(w)) => "indeterminate",
                _ => "confirmed_fixed",
            };
            (w, verdict.to_string())
        })
        .collect();
    row
}

fn buckets(rows: &[SessionOutcomeRow], windows: &[u32]) -> Vec<OutcomeBucket> {
    windows
        .iter()
        .map(|&w| {
            let mut b = OutcomeBucket { window_days: w, ..Default::default() };
            for row in rows {
                match row.per_window.iter().find(|(pw, _)| *pw == w).map(|(_, v)| v.as_str()) {
                    Some("confirmed_fixed") => b.confirmed_fixed += 1,
                    Some("comeback") => b.comeback += 1,
                    _ => b.indeterminate += 1,
                }
            }
            let denom = b.confirmed_fixed + b.comeback;
            b.comeback_rate = if denom == 0 { 0.0 } else { b.comeback as f64 / denom as f64 };
            b
        })
        .collect()
}

const SESSION_PROJECTION: &str = "SELECT id, hostname, status, computer_id, service_order, \
     started_at, ended_at, diagnosed_at FROM diagnostic_session";

async fn load_rows(
    session_ids: Option<Vec<super::RecordId>>,
    windows: &[u32],
) -> anyhow::Result<(Vec<SessionOutcomeRow>, usize)> {
    let session_sql = if session_ids.is_some() {
        format!("{SESSION_PROJECTION} WHERE id IN $sessions")
    } else {
        format!("{SESSION_PROJECTION} WHERE status IN ['resolved', 'escalated']")
    };
    let dbh = db();
    let mut q = dbh
        .query(session_sql)
        .query("SELECT id, computer, service_number, created_at FROM service_order WHERE computer != NONE")
        .query("SELECT VALUE id FROM computer WHERE is_internal == true");
    if let Some(ids) = session_ids {
        q = q.bind(("sessions", ids));
    }
    let mut res = q.await?;
    let session_vals: Vec<serde_json::Value> = res.take(0)?;
    let order_vals: Vec<serde_json::Value> = res.take(1)?;
    let internal_vals: Vec<serde_json::Value> = res.take(2).unwrap_or_default();

    let internal: HashSet<String> =
        internal_vals.iter().filter_map(|v| id_string(Some(v))).collect();
    let mut by_computer: HashMap<String, Vec<OrderLite>> = HashMap::new();
    for v in &order_vals {
        if let Some((computer, order)) = order_lite(v) {
            by_computer.entry(computer).or_default().push(order);
        }
    }
    for orders in by_computer.values_mut() {
        orders.sort_by_key(|o| o.created_at);
    }

    let now = Utc::now();
    let mut excluded_internal = 0usize;
    let mut rows = Vec::new();
    for v in &session_vals {
        let s = session_lite(v);
        if s.computer.as_ref().is_some_and(|c| internal.contains(c)) {
            excluded_internal += 1;
            continue;
        }
        let orders = s
            .computer
            .as_ref()
            .and_then(|c| by_computer.get(c))
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        rows.push(classify(&s, orders, windows, now));
    }
    Ok((rows, excluded_internal))
}

impl OutcomeRollup {
    pub async fn compute(windows: &[u32], include_sessions: bool) -> anyhow::Result<Self> {
        let (rows, excluded_internal) = load_rows(None, windows).await?;
        let resolved: Vec<_> = rows.iter().filter(|r| r.status == "resolved").cloned().collect();
        let escalated: Vec<_> = rows.iter().filter(|r| r.status == "escalated").cloned().collect();
        Ok(Self {
            computed_at_unix: Utc::now().timestamp(),
            windows: windows.to_vec(),
            overall: buckets(&rows, windows),
            resolved: buckets(&resolved, windows),
            escalated: buckets(&escalated, windows),
            sessions_considered: rows.len(),
            excluded_internal,
            no_computer: rows.iter().filter(|r| r.computer.is_none()).count(),
            no_ended_at: rows.iter().filter(|r| r.ended_at_unix.is_none()).count(),
            sessions: include_sessions.then_some(rows),
        })
    }
}

/// Outcome rows for specific sessions; internal machines are skipped.
pub async fn outcome_for_sessions(
    session_ids: &[super::RecordId],
    windows: &[u32],
) -> anyhow::Result<Vec<SessionOutcomeRow>> {
    if session_ids.is_empty() {
        return Ok(Vec::new());
    }
    let (rows, _) = load_rows(Some(session_ids.to_vec()), windows).await?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(ended_days_ago: i64) -> SessionLite {
        let ended = Utc::now() - Duration::days(ended_days_ago);
        SessionLite {
            id: "diagnostic_session:test".into(),
            hostname: "bench-1".into(),
            status: "resolved".into(),
            computer: Some("computer:bench-1:abc".into()),
            linked_order: Some("service_order:100".into()),
            started_at: Some(ended - Duration::hours(4)),
            ended_at: Some(ended),
            diagnosed: true,
        }
    }

    fn order(id: &str, sn: &str, created: DateTime<Utc>) -> OrderLite {
        OrderLite { id: id.into(), service_number: sn.into(), created_at: created }
    }

    fn verdict(row: &SessionOutcomeRow, window: u32) -> &str {
        row.per_window.iter().find(|(w, _)| *w == window).map(|(_, v)| v.as_str()).unwrap()
    }

    #[test]
    fn order_minutes_after_end_is_same_visit_not_comeback() {
        let s = session(120);
        let orders = [order("service_order:101", "101", s.ended_at.unwrap() + Duration::minutes(55))];
        let row = classify(&s, &orders, &[30], Utc::now());
        assert_eq!(row.own_orders, vec!["101"]);
        assert!(row.comeback.is_none());
        assert_eq!(verdict(&row, 30), "confirmed_fixed");
    }

    #[test]
    fn order_at_exact_grace_boundary_is_same_visit() {
        let s = session(120);
        let orders = [order("service_order:101", "101", s.ended_at.unwrap() + Duration::days(GRACE_DAYS))];
        let row = classify(&s, &orders, &[30], Utc::now());
        assert_eq!(row.own_orders, vec!["101"]);
        assert!(row.comeback.is_none());
    }

    #[test]
    fn forty_day_comeback_splits_windows() {
        let s = session(120);
        let orders = [order("service_order:102", "102", s.ended_at.unwrap() + Duration::days(40))];
        let row = classify(&s, &orders, &[30, 60], Utc::now());
        assert_eq!(row.comeback.as_ref().unwrap().days_after_end, 40);
        assert_eq!(verdict(&row, 30), "confirmed_fixed");
        assert_eq!(verdict(&row, 60), "comeback");
    }

    #[test]
    fn unelapsed_window_is_indeterminate() {
        let s = session(10);
        let row = classify(&s, &[], &[30], Utc::now());
        assert_eq!(verdict(&row, 30), "indeterminate");
    }

    #[test]
    fn early_comeback_beats_unelapsed_window() {
        let s = session(10);
        let orders = [order("service_order:103", "103", s.ended_at.unwrap() + Duration::days(5))];
        let row = classify(&s, &orders, &[30], Utc::now());
        assert_eq!(verdict(&row, 30), "comeback");
    }

    #[test]
    fn missing_ended_at_or_computer_is_indeterminate() {
        let mut s = session(120);
        s.ended_at = None;
        assert_eq!(verdict(&classify(&s, &[], &[30], Utc::now()), 30), "indeterminate");
        let mut s = session(120);
        s.computer = None;
        assert_eq!(verdict(&classify(&s, &[], &[30], Utc::now()), 30), "indeterminate");
    }

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    /// Session 6413f748 on DESKTOP-1JBT855 (dev DB, verified 2026-08-18):
    /// three orders predate the session, order 2152547 lands 6d17h after it ends.
    #[test]
    fn real_dev_db_comeback_case() {
        let s = SessionLite {
            id: "diagnostic_session:6413f748-3b4a-448b-8caf-7b7e5746c3df".into(),
            hostname: "DESKTOP-1JBT855".into(),
            status: "resolved".into(),
            computer: Some("computer:DESKTOP-1JBT855:5b8752583".into()),
            linked_order: None,
            started_at: Some(utc("2026-04-13T23:56:47Z")),
            ended_at: Some(utc("2026-08-03T23:05:30Z")),
            diagnosed: false,
        };
        let orders = [
            order("service_order:2126961", "2126961", utc("2025-11-06T22:37:00Z")),
            order("service_order:2133968", "2133968", utc("2026-01-06T19:01:32Z")),
            order("service_order:2137526", "2137526", utc("2026-01-29T19:19:13Z")),
            order("service_order:2152547", "2152547", utc("2026-08-10T16:54:21Z")),
        ];
        let row = classify(&s, &orders, &[30, 60, 90], utc("2026-08-18T20:00:00Z"));
        assert!(row.own_orders.is_empty(), "pre-session orders are history, not this visit");
        let cb = row.comeback.as_ref().expect("2152547 is a comeback");
        assert_eq!(cb.service_number, "2152547");
        // Whole elapsed days, truncated - a partial day never pushes a comeback
        // out of its window.
        assert_eq!(cb.days_after_end, 6);
        assert_eq!(verdict(&row, 30), "comeback");
    }

    #[test]
    fn linked_order_outside_grace_is_still_own_visit() {
        let s = session(120);
        let orders = [order("service_order:100", "100", s.ended_at.unwrap() + Duration::days(10))];
        let row = classify(&s, &orders, &[30], Utc::now());
        assert_eq!(row.own_orders, vec!["100"]);
        assert!(row.comeback.is_none());
    }
}
