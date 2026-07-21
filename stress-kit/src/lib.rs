//! CPU, memory, and disk stress helpers for UI-driven burn-in.
//!
//! Caps: [`StressConfig::memory_cap_mb`] bounds heap per memory worker (default 256 MiB);
//! other processes still consume RAM—keep headroom. Disk uses [`StressConfig::disk_file_mb`]
//! temp files under [`std::env::temp_dir`]. CPU checks cancel between bursts of
//! [`crate::stressors::cpu::OPS_PER_BURST`] float ops each.
//!
//! # Example
//!
//! ```rust,no_run
//! use stress_kit::{StressConfig, StressSession, Stressor};
//! use std::time::Duration;
//!
//! let config = StressConfig {
//!     stressor: Stressor::Cpu,
//!     threads: 0,          // 0 = logical CPU count
//!     timeout: Some(Duration::from_secs(10)),
//!     ..Default::default()
//! };
//!
//! let session = StressSession::start(config);
//!
//! // In an update loop:
//! if let Some(m) = session.try_recv() {
//!     println!("{:.1}s  throughput={:.2}", m.elapsed_secs, m.throughput);
//! }
//!
//! session.stop();
//! ```

mod stressors;
pub mod bench;
pub mod gpu_stack;
pub mod scenario;
pub mod telemetry;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use facet::Facet;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Facet)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[repr(u8)]
#[serde(rename_all = "snake_case")]
pub enum Stressor {
    #[facet(rename = "cpu")]
    Cpu,
    #[facet(rename = "memory")]
    Memory,
    #[facet(rename = "disk")]
    Disk,
    /// Square `f32` matmul; reports Mflop/s.
    #[facet(rename = "matrix")]
    Matrix,
    /// Bulk memcpy of paired buffers; reports GB/s.
    #[facet(rename = "memcpy")]
    Memcpy,
    /// Hot-loop bitops (popcount, ctlz, cttz, rotate); reports Mop/s.
    #[facet(rename = "bitops")]
    Bitops,
    /// Cache-line thrash with `_mm_prefetch`/`_mm_clflush` on x86_64; reports Mref/s.
    #[facet(rename = "cache")]
    Cache,
    /// Page-touch + churn to pressure the working set / page file; reports MiB/s.
    #[facet(rename = "vm")]
    Vm,
    /// STREAM-style memory bandwidth (copy/scale/add/triad); reports GB/s.
    #[facet(rename = "stream")]
    Stream,
    /// Data-dependent branches to fuzz the branch predictor; reports Mbranch/s.
    #[facet(rename = "branch")]
    Branch,
    /// Many cores fighting over one `AtomicU64`; reports Mop/s.
    #[facet(rename = "atomic")]
    Atomic,
    /// Many threads contending on a single mutex; reports Mop/s of lock-unlock pairs.
    #[facet(rename = "mutex")]
    Mutex,
    /// Paired threads ping-ponging on condvars to force OS context switches; reports Mctxsw/s.
    #[facet(rename = "switch")]
    Switch,
    /// Sieve of Eratosthenes; reports Mprime/s (primes found per second).
    #[facet(rename = "prime")]
    Prime,
    /// Independent FMA chains; reports Mflop/s.
    #[facet(rename = "fp")]
    Fp,
    /// FNV-1a hashing over a 1 MiB buffer; reports MiB/s.
    #[facet(rename = "hash")]
    Hash,
    /// Sequential + striped reads exercising the HW prefetcher; reports Mref/s.
    #[facet(rename = "prefetch")]
    Prefetch,
    /// Indirect calls through a 64-fn table to spill the i-cache; reports Mcall/s.
    #[facet(rename = "icache")]
    Icache,
    /// `rdtsc` read rate; reports Mread/s.
    #[facet(rename = "tsc")]
    Tsc,
    /// Pattern write/verify memory test (moving inversions, walking ones,
    /// address-in-address, random); reports MiB/s; mismatches counted in `errors`.
    /// DB label is `memtest`; the legacy `mem_test` wire token stays an accepted alias.
    #[facet(rename = "memtest")]
    #[serde(rename = "memtest", alias = "mem_test")]
    MemTest,
    /// Duplicate-execution integer+FP workload compare; reports Mop/s;
    /// mismatches counted in `errors`.
    #[facet(rename = "cpu_verify")]
    CpuVerify,
    /// LU solve with partial pivoting + residual check; reports GFLOPS;
    /// residual breaches counted in `errors`.
    #[facet(rename = "linpack")]
    Linpack,
    /// Combined CPU FMA + GPU compute load for max power draw; reports GFLOPS.
    #[facet(rename = "psu")]
    Psu,

