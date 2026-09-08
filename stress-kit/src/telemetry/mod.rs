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
pub mod live_dumps_windows;
#[cfg(target_os = "windows")]
mod thermal_windows;
pub mod cpu_ceiling;
#[cfg(feature = "lowlevel")]
mod cpu_die;
#[cfg(feature = "lowlevel")]
mod superio;
#[cfg(target_os = "windows")]
mod storage_thermal_windows;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sysinfo::{Components, CpuRefreshKind, Disks, MemoryRefreshKind, Networks, RefreshKind, System};

pub use crate::lowlevel::{AccessStatus, AccessTier, BackendId, RejectedBackend};

pub use self::cpu_ceiling::{CpuCeilingSource, CpuThermalCeiling, CpuVendor};

pub use self::core::CoreSample;
pub use self::core::{sample_cores, sample_cores_with_die};
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
    /// SuperIO board rails. Scaled with nominal dividers, so `calibrated` is
    /// false unless a per-board factor is known. A rail that drops below its
    /// plausible floor is published at its measured value, so a collapse reaches
    /// the verdict rules as a very low reading.
    #[serde(default)]
    pub voltages: Vec<VoltageReading>,
    /// The CPU's own sensor (Intel DTS MSRs, AMD Zen Tctl, or the Intel DPTF
    /// processor participant), kept out of `thermals` so no ACPI zone can stand
    /// in for it. `None` when no CPU-side sensor answered, including every run
    /// with no low-level backend. Its values are also copied into `thermals`
    /// under their labels for chart continuity.
    #[serde(default)]
    pub cpu_die: Option<CpuDieThermal>,
    /// Which low-level backend carried `cpu_die` and `voltages`, what it could
    /// reach, and why any other backend was skipped. Consult this before reading
    /// an absent sensor as a healthy one.
    #[serde(default)]
    pub access: AccessStatus,
    /// The CPU's own thermal ceiling. A part sits here under sustained load by
    /// design, so it is the temperature the firmware holds, not a fault line.
    /// `None` when the part is not one whose ceiling we can establish.
    #[serde(default)]
    pub cpu_ceiling: Option<CpuThermalCeiling>,
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

/// Rail labels the SuperIO reader publishes, in bank order; the reader's rail map
/// takes its labels from here so a caller grading per-rail availability cannot
/// drift from it.
pub const RAIL_LABELS: [&str; 5] = ["Vcore", "+5V", "3VCC (chip)", "+12V", "VBAT"];

/// Every rail label a caller can expect in [`TelemetrySnapshot::rails`]; a label
/// absent from `rails()` was not measured.
pub fn expected_rail_labels() -> &'static [&'static str] {
    &RAIL_LABELS
}

/// Register set a [`CpuDieThermal`] came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CpuDieReader {
    /// Intel digital-thermal-sensor MSRs (`IA32_(PACKAGE_)THERM_STATUS`).
    IntelDts,
    /// AMD Zen SMU Tctl over SMN.
    AmdTctl,
    /// Intel DPTF/ESIF processor participant over WMI.
    DptfParticipant,
}

impl CpuDieReader {
    /// Sensor class a caller grades this reader's values as.
    pub fn temp_source(self) -> CpuTempSource {
        match self {
            Self::IntelDts | Self::AmdTctl => CpuTempSource::Die,
            Self::DptfParticipant => CpuTempSource::DptfParticipant,
        }
    }
}

/// Readings from the CPU's own sensor. A value here came from the processor
/// itself; an ACPI board thermal zone can never appear in this struct. `reader`
/// says how it was read — grade it with [`CpuDieReader::temp_source`] rather
/// than assuming every value is a die register.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CpuDieThermal {
    /// Package-level die reading; `None` when the part exposes no package sensor.
    pub package_c: Option<f32>,
    /// Per-logical-core die readings indexed by core, `None` where that core has
    /// no sensor of its own. Empty on AMD Zen, which has no per-core sensor.
    pub cores: Vec<Option<f32>>,
    pub reader: CpuDieReader,
}

