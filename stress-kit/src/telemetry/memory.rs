//! Memory subsystem snapshot: RAM totals/used, the WSL `vmmem` working set when
//! present, and page-file usage. Page-file numbers come from the Win32
//! `GetPerformanceInfo` API on Windows; non-Windows targets leave them at zero.

use serde::{Deserialize, Serialize};
use sysinfo::System;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemorySample {
    pub total_mb: u64,
    pub used_mb: u64,
    pub used_pct: f32,
    /// MB resident in the WSL/Hyper-V `vmmem` process, if it is running.
    pub vmmem_mb: Option<u64>,
    pub page_file_total_mb: u64,
    pub page_file_used_mb: u64,
    pub page_file_used_pct: f32,
}

pub fn sample_memory(sys: &System) -> MemorySample {
    let total_bytes = sys.total_memory();
    let used_bytes = sys.used_memory();
    let total_mb = total_bytes / (1024 * 1024);
    let used_mb = used_bytes / (1024 * 1024);
    let used_pct = if total_bytes > 0 {
        (used_bytes as f32 / total_bytes as f32) * 100.0
    } else {
        0.0
    };

    let vmmem_mb = find_vmmem_mb(sys);
    let (pf_total_mb, pf_used_mb, pf_used_pct) = page_file_stats(sys);

    MemorySample {
        total_mb,
        used_mb,
        used_pct,
        vmmem_mb,
        page_file_total_mb: pf_total_mb,
        page_file_used_mb: pf_used_mb,
        page_file_used_pct: pf_used_pct,
    }
}

fn find_vmmem_mb(sys: &System) -> Option<u64> {
    sys.processes()
        .values()
        .find(|p| p.name().to_string_lossy().eq_ignore_ascii_case("vmmem"))
        .map(|p| p.memory() / (1024 * 1024))
}

#[cfg(target_os = "windows")]
fn page_file_stats(sys: &System) -> (u64, u64, f32) {
    use windows::Win32::System::ProcessStatus::{GetPerformanceInfo, PERFORMANCE_INFORMATION};

    let mut info = PERFORMANCE_INFORMATION::default();
    let size = std::mem::size_of::<PERFORMANCE_INFORMATION>() as u32;
    let ok = unsafe { GetPerformanceInfo(&mut info, size).is_ok() };
    if !ok {
        return (0, 0, 0.0);
    }

    let page = info.PageSize as u64;
    let commit_limit_bytes = (info.CommitLimit as u64) * page;
    let commit_total_bytes = (info.CommitTotal as u64) * page;

    // Approximate page-file size as (commit limit) - (physical RAM total).
    // Approximate page-file usage as committed bytes that don't fit in physical RAM:
    //     used ≈ max(0, commit_total - (phys_total - phys_avail))
    let phys_total = sys.total_memory();
    let phys_avail = sys.available_memory();
    let phys_used = phys_total.saturating_sub(phys_avail);

    let page_file_total = commit_limit_bytes.saturating_sub(phys_total);
    let page_file_used = commit_total_bytes.saturating_sub(phys_used);
    let page_file_used = page_file_used.min(page_file_total);

    let pct = if page_file_total > 0 {
        (page_file_used as f32 / page_file_total as f32) * 100.0
    } else {
        0.0
    };

    (
        page_file_total / (1024 * 1024),
        page_file_used / (1024 * 1024),
        pct,
    )
}

#[cfg(not(target_os = "windows"))]
fn page_file_stats(_sys: &System) -> (u64, u64, f32) {
    (0, 0, 0.0)
}