    /// GPU compute-shader FMA + scattered-load hammer; reports GFLOPS.
    #[facet(rename = "gpu")]
    Gpu,
    /// GPU NxN fp32 matmul; reports GFLOPS.
    #[facet(rename = "gpu_matmul")]
    GpuMatmul,
    /// GPU VRAM write-verify pattern walker; reports MiB/s; mismatches counted in `errors`.
    #[facet(rename = "gpu_vram")]
    GpuVram,
    /// CPU↔GPU buffer round-trip with full readback verify; reports GB/s;
    /// mismatches counted in `errors`.
    #[facet(rename = "gpu_pcie")]
    GpuPcie,
    /// Concurrent CPU FMA + RAM bandwidth + GPU compute load; reports combined CPU+GPU GFLOPS.
    #[facet(rename = "combined")]
    Combined,
}

impl Stressor {
    pub fn label(self) -> &'static str {
        match self {
            Self::Cpu => "CPU",
            Self::Memory => "Memory",
            Self::Disk => "Disk I/O",
            Self::Matrix => "Matrix",
            Self::Memcpy => "Memcpy",
            Self::Bitops => "Bitops",
            Self::Cache => "Cache",
            Self::Vm => "VM",
            Self::Stream => "Stream",
            Self::Branch => "Branch",
            Self::Atomic => "Atomic",
            Self::Mutex => "Mutex",
            Self::Switch => "Context Switch",
            Self::Prime => "Prime",
            Self::Fp => "FP/FMA",
            Self::Hash => "Hash",
            Self::Prefetch => "Prefetch",
            Self::Icache => "I-Cache",
            Self::Tsc => "TSC",
            Self::MemTest => "Memory Test",
            Self::CpuVerify => "CPU Verify",
            Self::Linpack => "Linpack",
            Self::Psu => "PSU Load",
            Self::Gpu => "GPU Compute",
            Self::GpuMatmul => "GPU Matmul",
            Self::GpuVram => "GPU VRAM",
            Self::GpuPcie => "GPU PCIe",
            Self::Combined => "Combined (CPU+RAM+GPU)",
        }
    }

    pub fn throughput_unit(self) -> &'static str {
        match self {
            Self::Cpu => "Mop/s",
            Self::Memory => "MiB/s",
            Self::Disk => "MiB/s",
            Self::Matrix => "Mflop/s",
            Self::Memcpy => "GB/s",
            Self::Bitops => "Mop/s",
            Self::Cache => "Mref/s",
            Self::Vm => "MiB/s",
            Self::Stream => "GB/s",
            Self::Branch => "Mbranch/s",
            Self::Atomic => "Mop/s",
            Self::Mutex => "Mop/s",
            Self::Switch => "Mctxsw/s",
            Self::Prime => "Mprime/s",
            Self::Fp => "Mflop/s",
            Self::Hash => "MiB/s",
            Self::Prefetch => "Mref/s",
            Self::Icache => "Mcall/s",
            Self::Tsc => "Mread/s",
            Self::MemTest => "MiB/s",
            Self::CpuVerify => "Mop/s",
            Self::Linpack => "GFLOPS",
            Self::Psu => "GFLOPS",
            Self::Gpu => "GFLOPS",
            Self::GpuMatmul => "GFLOPS",
            Self::GpuVram => "MiB/s",
            Self::GpuPcie => "GB/s",
            Self::Combined => "GFLOPS",
        }
    }

    pub fn is_gpu(self) -> bool {
        matches!(self, Self::Gpu | Self::GpuMatmul | Self::GpuVram | Self::GpuPcie)
    }

    /// `true` when the stressor verifies results and counts mismatches in
    /// [`Metrics::errors`] rather than only generating load.
    pub fn detects_errors(self) -> bool {
        matches!(
            self,
            Self::MemTest | Self::CpuVerify | Self::Linpack | Self::Disk | Self::GpuVram | Self::GpuPcie
        )
    }

    pub fn all() -> &'static [Stressor] {
        &[
            Self::Cpu,
            Self::Memory,
            Self::Disk,
            Self::Matrix,
            Self::Memcpy,
            Self::Bitops,
            Self::Cache,
            Self::Vm,
            Self::Stream,
            Self::Branch,
            Self::Atomic,
            Self::Mutex,
            Self::Switch,
            Self::Prime,
            Self::Fp,
            Self::Hash,
            Self::Prefetch,
            Self::Icache,
            Self::Tsc,
            Self::MemTest,
            Self::CpuVerify,
            Self::Linpack,
            Self::Psu,
            Self::Gpu,
            Self::GpuMatmul,
            Self::GpuVram,
            Self::GpuPcie,
            Self::Combined,
        ]
    }

    /// Canonical snake_case label — the DB, wire, and MCP vocabulary.
    pub fn as_str(self) -> &'static str {
        facet::Peek::new(&self)
            .into_enum()
            .ok()
            .and_then(|e| e.active_variant().ok())
            .and_then(|v| v.rename)
            .unwrap_or("cpu")
    }

    /// Parse a canonical label back to a variant; accepts the legacy `mem_test` spelling.
    pub fn from_str(s: &str) -> Option<Self> {
        let s = if s == "mem_test" { "memtest" } else { s };
        Self::all().iter().copied().find(|v| v.as_str() == s)
    }

    /// All canonical labels, reflected from SHAPE in declaration order.
    pub fn labels() -> impl Iterator<Item = &'static str> {
        variant_renames(Self::SHAPE)
    }

    /// `"cpu, memory, disk, …, combined"` for the MCP description strings.
    pub fn labels_csv() -> &'static str {
        static CSV: std::sync::LazyLock<String> =
            std::sync::LazyLock::new(|| Stressor::labels().collect::<Vec<_>>().join(", "));
        &CSV
    }

    /// Ready-made MCP field description.
    pub fn wire_description() -> &'static str {
        static D: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
            format!("Stressor (snake_case): {}", Stressor::labels_csv())
        });
        &D
    }
}