impl CpuDieThermal {
    /// Hottest die value across the package and per-core readings.
    pub fn hottest_c(&self) -> Option<f32> {
        self.cores
            .iter()
            .flatten()
            .copied()
            .chain(self.package_c)
            .fold(None::<f32>, |acc, t| Some(acc.map_or(t, |m: f32| m.max(t))))
    }

    /// One core's die reading; `None` when that core has no sensor.
    pub fn core_c(&self, core: usize) -> Option<f32> {
        self.cores.get(core).copied().flatten()
    }

    /// Number of cores that reported their own die temperature.
    pub fn core_temp_count(&self) -> usize {
        self.cores.iter().flatten().count()
    }

    /// Hottest die reading with the label it publishes under.
    pub fn hottest_reading(&self) -> Option<ThermalReading> {
        self.to_thermal_readings()
            .into_iter()
            .max_by(|a, b| a.temp_c.total_cmp(&b.temp_c))
    }

    /// These values as labelled thermal readings, for the `thermals` list.
    pub fn to_thermal_readings(&self) -> Vec<ThermalReading> {
        let mut out = Vec::with_capacity(self.cores.len() + 1);
        if let Some(temp_c) = self.package_c {
            out.push(ThermalReading { label: self.package_label().into(), temp_c });
        }
        for (core, temp_c) in self
            .cores
            .iter()
            .enumerate()
            .filter_map(|(i, t)| t.map(|t| (i, t)))
        {
            out.push(ThermalReading { label: format!("CPU Core {core}"), temp_c });
        }
        out
    }

    /// Label the package value publishes under.
    fn package_label(&self) -> &'static str {
        match self.reader {
            CpuDieReader::IntelDts => "CPU Package",
            CpuDieReader::AmdTctl => "CPU (Tctl)",
            CpuDieReader::DptfParticipant => "CPU Package (DPTF)",
        }
    }
}

/// Which class of sensor produced a snapshot's CPU temperature.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CpuTempSource {
    /// No CPU-side sensor answered; the CPU temperature is unknown.
    #[default]
    None,
    /// Only a firmware-named CPU thermal zone answered — a board-level reading
    /// that tracks the die loosely and can sit tens of degrees below it.
    AcpiZone,
    /// The Intel DPTF processor participant answered: a CPU-side sensor, whole
    /// degrees, and no per-core detail. Not a board zone.
    DptfParticipant,
    /// The CPU's own die sensor answered.
    Die,
}

/// Whether the WHEA machine-check log was readable, so "no errors" stays
/// distinguishable from "not checked".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WheaStatus {
    /// Event source opened and answered; the counters are real, zero included.
    Read,
    /// Event source could not be opened; the machine's error state is unknown.
    Unavailable,
    /// This capture path never sampled WHEA (one-shot capture, non-Windows).
    #[default]
    NotSampled,
}

impl WheaStatus {
    /// True when nothing was checked, so absence of errors was never established.
    pub fn is_evidence_missing(self) -> bool {
        !matches!(self, Self::Read)
    }
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

    /// The one CPU temperature this snapshot can report, with the class of sensor
    /// it came from: the hottest CPU-side reading when one answered, else the
    /// hottest firmware-named CPU zone, else nothing. A CPU-side reading outranks
    /// every zone however the two values compare, and a bare `TZnn` board zone is
    /// never a candidate. Single source of truth for "the CPU temperature".
    pub fn cpu_temp_reading(&self) -> Option<(ThermalReading, CpuTempSource)> {
        let die_source = self
            .cpu_die
            .as_ref()
            .map_or(CpuTempSource::Die, |d| d.reader.temp_source());
        self.cpu_die
            .as_ref()
            .and_then(CpuDieThermal::hottest_reading)
            .or_else(|| self.hottest_thermal(is_cpu_die_label).cloned())
            .map(|r| (r, die_source))
            .or_else(|| {
                self.hottest_thermal(is_cpu_acpi_zone_label)
                    .cloned()
                    .map(|r| (r, CpuTempSource::AcpiZone))
            })
    }

