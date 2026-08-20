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

/// A human verdict that supersedes the inferred one. Inference only ever sees
/// new service orders, so a tech who knows the real result outranks it.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeOverride {
    /// Fixed, whatever the orders say.
    ConfirmedFixed,
    /// Came back, even with no new order to prove it.
    Comeback,
    /// Not a real customer diagnostic; drop it from the rollup.
    Excluded,
}

impl OutcomeOverride {
    pub const ALL: [Self; 3] = [Self::ConfirmedFixed, Self::Comeback, Self::Excluded];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ConfirmedFixed => "confirmed_fixed",
            Self::Comeback => "comeback",
            Self::Excluded => "excluded",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|v| v.as_str() == s)
    }

    /// Verdict this forces on every window; `None` for rows dropped entirely.
    pub fn forced_verdict(self) -> Option<&'static str> {
        match self {
            Self::ConfirmedFixed => Some("confirmed_fixed"),
            Self::Comeback => Some("comeback"),
            Self::Excluded => None,
        }
    }

    /// Dashboard control label.
    pub fn label(self) -> &'static str {
        match self {
            Self::ConfirmedFixed => "Fixed - I know it stuck",
            Self::Comeback => "Came back",
            Self::Excluded => "Do not count this session",
        }
    }
}

/// Rebuilds a record id from the `table:`key`` form the engine reports rows in.
/// Keys may contain colons, so only the table prefix is stripped.
pub fn record_id_from_string(table: &str, s: &str) -> Option<super::RecordId> {
    let key = s
        .trim()
        .strip_prefix(&format!("{table}:"))
        .unwrap_or(s.trim())
        .trim_matches('`')
        .trim();
    (!key.is_empty()).then(|| super::RecordId::new(table, key))
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct OutcomeBucket {
    pub window_days: u32,
    pub confirmed_fixed: usize,
    pub comeback: usize,
    pub indeterminate: usize,
    /// comeback / (confirmed_fixed + comeback); 0.0 on an empty denominator.
    pub comeback_rate: f64,
    /// Sessions in this bucket whose verdict came from a human, not inference.
    #[serde(default)]
    pub overridden: usize,
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
    /// Set when a human verdict replaced the inferred one.
    #[serde(default)]
    pub outcome_override: Option<OutcomeOverride>,
    #[serde(default)]
    pub override_reason: Option<String>,
    #[serde(default)]
    pub override_by: Option<String>,
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
    /// Machines flagged staff/test fleet-wide; most have no sessions at all.
    #[serde(default)]
    pub internal_computer_count: usize,
    /// Only the flagged machines that actually cost a session here, so the flag
    /// can be revoked where it changes a number.
    #[serde(default)]
    pub internal_computers: Vec<String>,
    /// Sessions a human marked as not-a-real-diagnostic.
    #[serde(default)]
    pub excluded_override: usize,
    pub no_computer: usize,
    pub no_ended_at: usize,
    #[serde(default)]
    pub sessions: Option<Vec<SessionOutcomeRow>>,
    /// Rows a human excluded, kept out of every bucket but still listed so the
    /// exclusion can be revoked.
    #[serde(default)]
    pub excluded_sessions: Option<Vec<SessionOutcomeRow>>,
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
    override_verdict: Option<OutcomeOverride>,
    override_reason: Option<String>,
    override_by: Option<String>,
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
        override_verdict: v
            .get("outcome_override")
            .and_then(|x| x.as_str())
            .and_then(OutcomeOverride::parse),
        override_reason: v
            .get("outcome_override_reason")
            .and_then(|x| x.as_str())
            .map(str::to_string),
        override_by: v.get("outcome_override_by").and_then(|x| x.as_str()).map(str::to_string),
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
        outcome_override: s.override_verdict,
        override_reason: s.override_reason.clone(),
        override_by: s.override_by.clone(),
    };

    let forced = s.override_verdict.and_then(OutcomeOverride::forced_verdict);
    let every_window =
        |v: &str| -> Vec<(u32, String)> { windows.iter().map(|w| (*w, v.to_string())).collect() };

    // Dropped rows must never infer a positive verdict if one reaches here.
    if s.override_verdict == Some(OutcomeOverride::Excluded) {
        row.per_window = every_window("indeterminate");
        return row;
    }
    let Some(ended) = s.ended_at else {
        row.per_window = every_window(forced.unwrap_or("indeterminate"));
        return row;
    };
    if s.computer.is_none() {
        row.per_window = every_window(forced.unwrap_or("indeterminate"));
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

    if let Some(v) = forced {
        row.per_window = every_window(v);
        return row;
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
                if row.outcome_override.is_some() {
                    b.overridden += 1;
                }
            }
            let denom = b.confirmed_fixed + b.comeback;
            b.comeback_rate = if denom == 0 { 0.0 } else { b.comeback as f64 / denom as f64 };
            b
        })
        .collect()
}

