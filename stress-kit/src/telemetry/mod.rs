//! Hardware telemetry agent. Background thread refreshes a `sysinfo::System`
//! at a fixed cadence and publishes a serializable [`TelemetrySnapshot`] that
//! both `qc-app` and `Mastertech4.0` can consume.
//!
//! ```rust,no_run
//! use stress_kit::telemetry::TelemetryAgent;
//! let agent = TelemetryAgent::start(1000);
//! let snap = agent.snapshot();
//! println!("{} cores, {} MB RAM used", snap.cores.len(), snap.memory.used_mb);
//! ```

mod core;
mod disk;
mod gpu;
mod memory;
mod network;
mod processes;
#[cfg(target_os = "windows")]
mod whea_windows;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sysinfo::{Components, CpuRefreshKind, Disks, MemoryRefreshKind, Networks, RefreshKind, System};

pub use self::core::CoreSample;
pub use self::disk::DiskRateSample;
pub use self::gpu::GpuSample;
pub use self::memory::MemorySample;
pub use self::network::NetworkRateSample;
pub use self::processes::ProcessSample;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WheaCounters {
    pub delta_since_program_start: u64,
    pub absolute_since_boot: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TelemetrySnapshot {
    pub captured_at_unix_ms: u64,
    pub cores: Vec<CoreSample>,
    pub memory: MemorySample,
    pub disks: Vec<DiskRateSample>,
    pub networks: Vec<NetworkRateSample>,
    /// Top-N processes by CPU then RAM. Empty until the first refresh tick.
    #[serde(default)]
    pub processes: Vec<ProcessSample>,
    /// GPU components surfaced by sysinfo. Empty if no GPU sensors are visible.
    #[serde(default)]
    pub gpus: Vec<GpuSample>,
    /// `None` on non-Windows targets, or when the WHEA log isn't readable.
    pub whea: Option<WheaCounters>,
}

pub struct TelemetryAgent {
    snapshot: Arc<Mutex<TelemetrySnapshot>>,
    cancel: Arc<AtomicBool>,
}

impl TelemetryAgent {
    /// `refresh_ms` is clamped to ≥100 ms.
    pub fn start(refresh_ms: u64) -> Self {
        let snapshot = Arc::new(Mutex::new(TelemetrySnapshot::default()));
        let cancel = Arc::new(AtomicBool::new(false));

        let snap_clone = snapshot.clone();
        let cancel_clone = cancel.clone();
        let interval = Duration::from_millis(refresh_ms.max(100));

        thread::Builder::new()
            .name("stress-kit-telemetry".into())
            .spawn(move || sampler_loop(snap_clone, cancel_clone, interval))
            .expect("stress-kit: failed to spawn telemetry thread");

        Self { snapshot, cancel }
    }

    /// Cheap clone of the latest sample; default-initialized until the first tick lands.
    pub fn snapshot(&self) -> TelemetrySnapshot {
        self.snapshot
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    pub fn stop(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}

impl Drop for TelemetryAgent {
    fn drop(&mut self) {
        self.stop();
    }
}

fn sampler_loop(
    snapshot: Arc<Mutex<TelemetrySnapshot>>,
    cancel: Arc<AtomicBool>,
    interval: Duration,
) {
    let refresh_kind = RefreshKind::nothing()
        .with_cpu(CpuRefreshKind::everything())
        .with_memory(MemoryRefreshKind::everything())
        .with_processes(sysinfo::ProcessRefreshKind::nothing().with_memory());
    let mut sys = System::new_with_specifics(refresh_kind);
    let mut disks = Disks::new_with_refreshed_list();
    let mut networks = Networks::new_with_refreshed_list();
    let mut components = Components::new_with_refreshed_list();

    #[cfg(target_os = "windows")]
    let mut whea = whea_windows::WheaMonitor::open();
    #[cfg(not(target_os = "windows"))]
    let whea: Option<()> = None;

    // First refresh seeds counters; the next tick yields usable rates.
    sys.refresh_cpu_all();
    thread::sleep(interval);

    let mut last_tick = Instant::now();

    while !cancel.load(Ordering::Relaxed) {
        let elapsed = last_tick.elapsed().as_secs_f32();
        last_tick = Instant::now();

        sys.refresh_cpu_all();
        sys.refresh_memory();
        sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        disks.refresh(true);
        networks.refresh(true);
        components.refresh(true);

        let captured_at_unix_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        let snap = TelemetrySnapshot {
            captured_at_unix_ms,
            cores: core::sample_cores(&sys, &components),
            memory: memory::sample_memory(&sys),
            disks: disk::sample_disks(&disks, elapsed),
            networks: network::sample_networks(&networks, elapsed),
            processes: processes::sample_processes(&sys),
            gpus: gpu::sample_gpus(&components),
            #[cfg(target_os = "windows")]
            whea: whea.as_mut().map(|w| w.poll()),
            #[cfg(not(target_os = "windows"))]
            whea: {
                let _ = whea;
                None
            },
        };

        if let Ok(mut g) = snapshot.lock() {
            *g = snap;
        }

        thread::sleep(interval);
    }

    log::debug!("stress-kit/telemetry: thread exiting");
}