    /// Value of [`Self::cpu_temp_reading`]; pair it with [`Self::cpu_temp_source`]
    /// to know which class of sensor answered.
    pub fn cpu_package_temp_c(&self) -> Option<f32> {
        self.cpu_temp_reading().map(|(r, _)| r.temp_c)
    }

    /// Hottest reading from the CPU's own sensor (Intel DTS, AMD Zen Tctl, or the
    /// DPTF processor participant); `None` when none answered, whatever ACPI
    /// zones exist.
    pub fn cpu_die_temp_c(&self) -> Option<f32> {
        self.cpu_die.as_ref().and_then(CpuDieThermal::hottest_c)
    }

    /// The die-sensor block: package value, per-core values, and the reader.
    pub fn cpu_die(&self) -> Option<&CpuDieThermal> {
        self.cpu_die.as_ref()
    }

    /// Hottest firmware-named CPU thermal zone (`CPUZ_0`, `TCPU`…); excludes every
    /// die-sensor label, so it never returns a DTS or Tctl reading.
    pub fn cpu_acpi_zone_temp_c(&self) -> Option<f32> {
        self.hottest_thermal(is_cpu_acpi_zone_label).map(|r| r.temp_c)
    }

    /// Sensor class behind [`Self::cpu_package_temp_c`]: `Die` when a die sensor
    /// answered, `AcpiZone` when only a firmware CPU zone did, else `None`.
    pub fn cpu_temp_source(&self) -> CpuTempSource {
        self.cpu_temp_reading()
            .map_or(CpuTempSource::None, |(_, source)| source)
    }

    /// Whether the WHEA log was readable this tick.
    pub fn whea_status(&self) -> WheaStatus {
        if self.whea_unavailable {
            WheaStatus::Unavailable
        } else if self.whea.is_some() {
            WheaStatus::Read
        } else {
            WheaStatus::NotSampled
        }
    }

    /// Counters only when the log was actually read; `None` for every other status.
    pub fn whea_counters(&self) -> Option<&WheaCounters> {
        matches!(self.whea_status(), WheaStatus::Read)
            .then(|| self.whea.as_ref())
            .flatten()
    }

