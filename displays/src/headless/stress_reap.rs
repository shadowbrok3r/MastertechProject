//! Closes `stress_test_run` rows left `in_progress` by clients that rebooted or died mid-run.

use std::time::Duration;

use database::schema::{ORPHAN_GRACE, ReapScope, RecordIdExt, StressTestRun};

/// Delay between reap passes.
const PASS_SECS: u64 = 15 * 60;

/// Reaps once at startup, then every [`PASS_SECS`].
pub fn spawn_stress_reaper() {
    tokio::spawn(async {
        loop {
            match StressTestRun::reap_orphaned(ReapScope::Fleet, ORPHAN_GRACE).await {
                Ok(closed) if closed.is_empty() => {}
                Ok(closed) => {
                    let keys: Vec<String> = closed.iter().map(|(id, _)| id.key_string()).collect();
                    log::info!(
                        "stress reaper: closed {} orphaned run(s) as aborted: {}",
                        keys.len(),
                        keys.join(", ")
                    );
                }
                Err(e) => log::warn!("stress reaper: pass failed: {e}"),
            }
            tokio::time::sleep(Duration::from_secs(PASS_SECS)).await;
        }
    });
}
