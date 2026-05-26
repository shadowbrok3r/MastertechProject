//! Shared catalog of stress-test script names exposed via the Mastertech
//! Scripts tab + MCP `scripts_list` / `scripts_run`.
//!
//! Every entry persists `stress_test_run` + `stress_test_event` +
//! `hardware_component` rows via `drive_blocking`, so callers don't need to
//! special-case any script — they look it up by display name, build the spec,
//! and pass it to `drive_blocking`.
//!
//! `QC Benchmark` lives in `qc-app/src/qc_benchmark.rs` because the floor
//! tables travel with the qc-app MCP scoring tools; callers add it separately.

use database::schema::{RecordId, TargetKind, TestTool};
use stress_kit::Stressor;

use crate::{gpu_probe_spec, RunPlan, RunSpec, RunStage};

/// Mirror of `qc_app::qc_benchmark::QC_BENCHMARK_PRESET`. Kept in sync by hand;
/// stress-runner does not depend on qc-app because qc-app is a full eframe GUI
/// crate. Floor tables live in qc-app where the scoring MCP tools consume them.
pub const QC_BENCHMARK_PRESET: &str = "qc-mcp:benchmark-v1";

pub const STRESS_SCRIPT_NAMES: &[&str] = &[
    "GPU Stress Test",
    "Stress: CPU",
    "Stress: Matrix",
    "Stress: FP/FMA",
    "Stress: Bitops",
    "Stress: Branch",
    "Stress: Prime",
    "Stress: Hash",
    "Stress: Cache",
    "Stress: Prefetch",
    "Stress: I-Cache",
    "Stress: TSC",
    "Stress: Atomic",
    "Stress: Mutex",
    "Stress: Context Switch",
    "Stress: Memory",
    "Stress: Memcpy",
    "Stress: Stream",
    "Stress: VM",
    "Stress: Disk",
    "Stress: GPU Compute",
    "Stress: GPU Matmul",
    "Stress: GPU VRAM",
    "Stress: GPU PCIe",
];

pub fn is_stress_script(name: &str) -> bool {
    STRESS_SCRIPT_NAMES.contains(&name) || name == "QC Benchmark"
}

pub fn build_stress_script_spec(
    name: &str,
    computer: RecordId,
    duration_secs: u64,
) -> Option<RunSpec> {
    match name {
        "GPU Stress Test" => Some(gpu_probe_spec(computer, 1.0)),
        "QC Benchmark" => Some(qc_benchmark_spec(computer)),
        "Stress: CPU" => Some(single(computer, Stressor::Cpu, duration_secs, "cpu")),
        "Stress: Matrix" => Some(single(computer, Stressor::Matrix, duration_secs, "matrix")),
        "Stress: FP/FMA" => Some(single(computer, Stressor::Fp, duration_secs, "fp")),
        "Stress: Bitops" => Some(single(computer, Stressor::Bitops, duration_secs, "bitops")),
        "Stress: Branch" => Some(single(computer, Stressor::Branch, duration_secs, "branch")),
        "Stress: Prime" => Some(single(computer, Stressor::Prime, duration_secs, "prime")),
        "Stress: Hash" => Some(single(computer, Stressor::Hash, duration_secs, "hash")),
        "Stress: Cache" => Some(single(computer, Stressor::Cache, duration_secs, "cache")),
        "Stress: Prefetch" => Some(single(computer, Stressor::Prefetch, duration_secs, "prefetch")),
        "Stress: I-Cache" => Some(single(computer, Stressor::Icache, duration_secs, "icache")),
        "Stress: TSC" => Some(single(computer, Stressor::Tsc, duration_secs, "tsc")),
        "Stress: Atomic" => Some(single(computer, Stressor::Atomic, duration_secs, "atomic")),
        "Stress: Mutex" => Some(single(computer, Stressor::Mutex, duration_secs, "mutex")),
        "Stress: Context Switch" => Some(single(computer, Stressor::Switch, duration_secs, "switch")),
        "Stress: Memory" => Some(single(computer, Stressor::Memory, duration_secs, "memory")),
        "Stress: Memcpy" => Some(single(computer, Stressor::Memcpy, duration_secs, "memcpy")),
        "Stress: Stream" => Some(single(computer, Stressor::Stream, duration_secs, "stream")),
        "Stress: VM" => Some(single(computer, Stressor::Vm, duration_secs, "vm")),
        "Stress: Disk" => Some(single(computer, Stressor::Disk, duration_secs, "disk")),
        "Stress: GPU Compute" => Some(single(computer, Stressor::Gpu, duration_secs, "gpu")),
        "Stress: GPU Matmul" => Some(single(computer, Stressor::GpuMatmul, duration_secs, "gpu_matmul")),
        "Stress: GPU VRAM" => Some(single(computer, Stressor::GpuVram, duration_secs, "gpu_vram")),
        "Stress: GPU PCIe" => Some(single(computer, Stressor::GpuPcie, duration_secs, "gpu_pcie")),
        _ => None,
    }
}

fn single(computer: RecordId, s: Stressor, secs: u64, label: &str) -> RunSpec {
    let mut spec = RunSpec::single_stresskit(computer, s, Some(secs));
    spec.preset_label = Some(format!("scripts:single:{label}"));
    spec.tags.push(format!("preset:single:{label}"));
    spec
}

fn qc_benchmark_stages() -> Vec<RunStage> {
    let mk = |label: &str, stressor: Stressor| RunStage {
        label: label.to_string(),
        stressor,
        threads: 0,
        duration_secs: 20,
        memory_cap_mb: 1024,
        disk_file_mb: 16,
    };
    vec![
        mk("cpu", Stressor::Cpu),
        mk("matrix", Stressor::Matrix),
        mk("fp", Stressor::Fp),
        mk("stream", Stressor::Stream),
        mk("cache", Stressor::Cache),
        mk("branch", Stressor::Branch),
        mk("memory", Stressor::Memory),
        mk("vm", Stressor::Vm),
    ]
}

fn qc_benchmark_spec(computer: RecordId) -> RunSpec {
    let stages = qc_benchmark_stages();
    RunSpec {
        computer,
        tool: TestTool::StressKitScenario {
            name: Some(QC_BENCHMARK_PRESET.into()),
        },
        target_kind: TargetKind::Cpu,
        target_component: None,
        touched_components: Vec::new(),
        service_order: None,
        session_ref: None,
        task_ref: None,
        tech: None,
        hostname: None,
        machine_id: None,
        bios_settings: Default::default(),
        driver_versions: Default::default(),
        notes: None,
        preset_label: Some(QC_BENCHMARK_PRESET.into()),
        tags: vec!["preset:qc-benchmark".into()],
        plan: RunPlan::Scenario {
            stages,
            total_wall_secs: None,
            repeat_until_total: false,
        },
    }
}
