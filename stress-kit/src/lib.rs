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
//! In an `update` loop:
//! if let Some(m) = session.try_recv() {
//!     println!("{:.1}s  throughput={:.2}", m.elapsed_secs, m.throughput);
//! }
//!
//! session.stop();
//! ```

mod stressors;
pub mod scenario;
pub mod telemetry;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stressor {
    Cpu,
    Memory,
    Disk,
    /// Square `f32` matmul; reports Mflop/s.
    Matrix,
    /// Bulk memcpy of paired buffers; reports GB/s.
    Memcpy,
    /// Hot-loop bitops (popcount, ctlz, cttz, rotate); reports Mop/s.
    Bitops,
    /// Cache-line thrash with `_mm_prefetch`/`_mm_clflush` on x86_64; reports Mref/s.
    Cache,
    /// Page-touch + churn to pressure the working set / page file; reports MiB/s.
    Vm,
    /// STREAM-style memory bandwidth (copy/scale/add/triad); reports GB/s.
    Stream,
    /// Data-dependent branches to fuzz the branch predictor; reports Mbranch/s.
    Branch,
    /// Many cores fighting over one `AtomicU64`; reports Mop/s.
    Atomic,
    /// Many threads contending on a single mutex; reports Mop/s of lock-unlock pairs.
    Mutex,
    /// Paired threads ping-ponging on condvars to force OS context switches; reports Mctxsw/s.
    Switch,
    /// Sieve of Eratosthenes; reports Mprime/s (primes found per second).
    Prime,
    /// Independent FMA chains; reports Mflop/s.
    Fp,
    /// FNV-1a hashing over a 1 MiB buffer; reports MiB/s.
    Hash,
    /// Sequential + striped reads exercising the HW prefetcher; reports Mref/s.
    Prefetch,
    /// Indirect calls through a 64-fn table to spill the i-cache; reports Mcall/s.
    Icache,
    /// `rdtsc` read rate; reports Mread/s.
    Tsc,

    /// GPU compute-shader FMA + scattered-load hammer; reports GFLOPS.
    Gpu,
    /// GPU NxN fp32 matmul; reports GFLOPS.
    GpuMatmul,
    /// GPU VRAM write-verify pattern walker; reports MiB/s; mismatches surface via `last_error`.
    GpuVram,
    /// CPU↔GPU buffer round-trip; reports GB/s.
    GpuPcie,
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
            Self::Gpu => "GPU Compute",
            Self::GpuMatmul => "GPU Matmul",
            Self::GpuVram => "GPU VRAM",
            Self::GpuPcie => "GPU PCIe",
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
            Self::Gpu => "GFLOPS",
            Self::GpuMatmul => "GFLOPS",
            Self::GpuVram => "MiB/s",
            Self::GpuPcie => "GB/s",
        }
    }

    pub fn is_gpu(self) -> bool {
        matches!(self, Self::Gpu | Self::GpuMatmul | Self::GpuVram | Self::GpuPcie)
    }
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