    /// True only when the log was read and every counter since program start is
    /// zero — an unreadable or unsampled source returns false.
    pub fn whea_errors_confirmed_absent(&self) -> bool {
        self.whea_counters().is_some_and(|w| {
            w.delta_since_program_start == 0 && w.corrected_delta == 0 && w.fatal_delta == 0
        })
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
        self.cpu_die
            .as_ref()
            .is_some_and(|d| d.core_temp_count() > 0)
            || self.thermals.iter().any(|r| is_cpu_core_label(&r.label))
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

/// Any CPU-side thermal label: die package, die per-core, Tctl/Tdie, or a zone the
/// firmware names for the CPU. A bare `TZnn` zone is a board zone and is excluded.
pub fn is_cpu_thermal_label(label: &str) -> bool {
    let l = label.to_lowercase();
    l.contains("package") || l.contains("cpu") || l.contains("tctl") || l.contains("tdie")
}

/// A label only a CPU-side reader emits: `CPU Package`, `CPU Package (DPTF)`,
/// `CPU (Tctl)`, `CPU Core N`.
pub fn is_cpu_die_label(label: &str) -> bool {
    let l = label.to_lowercase();
    is_cpu_core_label(label) || l.contains("package") || l.contains("tctl") || l.contains("tdie")
}

/// A firmware-named CPU thermal zone (`CPUZ_0`, `TCPU`); never a die label and
/// never a bare `TZnn` board zone.
pub fn is_cpu_acpi_zone_label(label: &str) -> bool {
    is_cpu_thermal_label(label) && !is_cpu_die_label(label)
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
        cpu_die: None,
        // No backend is opened on this path, so the tier is None rather than a
        // claim that one was tried and failed.
        access: AccessStatus::default(),
        cpu_ceiling: cpu_ceiling::detect(None),
    }
}

/// First delay before re-opening a low-level backend that cannot read, and the
/// cap the delay doubles up to. A machine that can never load a driver must not
/// be asked once a second for the length of an eight-hour run.
#[cfg(feature = "lowlevel")]
const REACQUIRE_FIRST: Duration = Duration::from_secs(30);
#[cfg(feature = "lowlevel")]
const REACQUIRE_MAX: Duration = Duration::from_secs(900);
/// Attempts that open no provider at all before a sampler stops asking. Spans
/// roughly the first seven minutes of a run, which covers another tool holding
/// the driver across a restart; past that the machine has no loadable backend,
/// and each attempt stages a driver file and asks the service manager to start
/// it.
#[cfg(feature = "lowlevel")]
const MAX_EMPTY_ATTEMPTS: u32 = 5;

/// The low-level readers plus the handle they share, re-opened when that handle
/// stops being able to serve them.
///
/// Both the handle and the readers were previously acquired once per sampler
/// and never retried, so a sampler that started while another tool held the
/// driver — or whose provider was pulled out from under it — published no CPU
/// die temperature and no board rails for the rest of its life. Nothing
/// recovered it, because the process-wide handle cache hands a *new* caller a
/// fresh backend while leaving the running sampler on the dead one.
#[cfg(feature = "lowlevel")]
struct LowLevelReaders {
    access: crate::lowlevel::LowLevelAccess,
    cpu_die: Option<cpu_die::CpuDieMonitor>,
    superio: Option<superio::SuperIoMonitor>,
    backoff: Duration,
    next_attempt: Instant,
    /// A provider answered at least once, so this machine can load one and a
    /// dead handle stays worth retrying for the life of the sampler.
    provider_seen: bool,
    /// Consecutive attempts that opened no provider at all.
    empty_attempts: u32,
    /// This CPU has a die-sensor path on some backend. False on a vendor no
    /// reader covers, where a fresh handle would read the same nothing.
    die_vendor_supported: bool,
}

#[cfg(feature = "lowlevel")]
impl LowLevelReaders {
    fn open() -> Self {
        // Both readers hold their own clone of the backend handle, so neither
        // depends on the other opening and drop order between them does not
        // matter.
        let access = crate::lowlevel::select::open();
        let cpu_die = cpu_die::CpuDieMonitor::open(access.clone());
        let superio = superio::SuperIoMonitor::open(access.clone());
        let empty = access.is_absent();
        Self {
            access,
            cpu_die,
            superio,
            backoff: REACQUIRE_FIRST,
            next_attempt: Instant::now() + REACQUIRE_FIRST,
            provider_seen: !empty,
            empty_attempts: u32::from(empty),
            die_vendor_supported: cpu_ceiling::vendor() != CpuVendor::Other,
        }
    }

    /// A reader the live backend could serve is missing, so a fresh handle
    /// would change the answer. A reader this backend cannot reach at all is
    /// not a gap: retrying the same provider would read the same nothing.
    fn has_gap(&self) -> bool {
        self.access.is_dead()
            || (self.cpu_die.is_none()
                && self.die_vendor_supported
                && self.access.capabilities().any_die())
    }

    /// Whether another attempt can change the answer. A handle that once held a
    /// provider always can — the driver demonstrably loads on this machine. A
    /// run of attempts that opened nothing at all stops after
    /// [`MAX_EMPTY_ATTEMPTS`]: driver policy does not change mid-run.
    fn worth_retrying(&self) -> bool {
        self.has_gap() && (self.provider_seen || self.empty_attempts < MAX_EMPTY_ATTEMPTS)
    }

