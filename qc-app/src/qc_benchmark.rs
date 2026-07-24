//! QC benchmark recipe — moved into `stress-runner` as the single source of
//! truth (shared by the MCP `run_qc_benchmark` tool, the GUI stress panel, and
//! Mastertech4.0). Re-exported here to keep `crate::qc_benchmark` paths working.

pub use stress_runner::{
    gpu_probe_stages, qc_benchmark_stages, qc_floor_for, GPU_PROBE_PRESET, QC_BENCHMARK_PRESET,
};
