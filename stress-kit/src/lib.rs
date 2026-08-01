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

/// Identity of the adapter a GPU stressor actually bound, so a run record can
/// name the device it certified rather than leaving it unattributed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AdapterIdentity {
    pub name: String,
    pub vendor_id: u32,
    pub device_id: u32,
    /// wgpu `DeviceType`: `DiscreteGpu`, `IntegratedGpu`, `Cpu`, …
    pub device_type: String,
    pub backend: String,
    pub driver: String,
    /// A discrete adapter was requested but a non-discrete one was bound.
    pub integrated_fallback: bool,
}

static LAST_ADAPTER: Mutex<Option<AdapterIdentity>> = Mutex::new(None);

/// Adapter bound by the most recent GPU acquisition, or `None` when no GPU
/// stressor has run in this process.
pub fn last_selected_adapter() -> Option<AdapterIdentity> {
    LAST_ADAPTER.lock().ok().and_then(|g| g.clone())
}

/// Clears the recorded adapter so one run cannot inherit the previous one's.
pub fn clear_selected_adapter() {
    if let Ok(mut g) = LAST_ADAPTER.lock() {
        *g = None;
    }
}

pub(crate) fn record_selected_adapter(identity: AdapterIdentity) {
    if let Ok(mut g) = LAST_ADAPTER.lock() {
        *g = Some(identity);
    }
}

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
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
    /// Square-wave GPU bursts over a continuous all-core CPU FMA load to drive
    /// 12V rail transients; reports combined CPU+GPU GFLOPS.
    /// Appended last so existing bincode variant indices stay stable.
    #[facet(rename = "psu_transient")]
    PsuTransient,
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
            Self::PsuTransient => "PSU Transient",
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
            Self::PsuTransient => "GFLOPS",
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
            Self::PsuTransient,
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

/// Stand-in reason when a stressor sends `fatal` with no `last_error`.
const FATAL_WITHOUT_REASON: &str = "stressor aborted without a reason";

/// Background run; [`Drop`] calls [`StressSession::stop`].
pub struct StressSession {
    cancel: Arc<AtomicBool>,
    metrics_rx: mpsc::Receiver<Metrics>,
    started_at: Instant,
    /// Reason from the first fatal sample drained; never cleared.
    fatal_latch: Mutex<Option<String>>,
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
            fatal_latch: Mutex::new(None),
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

    /// Latest metrics only; drains the channel with `try_recv`. `fatal` latches:
    /// once a fatal sample is drained, every sample returned afterwards carries
    /// `fatal` and the first fatal reason, so a newer tick cannot mask it.
    pub fn try_recv(&self) -> Option<Metrics> {
        let mut latest: Option<Metrics> = None;
        while let Ok(m) = self.metrics_rx.try_recv() {
            if m.fatal {
                self.latch_fatal(m.last_error.as_deref());
            }
            latest = Some(m);
        }
        let mut latest = latest?;
        if let Some(reason) = self.fatal_reason() {
            latest.fatal = true;
            latest.last_error = Some(reason);
        }
        Some(latest)
    }

    /// Stores the first fatal reason seen.
    fn latch_fatal(&self, reason: Option<&str>) {
        if let Ok(mut latched) = self.fatal_latch.lock() {
            if latched.is_none() {
                *latched = Some(reason.unwrap_or(FATAL_WITHOUT_REASON).to_string());
            }
        }
    }

    /// Reason from the first fatal sample [`StressSession::try_recv`] drained,
    /// readable after the stressor has stopped sending.
    pub fn fatal_reason(&self) -> Option<String> {
        self.fatal_latch.lock().ok().and_then(|g| g.clone())
    }
}

impl Drop for StressSession {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
impl StressSession {
    /// Session with no supervisor thread, wired to a caller-held sender.
    fn detached() -> (Self, mpsc::Sender<Metrics>) {
        let (tx, rx) = mpsc::channel();
        (
            Self {
                cancel: Arc::new(AtomicBool::new(false)),
                metrics_rx: rx,
                started_at: Instant::now(),
                fatal_latch: Mutex::new(None),
            },
            tx,
        )
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

    fn sample(throughput: f64, fatal: bool, last_error: Option<&str>) -> Metrics {
        Metrics {
            elapsed_secs: 1.0,
            throughput,
            last_error: last_error.map(str::to_string),
            fatal,
            errors: 0,
        }
    }

    #[test]
    fn fatal_survives_a_newer_tick_in_the_same_drain() {
        let (session, tx) = StressSession::detached();
        tx.send(sample(1.0, true, Some("gpu leg never ran"))).unwrap();
        tx.send(sample(2.0, false, None)).unwrap();

        let m = session.try_recv().expect("drain returned nothing");
        assert!(m.fatal, "newest-only drain dropped the fatal");
        assert_eq!(m.last_error.as_deref(), Some("gpu leg never ran"));
        assert_eq!(m.throughput, 2.0, "newest throughput is still reported");
    }

    #[test]
    fn fatal_latches_across_drains() {
        let (session, tx) = StressSession::detached();
        tx.send(sample(1.0, true, Some("boom"))).unwrap();
        assert!(session.try_recv().expect("first drain").fatal);

        tx.send(sample(3.0, false, Some("just a warning"))).unwrap();
        let m = session.try_recv().expect("second drain");
        assert!(m.fatal, "fatal did not latch across calls");
        assert_eq!(m.last_error.as_deref(), Some("boom"));
        assert_eq!(session.fatal_reason().as_deref(), Some("boom"));
    }

    #[test]
    fn fatal_without_reason_gets_a_placeholder() {
        let (session, tx) = StressSession::detached();
        tx.send(sample(0.0, true, None)).unwrap();
        let m = session.try_recv().expect("drain returned nothing");
        assert_eq!(m.last_error.as_deref(), Some(FATAL_WITHOUT_REASON));
    }

    #[test]
    fn empty_channel_returns_none_even_after_a_fatal() {
        let (session, tx) = StressSession::detached();
        tx.send(sample(0.0, true, Some("boom"))).unwrap();
        assert!(session.try_recv().is_some());
        assert!(session.try_recv().is_none(), "latch must not synthesize ticks");
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
