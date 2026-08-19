//! Fleet-wide ROI aggregates for the analytics dashboard.
//!
//! One batch of projected queries per refresh, joined in Rust; per-service
//! ServiceMetrics::compute is deliberately never called in a loop. Every
//! headline number ships with the counters that bound it, so a gap in the data
//! reads as a gap rather than as a zero.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

use super::business_calendar::{business_seconds, OPEN_SECS_PER_DAY};
use super::outcome::OutcomeRollup;
use crate::db;

/// Technician pay band, USD per hour.
pub const TECH_RATE_LOW: f64 = 12.0;
pub const TECH_RATE_HIGH: f64 = 14.0;

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct TurnaroundStats {
    pub n: usize,
    pub median_wall_secs: Option<i64>,
    pub median_business_secs: Option<i64>,
    /// Completed inside a single open day of shop time.
    pub within_one_open_day: usize,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct DataGaps {
    pub sessions_total: usize,
    pub sessions_open: usize,
    pub sessions_unlinked: usize,
    pub sessions_without_diagnosed: usize,
    pub orders_without_computer: usize,
    pub tasks_without_origin: usize,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct RoiSummary {
    pub generated_unix: i64,
    /// Lookback for the cost, labor and turnaround panels.
    pub window_days: u32,
    pub outcome: OutcomeRollup,
    pub ai_cost_usd: f64,
    pub ai_usage_rows: usize,
    pub ai_turnaround: TurnaroundStats,
    pub tech_turnaround: TurnaroundStats,
    pub tech_active_minutes: usize,
    pub tech_labor_low_usd: f64,
    pub tech_labor_high_usd: f64,
    pub gaps: DataGaps,
}

/// Projected rows arrive as raw values so one drifted row degrades alone.
fn text_at(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| x.as_str()).map(str::to_string)
}

fn int_at(v: &serde_json::Value, key: &str) -> Option<i64> {
    v.get(key).and_then(serde_json::Value::as_i64)
}

fn median(mut v: Vec<i64>) -> Option<i64> {
    if v.is_empty() {
        return None;
    }
    v.sort_unstable();
    Some(v[v.len() / 2])
}

fn stats(spans: &[(i64, i64)]) -> TurnaroundStats {
    let mut wall = Vec::with_capacity(spans.len());
    let mut business = Vec::with_capacity(spans.len());
    let mut within = 0usize;
    for &(from, to) in spans {
        wall.push(to - from);
        let secs = match (DateTime::from_timestamp(from, 0), DateTime::from_timestamp(to, 0)) {
            (Some(a), Some(b)) => business_seconds(a, b),
            _ => 0,
        };
        if secs <= OPEN_SECS_PER_DAY {
            within += 1;
        }
        business.push(secs);
    }
    TurnaroundStats {
        n: spans.len(),
        median_wall_secs: median(wall),
        median_business_secs: median(business),
        within_one_open_day: within,
    }
}

fn count_of(rows: Vec<serde_json::Value>) -> usize {
    rows.first()
        .and_then(|r| r.get("count"))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as usize
}

impl RoiSummary {
    pub async fn compute(window_days: u32) -> anyhow::Result<Self> {
        let since = Utc::now() - Duration::days(i64::from(window_days));
        let dbh = db();

        let mut res = dbh
            .query(
                "SELECT record::id(id) AS task, origin, service_number FROM task \
                 WHERE created_at >= $since",
            )
            .query(
                "SELECT record::id(task_id) AS task, time::unix(<datetime>created_at) AS at \
                 FROM task_history WHERE <datetime>created_at >= $since",
            )
            .query(
                "SELECT record::id(task_id) AS task, time::unix(<datetime>created_at) AS at \
                 FROM task_history WHERE <datetime>created_at >= $since \
                 AND (diff.completed.new = true OR diff.status.new = 'Complete')",
            )
            .query(
                "SELECT service_number, time::unix(created_at) AS at FROM service_order \
                 WHERE created_at >= $since AND service_number != NONE",
            )
            .query("SELECT VALUE cost_usd FROM ai_usage WHERE at >= $since AND cost_usd != NONE")
            .bind(("since", super::Datetime::from(since)))
            .await?;

        let tasks: Vec<serde_json::Value> = res.take(0).unwrap_or_default();
        let history: Vec<serde_json::Value> = res.take(1).unwrap_or_default();
        let completions: Vec<serde_json::Value> = res.take(2).unwrap_or_default();
        let orders: Vec<serde_json::Value> = res.take(3).unwrap_or_default();
        let costs: Vec<f64> = res.take(4).unwrap_or_default();

        let mut checkin: HashMap<String, i64> = HashMap::new();
        for o in &orders {
            if let (Some(sn), Some(at)) = (text_at(o, "service_number"), int_at(o, "at")) {
                checkin.entry(sn).and_modify(|e| *e = (*e).min(at)).or_insert(at);
            }
        }
        let mut first_done: HashMap<String, i64> = HashMap::new();
        for c in &completions {
            if let (Some(task), Some(at)) = (text_at(c, "task"), int_at(c, "at")) {
                first_done.entry(task).and_modify(|e| *e = (*e).min(at)).or_insert(at);
            }
        }

        let mut ai_spans: Vec<(i64, i64)> = Vec::new();
        let mut tech_spans: Vec<(i64, i64)> = Vec::new();
        let mut without_origin = 0usize;
        for row in &tasks {
            let origin = text_at(row, "origin");
            if origin.is_none() {
                without_origin += 1;
            }
            let span = text_at(row, "service_number")
                .and_then(|sn| checkin.get(&sn).copied())
                .zip(text_at(row, "task").and_then(|k| first_done.get(&k).copied()))
                .filter(|(from, to)| to >= from);
            if let Some(span) = span {
                if origin.as_deref() == Some("ai") {
                    ai_spans.push(span);
                } else {
                    tech_spans.push(span);
                }
            }
        }

        // Distinct minutes carrying at least one audited task edit.
        let tech_active_minutes =
            history.iter().filter_map(|e| int_at(e, "at")).map(|at| at / 60).collect::<HashSet<_>>().len();
        let tech_hours = tech_active_minutes as f64 / 60.0;

        let mut gaps = dbh
            .query("SELECT count() FROM diagnostic_session GROUP ALL")
            .query("SELECT count() FROM diagnostic_session WHERE status = 'open' GROUP ALL")
            .query(
                "SELECT count() FROM diagnostic_session \
                 WHERE task_ref = NONE AND service_order = NONE GROUP ALL",
            )
            .query(
                "SELECT count() FROM diagnostic_session \
                 WHERE status != 'open' AND diagnosed_at = NONE GROUP ALL",
            )
            .query("SELECT count() FROM service_order WHERE computer = NONE GROUP ALL")
            .await?;

        Ok(Self {
            generated_unix: Utc::now().timestamp(),
            window_days,
            outcome: OutcomeRollup::compute(&[30, 60, 90], false).await?,
            ai_cost_usd: costs.iter().sum(),
            ai_usage_rows: costs.len(),
            ai_turnaround: stats(&ai_spans),
            tech_turnaround: stats(&tech_spans),
            tech_active_minutes,
            tech_labor_low_usd: tech_hours * TECH_RATE_LOW,
            tech_labor_high_usd: tech_hours * TECH_RATE_HIGH,
            gaps: DataGaps {
                sessions_total: count_of(gaps.take(0).unwrap_or_default()),
                sessions_open: count_of(gaps.take(1).unwrap_or_default()),
                sessions_unlinked: count_of(gaps.take(2).unwrap_or_default()),
                sessions_without_diagnosed: count_of(gaps.take(3).unwrap_or_default()),
                orders_without_computer: count_of(gaps.take(4).unwrap_or_default()),
                tasks_without_origin: without_origin,
            },
        })
    }
}
