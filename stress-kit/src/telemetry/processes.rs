//! Top-N process sampler.
//!
//! `sysinfo`'s `System` already has `refresh_processes` called by the
//! supervisor loop. We piggyback on that snapshot to publish the heaviest
//! processes by CPU% (with RAM as the tiebreaker). Capping at `TOP_N` keeps
//! the `TelemetrySnapshot` small enough that MCP clients can render it
//! without pagination on the typical 200–500 process Windows workstation.

use serde::{Deserialize, Serialize};
use sysinfo::{Pid, System};

pub const TOP_N: usize = 64;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProcessSample {
    pub pid: u32,
    /// Best-effort process name from `sysinfo` (truncated, no extension stripping).
    pub name: String,
    /// 0–N*100 (sysinfo returns per-thread aggregate). Normalize on the
    /// client side if you want 0–100 across all cores.
    pub cpu_pct: f32,
    /// Resident memory in MB.
    pub mem_mb: u64,
    /// Number of running threads, when reported by sysinfo.
    pub thread_count: Option<u32>,
    /// Parent PID, when available.
    pub parent_pid: Option<u32>,
}

pub fn sample_processes(sys: &System) -> Vec<ProcessSample> {
    let mut all: Vec<ProcessSample> = sys
        .processes()
        .iter()
        .map(|(pid, p)| ProcessSample {
            pid: pid_to_u32(*pid),
            name: p.name().to_string_lossy().to_string(),
            cpu_pct: p.cpu_usage(),
            mem_mb: p.memory() / (1024 * 1024),
            thread_count: None, // sysinfo 0.39 doesn't surface this uniformly.
            parent_pid: p.parent().map(pid_to_u32),
        })
        .collect();

    // Sort heaviest first by (CPU desc, RAM desc) so the truncation keeps
    // the rows an operator actually wants to see.
    all.sort_by(|a, b| {
        b.cpu_pct
            .partial_cmp(&a.cpu_pct)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.mem_mb.cmp(&a.mem_mb))
    });
    all.truncate(TOP_N);
    all
}

#[inline]
fn pid_to_u32(p: Pid) -> u32 {
    p.as_u32()
}
