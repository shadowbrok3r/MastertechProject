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
    /// Volume capacity; 0 when the payload carried no capacity.
    #[serde(default)]
    pub total_bytes: u64,
    #[serde(default)]
    pub available_bytes: u64,
    #[serde(default)]
    pub file_system: String,
}

impl DiskRateSample {
    /// Fraction of the volume in use, or `None` when capacity is absent.
    pub fn used_fraction(&self) -> Option<f32> {
        (self.total_bytes > 0 && self.available_bytes <= self.total_bytes).then(|| {
            let used = self.total_bytes - self.available_bytes;
            used as f32 / self.total_bytes as f32
        })
    }
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
                total_bytes: d.total_space(),
                available_bytes: d.available_space(),
                file_system: d.file_system().to_string_lossy().into_owned(),
            }
        })
        .collect()
}
