//! Detected-hardware collection feeding the shared spec-check engine
//! (`database::orders::spec_check`). The pure comparison logic lives in the
//! database crate so the terminal-mode front end and audit tools can reuse it.

pub use database::orders::spec_check::{compare, CheckStatus, SpecCheckReport};
use database::orders::spec_check::{DetectedDisk, DetectedHardware};
use stress_kit::telemetry::TelemetrySnapshot;

/// Snapshot the machine. Disk capacities come from sysinfo volumes, so a
/// multi-partition drive reports per-volume sizes.
pub fn collect_detected(snapshot: &TelemetrySnapshot) -> DetectedHardware {
    let cpu = snapshot
        .cores
        .first()
        .map(|c| c.brand.clone())
        .unwrap_or_default();
    let gpus = snapshot
        .gpus
        .iter()
        .map(|g| {
            if g.name.to_lowercase().contains(&g.vendor.to_lowercase()) {
                g.name.clone()
            } else {
                format!("{} {}", g.vendor, g.name).trim().to_string()
            }
        })
        .collect();

    let disks = sysinfo::Disks::new_with_refreshed_list()
        .list()
        .iter()
        .map(|d| DetectedDisk {
            name: format!(
                "{} ({})",
                d.name().to_string_lossy(),
                d.mount_point().to_string_lossy()
            ),
            total_gb: d.total_space() as f64 / 1_000_000_000.0,
        })
        .collect();

    DetectedHardware {
        cpu,
        ram_total_mb: snapshot.memory.total_mb,
        gpus,
        disks,
        os: sysinfo::System::long_os_version().unwrap_or_default(),
    }
}
