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
#[cfg(target_os = "windows")]
mod tdr_windows;
#[cfg(target_os = "windows")]
mod thermal_windows;
#[cfg(all(target_os = "windows", feature = "winring0-thermal"))]
mod cpu_thermal_windows;
#[cfg(all(target_os = "windows", feature = "winring0-thermal"))]
mod superio_windows;
#[cfg(target_os = "windows")]
mod storage_thermal_windows;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sysinfo::{Components, CpuRefreshKind, Disks, MemoryRefreshKind, Networks, RefreshKind, System};

pub use self::core::CoreSample;
pub use self::core::sample_cores;
pub use self::disk::DiskRateSample;
pub use self::gpu::GpuSample;
pub use self::memory::MemorySample;
pub use self::network::NetworkRateSample;
pub use self::processes::ProcessSample;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WheaCounters {
    pub delta_since_program_start: u64,
    /// Total WHEA error records retained in the log; spans reboots.
    #[serde(alias = "absolute_since_boot")]
    pub total_retained: u64,
    /// Corrected (Level 3) errors within `delta_since_program_start`.
    #[serde(default)]
    pub corrected_delta: u64,
    /// Fatal/uncorrected (Level 1/2) errors within `delta_since_program_start`.
    #[serde(default)]
    pub fatal_delta: u64,
}

#[cfg(target_os = "windows")]
pub use self::tdr_windows::TdrCounters;

#[cfg(not(target_os = "windows"))]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TdrCounters {
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
    #[serde(default)]
    pub processes: Vec<ProcessSample>,
    #[serde(default)]
    pub gpus: Vec<GpuSample>,
    pub whea: Option<WheaCounters>,
    /// Windows run where the WHEA event source couldn't be opened, so `whea`
    /// is absent for a reason (not a clean "no errors"). Always false
    /// off-Windows and on the one-shot capture path.
    #[serde(default)]
    pub whea_unavailable: bool,
    #[serde(default)]
    pub tdr: Option<TdrCounters>,
    /// ACPI thermal-zone readings on Windows (sysinfo's per-component
    /// temperature surface returns empty on modern Windows builds; we
    /// fall back to WMI's `MSAcpi_ThermalZoneTemperature`). Empty on
    /// non-Windows or when the WMI query isn't available.
    #[serde(default)]
    pub thermals: Vec<ThermalReading>,
    /// SuperIO board rails on Windows (`winring0-thermal`). Scaled with nominal
    /// dividers, so `calibrated` is false unless a per-board factor is known. A
    /// rail that drops below its plausible floor is published at its measured
    /// value, so a collapse reaches the verdict rules as a very low reading.
    #[serde(default)]
    pub voltages: Vec<VoltageReading>,
}

/// One ACPI thermal-zone reading. Mirrors the lightweight shape we
/// already use for `database::schema::component_temps` — `label` is
/// usually the zone identifier (e.g. `TZ00_0`, `CPUZ_0`), `temp_c` is
/// the live value in Celsius.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThermalReading {
    pub label: String,
    pub temp_c: f32,
}

/// One board voltage rail. `label` is the rail name (`+12V`, `+5V`,
/// `3VCC (chip)`, `Vcore`, `VBAT`); `calibrated` is false when a nominal
/// divider was assumed instead of a known per-board ratio.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoltageReading {
    pub label: String,
    pub volts: f32,
    pub calibrated: bool,
}

/// Which CPU thermal sensors answered this tick. AMD Zen publishes only the
/// package-level Tctl through the SMU — it has no per-core sensor — so
/// [`CpuTempCoverage::PackageOnly`] is the normal, correct state there and a
/// per-core list is legitimately empty.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CpuTempCoverage {
    /// No CPU thermal sensor answered.
    #[default]
    None,
    /// One overall CPU reading only (AMD Zen Tctl, or a lone package sensor).
    PackageOnly,
    /// Per-core readings answered (Intel DTS).
    PerCore,
}

impl TelemetrySnapshot {
    /// True when the sampler has produced at least one real sysinfo refresh.
    pub fn is_populated(&self) -> bool {
        self.captured_at_unix_ms > 0 && !self.cores.is_empty() && self.memory.total_mb > 0
    }

    /// Hottest CPU-side thermal reading this tick — package, per-core, or an
    /// ACPI CPU zone, whichever is hottest.
    pub fn cpu_package_temp_c(&self) -> Option<f32> {
        self.hottest_thermal(is_cpu_thermal_label).map(|r| r.temp_c)
    }

    /// The one overall CPU sensor: `CPU Package` (Intel) or `CPU (Tctl)` (AMD
    /// Zen), falling back to the hottest CPU zone; never a per-core reading.
    pub fn cpu_package_reading(&self) -> Option<&ThermalReading> {
        self.hottest_thermal(is_cpu_package_label).or_else(|| {
            self.hottest_thermal(|l| is_cpu_thermal_label(l) && !is_cpu_core_label(l))
        })
    }

