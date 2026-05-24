//! Shared definition of the **QC benchmark** preset.
//!
//! Used by both the MCP `run_qc_benchmark` tool and the GUI Stress Panel's
//! "QC Benchmark" mode so the wire-driven and operator-driven paths can never
//! disagree on what a "QC Benchmark v1" actually runs.
//!
//! The benchmark is the canonical 8-stage burn-in: cpu, matrix, fp, stream,
//! cache, branch, memory, vm. Order is deliberate — pure CPU + FP first to
//! warm the cores, then memory bandwidth + hierarchy under that thermal
//! load, then memory subsystem last so any heat-soaked memory controller has
//! had a chance to drift.
//!
//! Throughput floors are hard-coded permissive minimums. They're a
//! "this machine is broken" smoke check, not a competitive benchmark, and
//! will be superseded by per-component `hardware_test_baseline` lookups in
//! the planned `compare_to_baseline` MCP tool. See [`qc_floor_for`] for the
//! current numbers and how they were chosen.

use stress_kit::Stressor;
use stress_runner::RunStage;

/// Stable preset identifier persisted into `stress_test_run.preset_label`
/// and tagged with `"preset:qc-benchmark"` for cross-machine queries.
pub const QC_BENCHMARK_PRESET: &str = "qc-mcp:benchmark-v1";

pub const GPU_PROBE_PRESET: &str = "qc-mcp:gpu-probe-v1";

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
        mk("cpu",    Stressor::Cpu,    20),
        mk("matrix", Stressor::Matrix, 20),
        mk("fp",     Stressor::Fp,     20),
        mk("stream", Stressor::Stream, 20),
        mk("cache",  Stressor::Cache,  20),
        mk("branch", Stressor::Branch, 20),
        mk("memory", Stressor::Memory, 20),
        mk("vm",     Stressor::Vm,     20),
    ]
}

pub fn gpu_probe_stages(mult: f32) -> Vec<RunStage> {
    fn dur(base: u64, mult: f32) -> u64 {
        ((base as f32) * mult).round().max(1.0) as u64
    }
    let mk = |label: &str, stressor: Stressor, base_secs: u64, mem_mb: u64| RunStage {
        label: label.to_string(),
        stressor,
        threads: 0,
        duration_secs: dur(base_secs, mult),
        memory_cap_mb: mem_mb,
        disk_file_mb: 16,
    };
    vec![
        mk("gpu_compute", Stressor::Gpu,       30, 256),
        mk("gpu_matmul",  Stressor::GpuMatmul, 30, 256),
        mk("gpu_vram",    Stressor::GpuVram,   45, 1024),
        mk("gpu_pcie",    Stressor::GpuPcie,   20, 64),
    ]
}

/// Throughput floor below which a stage is graded `fail` (or `warn` between
/// 0.9× and 1.0×). Permissive — any modern (post-2018) consumer CPU should
/// clear these. Will be replaced by `hardware_test_baseline` lookups keyed
/// on `(target_component, tool_label)` once enough baseline data exists.
///
/// Reference points (single 9950X core at idle, observed Mflop/s etc.):
///   * cpu       ~600  Mop/s     → floor 50
///   * matrix    ~3000 Mflop/s   → floor 200
///   * fp        ~4500 Mflop/s   → floor 200
///   * stream    ~50   GB/s      → floor 5
///   * cache     ~700  Mref/s    → floor 50
///   * branch    ~1200 Mbranch/s → floor 100
///   * memory    ~20   MiB/s     → floor 2
///   * vm        ~4000 MiB/s     → floor 200
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
        Stressor::Gpu => 100.0,
        Stressor::GpuMatmul => 100.0,
        Stressor::GpuVram => 1000.0,
        Stressor::GpuPcie => 1.0,
    }
}
