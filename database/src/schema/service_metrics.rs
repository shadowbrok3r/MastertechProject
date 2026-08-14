//! On-demand per-service activity metrics for turnaround / labor reporting.
//!
//! Active minutes are distinct wall-clock minutes containing at least one
//! recorded event, so idle gaps never inflate the number. Tech minutes come
//! from `task_history` edits; AI minutes from `diagnostic_entry` writes and
//! stress-run start/stop marks. Both are floors: work producing no record
//! is invisible.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::db;

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct ServiceMetrics {
    pub service_number: String,
    pub checkin_at_unix: Option<i64>,
    /// First status→Complete / completed=true audit event.
    pub first_completion_unix: Option<i64>,
    pub diagnosed_at_unix: Option<i64>,
    pub turnaround_secs: Option<i64>,
    pub sessions: usize,
    pub stress_runs: usize,
    pub tech_events: usize,
    pub tech_active_minutes: usize,
    pub ai_events: usize,
    pub ai_active_minutes: usize,
    pub ai_cost_usd: f64,
}

async fn unix_by_tasks(sql: &str, tasks: &[super::RecordId]) -> anyhow::Result<Vec<i64>> {
    Ok(db()
        .query(sql)
        .bind(("tasks", tasks.to_vec()))
        .await?
        .take::<Vec<i64>>(0)?)
}

async fn unix_by_sessions(sql: &str, sessions: &[super::RecordId], sn: &str) -> anyhow::Result<Vec<i64>> {
    Ok(db()
        .query(sql)
        .bind(("sessions", sessions.to_vec()))
        .bind(("sn", sn.to_string()))
        .await?
        .take::<Vec<i64>>(0)?)
}

fn minutes(ts: &[i64]) -> usize {
    ts.iter().map(|t| t / 60).collect::<HashSet<_>>().len()
}

impl ServiceMetrics {
    pub async fn compute(service_number: &str) -> anyhow::Result<Self> {
        let sn = service_number.trim().to_string();
        let dbh = db();

        // Anchor rows: the order, its tasks, its sessions.
        let mut res = dbh
            .query("SELECT VALUE time::unix(created_at) FROM service_order WHERE service_number = $sn")
            .query("SELECT VALUE id FROM task WHERE service_number = $sn")
            .query(
                "SELECT VALUE id FROM diagnostic_session WHERE service_order IN \
                 (SELECT VALUE id FROM service_order WHERE service_number = $sn) \
                 OR task_ref IN (SELECT VALUE id FROM task WHERE service_number = $sn)",
            )
            .bind(("sn", sn.clone()))
            .await?;
        let order_created: Vec<i64> = res.take(0)?;
        let tasks: Vec<super::RecordId> = res.take(1)?;
        let sessions: Vec<super::RecordId> = res.take(2)?;

        let mut out = Self {
            service_number: sn.clone(),
            checkin_at_unix: order_created.into_iter().min(),
            sessions: sessions.len(),
            ..Default::default()
        };

        // Tech activity: every audited task edit.
        let tech_ts = unix_by_tasks(
            "SELECT VALUE time::unix(<datetime>created_at) FROM task_history WHERE task_id IN $tasks",
            &tasks,
        )
        .await
        .unwrap_or_default();
        out.tech_events = tech_ts.len();
        out.tech_active_minutes = minutes(&tech_ts);

        out.first_completion_unix = unix_by_tasks(
            "SELECT VALUE time::unix(<datetime>created_at) FROM task_history \
             WHERE task_id IN $tasks AND (diff.completed.new = true OR diff.status.new = 'Complete')",
            &tasks,
        )
        .await
        .unwrap_or_default()
        .into_iter()
        .min();
        out.turnaround_secs = match (out.checkin_at_unix, out.first_completion_unix) {
            (Some(a), Some(b)) if b >= a => Some(b - a),
            _ => None,
        };

        if !sessions.is_empty() {
            out.diagnosed_at_unix = unix_by_sessions(
                "SELECT VALUE time::unix(diagnosed_at) FROM diagnostic_session \
                 WHERE id IN $sessions AND diagnosed_at != NONE",
                &sessions,
                &sn,
            )
            .await
            .unwrap_or_default()
            .into_iter()
            .min();

            let mut ai_ts = unix_by_sessions(
                "SELECT VALUE time::unix(timestamp) FROM diagnostic_entry WHERE session_ref IN $sessions",
                &sessions,
                &sn,
            )
            .await
            .unwrap_or_default();

            // Stress runs mark attention at start and stop, not their whole
            // unattended span.
            let stress_marks = unix_by_sessions(
                "SELECT VALUE time::unix(started_at) FROM stress_test_run \
                 WHERE session_ref IN $sessions OR service_order IN \
                 (SELECT VALUE id FROM service_order WHERE service_number = $sn)",
                &sessions,
                &sn,
            )
            .await
            .unwrap_or_default();
            out.stress_runs = stress_marks.len();
            ai_ts.extend(stress_marks);

            out.ai_events = ai_ts.len();
            out.ai_active_minutes = minutes(&ai_ts);

            // Hook rows carry bare session UUID keys.
            let keys: Vec<String> = {
                use super::RecordIdExt;
                sessions.iter().map(|s| s.key_string()).collect()
            };
            let costs: Vec<f64> = dbh
                .query("SELECT VALUE cost_usd FROM ai_usage WHERE session_refs CONTAINSANY $keys AND cost_usd != NONE")
                .bind(("keys", keys))
                .await?
                .take(0)
                .unwrap_or_default();
            out.ai_cost_usd = costs.into_iter().sum();
        }

        Ok(out)
    }
}