    /// Per-core CPU readings (`CPU Core N`); empty on AMD Zen, which exposes no
    /// per-core sensor — never fill it by copying [`Self::cpu_package_reading`].
    pub fn cpu_core_readings(&self) -> Vec<&ThermalReading> {
        self.thermals
            .iter()
            .filter(|r| is_cpu_core_label(&r.label))
            .collect()
    }

    /// True when this platform reported at least one per-core CPU temperature.
    pub fn has_per_core_cpu_temps(&self) -> bool {
        self.thermals.iter().any(|r| is_cpu_core_label(&r.label))
    }

    /// One call telling a UI which CPU-temperature layout it can honestly render.
    pub fn cpu_temp_coverage(&self) -> CpuTempCoverage {
        if self.has_per_core_cpu_temps() {
            CpuTempCoverage::PerCore
        } else if self.cpu_package_reading().is_some() {
            CpuTempCoverage::PackageOnly
        } else {
            CpuTempCoverage::None
        }
    }

    /// Hottest reading whose label passes `pred`.
    fn hottest_thermal(&self, pred: impl Fn(&str) -> bool) -> Option<&ThermalReading> {
        self.thermals
            .iter()
            .filter(|r| pred(r.label.as_str()))
            .max_by(|a, b| a.temp_c.total_cmp(&b.temp_c))
    }

    /// Every board rail published this tick, each with its label, volts and
    /// `calibrated` flag; empty when no SuperIO sensor answered.
    pub fn rails(&self) -> &[VoltageReading] {
        &self.voltages
    }

    /// One rail by label, matched case-insensitively (`+12V`, `Vcore`, `VBAT`…).
    pub fn rail_reading(&self, label: &str) -> Option<&VoltageReading> {
        self.voltages
            .iter()
            .find(|v| v.label.eq_ignore_ascii_case(label))
    }

    /// True when any published rail was scaled with an assumed nominal divider.
    pub fn any_uncalibrated_rails(&self) -> bool {
        self.voltages.iter().any(|v| !v.calibrated)
    }

    pub fn rail_12v(&self) -> Option<f32> {
        self.rail("+12V")
    }

    pub fn rail_5v(&self) -> Option<f32> {
        self.rail("+5V")
    }

    /// Sensor chip's own 3.3V supply, not the board's +3.3V PSU rail.
    pub fn rail_3vcc(&self) -> Option<f32> {
        self.rail("3VCC (chip)")
    }

    pub fn vcore(&self) -> Option<f32> {
        self.rail("Vcore")
    }

    fn rail(&self, label: &str) -> Option<f32> {
        self.rail_reading(label).map(|v| v.volts)
    }
}

/// Any CPU-side thermal label: package, per-core, Tctl/Tdie, or an ACPI zone.
fn is_cpu_thermal_label(label: &str) -> bool {
    let l = label.to_lowercase();
    l.contains("package")
        || l.contains("cpu")
        || l.contains("tctl")
        || l.contains("tdie")
        || l.starts_with("tz")
}

/// A per-core label, as emitted by the Intel DTS reader.
fn is_cpu_core_label(label: &str) -> bool {
    label.to_lowercase().starts_with("cpu core")
}

