//! Conversion helpers between stress-kit runtime types and database schema types.
//!
//! stress-kit doesn't know about the database. database doesn't know about
//! stress-kit. This module is the only place that knows about both.

use database::schema::{
    random_record_id, CoreSampleRow, DiskRateRow, NetworkRateRow, RecordId, StressKitStressor,
    StressTestMetric, TargetKind, STRESS_TEST_METRIC_TABLE,
};
use stress_kit::{
    telemetry::TelemetrySnapshot,
    Stressor,
};

/// stress-kit's runtime `Stressor` enum → database's persisted `StressKitStressor`.
pub fn stressor_to_db(s: Stressor) -> StressKitStressor {
    match s {
        Stressor::Cpu => StressKitStressor::Cpu,
        Stressor::Memory => StressKitStressor::Memory,
        Stressor::Disk => StressKitStressor::Disk,
        Stressor::Matrix => StressKitStressor::Matrix,
        Stressor::Memcpy => StressKitStressor::Memcpy,
        Stressor::Bitops => StressKitStressor::Bitops,
        Stressor::Cache => StressKitStressor::Cache,
        Stressor::Vm => StressKitStressor::Vm,
        Stressor::Stream => StressKitStressor::Stream,
        Stressor::Branch => StressKitStressor::Branch,
        Stressor::Atomic => StressKitStressor::Atomic,
        Stressor::Mutex => StressKitStressor::Mutex,
        Stressor::Switch => StressKitStressor::Switch,
        Stressor::Prime => StressKitStressor::Prime,
        Stressor::Fp => StressKitStressor::Fp,
        Stressor::Hash => StressKitStressor::Hash,
        Stressor::Prefetch => StressKitStressor::Prefetch,
        Stressor::Icache => StressKitStressor::Icache,
        Stressor::Tsc => StressKitStressor::Tsc,
        Stressor::Gpu => StressKitStressor::Gpu,
        Stressor::GpuMatmul => StressKitStressor::GpuMatmul,
        Stressor::GpuVram => StressKitStressor::GpuVram,
        Stressor::GpuPcie => StressKitStressor::GpuPcie,
    }
}

/// Database's persisted enum → stress-kit's runtime enum (e.g. for spawning a run
/// from a saved TestTool::StressKit { stressor } variant).
pub fn stressor_from_db(s: StressKitStressor) -> Stressor {
    match s {
        StressKitStressor::Cpu => Stressor::Cpu,
        StressKitStressor::Memory => Stressor::Memory,
        StressKitStressor::Disk => Stressor::Disk,
        StressKitStressor::Matrix => Stressor::Matrix,
        StressKitStressor::Memcpy => Stressor::Memcpy,
        StressKitStressor::Bitops => Stressor::Bitops,
        StressKitStressor::Cache => Stressor::Cache,
        StressKitStressor::Vm => Stressor::Vm,
        StressKitStressor::Stream => Stressor::Stream,
        StressKitStressor::Branch => Stressor::Branch,
        StressKitStressor::Atomic => Stressor::Atomic,
        StressKitStressor::Mutex => Stressor::Mutex,
        StressKitStressor::Switch => Stressor::Switch,
        StressKitStressor::Prime => Stressor::Prime,
        StressKitStressor::Fp => Stressor::Fp,
        StressKitStressor::Hash => Stressor::Hash,
        StressKitStressor::Prefetch => Stressor::Prefetch,
        StressKitStressor::Icache => Stressor::Icache,
        StressKitStressor::Tsc => Stressor::Tsc,
        StressKitStressor::Gpu => Stressor::Gpu,
        StressKitStressor::GpuMatmul => Stressor::GpuMatmul,
        StressKitStressor::GpuVram => Stressor::GpuVram,
        StressKitStressor::GpuPcie => Stressor::GpuPcie,
    }
}

