//! Stale diagnostic-session sweep.
//!
//! Every hour, move `open` sessions older than the threshold to `abandoned`.
//! Nothing else closes a session whose agent turn died mid-run: the dispatcher
//! only writes the `assist_request` row, and an operator who walks away leaves
//! one open forever. 38 of 101 sessions were stranded when this was written,
//! the oldest ten days, which the outcome engine cannot see because it only
//! scores `resolved` and `escalated`.
//!
//! `abandoned` rather than a guessed verdict: closing one `resolved` would
//! invent a fix that nobody confirmed, and the outcome engine already ignores
//! any status outside those two, so no rollup change is needed.
//!
//! Age only, deliberately. Keying off the newest `diagnostic_entry` needs a
//! correlated subquery that measured 4.7s for three rows — too slow for an
//! hourly predicate — and nobody works a single diagnostic for two days
//! without closing it.

use anyhow::Result;
use tokio_cron_scheduler::{Job, JobScheduler};

/// How long an `open` session may sit untouched before it is abandoned.
const STALE_AFTER: &str = "2d";

/// Register the stale-session sweep on the shared scheduler.
pub async fn register(sched: &JobScheduler) -> Result<()> {
    sched
        .add(Job::new_async("0 7 * * * *", |_, _| {
            Box::pin(async move {
                let sql = format!(
                    "UPDATE diagnostic_session \
                     SET status = 'abandoned', ended_at = time::now() \
                     WHERE status = 'open' AND started_at < time::now() - {STALE_AFTER} \
                     RETURN VALUE id"
                );
                match database::db().query(sql).await {
                    Ok(mut response) => {
                        match response.take::<Vec<database::schema::RecordId>>(0) {
                            Ok(ids) if !ids.is_empty() => log::info!(
                                "stale_session_sweep -> abandoned {} session(s)",
                                ids.len()
                            ),
                            Ok(_) => log::debug!("stale_session_sweep -> nothing stale"),
                            Err(e) => log::warn!("stale_session_sweep -> read failed: {e}"),
                        }
                    }
                    Err(e) => log::warn!("stale_session_sweep -> query failed: {e}"),
                }
            })
        })?)
        .await?;
    log::info!("axum_server -> stale_session_sweep registered (open > {STALE_AFTER})");
    Ok(())
}
