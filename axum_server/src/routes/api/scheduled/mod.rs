//! Background cron jobs that run inside the axum_server process.
//!
//! Today this only owns the heartbeat sweep ([`heartbeat_sweep`]), but
//! the module is structured around a single shared [`tokio_cron_scheduler::JobScheduler`]
//! so additional periodic chores can be added without each one spawning
//! its own scheduler (the pattern currently used in
//! `routes/api/orders/schedule.rs` leaks a scheduler per request — that
//! file is left untouched for now to keep this change focused, but new
//! cron work should go through [`spawn_cron_scheduler`] instead).

use anyhow::Result;
use tokio_cron_scheduler::JobScheduler;

pub mod heartbeat_sweep;
pub mod stale_session_sweep;

/// Build and start a single [`JobScheduler`] with every cron registered.
///
/// Called once from `main.rs` during process startup. Returns the
/// scheduler so the caller can keep it alive for the lifetime of the
/// process (dropping it would silently cancel every job).
pub async fn spawn_cron_scheduler() -> Result<JobScheduler> {
    let sched = JobScheduler::new().await?;
    heartbeat_sweep::register(&sched).await?;
    stale_session_sweep::register(&sched).await?;
    sched.start().await?;
    log::info!("axum_server -> cron scheduler started ({} job(s))", 2);
    Ok(sched)
}