    /// Re-opens the backend and both readers once the backoff has elapsed.
    /// A no-op while the handle is healthy, and cheap when it is not: the
    /// process-wide cache hands back the same provider unless it is dead.
    fn reacquire_if_needed(&mut self) {
        if !self.worth_retrying() || Instant::now() < self.next_attempt {
            return;
        }
        self.access = crate::lowlevel::select::open();
        if self.access.is_absent() {
            self.empty_attempts = self.empty_attempts.saturating_add(1);
        } else {
            self.provider_seen = true;
            self.empty_attempts = 0;
        }
        self.cpu_die = cpu_die::CpuDieMonitor::open(self.access.clone());
        self.superio = superio::SuperIoMonitor::open(self.access.clone());
        if self.has_gap() {
            self.backoff = (self.backoff * 2).min(REACQUIRE_MAX);
            if self.worth_retrying() {
                log::debug!(
                    "stress-kit/telemetry: low-level backend still unreadable ({}); retrying in \
                     {:?}",
                    self.access.status().detail,
                    self.backoff
                );
            } else {
                log::warn!(
                    "stress-kit/telemetry: no low-level backend opened in {MAX_EMPTY_ATTEMPTS} \
                     attempts ({}); CPU die temperature and board rails stay unavailable \
                     for this sampler",
                    self.access.status().detail
                );
            }
        } else {
            self.backoff = REACQUIRE_FIRST;
            log::info!(
                "stress-kit/telemetry: low-level backend re-acquired ({}); CPU die temperature \
                 and board rails resume",
                self.access.id().label()
            );
        }
        self.next_attempt = Instant::now() + self.backoff;
    }

    fn poll_die(&mut self) -> Option<CpuDieThermal> {
        self.cpu_die.as_mut().and_then(|c| c.poll())
    }

    /// Per-domain readings behind the last die poll; empty for every reader
    /// whose sensor has no domains.
    fn die_domains(&self) -> Vec<ThermalReading> {
        self.cpu_die
            .as_ref()
            .map(cpu_die::CpuDieMonitor::domain_thermals)
            .unwrap_or_default()
    }

    fn poll_rails(&mut self) -> Vec<VoltageReading> {
        self.superio.as_mut().map(|s| s.poll()).unwrap_or_default()
    }

    fn ceiling(&self) -> Option<CpuThermalCeiling> {
        self.cpu_die
            .as_ref()
            .and_then(cpu_die::CpuDieMonitor::ceiling)
            .or_else(|| cpu_ceiling::detect(None))
    }