/// Reflected `#[facet(rename)]` (falling back to variant name) in declaration order.
fn variant_renames(shape: &'static facet::Shape) -> impl Iterator<Item = &'static str> {
    use facet::{Type, UserType};
    let variants: &'static [facet::Variant] = match shape.ty {
        Type::User(UserType::Enum(ref e)) => e.variants,
        _ => &[],
    };
    variants.iter().map(|v| v.rename.unwrap_or(v.name))
}

/// Single-run settings. In [`scenario::ScenarioStage`], `timeout` is ignored; stage length is `duration_secs`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StressConfig {
    pub stressor: Stressor,
    /// `0` = logical CPU count.
    pub threads: usize,
    pub timeout: Option<Duration>,
    /// Memory stressor only: MiB cap split across workers.
    pub memory_cap_mb: u64,
    /// Disk stressor only: MiB per write/read file.
    pub disk_file_mb: u64,
}

impl Default for StressConfig {
    fn default() -> Self {
        Self {
            stressor: Stressor::Cpu,
            threads: 0,
            timeout: None,
            memory_cap_mb: 256,
            disk_file_mb: 16,
        }
    }
}

/// Sample from the supervisor (~500 ms): elapsed wall time, throughput (`Stressor::throughput_unit`), optional worker warning.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Metrics {
    pub elapsed_secs: f64,
    pub throughput: f64,
    pub last_error: Option<String>,
    /// `true` when the stressor is giving up — paired with `last_error`
    /// carrying the reason. Downstream (scenario, controller, MCP) treats
    /// this as a stage-level abort signal.
    #[serde(default)]
    pub fatal: bool,
    /// Cumulative detected-error count since the stressor started (data
    /// mismatches, residual breaches). `0` for load-only stressors.
    #[serde(default)]
    pub errors: u64,
}

/// Background run; [`Drop`] calls [`StressSession::stop`].
pub struct StressSession {
    cancel: Arc<AtomicBool>,
    metrics_rx: mpsc::Receiver<Metrics>,
    started_at: Instant,
}