/// Sensible default `TargetKind` from a Stressor. Callers can override; this is
/// just a "best guess" for the common case where the operator hasn't picked one.
pub fn default_target_kind(s: Stressor) -> TargetKind {
    match s {
        Stressor::Cpu
        | Stressor::Matrix
        | Stressor::Bitops
        | Stressor::Cache
        | Stressor::Branch
        | Stressor::Atomic
        | Stressor::Mutex
        | Stressor::Switch
        | Stressor::Prime
        | Stressor::Fp
        | Stressor::Hash
        | Stressor::Prefetch
        | Stressor::Icache
        | Stressor::Tsc => TargetKind::Cpu,
        Stressor::Memory | Stressor::Memcpy | Stressor::Vm | Stressor::Stream => {
            TargetKind::Memory
        }
        Stressor::Disk => TargetKind::Storage,
        Stressor::Gpu | Stressor::GpuMatmul | Stressor::GpuVram | Stressor::GpuPcie => {
            TargetKind::Gpu
        }
    }
}

/// Build a `StressTestMetric` row from a telemetry snapshot + the latest stress-kit
/// throughput tick. Caller supplies `run_ref`, `stage_index`/`stage_label` (None for
/// single-stressor runs), and the throughput unit string.
pub fn metric_from_snapshot(
    run_ref: RecordId,
    snapshot: &TelemetrySnapshot,
    throughput: Option<f64>,
    throughput_unit: Option<&str>,
    last_error: Option<&str>,
    stage_index: Option<u32>,
    stage_label: Option<String>,
) -> StressTestMetric {
    let captured_at = snapshot_to_datetime(snapshot.captured_at_unix_ms);

    let cores = snapshot
        .cores
        .iter()
        .map(|c| CoreSampleRow {
            index: c.index as u32,
            brand: c.brand.clone(),
            usage_pct: c.usage_pct,
            freq_mhz: c.freq_mhz,
            temp_c: c.temp_c,
        })
        .collect();

    let disks = snapshot
        .disks
        .iter()
        .map(|d| DiskRateRow {
            name: d.name.clone(),
            read_mb_per_s: d.read_mb_per_s as f64,
            write_mb_per_s: d.write_mb_per_s as f64,
        })
        .collect();

    let networks = snapshot
        .networks
        .iter()
        .map(|n| NetworkRateRow {
            name: n.name.clone(),
            rx_mbps: n.rx_mbps as f64,
            tx_mbps: n.tx_mbps as f64,
        })
        .collect();

    StressTestMetric {
        id: random_record_id(STRESS_TEST_METRIC_TABLE),
        run_ref,
        captured_at,
        stage_index,
        stage_label,
        cores,
        memory_used_pct: Some(snapshot.memory.used_pct),
        memory_used_mb: Some(snapshot.memory.used_mb),
        page_file_used_pct: Some(snapshot.memory.page_file_used_pct),
        disks,
        networks,
        throughput,
        throughput_unit: throughput_unit.map(|s| s.to_string()),
        whea_delta_count: snapshot
            .whea
            .as_ref()
            .map(|w| w.delta_since_program_start as u32),
        last_error: last_error.map(|s| s.to_string()),
    }
}

/// Unix-ms → `Datetime` (chrono::DateTime<Utc> wrapped by SurrealDB).
fn snapshot_to_datetime(unix_ms: u64) -> database::schema::Datetime {
    let secs = (unix_ms / 1000) as i64;
    let nanos = ((unix_ms % 1000) * 1_000_000) as u32;
    chrono::DateTime::<chrono::Utc>::from_timestamp(secs, nanos)
        .unwrap_or_else(chrono::Utc::now)
        .into()
}

/// Stable per-machine ID used to correlate runs across DB resets. Mirrors the
/// qc-app `reporting::machine_id` formula (sha256 of hostname + CPU brand) so
/// runs persisted from both apps collapse to the same machine string.
pub fn compute_machine_id(hostname: &str, cpu_brand: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(hostname.trim().to_ascii_lowercase().as_bytes());
    hasher.update(b"|");
    hasher.update(cpu_brand.trim().to_ascii_lowercase().as_bytes());
    let digest = hasher.finalize();
    hex::encode(&digest[..16])
}
