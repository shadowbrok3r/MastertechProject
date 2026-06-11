//! Schema for benchmark scores. Companion to `database/schema/benchmark_result.surql`.
//!
//! One [`BenchmarkResult`] row per scored benchmark execution. Most
//! benchmarks ride on a `stress_test_run` (linked via `run_ref`) so the
//! full 1 Hz telemetry stays queryable; one-shot measurements like the
//! memory-latency ladder persist standalone with `run_ref = NONE`.

use crate::DATABASE;
use serde::{Deserialize, Serialize};

use super::{random_record_id, Datetime, RecordId, SurrealValue, BENCHMARK_RESULT_TABLE};

/// Lowercase benchmark identifiers; indexed and used for baseline grouping.
/// Keep stable — these strings are how scores compare across machines.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, SurrealValue)]
#[serde(rename_all = "snake_case")]
#[surreal(untagged)]
pub enum BenchmarkKind {
    /// Single-thread FMA chain throughput (Mflop/s).
    #[surreal(value = "cpu_single")]
    CpuSingle,
    /// All-thread FMA chain throughput (Mflop/s).
    #[surreal(value = "cpu_multi")]
    CpuMulti,
    /// Single-thread matrix multiply (Mflop/s).
    #[surreal(value = "matrix_single")]
    MatrixSingle,
    /// All-thread matrix multiply (Mflop/s).
    #[surreal(value = "matrix_multi")]
    MatrixMulti,
    /// LU solve GFLOPS with residual verification.
    #[surreal(value = "linpack")]
    Linpack,
    /// STREAM copy/scale/add/triad bandwidth (GB/s).
    #[surreal(value = "memory_bandwidth")]
    MemoryBandwidth,
    /// Bulk memcpy bandwidth (GB/s).
    #[surreal(value = "memcpy")]
    Memcpy,
    /// Pointer-chase latency ladder; score is RAM-footprint ns/access.
    #[surreal(value = "memory_latency")]
    MemoryLatency,
    /// Temp-file write+sync+read cycle throughput (MiB/s).
    #[surreal(value = "disk")]
    Disk,
    /// GPU compute-shader FMA throughput (GFLOPS).
    #[surreal(value = "gpu_compute")]
    GpuCompute,
    /// GPU NxN matmul throughput (GFLOPS).
    #[surreal(value = "gpu_matmul")]
    GpuMatmul,
    /// VRAM write+verify bandwidth (MiB/s).
    #[surreal(value = "gpu_vram")]
    GpuVram,
    /// CPU<->GPU transfer bandwidth (GB/s).
    #[surreal(value = "gpu_pcie")]
    GpuPcie,
}

impl BenchmarkKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::CpuSingle => "cpu_single",
            Self::CpuMulti => "cpu_multi",
            Self::MatrixSingle => "matrix_single",
            Self::MatrixMulti => "matrix_multi",
            Self::Linpack => "linpack",
            Self::MemoryBandwidth => "memory_bandwidth",
            Self::Memcpy => "memcpy",
            Self::MemoryLatency => "memory_latency",
            Self::Disk => "disk",
            Self::GpuCompute => "gpu_compute",
            Self::GpuMatmul => "gpu_matmul",
            Self::GpuVram => "gpu_vram",
            Self::GpuPcie => "gpu_pcie",
        }
    }

    pub fn all() -> &'static [BenchmarkKind] {
        &[
            Self::CpuSingle,
            Self::CpuMulti,
            Self::MatrixSingle,
            Self::MatrixMulti,
            Self::Linpack,
            Self::MemoryBandwidth,
            Self::Memcpy,
            Self::MemoryLatency,
            Self::Disk,
            Self::GpuCompute,
            Self::GpuMatmul,
            Self::GpuVram,
            Self::GpuPcie,
        ]
    }
}

/// One scored benchmark execution against one computer.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, SurrealValue)]
pub struct BenchmarkResult {
    pub id: RecordId,
    pub computer: RecordId,
    /// Backing stress run when the benchmark drove a stressor; NONE for
    /// one-shot measurements (memory_latency).
    pub run_ref: Option<RecordId>,
    pub kind: BenchmarkKind,
    /// Denormalized `kind.as_str()` for indexed filtering.
    pub kind_label: String,
    /// Steady-state score (warmup samples discarded). Unit in `unit`.
    /// For `memory_latency` lower is better; throughput kinds higher is better.
    pub score: f64,
    pub unit: String,
    pub peak: Option<f64>,
    pub low: Option<f64>,
    /// Telemetry tick samples contributing to `score`.
    pub samples: u32,
    pub threads: u32,
    pub duration_secs: f64,
    /// Errors observed during the measurement window (mismatches, WHEA).
    /// Non-zero invalidates the score for baseline purposes.
    pub errors: u32,
    pub max_temp_c: Option<f32>,
    pub avg_temp_c: Option<f32>,
    /// Full ladder for `memory_latency` (array of {size_kb, latency_ns, read_gb_per_s}).
    pub detail: Option<serde_json::Value>,
    pub hostname: Option<String>,
    pub machine_id: Option<String>,
    pub captured_at: Datetime,
    pub tags: Vec<String>,
    pub notes: Option<String>,
}

impl BenchmarkResult {
    pub fn new(computer: RecordId, kind: BenchmarkKind, score: f64, unit: impl Into<String>) -> Self {
        Self {
            id: random_record_id(BENCHMARK_RESULT_TABLE),
            computer,
            run_ref: None,
            kind,
            kind_label: kind.as_str().to_string(),
            score,
            unit: unit.into(),
            peak: None,
            low: None,
            samples: 0,
            threads: 0,
            duration_secs: 0.0,
            errors: 0,
            max_temp_c: None,
            avg_temp_c: None,
            detail: None,
            hostname: None,
            machine_id: None,
            captured_at: chrono::Utc::now().into(),
            tags: Vec::new(),
            notes: None,
        }
    }

    pub async fn create(result: &Self) -> anyhow::Result<RecordId> {
        let value = result.clone().into_value();
        let created: Option<Self> = DATABASE
            .create(result.id.clone())
            .content(value)
            .await?;
        Ok(created.map(|c| c.id).unwrap_or_else(|| result.id.clone()))
    }

    /// Score history for one machine, newest first, optionally one kind.
    pub async fn list_for_computer(
        computer: &RecordId,
        kind: Option<BenchmarkKind>,
        limit: usize,
    ) -> anyhow::Result<Vec<Self>> {
        let rows: Vec<Self> = DATABASE
            .query(
                "SELECT * FROM benchmark_result \
                 WHERE computer = $c AND ($k = NONE OR kind_label = $k) \
                 ORDER BY captured_at DESC LIMIT $l",
            )
            .bind(("c", computer.clone()))
            .bind(("k", kind.map(|k| k.as_str().to_string())))
            .bind(("l", limit.clamp(1, 500) as i64))
            .await?
            .take(0)?;
        Ok(rows)
    }

    /// Cross-machine history for one benchmark kind (population comparison).
    pub async fn list_for_kind(kind: BenchmarkKind, limit: usize) -> anyhow::Result<Vec<Self>> {
        let rows: Vec<Self> = DATABASE
            .query(
                "SELECT * FROM benchmark_result \
                 WHERE kind_label = $k ORDER BY captured_at DESC LIMIT $l",
            )
            .bind(("k", kind.as_str().to_string()))
            .bind(("l", limit.clamp(1, 1000) as i64))
            .await?
            .take(0)?;
        Ok(rows)
    }
}
