//! Conversion helpers between stress-kit runtime types and database schema types.
//!
//! stress-kit doesn't know about the database. database doesn't know about
//! stress-kit. This module is the only place that knows about both.

use database::schema::{
    random_record_id, CoreSampleRow, DiskRateRow, NetworkRateRow, RecordId, StressKitStressor,
    StressTestMetric, TargetKind, COMPUTER_TABLE, STRESS_TEST_METRIC_TABLE,
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
        Stressor::MemTest => StressKitStressor::MemTest,
        Stressor::CpuVerify => StressKitStressor::CpuVerify,
        Stressor::Linpack => StressKitStressor::Linpack,
        Stressor::Psu => StressKitStressor::Psu,
        Stressor::Gpu => StressKitStressor::Gpu,
        Stressor::GpuMatmul => StressKitStressor::GpuMatmul,
        Stressor::GpuVram => StressKitStressor::GpuVram,
        Stressor::GpuPcie => StressKitStressor::GpuPcie,
        // No persisted variant yet; recorded as the closest existing combined-load kind.
        Stressor::Combined => StressKitStressor::Psu,
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
        StressKitStressor::MemTest => Stressor::MemTest,
        StressKitStressor::CpuVerify => Stressor::CpuVerify,
        StressKitStressor::Linpack => Stressor::Linpack,
        StressKitStressor::Psu => Stressor::Psu,
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
        | Stressor::Tsc
        | Stressor::CpuVerify
        | Stressor::Linpack => TargetKind::Cpu,
        Stressor::Memory
        | Stressor::Memcpy
        | Stressor::Vm
        | Stressor::Stream
        | Stressor::MemTest => TargetKind::Memory,
        Stressor::Psu => TargetKind::Psu,
        Stressor::Disk => TargetKind::Storage,
        Stressor::Gpu | Stressor::GpuMatmul | Stressor::GpuVram | Stressor::GpuPcie => {
            TargetKind::Gpu
        }
        Stressor::Combined => TargetKind::System,
    }
}

/// Build a `StressTestMetric` row from a populated telemetry snapshot + throughput tick.
pub fn metric_from_snapshot(
    run_ref: RecordId,
    snapshot: &TelemetrySnapshot,
    throughput: Option<f64>,
    throughput_unit: Option<&str>,
    last_error: Option<&str>,
    stage_index: Option<u32>,
    stage_label: Option<String>,
) -> anyhow::Result<StressTestMetric> {
    validate_snapshot_for_metric(snapshot)?;

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

    Ok(StressTestMetric {
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
        cpu_temp_c: snapshot.cpu_package_temp_c(),
        gpu_temp_c: fold_gpu_max(snapshot, |g| g.temp_c),
        gpu_clock_mhz: fold_gpu_max(snapshot, |g| g.gpu_clock_mhz),
        gpu_power_w: fold_gpu_max(snapshot, |g| g.power_w),
        gpu_usage_pct: fold_gpu_max(snapshot, |g| g.usage_pct),
        tdr_delta_count: snapshot
            .tdr
            .as_ref()
            .map(|t| t.delta_since_program_start as u32),
        cpu_usage_pct: mean_core_usage_pct(snapshot),
        clock_mhz: mean_core_clock_mhz(snapshot),
        power_w: gpu_power_sum_w(snapshot),
    })
}

/// Mean `usage_pct` across logical cores; `None` when no cores sampled.
fn mean_core_usage_pct(snapshot: &TelemetrySnapshot) -> Option<f32> {
    if snapshot.cores.is_empty() {
        return None;
    }
    let sum: f32 = snapshot.cores.iter().map(|c| c.usage_pct).sum();
    Some(sum / snapshot.cores.len() as f32)
}

/// Mean core clock (MHz) across cores reporting a nonzero frequency.
fn mean_core_clock_mhz(snapshot: &TelemetrySnapshot) -> Option<u32> {
    let (sum, count) = snapshot
        .cores
        .iter()
        .map(|c| c.freq_mhz)
        .filter(|&mhz| mhz > 0)
        .fold((0u64, 0u64), |(s, n), mhz| (s + mhz, n + 1));
    (count > 0).then(|| (sum / count) as u32)
}

/// Board power (W) summed across GPUs; the PSU-load proxy (no portable CPU power source).
fn gpu_power_sum_w(snapshot: &TelemetrySnapshot) -> Option<f32> {
    let sum: f32 = snapshot.gpus.iter().filter_map(|g| g.power_w).sum();
    (sum > 0.0).then_some(sum)
}

/// Max of one optional field across all sampled GPUs.
fn fold_gpu_max<T: PartialOrd + Copy>(
    snapshot: &TelemetrySnapshot,
    f: impl Fn(&stress_kit::telemetry::GpuSample) -> Option<T>,
) -> Option<T> {
    snapshot
        .gpus
        .iter()
        .filter_map(f)
        .fold(None, |acc: Option<T>, v| match acc {
            Some(m) if m >= v => Some(m),
            _ => Some(v),
        })
}

