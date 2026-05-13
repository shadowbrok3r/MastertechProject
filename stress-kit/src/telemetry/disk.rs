//! Per-disk read/write rate sample. `sysinfo::DiskUsage` exposes the bytes
//! moved since the last `Disks::refresh`, so we just divide by the elapsed
//! tick interval.

use serde::{Deserialize, Serialize};
use sysinfo::Disks;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DiskRateSample {
    pub name: String,
    pub read_mb_per_s: f32,
    pub write_mb_per_s: f32,
}

pub fn sample_disks(disks: &Disks, interval_secs: f32) -> Vec<DiskRateSample> {
    let interval = interval_secs.max(f32::EPSILON);
    disks
        .iter()
        .map(|d| {
            let usage = d.usage();
            let read_mb = usage.read_bytes as f32 / (1024.0 * 1024.0);
            let write_mb = usage.written_bytes as f32 / (1024.0 * 1024.0);
            DiskRateSample {
                name: d.name().to_string_lossy().into_owned(),
                read_mb_per_s: read_mb / interval,
                write_mb_per_s: write_mb / interval,
            }
        })
        .collect()
}
