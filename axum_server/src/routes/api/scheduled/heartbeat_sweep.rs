//! Heartbeat sweep cron job.
//!
//! Every minute, flip `connected = false` on any `connected_client` row
//! whose `last_update` is older than the stale threshold (30 minutes).
//! The agent heartbeat writer (in `Mastertech4.0/src/tcp_listener.rs`)
//! bumps `last_update` every 15 minutes and the relay's activity stamp
//! every 5, so a 30-minute gap is strong evidence the agent process is
//! stuck, crashed without graceful shutdown, or its DB path is broken.
//!
//! This is the *catch-all* for stale-flag handling — the graceful-exit
//! writer (`Mastertech4.0/src/main.rs::on_exit`) flips the flag within
//! ~1 s on a clean window-close, but a `kill -9`, BSOD, or power-loss
//! never gets that far. Without this sweep, the admin UI shows a
//! permanent "online" badge for crashed agents until the 2-day cleanup
//! script (`database/src/schema/utilities.rs`) finally runs.
//!
//! The threshold (`30m`) is intentionally generous compared to the 15-min
//! heartbeat cadence so a single missed write or transient network blip
//! doesn't flip a healthy agent offline. Tighten only if the false-
//! positive rate is low and faster offline detection matters.

use anyhow::Result;
use tokio_cron_scheduler::{Job, JobScheduler};

/// Register the heartbeat sweep on the shared scheduler.
pub async fn register(sched: &JobScheduler) -> Result<()> {
    sched
        .add(Job::new_async("0 * * * * *", |_, _| {
            Box::pin(async move {
                // Send through the shared DATABASE client so a DB outage
                // simply delays the sweep until the connection recovers —
                // no missed-tick fan-out, no panicked job.
                let res = database::db()
                    .query(
                        "UPDATE connected_client \
                         SET connected = false \
                         WHERE connected == true \
                           AND (last_update IS NONE OR last_update < time::now() - 30m)",
                    )
                    .await;
                match res {
                    Ok(mut response) => {
                        // Surface the affected count when tracing is on so
                        // an operator watching logs can correlate a sudden
                        // wave of "Offline" badges with the sweep.
                        if let Ok(updated) = response.take::<Vec<serde_json::Value>>(0) {
                            if !updated.is_empty() {
                                log::info!(
                                    "heartbeat_sweep -> flipped {} stale connected_client row(s): {updated:#?}",
                                    updated.len()
                                );
                            }
                        }
                    }
                    Err(e) => {
                        // Don't escalate — the next tick will retry. A
                        // persistent failure (DB down for hours) is a
                        // separate problem from agent liveness tracking.
                        log::warn!("heartbeat_sweep -> query failed: {e:?}");
                    }
                }
            })
        })?)
        .await?;
    log::info!("heartbeat_sweep -> registered (every minute, 30m staleness window)");
    Ok(())
}