const SESSION_PROJECTION: &str = "SELECT id, hostname, status, computer_id, service_order, \
     started_at, ended_at, diagnosed_at, outcome_override, outcome_override_reason, \
     outcome_override_by FROM diagnostic_session";

struct Loaded {
    rows: Vec<SessionOutcomeRow>,
    excluded_rows: Vec<SessionOutcomeRow>,
    excluded_internal: usize,
    internal_computer_count: usize,
    internal_computers: Vec<String>,
}

async fn load_rows(
    session_ids: Option<Vec<super::RecordId>>,
    windows: &[u32],
) -> anyhow::Result<Loaded> {
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
    let mut internal_hits: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut excluded_rows = Vec::new();
    let mut rows = Vec::new();
    for v in &session_vals {
        let s = session_lite(v);
        if s.override_verdict == Some(OutcomeOverride::Excluded) {
            excluded_rows.push(classify(&s, &[], windows, now));
            continue;
        }
        if let Some(c) = s.computer.as_ref().filter(|c| internal.contains(*c)) {
            excluded_internal += 1;
            internal_hits.insert(c.clone());
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
    Ok(Loaded {
        rows,
        excluded_rows,
        excluded_internal,
        internal_computer_count: internal.len(),
        internal_computers: internal_hits.into_iter().collect(),
    })
}

impl OutcomeRollup {
    pub async fn compute(windows: &[u32], include_sessions: bool) -> anyhow::Result<Self> {
        let Loaded {
            rows,
            excluded_rows,
            excluded_internal,
            internal_computer_count,
            internal_computers,
        } = load_rows(None, windows).await?;
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
            internal_computer_count,
            internal_computers,
            excluded_override: excluded_rows.len(),
            no_computer: rows.iter().filter(|r| r.computer.is_none()).count(),
            no_ended_at: rows.iter().filter(|r| r.ended_at_unix.is_none()).count(),
            sessions: include_sessions.then_some(rows),
            excluded_sessions: include_sessions.then_some(excluded_rows),
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
    Ok(load_rows(Some(session_ids.to_vec()), windows).await?.rows)
}

/// Records a human verdict on a session, or clears it when `verdict` is `None`.
/// A set override always carries a reason so the correction stays auditable.
pub async fn set_outcome_override(
    session: &super::RecordId,
    verdict: Option<OutcomeOverride>,
    reason: Option<&str>,
    by: &str,
) -> anyhow::Result<()> {
    let reason = reason.map(str::trim).filter(|r| !r.is_empty()).map(str::to_string);
    let mut res = match verdict {
        // Literal NONE: a bound Option::None writes NULL, which the field type rejects.
        None => {
            db().query(
                "UPDATE $id SET outcome_override = NONE, outcome_override_reason = NONE, \
                 outcome_override_by = NONE, outcome_override_at = NONE RETURN VALUE id",
            )
            .bind(("id", session.clone()))
            .await?
        },
        Some(verdict) => {
            let Some(reason) = reason else {
                anyhow::bail!("an outcome override needs a reason");
            };
            db().query(
                "UPDATE $id SET outcome_override = $verdict, outcome_override_reason = $reason, \
                 outcome_override_by = $by, outcome_override_at = time::now() RETURN VALUE id",
            )
            .bind(("id", session.clone()))
            .bind(("verdict", verdict.as_str().to_string()))
            .bind(("reason", reason))
            .bind(("by", by.trim().to_string()))
            .await?
        },
    };
    ensure_written(res.take(0).unwrap_or_default())
}

/// Flags or clears a staff/test machine, which excludes every session on it.
pub async fn set_computer_internal(
    computer: &super::RecordId,
    internal: bool,
) -> anyhow::Result<()> {
    let mut res = db()
        .query("UPDATE $id SET is_internal = $internal RETURN VALUE id")
        .bind(("id", computer.clone()))
        .bind(("internal", internal))
        .await?;
    ensure_written(res.take(0).unwrap_or_default())
}

/// Every computer row a connected client can resolve to: the canonical
/// `HOST:hash9` key first, then whatever `connected_client.computer` points at.
/// Both matter because either can be the row a reader consults, and the two
/// disagree whenever a client was linked before the canonical key existed.
pub async fn client_computer_ids(connection_string: &str) -> anyhow::Result<Vec<super::RecordId>> {
    use super::RecordIdExt;

    let cs = connection_string.trim();
    if cs.is_empty() {
        anyhow::bail!("connection_string is required");
    }
    let mut res = db()
        .query(
            "SELECT VALUE computer FROM connected_client WHERE connection_string == $cs
             AND computer != NONE",
        )
        .bind(("cs", cs.to_string()))
        .await?;
    let linked: Vec<super::RecordId> = res.take(0).unwrap_or_default();

    let mut ids = vec![super::entity_link::canonical_computer_id(cs)];
    for id in linked {
        if !ids.iter().any(|i| i.key_string() == id.key_string()) {
            ids.push(id);
        }
    }
    Ok(ids)
}

/// The staff-machine computer row for a connected client, canonical key first,
/// or `None` when this client is not flagged internal.
pub async fn internal_computer_for_client(
    connection_string: &str,
) -> anyhow::Result<Option<super::RecordId>> {
    use super::RecordIdExt;

    let ids = client_computer_ids(connection_string).await?;
    let mut res = db()
        .query("SELECT VALUE id FROM computer WHERE id IN $ids AND is_internal == true")
        .bind(("ids", ids.clone()))
        .await?;
    let flagged: Vec<super::RecordId> = res.take(0).unwrap_or_default();
    Ok(ids
        .into_iter()
        .find(|id| flagged.iter().any(|f| f.key_string() == id.key_string())))
}

/// Flags or clears every computer row a connected client resolves to, so the
/// flag holds whichever row a reader lands on. Returns the keys written.
///
/// Flagging also mints the canonical row when it is absent: the task path
/// upserts `computer:HOST:hash9` on its own, so leaving that key empty lets a
/// later check-in create an unflagged row and adopt a customer onto it.
pub async fn set_client_internal(
    connection_string: &str,
    internal: bool,
) -> anyhow::Result<Vec<String>> {
    use super::RecordIdExt;

    let cs = connection_string.trim().to_string();
    let ids = client_computer_ids(&cs).await?;
    let canonical = super::entity_link::canonical_computer_id(&cs);

    let mut res = db()
        .query("UPDATE computer SET is_internal = $internal WHERE id IN $ids RETURN VALUE id")
        .bind(("internal", internal))
        .bind(("ids", ids))
        .await?;
    let mut written: Vec<String> = res
        .take::<Vec<super::RecordId>>(0)
        .unwrap_or_default()
        .iter()
        .map(RecordIdExt::key_string)
        .collect();

    if internal && !written.iter().any(|k| *k == canonical.key_string()) {
        let hostname = cs.split_once(':').map(|(h, _)| h).unwrap_or(&cs);
        super::entity_link::upsert_computer_record(&canonical, hostname, None, None).await?;
        set_computer_internal(&canonical, true).await?;
        written.push(canonical.key_string());
    }
    Ok(written)
}

/// An UPDATE that matched nothing is a silent no-op; surface it as an error.
fn ensure_written(ids: Vec<super::RecordId>) -> anyhow::Result<()> {
    if ids.is_empty() {
        anyhow::bail!("no record matched; nothing was written");
    }
    Ok(())
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
            override_verdict: None,
            override_reason: None,
            override_by: None,
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
            override_verdict: None,
            override_reason: None,
            override_by: None,
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
    fn override_fixed_beats_a_real_comeback() {
        let mut s = session(120);
        s.override_verdict = Some(OutcomeOverride::ConfirmedFixed);
        let orders = [order("service_order:102", "102", s.ended_at.unwrap() + Duration::days(10))];
        let row = classify(&s, &orders, &[30, 60], Utc::now());
        assert_eq!(verdict(&row, 30), "confirmed_fixed");
        assert_eq!(verdict(&row, 60), "confirmed_fixed");
        // The overruled comeback stays visible, so the correction can be audited.
        assert_eq!(row.comeback.as_ref().unwrap().service_number, "102");
    }

    #[test]
    fn override_comeback_decides_an_unelapsed_window() {
        let mut s = session(2);
        s.override_verdict = Some(OutcomeOverride::Comeback);
        let row = classify(&s, &[], &[30, 90], Utc::now());
        assert_eq!(verdict(&row, 30), "comeback");
        assert_eq!(verdict(&row, 90), "comeback");
    }

    #[test]
    fn override_scores_a_session_inference_cannot() {
        let mut s = session(120);
        s.ended_at = None;
        s.override_verdict = Some(OutcomeOverride::ConfirmedFixed);
        assert_eq!(verdict(&classify(&s, &[], &[30], Utc::now()), 30), "confirmed_fixed");
        let mut s = session(120);
        s.computer = None;
        s.override_verdict = Some(OutcomeOverride::ConfirmedFixed);
        assert_eq!(verdict(&classify(&s, &[], &[30], Utc::now()), 30), "confirmed_fixed");
    }

    #[test]
    fn excluded_override_never_reaches_a_bucket() {
        let mut s = session(120);
        s.override_verdict = Some(OutcomeOverride::Excluded);
        // load_rows drops these; classify must not invent a verdict if one slips through.
        let row = classify(&s, &[], &[30], Utc::now());
        assert_eq!(verdict(&row, 30), "indeterminate");
        assert_eq!(buckets(&[row], &[30])[0].confirmed_fixed, 0);
    }

    #[test]
    fn overridden_rows_are_counted_in_their_bucket() {
        let mut s = session(120);
        s.override_verdict = Some(OutcomeOverride::ConfirmedFixed);
        let plain = classify(&session(120), &[], &[30], Utc::now());
        let forced = classify(&s, &[], &[30], Utc::now());
        let b = &buckets(&[plain, forced], &[30])[0];
        assert_eq!(b.confirmed_fixed, 2);
        assert_eq!(b.overridden, 1);
    }

    #[test]
    fn record_id_round_trips_a_colon_bearing_key() {
        use super::super::RecordIdExt;
        let id = record_id_from_string("computer", "computer:`HBCD_PE:9040045e1`").unwrap();
        assert_eq!(id.key_string(), "HBCD_PE:9040045e1");
        let id = record_id_from_string("computer", "computer:HBCD_PE:9040045e1").unwrap();
        assert_eq!(id.key_string(), "HBCD_PE:9040045e1");
        let id = record_id_from_string(
            "diagnostic_session",
            "diagnostic_session:`052cd27b-b79f-44e7-8130-929c7c339ee7`",
        )
        .unwrap();
        assert_eq!(id.key_string(), "052cd27b-b79f-44e7-8130-929c7c339ee7");
        assert!(record_id_from_string("computer", "computer:``").is_none());
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