/// An overall/package CPU label, including AMD's `CPU (Tctl)`.
fn is_cpu_package_label(label: &str) -> bool {
    let l = label.to_lowercase();
    !is_cpu_core_label(label)
        && (l.contains("package") || l.contains("tctl") || l.contains("tdie"))
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

    /// One-shot sysinfo refresh for callers that need CPU/GPU identity before the
    /// background sampler's first tick (e.g. stress-runner hardware upsert).
    pub fn capture_now() -> TelemetrySnapshot {
        capture_snapshot_blocking()
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

fn capture_snapshot_blocking() -> TelemetrySnapshot {
    let refresh_kind = RefreshKind::nothing()
        .with_cpu(CpuRefreshKind::everything())
        .with_memory(MemoryRefreshKind::everything());
    let mut sys = System::new_with_specifics(refresh_kind);
    let mut components = Components::new_with_refreshed_list();
    sys.refresh_cpu_all();
    sys.refresh_memory();
    components.refresh(true);

    let captured_at_unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    TelemetrySnapshot {
        captured_at_unix_ms,
        cores: core::sample_cores(&sys, &components),
        memory: memory::sample_memory(&sys),
        disks: Vec::new(),
        networks: Vec::new(),
        processes: Vec::new(),
        gpus: gpu::sample_gpus(&components),
        whea: None,
        whea_unavailable: false,
        tdr: None,
        // capture_now is the synchronous one-shot path; it skips
        // building a WMI connection (COM init isn't cheap) and lets
        // the long-running agent populate thermals on its sampler
        // thread instead.
        thermals: Vec::new(),
        voltages: Vec::new(),
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
    #[cfg(target_os = "windows")]
    let whea_unavailable = whea.is_none();
    #[cfg(not(target_os = "windows"))]
    let whea: Option<()> = None;

    #[cfg(target_os = "windows")]
    let mut tdr = tdr_windows::TdrMonitor::open();
    #[cfg(not(target_os = "windows"))]
    let tdr: Option<()> = None;

    #[cfg(target_os = "windows")]
    let mut thermal = thermal_windows::ThermalMonitor::open();
    #[cfg(not(target_os = "windows"))]
    let thermal: Option<()> = None;

    #[cfg(all(target_os = "windows", feature = "winring0-thermal"))]
    let mut cpu_thermal = cpu_thermal_windows::CpuThermalMonitor::open();
    // Shares the WinRing0 handle above, so it must not outlive `cpu_thermal`.
    #[cfg(all(target_os = "windows", feature = "winring0-thermal"))]
    let mut superio = cpu_thermal
        .as_ref()
        .and_then(|c| superio_windows::SuperIoMonitor::open(c.io_ports()));

    #[cfg(target_os = "windows")]
    let mut storage_thermal = storage_thermal_windows::StorageThermalMonitor::open();

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
            #[cfg(target_os = "windows")]
            whea_unavailable,
            #[cfg(not(target_os = "windows"))]
            whea_unavailable: false,
            #[cfg(target_os = "windows")]
            tdr: tdr.as_mut().map(|t| t.poll()),
            #[cfg(not(target_os = "windows"))]
            tdr: {
                let _ = tdr;
                None
            },
            #[cfg(target_os = "windows")]
            thermals: {
                let mut v = thermal.as_mut().map(|t| t.poll()).unwrap_or_default();
                #[cfg(feature = "winring0-thermal")]
                if let Some(c) = cpu_thermal.as_mut() {
                    v.extend(c.poll());
                }
                if let Some(s) = storage_thermal.as_mut() {
                    v.extend(s.poll());
                }
                v
            },
            #[cfg(not(target_os = "windows"))]
            thermals: {
                let _ = thermal;
                Vec::new()
            },
            #[cfg(all(target_os = "windows", feature = "winring0-thermal"))]
            voltages: superio.as_mut().map(|s| s.poll()).unwrap_or_default(),
            #[cfg(not(all(target_os = "windows", feature = "winring0-thermal")))]
            voltages: Vec::new(),
        };

        if let Ok(mut g) = snapshot.lock() {
            *g = snap;
        }

        thread::sleep(interval);
    }

    log::debug!("stress-kit/telemetry: thread exiting");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(labels: &[(&str, f32)]) -> TelemetrySnapshot {
        TelemetrySnapshot {
            thermals: labels
                .iter()
                .map(|&(label, temp_c)| ThermalReading { label: label.into(), temp_c })
                .collect(),
            ..Default::default()
        }
    }

    #[test]
    fn amd_tctl_is_package_only_with_no_cores() {
        let s = snap(&[("CPU (Tctl)", 61.5)]);
        assert_eq!(s.cpu_temp_coverage(), CpuTempCoverage::PackageOnly);
        assert!(!s.has_per_core_cpu_temps());
        assert!(s.cpu_core_readings().is_empty());
        assert_eq!(s.cpu_package_reading().map(|r| r.label.as_str()), Some("CPU (Tctl)"));
    }

    #[test]
    fn intel_package_and_cores_are_separated() {
        let s = snap(&[("CPU Package", 70.0), ("CPU Core 0", 68.0), ("CPU Core 1", 72.0)]);
        assert_eq!(s.cpu_temp_coverage(), CpuTempCoverage::PerCore);
        assert_eq!(s.cpu_core_readings().len(), 2);
        assert_eq!(s.cpu_package_reading().map(|r| r.label.as_str()), Some("CPU Package"));
        assert_eq!(s.cpu_package_temp_c(), Some(72.0));
    }

    #[test]
    fn acpi_zone_only_still_yields_a_package_reading() {
        let s = snap(&[("TZ00_0", 44.0), ("NVMe Disk 0", 38.0)]);
        assert_eq!(s.cpu_temp_coverage(), CpuTempCoverage::PackageOnly);
        assert_eq!(s.cpu_package_reading().map(|r| r.label.as_str()), Some("TZ00_0"));
    }

    #[test]
    fn no_cpu_sensor_reports_no_coverage() {
        let s = snap(&[("NVMe Disk 0", 38.0)]);
        assert_eq!(s.cpu_temp_coverage(), CpuTempCoverage::None);
        assert!(s.cpu_package_reading().is_none());
    }

    #[test]
    fn rails_expose_labels_and_calibration() {
        let s = TelemetrySnapshot {
            voltages: vec![VoltageReading { label: "+12V".into(), volts: 11.9, calibrated: false }],
            ..Default::default()
        };
        assert_eq!(s.rails().len(), 1);
        assert!(s.any_uncalibrated_rails());
        assert_eq!(s.rail_reading("+12v").map(|v| v.volts), Some(11.9));
        assert_eq!(s.rail_12v(), Some(11.9));
        assert!(!TelemetrySnapshot::default().any_uncalibrated_rails());
    }
}