    fn status(&self) -> AccessStatus {
        self.access.status()
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

    #[cfg(feature = "lowlevel")]
    let mut readers = LowLevelReaders::open();

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

        #[cfg(feature = "lowlevel")]
        let (die, die_domains) = {
            readers.reacquire_if_needed();
            let die = readers.poll_die();
            (die, readers.die_domains())
        };
        #[cfg(not(feature = "lowlevel"))]
        let (die, die_domains): (Option<CpuDieThermal>, Vec<ThermalReading>) = (None, Vec::new());

        #[cfg(target_os = "windows")]
        let thermals = {
            let mut v = thermal.as_mut().map(|t| t.poll()).unwrap_or_default();
            if let Some(d) = die.as_ref() {
                v.extend(d.to_thermal_readings());
            }
            v.extend(die_domains);
            if let Some(s) = storage_thermal.as_mut() {
                v.extend(s.poll());
            }
            v
        };
        #[cfg(not(target_os = "windows"))]
        let thermals = {
            let _ = (thermal, die_domains);
            Vec::new()
        };

        let snap = TelemetrySnapshot {
            captured_at_unix_ms,
            cores: core::sample_cores_with_die(&sys, &components, die.as_ref()),
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
            thermals,
            #[cfg(feature = "lowlevel")]
            voltages: readers.poll_rails(),
            #[cfg(not(feature = "lowlevel"))]
            voltages: Vec::new(),
            cpu_die: die,
            #[cfg(feature = "lowlevel")]
            access: readers.status(),
            #[cfg(not(feature = "lowlevel"))]
            access: AccessStatus::default(),
            #[cfg(feature = "lowlevel")]
            cpu_ceiling: readers.ceiling(),
            #[cfg(not(feature = "lowlevel"))]
            cpu_ceiling: None,
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
    fn a_bare_board_zone_is_not_a_cpu_reading() {
        let s = snap(&[("TZ00_0", 40.0), ("NVMe Disk 0", 38.0)]);
        assert_eq!(s.cpu_package_temp_c(), None);
        assert!(s.cpu_package_reading().is_none());
        assert_eq!(s.cpu_temp_source(), CpuTempSource::None);
        assert_eq!(s.cpu_temp_coverage(), CpuTempCoverage::None);
    }

    #[test]
    fn a_firmware_named_cpu_zone_reads_as_a_zone_not_a_die() {
        let s = snap(&[("CPUZ_0", 44.0), ("TZ00_0", 40.0)]);
        assert_eq!(s.cpu_temp_source(), CpuTempSource::AcpiZone);
        assert_eq!(s.cpu_package_temp_c(), Some(44.0));
        assert_eq!(s.cpu_acpi_zone_temp_c(), Some(44.0));
        assert_eq!(s.cpu_die_temp_c(), None);
        assert_eq!(s.cpu_package_reading().map(|r| r.label.as_str()), Some("CPUZ_0"));
    }

    #[test]
    fn no_cpu_sensor_reports_no_coverage() {
        let s = snap(&[("NVMe Disk 0", 38.0)]);
        assert_eq!(s.cpu_temp_coverage(), CpuTempCoverage::None);
        assert!(s.cpu_package_reading().is_none());
        assert_eq!(s.cpu_temp_source(), CpuTempSource::None);
        assert_eq!(s.cpu_die_temp_c(), None);
    }

    #[test]
    fn a_die_block_outranks_a_zone_and_publishes_its_labels() {
        let die = CpuDieThermal {
            package_c: Some(70.0),
            cores: vec![Some(68.0), None, Some(74.0)],
            reader: CpuDieReader::IntelDts,
        };
        assert_eq!(die.hottest_c(), Some(74.0));
        assert_eq!(die.core_c(1), None);
        assert_eq!(die.core_temp_count(), 2);
        let labels: Vec<String> = die.to_thermal_readings().into_iter().map(|r| r.label).collect();
        assert_eq!(labels, ["CPU Package", "CPU Core 0", "CPU Core 2"]);

        let mut s = snap(&[("CPUZ_0", 44.0)]);
        s.thermals.extend(die.to_thermal_readings());
        s.cpu_die = Some(die);
        assert_eq!(s.cpu_temp_source(), CpuTempSource::Die);
        assert_eq!(s.cpu_die_temp_c(), Some(74.0));
        assert_eq!(s.cpu_acpi_zone_temp_c(), Some(44.0));
        assert_eq!(s.cpu_temp_coverage(), CpuTempCoverage::PerCore);
    }

    #[test]
    fn an_amd_die_block_is_package_only() {
        let die = CpuDieThermal {
            package_c: Some(61.5),
            cores: Vec::new(),
            reader: CpuDieReader::AmdTctl,
        };
        assert_eq!(die.core_temp_count(), 0);
        assert_eq!(
            die.to_thermal_readings().first().map(|r| r.label.clone()),
            Some("CPU (Tctl)".to_string())
        );
        let s = TelemetrySnapshot {
            thermals: die.to_thermal_readings(),
            cpu_die: Some(die),
            ..Default::default()
        };
        assert_eq!(s.cpu_temp_coverage(), CpuTempCoverage::PackageOnly);
        assert_eq!(s.cpu_die_temp_c(), Some(61.5));
        assert!(!s.has_per_core_cpu_temps());
    }

    #[test]
    fn a_die_reading_outranks_a_hotter_zone() {
        let die = CpuDieThermal {
            package_c: Some(61.5),
            cores: Vec::new(),
            reader: CpuDieReader::AmdTctl,
        };
        let mut s = snap(&[("CPUZ_0", 90.0)]);
        s.thermals.extend(die.to_thermal_readings());
        s.cpu_die = Some(die);
        let (reading, source) = s.cpu_temp_reading().expect("die reading");
        assert_eq!(reading.label, "CPU (Tctl)");
        assert_eq!(source, CpuTempSource::Die);
        assert_eq!(s.cpu_package_temp_c(), Some(61.5));
        assert_eq!(s.cpu_acpi_zone_temp_c(), Some(90.0));
    }

    /// One definition: the value, its label and its source class always come from
    /// the same pick, whichever sensor class answered.
    #[test]
    fn the_cpu_temperature_and_its_source_always_agree() {
        for labels in [
            &[("CPU Core 0", 68.0), ("CPUZ_0", 91.0)][..],
            &[("CPUZ_0", 44.0), ("TZ00_0", 40.0)][..],
            &[("TZ00_0", 40.0)][..],
            &[][..],
        ] {
            let s = snap(labels);
            let pick = s.cpu_temp_reading();
            assert_eq!(s.cpu_package_temp_c(), pick.as_ref().map(|(r, _)| r.temp_c));
            assert_eq!(
                s.cpu_temp_source(),
                pick.as_ref()
                    .map_or(CpuTempSource::None, |(_, source)| *source)
            );
            if let Some((reading, CpuTempSource::AcpiZone)) = pick.as_ref() {
                assert!(is_cpu_acpi_zone_label(&reading.label));
            }
        }
    }

    #[test]
    fn the_expected_rail_labels_are_the_readers_rail_map() {
        assert_eq!(
            expected_rail_labels(),
            ["Vcore", "+5V", "3VCC (chip)", "+12V", "VBAT"]
        );
        let s = TelemetrySnapshot {
            voltages: vec![VoltageReading { label: "+12V".into(), volts: 11.9, calibrated: false }],
            ..Default::default()
        };
        let missing: Vec<&str> = expected_rail_labels()
            .iter()
            .copied()
            .filter(|l| s.rail_reading(l).is_none())
            .collect();
        assert_eq!(missing, ["Vcore", "+5V", "3VCC (chip)", "VBAT"]);
    }

    #[test]
    fn an_unreadable_whea_source_is_not_a_clean_bill() {
        let read_clean = TelemetrySnapshot {
            whea: Some(WheaCounters::default()),
            ..Default::default()
        };
        assert_eq!(read_clean.whea_status(), WheaStatus::Read);
        assert!(read_clean.whea_errors_confirmed_absent());
        assert!(read_clean.whea_counters().is_some());

        let unavailable = TelemetrySnapshot {
            whea_unavailable: true,
            ..Default::default()
        };
        assert_eq!(unavailable.whea_status(), WheaStatus::Unavailable);
        assert!(!unavailable.whea_errors_confirmed_absent());
        assert!(unavailable.whea_counters().is_none());
        assert!(unavailable.whea_status().is_evidence_missing());

        let one_shot = TelemetrySnapshot::default();
        assert_eq!(one_shot.whea_status(), WheaStatus::NotSampled);
        assert!(!one_shot.whea_errors_confirmed_absent());
        assert!(one_shot.whea_status().is_evidence_missing());
    }

    #[test]
    fn a_read_source_with_hits_is_not_confirmed_absent() {
        let s = TelemetrySnapshot {
            whea: Some(WheaCounters {
                delta_since_program_start: 1,
                total_retained: 4,
                corrected_delta: 1,
                fatal_delta: 0,
            }),
            ..Default::default()
        };
        assert_eq!(s.whea_status(), WheaStatus::Read);
        assert!(!s.whea_errors_confirmed_absent());
        assert!(!s.whea_status().is_evidence_missing());
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
