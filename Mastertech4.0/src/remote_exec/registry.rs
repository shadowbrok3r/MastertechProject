//! Process-wide registry of RemoteExec jobs.
//!
//! The client is the authority on job state: jobs outlive the admin session
//! that started them, and a reconnecting admin recovers them from here.

use std::collections::HashMap;
use std::sync::Mutex;

use displays::remote_exec::{JobSnapshot, JobState};

use super::job::{now_ms, JobHandle};

/// Concurrent running jobs allowed per client.
pub const MAX_CONCURRENT_JOBS: u32 = 8;

/// How long a terminal job stays readable, so a reconnecting admin can still
/// collect its exit record.
const RETAIN_TERMINAL_MS: u64 = 10 * 60 * 1000;

static JOBS: Mutex<Option<HashMap<String, JobHandle>>> = Mutex::new(None);

fn with_jobs<R>(f: impl FnOnce(&mut HashMap<String, JobHandle>) -> R) -> Option<R> {
    let mut guard = JOBS.lock().ok()?;
    Some(f(guard.get_or_insert_with(HashMap::new)))
}

/// Drops terminal jobs past their retention window.
fn sweep(map: &mut HashMap<String, JobHandle>) {
    let cutoff = now_ms().saturating_sub(RETAIN_TERMINAL_MS);
    map.retain(|_, h| match h.ended_at_ms() {
        Some(ended) => ended > cutoff,
        None => true,
    });
}

pub fn running_count() -> u32 {
    with_jobs(|map| {
        map.values()
            .filter(|h| !h.state().is_terminal())
            .count() as u32
    })
    .unwrap_or(0)
}

/// Registers a job. `Err` when the concurrency cap is already reached.
pub fn insert(handle: JobHandle) -> Result<(), String> {
    with_jobs(|map| {
        sweep(map);
        let running = map.values().filter(|h| !h.state().is_terminal()).count() as u32;
        if running >= MAX_CONCURRENT_JOBS {
            return Err(format!(
                "client already has {running} running jobs (cap {MAX_CONCURRENT_JOBS})"
            ));
        }
        map.insert(handle.job_id.clone(), handle);
        Ok(())
    })
    .unwrap_or_else(|| Err("job registry lock poisoned".into()))
}

pub fn get(job_id: &str) -> Option<JobHandle> {
    with_jobs(|map| map.get(job_id).cloned()).flatten()
}

/// Snapshot of one job, or every retained job when `job_id` is `None`.
pub fn snapshot(
    job_id: Option<&str>,
    from_seq: Option<u64>,
    max_bytes: u32,
) -> Vec<JobSnapshot> {
    with_jobs(|map| {
        sweep(map);
        match job_id {
            Some(id) => map
                .get(id)
                .map(|h| vec![h.snapshot(from_seq, max_bytes)])
                .unwrap_or_default(),
            // A listing omits output; callers tail a specific job for that.
            None => {
                let mut all: Vec<_> = map.values().map(|h| h.snapshot(None, 0)).collect();
                all.sort_by_key(|s| s.started_at_ms);
                all
            }
        }
    })
    .unwrap_or_default()
}

/// Requests cancellation of every running job; returns how many were signalled.
pub fn cancel_all() -> u32 {
    with_jobs(|map| {
        let mut n = 0;
        for h in map.values() {
            if !h.state().is_terminal() {
                h.cancel
                    .store(true, std::sync::atomic::Ordering::SeqCst);
                h.tree.terminate();
                n += 1;
            }
        }
        n
    })
    .unwrap_or(0)
}

/// Marks every non-terminal job `Orphaned`. Called at startup: a job recorded
/// as running by a process that is gone must never be resumed, because a
/// half-finished install repeating is worse than one that stopped.
pub fn orphan_stale() -> u32 {
    with_jobs(|map| {
        let mut n = 0;
        for h in map.values() {
            if !h.state().is_terminal() {
                h.set_state(JobState::Orphaned);
                n += 1;
            }
        }
        n
    })
    .unwrap_or(0)
}
