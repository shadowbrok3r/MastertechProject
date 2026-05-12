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

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Stressor {
    Cpu,
    Memory,
    Disk,
}

impl Stressor {
    pub fn label(self) -> &'static str {
        match self {
            Self::Cpu => "CPU",
            Self::Memory => "Memory",
            Self::Disk => "Disk I/O",
        }
    }

    pub fn throughput_unit(self) -> &'static str {
        match self {
            Self::Cpu => "Mop/s",
            Self::Memory => "MiB/s",
            Self::Disk => "MiB/s",
        }
    }
}

/// Single-run settings. In [`scenario::ScenarioStage`], `timeout` is ignored; stage length is `duration_secs`.
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone)]
pub struct Metrics {
    pub elapsed_secs: f64,
    pub throughput: f64,
    pub last_error: Option<String>,
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
