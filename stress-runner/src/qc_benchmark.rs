//! Shared definition of the **QC benchmark** preset.
//!
//! Used by the MCP `run_qc_benchmark` tool and every operator-driven stress
//! panel so the wire-driven and operator-driven paths can never disagree on
//! what a "QC Benchmark v1" actually runs.
//!
//! The benchmark is the canonical 8-stage burn-in: cpu, matrix, fp, stream,
//! cache, branch, memory, vm. Order is deliberate — pure CPU + FP first to
//! warm the cores, then memory bandwidth + hierarchy under that thermal load,
//! then memory subsystem last so any heat-soaked memory controller has had a
//! chance to drift.
//!
//! Throughput floors are hard-coded permissive minimums — a "this machine is
//! broken" smoke check, not a competitive benchmark. See [`qc_floor_for`].

use crate::RunStage;
use stress_kit::Stressor;

/// Stable preset identifier persisted into `stress_test_run.preset_label`
/// and tagged with `"preset:qc-benchmark"` for cross-machine queries.
pub const QC_BENCHMARK_PRESET: &str = "qc-mcp:benchmark-v1";

/// Build the 8-stage QC benchmark, scaled by `mult` (clamped at the caller).
///
/// `mult = 1.0` → ~20 s/stage, ~2.7 min total.
/// `mult = 0.25` → ~5 s/stage, ~40 s total (smoke).
/// `mult = 2.0` → ~40 s/stage, ~5.5 min total (thorough).
pub fn qc_benchmark_stages(mult: f32) -> Vec<RunStage> {
    fn dur(base: u64, mult: f32) -> u64 {
        ((base as f32) * mult).round().max(1.0) as u64
    }
    let mk = |label: &str, stressor: Stressor, base_secs: u64| RunStage {
        label: label.to_string(),
        stressor,
        threads: 0,
        duration_secs: dur(base_secs, mult),
        memory_cap_mb: 1024,
        disk_file_mb: 16,
    };
    vec![
        mk("cpu", Stressor::Cpu, 20),
        mk("matrix", Stressor::Matrix, 20),
        mk("fp", Stressor::Fp, 20),
        mk("stream", Stressor::Stream, 20),
        mk("cache", Stressor::Cache, 20),
        mk("branch", Stressor::Branch, 20),
        mk("memory", Stressor::Memory, 20),
        mk("vm", Stressor::Vm, 20),
    ]
}

/// Throughput floor below which a stage is graded `fail` (or `warn` between
/// 0.9× and 1.0×). Permissive — any modern (post-2018) consumer CPU should
/// clear these. Will be replaced by `hardware_test_baseline` lookups keyed
/// on `(target_component, tool_label)` once enough baseline data exists.
pub fn qc_floor_for(stressor: Stressor) -> f64 {
    match stressor {
        Stressor::Cpu => 50.0,
        Stressor::Matrix => 200.0,
        Stressor::Fp => 200.0,
        Stressor::Stream => 5.0,
        Stressor::Cache => 50.0,
        Stressor::Branch => 100.0,
        Stressor::Memory => 2.0,
        Stressor::Vm => 200.0,
        Stressor::Memcpy => 2.0,
        Stressor::Bitops => 50.0,
        Stressor::Disk => 5.0,
        Stressor::Atomic => 5.0,
        Stressor::Mutex => 0.5,
        Stressor::Switch => 0.05,
        Stressor::Prime => 0.5,
        Stressor::Hash => 50.0,
        Stressor::Prefetch => 50.0,
        Stressor::Icache => 5.0,
        Stressor::Tsc => 5.0,
        // Verify-style stressors spend most cycles on checking; floors are
        // "is it making progress at all" sanity, not performance grades.
        Stressor::MemTest => 100.0,
        Stressor::CpuVerify => 10.0,
        Stressor::Linpack => 1.0,
        Stressor::Psu => 1.0,
        Stressor::Gpu => 100.0,
        Stressor::GpuMatmul => 100.0,
        Stressor::GpuVram => 1000.0,
        Stressor::GpuPcie => 1.0,
        Stressor::Combined => 1.0,
    }
}