fn validate_snapshot_for_metric(snapshot: &TelemetrySnapshot) -> anyhow::Result<()> {
    if snapshot.captured_at_unix_ms == 0 {
        anyhow::bail!(
            "telemetry snapshot has captured_at_unix_ms=0 (default/uninitialized)"
        );
    }
    if snapshot.cores.is_empty() {
        anyhow::bail!("telemetry snapshot has no CPU core samples");
    }
    if snapshot.memory.total_mb == 0 {
        anyhow::bail!("telemetry snapshot has memory.total_mb=0 (sysinfo not refreshed)");
    }
    Ok(())
}

/// Unix-ms → `Datetime` (chrono::DateTime<Utc> wrapped by SurrealDB).
fn snapshot_to_datetime(unix_ms: u64) -> database::schema::Datetime {
    let secs = (unix_ms / 1000) as i64;
    let nanos = ((unix_ms % 1000) * 1_000_000) as u32;
    chrono::DateTime::<chrono::Utc>::from_timestamp(secs, nanos)
        .filter(|dt| dt.timestamp() > 0)
        .unwrap_or_else(chrono::Utc::now)
        .into()
}

/// Pure SHA-256 of `hostname-cpu-PROCESSOR_IDENTIFIER`. Seed for `stable_machine_hash`.
pub fn hardware_hash(hostname: &str, cpu_brand: &str) -> String {
    use sha2::{Digest, Sha256};
    let cpu_id = std::env::var("PROCESSOR_IDENTIFIER").unwrap_or_else(|_| "unknown-cpu".to_string());
    let combined = format!("{}-{}-{}", hostname, cpu_brand.trim(), cpu_id);
    let mut hasher = Sha256::new();
    hasher.update(combined.as_bytes());
    hex::encode(hasher.finalize())
}

/// Frozen identity hash for this host; args ignored, value from `stable_machine_hash`.
pub fn generate_client_hash(_hostname: &str, _cpu_brand: &str) -> String {
    stable_machine_hash()
}

/// Persisted machine-id file path (`machine_id.txt`).
fn machine_id_path() -> Option<std::path::PathBuf> {
    directories::ProjectDirs::from("com", "Mastertech", "MastertechQC")
        .map(|p| p.data_local_dir().join("machine_id.txt"))
}

/// Live hardware hash for this host.
fn hardware_hash_local() -> String {
    use sysinfo::{CpuRefreshKind, RefreshKind, System};
    let hostname = System::host_name().unwrap_or_default();
    let mut sys =
        System::new_with_specifics(RefreshKind::nothing().with_cpu(CpuRefreshKind::everything()));
    sys.refresh_cpu_list(CpuRefreshKind::everything());
    let cpu = sys
        .cpus()
        .first()
        .map(|c| c.brand().trim().to_string())
        .unwrap_or_default();
    hardware_hash(&hostname, &cpu)
}

/// This machine's frozen identity hash; seeds from hardware once, then reads `machine_id.txt`.
pub fn stable_machine_hash() -> String {
    use std::sync::OnceLock;
    static CACHED: OnceLock<String> = OnceLock::new();
    CACHED
        .get_or_init(|| {
            let path = machine_id_path();
            if let Some(p) = &path {
                if let Ok(existing) = std::fs::read_to_string(p) {
                    let trimmed = existing.trim();
                    if trimmed.len() >= 32 && trimmed.bytes().all(|b| b.is_ascii_hexdigit()) {
                        log::info!("stable_machine_hash: loaded {} = {trimmed}", p.display());
                        return trimmed.to_string();
                    }
                }
            }
            let seed = hardware_hash_local();
            if let Some(p) = &path {
                if let Some(parent) = p.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                if std::fs::write(p, &seed).is_ok() {
                    log::info!("stable_machine_hash: seeded {} = {seed}", p.display());
                }
            }
            seed
        })
        .clone()
}

/// Record key for `computer` / `connected_client` (`HOSTNAME:hash_prefix`).
pub fn computer_record_key(hostname: &str, cpu_brand: &str) -> String {
    let hash = generate_client_hash(hostname, cpu_brand);
    format!("{}:{}", hostname, &hash[..9])
}

/// Full client hash stored on `StressTestRun.machine_id` for correlation.
pub fn compute_machine_id(hostname: &str, cpu_brand: &str) -> String {
    generate_client_hash(hostname, cpu_brand)
}

/// Stable `computer:<HOSTNAME:hash9>` for the local host.
pub fn local_computer_record() -> RecordId {
    use std::sync::OnceLock;
    use sysinfo::{CpuRefreshKind, RefreshKind, System};

    static CACHED: OnceLock<RecordId> = OnceLock::new();
    CACHED
        .get_or_init(|| {
            let hostname = System::host_name().unwrap_or_default();
            let mut sys = System::new_with_specifics(
                RefreshKind::nothing().with_cpu(CpuRefreshKind::everything()),
            );
            sys.refresh_cpu_list(CpuRefreshKind::everything());
            let cpu = sys
                .cpus()
                .first()
                .map(|c| c.brand().trim().to_string())
                .unwrap_or_default();
            let key = computer_record_key(&hostname, &cpu);
            RecordId::new(COMPUTER_TABLE, key)
        })
        .clone()
}