impl StressSession {
    pub fn start(config: StressConfig) -> Self {
        let cancel = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel::<Metrics>();

        let thread_count = if config.threads == 0 {
            thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
        } else {
            config.threads
        };

        let cancel_clone = cancel.clone();
        let started_at = Instant::now();

        thread::Builder::new()
            .name("stress-kit-supervisor".into())
            .spawn(move || {
                stressors::run(config, thread_count, cancel_clone, tx, started_at);
            })
            .expect("stress-kit: failed to spawn supervisor thread");

        Self {
            cancel,
            metrics_rx: rx,
            started_at,
        }
    }

    pub fn stop(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    /// `true` after `stop`, timeout, or drop; workers may still be winding down.
    pub fn is_stopping(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    pub fn elapsed(&self) -> Duration {
        self.started_at.elapsed()
    }

    /// Latest metrics only; drains the channel with `try_recv`.
    pub fn try_recv(&self) -> Option<Metrics> {
        let mut latest = None;
        while let Ok(m) = self.metrics_rx.try_recv() {
            latest = Some(m);
        }
        latest
    }
}

impl Drop for StressSession {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_roundtrip_matches_serde() {
        for &s in Stressor::all() {
            assert_eq!(Stressor::from_str(s.as_str()), Some(s), "roundtrip {s:?}");
            assert_eq!(
                serde_json::to_value(s).unwrap(),
                serde_json::json!(s.as_str()),
                "serde vs as_str disagree for {s:?}"
            );
        }
    }

    #[test]
    fn labels_reflect_all_in_order() {
        let reflected: Vec<&str> = Stressor::labels().collect();
        let via_all: Vec<&str> = Stressor::all().iter().map(|s| s.as_str()).collect();
        assert_eq!(reflected, via_all);
        assert!(!reflected.is_empty());
        assert!(reflected.contains(&"combined"));
    }

    #[test]
    fn memtest_accepts_legacy_alias() {
        assert_eq!(Stressor::from_str("mem_test"), Some(Stressor::MemTest));
        assert_eq!(Stressor::from_str("memtest"), Some(Stressor::MemTest));
        assert_eq!(Stressor::MemTest.as_str(), "memtest");
        assert_eq!(
            serde_json::from_str::<Stressor>("\"mem_test\"").unwrap(),
            Stressor::MemTest
        );
    }

    #[test]
    fn unknown_label_is_none() {
        assert_eq!(Stressor::from_str("nonsense"), None);
    }

    /// Run a stressor briefly and return the last metrics sample seen.
    fn run_briefly(stressor: Stressor, memory_cap_mb: u64, secs: u64) -> Metrics {
        let session = StressSession::start(StressConfig {
            stressor,
            threads: 1,
            timeout: Some(Duration::from_secs(secs)),
            memory_cap_mb,
            disk_file_mb: 1,
        });
        let deadline = Instant::now() + Duration::from_secs(secs + 8);
        let mut last: Option<Metrics> = None;
        while Instant::now() < deadline {
            if let Some(m) = session.try_recv() {
                last = Some(m);
            }
            if session.is_stopping() {
                thread::sleep(Duration::from_millis(200));
                if let Some(m) = session.try_recv() {
                    last = Some(m);
                }
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
        last.expect("stressor emitted no metrics")
    }

    #[test]
    fn memtest_clean_on_healthy_memory() {
        let m = run_briefly(Stressor::MemTest, 32, 3);
        assert_eq!(m.errors, 0, "memtest reported errors: {:?}", m.last_error);
        assert!(m.throughput > 0.0, "memtest produced no throughput");
    }

    #[test]
    fn cpu_verify_deterministic() {
        let m = run_briefly(Stressor::CpuVerify, 16, 2);
        assert_eq!(m.errors, 0, "cpu_verify mismatch: {:?}", m.last_error);
        assert!(m.throughput > 0.0);
    }

    #[test]
    fn linpack_residual_within_threshold() {
        // 1 MiB cap clamps to the N=256 floor so debug-mode solves finish
        // well inside the window.
        let m = run_briefly(Stressor::Linpack, 1, 5);
        assert_eq!(m.errors, 0, "linpack residual breach: {:?}", m.last_error);
        assert!(m.throughput > 0.0, "linpack completed no solves in window");
    }

    #[test]
    fn latency_ladder_sane() {
        let points = bench::measure_ladder(&[64, 4096]);
        assert_eq!(points.len(), 2);
        for p in &points {
            assert!(p.latency_ns > 0.0 && p.latency_ns < 10_000.0);
            assert!(p.read_gb_per_s > 0.0);
        }
    }
}
