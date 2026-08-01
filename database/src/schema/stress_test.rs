//! Schema for stress-test telemetry. Companion to migration
//! `database/migrations/002_stress_test.surql`.
//!
//! Five tables work together:
//! - [`HardwareComponent`] is the normalized catalog (one row per
//!   canonical CPU/GPU/RAM/SSD/PSU/mobo/cooler model). Lets cross-machine
//!   queries collapse "RTX 4070 SUPER" no matter how the host reports it.
//! - [`StressTestRun`] is one execution of one tool against one
//!   computer, with summary stats + verdict.
//! - [`StressTestMetric`] is a 1 Hz telemetry sample tied to a run.
//! - [`StressTestEvent`] is a discrete event (stage transitions, WHEA
//!   hits, BSODs, throttle crossings).
//! - `hardware_test_baseline` is a SurrealDB materialized view
//!   aggregating runs per `(target_component, tool_label)` — read-only
//!   from Rust via [`HardwareTestBaseline`].
//!
//! Designed to back AI analysis: given a single computer's symptom,
//! pull its run history, then compare against the per-component
//! population baselines to decide if the observed temps / errors /
//! throughput are normal or anomalous.

use crate::db;
use super::stress_test_sql;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    random_record_id, utilities::record_exists, Datetime, RecordId, SurrealValue,
    HARDWARE_COMPONENT_TABLE, STRESS_TEST_EVENT_TABLE, STRESS_TEST_METRIC_TABLE,
    STRESS_TEST_RUN_TABLE,
};

/// SurrealDB CREATE content. Strips empty `embedding` arrays (HNSW rejects len 0).
/// Record ids stay in SurrealValue form — same pattern as `DiagnosticSession::create`.
fn surreal_create_content<T: Clone + SurrealValue>(
    record: &T,
    strip_empty_embedding: bool,
) -> surrealdb::types::Value {
    let mut value = record.clone().into_value();
    if strip_empty_embedding {
        if let surrealdb::types::Value::Object(obj) = &mut value {
            if matches!(
                obj.get("embedding"),
                Some(surrealdb::types::Value::Array(a)) if a.is_empty()
            ) {
                obj.remove("embedding");
            }
        }
    }
    value
}

// ============================================================
// Hardware catalog
// ============================================================

/// SurrealValue ignores `#[serde(rename_all)]`, so unit enums get an
/// explicit `#[surreal(value = "...")]` per variant to lock in the
/// snake_case string that the DB stores and that `WHERE kind = '...'`
/// queries match against. The Rust `as_str()` helper returns the same
/// string for code paths (like ID hashing) that need it.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, SurrealValue)]
#[serde(rename_all = "snake_case")]
#[surreal(untagged)]
pub enum HardwareKind {
    #[surreal(value = "cpu")]
    Cpu,
    #[surreal(value = "gpu")]
    Gpu,
    /// A populated RAM slot's module (one record per stick).
    #[surreal(value = "ram_module")]
    RamModule,
    /// A complete labeled RAM kit (e.g. "G.Skill Trident Z5 2x32GB DDR5-6400").
    /// Lets us record kit-level test results without re-keying per stick.
    #[surreal(value = "ram_kit")]
    RamKit,
    #[surreal(value = "ssd")]
    Ssd,
    #[surreal(value = "hdd")]
    Hdd,
    #[surreal(value = "motherboard")]
    Motherboard,
    #[surreal(value = "psu")]
    Psu,
    #[surreal(value = "cooler")]
    Cooler,
}

impl HardwareKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Gpu => "gpu",
            Self::RamModule => "ram_module",
            Self::RamKit => "ram_kit",
            Self::Ssd => "ssd",
            Self::Hdd => "hdd",
            Self::Motherboard => "motherboard",
            Self::Psu => "psu",
            Self::Cooler => "cooler",
        }
    }
}

/// One row per canonical hardware part. Strings are normalized
/// (lowercase, trimmed, single-spaced) before hashing into the ID.
/// `specs` is intentionally a free-form JSON object — fields differ
/// wildly across kinds (cores/threads for CPUs, vram_gb for GPUs,
/// timings for RAM, tbw for SSDs, wattage/cert for PSUs).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, SurrealValue)]
pub struct HardwareComponent {
    pub id: RecordId,
    pub kind: HardwareKind,
    pub vendor: String,
    pub model: String,
    /// Vendor SKU / part number if known (e.g. "100-100000910WOF" for a
    /// Ryzen 7 9700X). Optional; many lookups will only have vendor+model.
    pub sku: Option<String>,
    pub display_name: String,
    pub specs: Option<serde_json::Value>,
    pub first_seen: Datetime,
    pub last_seen: Datetime,
    /// How many `computer` records reference this component. Bumped by
    /// the normalizer when it links a new machine.
    pub occurrence_count: u64,
    /// 768-dim embedding computed by `fn::embed_text(kind + vendor + model + display_name)`.
    /// `VALUE` in the DB schema means SurrealDB always overwrites this on insert/update.
    #[serde(default)]
    pub embedding: Vec<f32>,
}

impl HardwareComponent {
    /// Stable canonical ID so identical parts collapse to one row
    /// regardless of where they were discovered. The hash inputs are
    /// trimmed + lowercased so trivial casing differences don't fork.
    pub fn canonical_id(kind: HardwareKind, vendor: &str, model: &str) -> RecordId {
        let mut hasher = Sha256::new();
        hasher.update(kind.as_str().as_bytes());
        hasher.update(b"|");
        hasher.update(vendor.trim().to_ascii_lowercase().as_bytes());
        hasher.update(b"|");
        hasher.update(model.trim().to_ascii_lowercase().as_bytes());
        let digest = hasher.finalize();
        // First 16 bytes = 128 bits = plenty of collision resistance for a
        // hardware catalog. Hex-encode to keep the key SQL-safe.
        let key = hex::encode(&digest[..16]);
        RecordId::new(HARDWARE_COMPONENT_TABLE, key)
    }

    /// Build a new catalog entry with the canonical ID and `first_seen` /
    /// `last_seen` set to now. Caller is responsible for upserting (so
    /// existing rows have their `occurrence_count` / `last_seen` bumped
    /// rather than overwritten).
    pub fn new(
        kind: HardwareKind,
        vendor: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        let vendor = vendor.into();
        let model = model.into();
        let display_name = format!("{vendor} {model}").trim().to_string();
        let id = Self::canonical_id(kind, &vendor, &model);
        let now: Datetime = chrono::Utc::now().into();
        Self {
            id,
            kind,
            vendor,
            model,
            sku: None,
            display_name,
            specs: None,
            first_seen: now.clone(),
            last_seen: now,
            occurrence_count: 0,
            embedding: Vec::new(),
        }
    }

    /// Text passed to `fn::embed_text` when the row has no embedding yet.
    pub fn embed_source(&self) -> String {
        format!(
            "{} {} {} {}",
            self.kind.as_str(),
            self.vendor,
            self.model,
            self.display_name
        )
    }

    /// Upsert + bump occurrence_count + refresh last_seen in one round-trip.
    /// Verifies via read-back so a silent "query succeeded but row didn't
    /// land" (permissions, missing table, etc.) becomes a hard error
    /// rather than a fake Ok the caller can't distinguish from a real one.
    pub async fn upsert_seen(component: &Self) -> anyhow::Result<RecordId> {
        super::utilities::spawn_embedding_backfill();
        // NONE keeps an existing embedding via `?? embedding` in the MERGE.
        let embedding = match super::utilities::embed_text(&component.embed_source()).await {
            Ok(v) => Some(v),
            Err(e) => {
                log::warn!("embed_text failed for hardware_component {:?}: {e:?}", component.id);
                None
            }
        };
        let sql = stress_test_sql::HW_COMPONENT_UPSERT;
        let mut response = db()
            .query(sql)
            .bind(("id", component.id.clone()))
            .bind(("kind", component.kind.as_str().to_string()))
            .bind(("vendor", component.vendor.clone()))
            .bind(("model", component.model.clone()))
            .bind(("sku", component.sku.clone()))
            .bind(("display", component.display_name.clone()))
            .bind(("specs", component.specs.clone()))
            .bind(("embedding", embedding))
            .await?;
        let ids: Vec<RecordId> = response.take(0)?;
        if ids.is_empty() {
            anyhow::bail!(
                "hardware_component UPSERT for {:?} returned no row id (table missing or permissions?)",
                component.id
            );
        }

        if !Self::exists(&component.id).await? {
            anyhow::bail!(
                "hardware_component row {:?} not readable after UPSERT",
                component.id
            );
        }

        Ok(component.id.clone())
    }

    /// True when the hardware_component row is present in SurrealDB.
    pub async fn exists(id: &RecordId) -> anyhow::Result<bool> {
        Ok(matches!(record_exists(id.clone()).await, Ok(Some(true))))
    }

    pub async fn list_by_kind(kind: HardwareKind) -> anyhow::Result<Vec<Self>> {
        let rows: Vec<Self> = db()
            .query(
                "SELECT * FROM hardware_component \
                 WHERE kind == $k ORDER BY display_name LIMIT 500",
            )
            .bind(("k", kind.as_str().to_string()))
            .await?
            .take(0)?;
        Ok(rows)
    }
}

// ============================================================
// Test catalog
// ============================================================

/// What's being stressed. Drives the failure-mode rubric we apply to a
/// run (a CPU run is failing if WHEA delta > 0; a disk run is failing
/// if any `Metrics.last_error` populated; etc.).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, SurrealValue)]
#[serde(rename_all = "snake_case")]
#[surreal(untagged)]
pub enum TargetKind {
    #[surreal(value = "cpu")]
    Cpu,
    #[surreal(value = "gpu")]
    Gpu,
    #[surreal(value = "memory")]
    Memory,
    #[surreal(value = "storage")]
    Storage,
    #[surreal(value = "psu")]
    Psu,
    #[surreal(value = "motherboard")]
    Motherboard,
    /// Whole-system run (Combined Prime95+FurMark, OCCT Power, etc.).
    #[surreal(value = "system")]
    System,
    /// Multi-component stage run that touches more than one subsystem;
    /// the participants are listed in `StressTestRun.touched_components`.
    #[surreal(value = "mixed")]
    Mixed,
}

impl TargetKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Gpu => "gpu",
            Self::Memory => "memory",
            Self::Storage => "storage",
            Self::Psu => "psu",
            Self::Motherboard => "motherboard",
            Self::System => "system",
            Self::Mixed => "mixed",
        }
    }
}

/// Prime95 workloads — see https://www.mersenne.org/download/.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, SurrealValue)]
#[serde(rename_all = "snake_case")]
#[surreal(untagged)]
pub enum Prime95Workload {
    #[surreal(value = "smallest")]
    Smallest,
    #[surreal(value = "small")]
    Small,
    #[surreal(value = "large")]
    Large,
    #[surreal(value = "blend")]
    Blend,
    /// "Just Stress Testing" — the canonical option that runs Small FFTs
    /// indefinitely with maximum heat.
    #[surreal(value = "stress_testing")]
    StressTesting,
}

/// OCCT profile. Matches the dropdown choices in the OCCT GUI.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, SurrealValue)]
#[serde(rename_all = "snake_case")]
#[surreal(untagged)]
pub enum OcctProfile {
    #[surreal(value = "cpu_occt")]
    CpuOcct,
    #[surreal(value = "cpu_avx2")]
    CpuAvx2,
    #[surreal(value = "cpu_linpack")]
    CpuLinpack,
    #[surreal(value = "memory")]
    Memory,
    #[surreal(value = "power_supply")]
    PowerSupply,
    #[surreal(value = "gpu_3d")]
    GpuThreeD,
    #[surreal(value = "gpu_memtest")]
    GpuMemtest,
}

/// All the stress tools we know how to record. Internal (stress-kit)
/// runs share two variants; everything else maps to a recognized
/// industry tool so cross-shop comparisons are possible.
///
/// Add new variants here when a new tool gets bench-approved. Don't
/// abuse `Other(_)` for tools you control — give them a first-class
/// variant so `tool_label()` stays stable.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, SurrealValue)]
pub enum TestTool {
    /// One of stress-kit's single stressors, persisted as its canonical label.
    StressKit {
        stressor: String,
    },
    /// A stress-kit `ScenarioDefinition` (multi-stage).
    StressKitScenario {
        /// Optional human label for the saved scenario, e.g. "burn-in v1".
        name: Option<String>,
    },

    // ---- CPU ----
    Prime95 {
        workload: Prime95Workload,
    },
    Occt {
        profile: OcctProfile,
    },
    CinebenchR23,
    CinebenchR24,
    Aida64Stability {
        /// Checked components ("CPU", "FPU", "Cache", "RAM", "GPU", "Disk").
        components: Vec<String>,
    },
    Linpack,

    // ---- GPU ----
    FurMark {
        resolution: String,
        msaa: u32,
    },
    OcctGpu,
    ThreeDMarkStress {
        /// "Steel Nomad", "Speed Way", "Solar Bay Extreme", "Wild Life", …
        test: String,
    },
    HeavenBenchmark,
    Superposition,
    MsiKombustor,

    // ---- Memory ----
    MemTest86 {
        passes: u32,
    },
    /// Windows Memory Diagnostic (`mdsched.exe`).
    MdSched,
    HciMemTest {
        coverage_pct: u32,
    },
    Karhu {
        coverage_pct: u32,
    },
    Tm5 {
        /// Config preset — "anta777", "1usmus_v3", "extreme1", "absolut", …
        config: String,
    },

    // ---- Storage ----
    SmartShort,
    SmartExtended,
    ChkDsk {
        drive: String,
        switches: Vec<String>,
    },
    HdTune,
    CrystalDiskMark,
    HddScan,

    // ---- Whole-system / PSU ----
    OcctPower,
    /// User-built combination (e.g. Prime95 + FurMark in parallel).
    Combined {
        tools: Vec<String>,
    },

    /// Escape hatch — DO NOT use for tools you control. Reserved for
    /// one-off vendor utilities we don't expect to see twice.
    Other(String),
}

impl TestTool {
    /// Lowercase, indexable label written to `StressTestRun.tool_label`
    /// so SurrealDB queries can filter without destructuring the enum.
    /// Keep stable — these strings index every materialized view row.
    pub fn label(&self) -> String {
        match self {
            Self::StressKit { stressor } => format!("stresskit:{stressor}"),
            Self::StressKitScenario { .. } => "stresskit:scenario".to_string(),
            Self::Prime95 { .. } => "prime95".to_string(),
            Self::Occt { .. } => "occt".to_string(),
            Self::CinebenchR23 => "cinebench_r23".to_string(),
            Self::CinebenchR24 => "cinebench_r24".to_string(),
            Self::Aida64Stability { .. } => "aida64_stability".to_string(),
            Self::Linpack => "linpack".to_string(),
            Self::FurMark { .. } => "furmark".to_string(),
            Self::OcctGpu => "occt_gpu".to_string(),
            Self::ThreeDMarkStress { .. } => "3dmark_stress".to_string(),
            Self::HeavenBenchmark => "heaven".to_string(),
            Self::Superposition => "superposition".to_string(),
            Self::MsiKombustor => "kombustor".to_string(),
            Self::MemTest86 { .. } => "memtest86".to_string(),
            Self::MdSched => "mdsched".to_string(),
            Self::HciMemTest { .. } => "hci_memtest".to_string(),
            Self::Karhu { .. } => "karhu".to_string(),
            Self::Tm5 { .. } => "tm5".to_string(),
            Self::SmartShort => "smart_short".to_string(),
            Self::SmartExtended => "smart_extended".to_string(),
            Self::ChkDsk { .. } => "chkdsk".to_string(),
            Self::HdTune => "hd_tune".to_string(),
            Self::CrystalDiskMark => "crystaldiskmark".to_string(),
            Self::HddScan => "hddscan".to_string(),
            Self::OcctPower => "occt_power".to_string(),
            Self::Combined { .. } => "combined".to_string(),
            Self::Other(s) => format!("other:{}", s.to_ascii_lowercase()),
        }
    }
}

// ============================================================
// Run verdict + failure rubric
// ============================================================

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, SurrealValue)]
#[serde(rename_all = "snake_case")]
#[surreal(untagged)]
pub enum RunResult {
    /// Tool finished cleanly with no failure signals (no WHEA delta, no
    /// disk I/O errors, no BSOD, no operator override). Note: a "pass"
    /// from a stress test isn't proof of health — it's the absence of
    /// observed faults during this run.
    #[surreal(value = "pass")]
    Pass,
    /// At least one objective failure signal fired (WHEA, BSOD, disk I/O
    /// error, throttle past threshold, value mismatch, …).
    #[surreal(value = "fail")]
    Fail,
    /// Run did not complete (cancelled by operator, hung, timeout).
    #[surreal(value = "aborted")]
    Aborted,
    /// Run completed but the result is ambiguous (e.g. monitoring tools
    /// crashed but the stress ran). Operator review required.
    #[surreal(value = "inconclusive")]
    Inconclusive,
    /// Set when the run record is created and cleared when finalized.
    #[surreal(value = "in_progress")]
    InProgress,
}

impl RunResult {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Aborted => "aborted",
            Self::Inconclusive => "inconclusive",
            Self::InProgress => "in_progress",
        }
    }
}

/// The dominant failure signal observed in a run, if any. Used by the
/// AI as the first cut at root cause.
///
/// Serializes via SurrealValue's default external tagging, so the DB
/// shape is `{"Bsod": {"code": "...", ...}}`. For AI-friendly lowercase
/// filtering see `StressTestRun.failure_kind` (`"bsod"`, `"tdr"`, …).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, SurrealValue)]
pub enum FailureMode {
    None,
    /// Application or worker thread exited with non-zero status without
    /// a BSOD. Possible OS-level fault, driver kill, or app bug.
    AppError {
        exit_code: Option<i32>,
        message: String,
    },
    /// Windows kernel bugcheck observed during or shortly after the run.
    Bsod {
        code: Option<String>,
        bugcheck_args: Option<Vec<String>>,
    },
    /// Display driver reset / Timeout Detection and Recovery (nvlddmkm
    /// event 4101 et al).
    Tdr {
        count: u32,
    },
    /// GPU stressor reported device loss/removal (wgpu device-lost or
    /// uncaptured device error) without a logged TDR event.
    GpuDeviceLost {
        message: String,
    },
    /// WHEA-Logger event count exceeded baseline during the run.
    WheaError {
        count: u32,
    },
    /// CPU/GPU temperature crossed the throttle threshold for the
    /// component being tested.
    ThermalThrottle {
        peak_temp_c: f32,
    },
    /// Disk stressor I/O failure reported via `Metrics.last_error`.
    DiskIoError {
        message: String,
    },
    /// Memory test reported a value mismatch (HCI / TM5 / Karhu / MemTest86).
    DataMismatch {
        addresses: Option<Vec<String>>,
    },
    /// Sustained clock drop under load without a matching temperature breach.
    ClockCollapse {
        stage_label: String,
        below_pct: f32,
    },
    /// Tick-throughput variance exceeded the configured band after warmup.
    ThroughputUnstable {
        stage_label: String,
        cv: f64,
    },
    /// Whole system rebooted or hard-hung during the run (Kernel-Power
    /// event 41, unexpected shutdown).
    Reboot,
    /// Wall-clock timeout hit before the planned duration finished and
    /// without a verdict from the tool.
    Timeout,
    /// Operator marked the run failed without a tool-level signal.
    OperatorOverride {
        reason: String,
    },
    /// A board voltage rail stayed below its configured floor under load. Only
    /// reachable when an operator opts in per board — the SuperIO divider is
    /// assumed, so no built-in policy sets a rail floor. Appended last so
    /// existing variant indices stay stable.
    RailDroop {
        rail: String,
        min_v: f32,
    },
}

impl FailureMode {
    /// Lowercase tag for `StressTestRun.failure_kind`. Stable across
    /// variant restructures — when adding a variant, add a `kind` here.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::AppError { .. } => "app_error",
            Self::Bsod { .. } => "bsod",
            Self::Tdr { .. } => "tdr",
            Self::GpuDeviceLost { .. } => "gpu_device_lost",
            Self::WheaError { .. } => "whea_error",
            Self::ThermalThrottle { .. } => "thermal_throttle",
            Self::DiskIoError { .. } => "disk_io_error",
            Self::DataMismatch { .. } => "data_mismatch",
            Self::ClockCollapse { .. } => "clock_collapse",
            Self::ThroughputUnstable { .. } => "throughput_unstable",
            Self::Reboot => "reboot",
            Self::Timeout => "timeout",
            Self::OperatorOverride { .. } => "operator_override",
            Self::RailDroop { .. } => "rail_droop",
        }
    }
}

/// Mirrors stress-kit's `scenario::FinishReason` so the database stores
/// why the run ended without coupling to the stress-kit crate.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, SurrealValue)]
#[serde(rename_all = "snake_case")]
#[surreal(untagged)]
pub enum FinishReason {
    #[surreal(value = "completed")]
    Completed,
    #[surreal(value = "cancelled")]
    Cancelled,
    #[surreal(value = "total_time")]
    TotalTime,
    /// Timeout hit (only meaningful for single-stressor runs with
    /// `StressConfig.timeout`).
    #[surreal(value = "timeout")]
    Timeout,
    /// Run crashed before reaching a normal finish (e.g. supervisor
    /// thread panicked).
    #[surreal(value = "crashed")]
    Crashed,
}

// ============================================================
// Run-scoped sub-structures
// ============================================================

/// Snapshot of BIOS/firmware settings relevant to stress-test outcomes.
/// Almost everything is `Option` because operators rarely fill all of
/// these in — record what's available, leave the rest `None`.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, SurrealValue)]
pub struct BiosSettings {
    /// "auto", "xmp1", "xmp2", "expo1", "expo2", "manual_<speed>" …
    pub memory_profile: Option<String>,
    pub xmp_expo_enabled: Option<bool>,
    pub cpu_undervolt_mv: Option<i32>,
    pub cpu_overclock_mhz: Option<u32>,
    pub gpu_overclock_mhz: Option<u32>,
    pub power_limit_w: Option<u32>,
    pub pbo_enabled: Option<bool>,
    pub resizable_bar: Option<bool>,
    pub virtualization_enabled: Option<bool>,
    pub bios_version: Option<String>,
    /// Catch-all for site-specific notes — "RAM kit reseated 5/14",
    /// "running with 1 stick only", etc.
    pub notes: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, SurrealValue)]
pub struct DriverVersions {
    pub gpu: Option<String>,
    pub chipset: Option<String>,
    pub storage_controller: Option<String>,
    pub network: Option<String>,
}

/// Per-stage summary inside a scenario run. Matches the shape that
/// stress-kit's `ScenarioRunner` already emits, plus a label/duration
/// so the run row is self-contained without joining metrics.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, SurrealValue)]
pub struct ScenarioStageSummary {
    pub index: u32,
    pub label: String,
    /// Canonical stressor label (`Stressor::as_str`) so scenario runs
    /// filter by what they actually exercised.
    pub stressor: String,
    pub threads: u32,
    pub duration_planned_secs: u64,
    pub duration_actual_secs: f64,
    pub peak_throughput: Option<f64>,
    pub avg_throughput: Option<f64>,
    pub throughput_unit: String,
    pub had_error: bool,
    pub last_error: Option<String>,
    /// `"pass"` / `"fail"`; NONE when no verdict rules were attached.
    #[serde(default)]
    #[surreal(default)]
    pub result: Option<String>,
    /// Human-readable rule breaches for a failed stage.
    #[serde(default)]
    #[surreal(default)]
    pub violations: Vec<String>,
    #[serde(default)]
    #[surreal(default)]
    pub max_temp_c: Option<f32>,
    #[serde(default)]
    #[surreal(default)]
    pub avg_temp_c: Option<f32>,
    #[serde(default)]
    #[surreal(default)]
    pub max_gpu_temp_c: Option<f32>,
    /// Lowest +12V rail reading during this stage. Uncalibrated SuperIO value —
    /// trend/droop data, not an absolute. Missing on older rows.
    #[serde(default)]
    #[surreal(default)]
    pub min_v12_v: Option<f32>,
    #[serde(default)]
    #[surreal(default)]
    pub max_clock_mhz: Option<u32>,
    /// Stress-kit `Metrics.errors` accumulated within this stage.
    #[serde(default)]
    #[surreal(default)]
    pub errors: u64,
    /// WHEA counter movement between stage start and end.
    #[serde(default)]
    #[surreal(default)]
    pub whea_delta: u32,
    /// TDR counter movement between stage start and end.
    #[serde(default)]
    #[surreal(default)]
    pub tdr_delta: u32,
    /// Post-warmup coefficient of variation of tick throughput.
    #[serde(default)]
    #[surreal(default)]
    pub throughput_cv: Option<f64>,
    /// Longest consecutive run of collapsed-clock ticks under load.
    #[serde(default)]
    #[surreal(default)]
    pub clock_collapse_ticks: u32,
}

/// Rolled-up metrics for the run. Populated by the qc-app supervisor
/// from the telemetry stream when the run finalizes. The AI almost
/// always reads from here rather than scanning the metric series.
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, SurrealValue)]
pub struct RunSummary {
    pub max_temp_c: Option<f32>,
    pub avg_temp_c: Option<f32>,
    pub max_clock_mhz: Option<u32>,
    pub avg_clock_mhz: Option<u32>,
    pub max_cpu_usage_pct: Option<f32>,
    pub avg_cpu_usage_pct: Option<f32>,
    pub max_power_w: Option<u32>,
    pub max_fan_rpm: Option<u32>,
    pub peak_throughput: Option<f64>,
    pub avg_throughput: Option<f64>,
    pub throughput_unit: Option<String>,
    pub thermal_throttle_detected: bool,
    pub vrm_throttle_detected: bool,
    /// `WheaCounters.delta_since_program_start` at run end.
    pub whea_delta_count: u32,
    /// Count of `stress_test_event` rows with `kind == "tdr"`.
    pub tdr_count: u32,
    pub bsod_detected: bool,
    pub bsod_code: Option<String>,
    /// Count of `stress_test_event` rows with `kind == "disk_io_error"`.
    pub disk_io_errors: u32,
    /// Count of memory data-mismatch events from HCI / TM5 / Karhu.
    pub memory_errors: u32,
    /// Cumulative `Metrics.errors` from verifying stress-kit stressors
    /// (memtest mismatches, cpu_verify divergences, linpack residual
    /// breaches, VRAM mismatches). Missing on rows older than this field.
    #[serde(default)]
    #[surreal(default)]
    pub test_errors: u32,
    /// Max GPU temperature across all cards. Missing on older rows.
    #[serde(default)]
    #[surreal(default)]
    pub max_gpu_temp_c: Option<f32>,
    /// Max CPU/package temperature from thermals. Missing on older rows.
    #[serde(default)]
    #[surreal(default)]
    pub max_cpu_temp_c: Option<f32>,
    /// Lowest +12V rail reading of the run — droop, not peak, is the PSU signal.
    /// Uncalibrated SuperIO value. Missing on older rows.
    #[serde(default)]
    #[surreal(default)]
    pub min_v12_v: Option<f32>,
    /// Adapter the GPU work actually bound, as reported by wgpu. NONE on runs
    /// with no GPU stage and on rows older than this field.
    #[serde(default)]
    #[surreal(default)]
    pub gpu_adapter_name: Option<String>,
    /// wgpu `DeviceType` of that adapter: `DiscreteGpu`, `IntegratedGpu`, `Cpu`, …
    #[serde(default)]
    #[surreal(default)]
    pub gpu_adapter_device_type: Option<String>,
    /// `true` when a discrete adapter was requested but a non-discrete one was bound.
    #[serde(default)]
    #[surreal(default)]
    pub gpu_adapter_integrated_fallback: Option<bool>,
}

// ============================================================
// Top-level run
// ============================================================

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, SurrealValue)]
pub struct StressTestRun {
    pub id: RecordId,

    // --- Links (per design choices) ---
    /// The machine being tested. Required.
    pub computer: RecordId,
    /// Optional link to the work order this run belongs to.
    pub service_order: Option<RecordId>,
    /// Optional link to the diagnostic session that triggered this run.
    pub session_ref: Option<RecordId>,
    /// Optional link to the in-house task.
    pub task_ref: Option<RecordId>,

    // --- What was tested ---
    pub target_kind: TargetKind,
    /// Primary component under test. NONE for `System` / `Mixed` runs.
    /// Indexed; the materialized baseline view groups on this.
    pub target_component: Option<RecordId>,
    /// Every component the run touched (for Mixed/System runs). Lets
    /// the AI find "every run that exercised this RAM kit" even when
    /// it wasn't the primary target.
    pub touched_components: Vec<RecordId>,

    // --- The tool & how it ran ---
    pub tool: TestTool,
    /// Denormalized lowercase tool tag (see `TestTool::label`). Maintained
    /// by `StressTestRun::set_tool` so it never drifts from `tool`.
    pub tool_label: String,
    pub tool_version: Option<String>,
    /// Free-form human label for the preset/profile ("Small FFTs",
    /// "Burn-in v1", "8 thread", …). The structured params live inside
    /// the `tool` variant.
    pub preset_label: Option<String>,
    /// For stress-kit scenario runs — the stage breakdown. Empty for
    /// single-stressor or non-stress-kit tools.
    pub scenario_stages: Vec<ScenarioStageSummary>,

    // --- Timing ---
    pub started_at: Datetime,
    pub ended_at: Option<Datetime>,
    pub duration_planned_secs: Option<u64>,
    pub duration_actual_secs: Option<f64>,

    // --- Environment ---
    /// Operator / tech email or initials.
    pub tech: Option<String>,
    /// Convenience copy of `computer.hostname` at run-time.
    pub hostname: Option<String>,
    /// Convenience copy of the full `generate_client_id` SHA-256 hex
    /// (`client_hash` on `ConnectedClient`). The `computer` FK uses
    /// `{hostname}:{hash[..9]}` instead.
    pub machine_id: Option<String>,
    pub ambient_temp_c: Option<f32>,
    pub environment_notes: Option<String>,
    pub bios_settings: BiosSettings,
    pub driver_versions: DriverVersions,

    // --- Verdict ---
    pub result: RunResult,
    pub finish_reason: Option<FinishReason>,
    pub failure_mode: FailureMode,
    /// Denormalized lowercase tag of `failure_mode` (see [`FailureMode::kind`]).
    /// Kept in sync by [`StressTestRun::set_failure_mode`] and
    /// [`StressTestRun::finalize`] so AI queries can filter
    /// `WHERE failure_kind = 'bsod'` without destructuring the object.
    pub failure_kind: String,

    // --- Aggregates (rolled-up from the metric stream) ---
    pub summary: RunSummary,

    // --- Artifacts ---
    /// Bucket paths to raw tool logs (e.g. Prime95 results.txt).
    pub raw_log_refs: Vec<String>,
    /// Bucket paths to screenshots (HWiNFO panel, OCCT result page, etc.).
    pub screenshot_refs: Vec<String>,

    // --- Annotations ---
    pub notes: Option<String>,
    /// Reserved for AI-generated analysis (root-cause hypothesis, plain-
    /// English summary, comparison to baseline). Schema-free so we can
    /// iterate on the prompt without migrations.
    pub ai_assessment: Option<serde_json::Value>,
    pub tags: Vec<String>,

    /// 768-dim embedding for semantic similarity search. `none | array<float>`
    /// in the DB so stress tests created offline (no Ollama) store NONE and
    /// the HNSW index simply skips them. `skip_serializing_if` prevents Rust
    /// from sending `[]` on insert, which would hit the HNSW dimension check.
    /// A backfill job or AI review pass populates embeddings for completed runs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub embedding: Vec<f32>,
}

impl StressTestRun {
    /// Build a new run record in the `InProgress` state. Caller is
    /// expected to fill in the typed fields, persist with `create`,
    /// and finalize with `finalize` once the supervisor reports a
    /// finish reason.
    pub fn new_for(computer: RecordId, tool: TestTool, target_kind: TargetKind) -> Self {
        let now: Datetime = chrono::Utc::now().into();
        let tool_label = tool.label();
        Self {
            id: random_record_id(STRESS_TEST_RUN_TABLE),
            computer,
            service_order: None,
            session_ref: None,
            task_ref: None,
            target_kind,
            target_component: None,
            touched_components: Vec::new(),
            tool,
            tool_label,
            tool_version: None,
            preset_label: None,
            scenario_stages: Vec::new(),
            started_at: now,
            ended_at: None,
            duration_planned_secs: None,
            duration_actual_secs: None,
            tech: None,
            hostname: None,
            machine_id: None,
            ambient_temp_c: None,
            environment_notes: None,
            bios_settings: BiosSettings::default(),
            driver_versions: DriverVersions::default(),
            result: RunResult::InProgress,
            finish_reason: None,
            failure_mode: FailureMode::None,
            failure_kind: FailureMode::None.kind().to_string(),
            summary: RunSummary::default(),
            raw_log_refs: Vec::new(),
            screenshot_refs: Vec::new(),
            notes: None,
            ai_assessment: None,
            tags: Vec::new(),
            embedding: Vec::new(),
        }
    }

    /// Swap the tool variant and keep `tool_label` in sync.
    pub fn set_tool(&mut self, tool: TestTool) {
        self.tool_label = tool.label();
        self.tool = tool;
    }

    /// Swap the failure mode and keep `failure_kind` in sync.
    pub fn set_failure_mode(&mut self, failure_mode: FailureMode) {
        self.failure_kind = failure_mode.kind().to_string();
        self.failure_mode = failure_mode;
    }

    /// Text passed to `fn::embed_text` when the run row has no local embedding.
    pub fn embed_source(&self) -> String {
        format!(
            "{} {} {} {}",
            self.tool_label,
            self.preset_label.as_deref().unwrap_or(""),
            self.target_kind.as_str(),
            self.hostname.as_deref().unwrap_or(""),
        )
    }

}

/// Coerce CREATE content to satisfy `stress_test_run` field types.
fn ensure_run_content_objects(content: &mut surrealdb::types::Value, run: &StressTestRun) {
    if let surrealdb::types::Value::Object(obj) = content {
        if matches!(run.failure_mode, FailureMode::None) {
            obj.insert(
                "failure_mode".to_string(),
                surrealdb::types::Value::Object(
                    [(
                        "None".to_string(),
                        surrealdb::types::Value::Object(Default::default()),
                    )]
                    .into_iter()
                    .collect(),
                ),
            );
        }

        for key in ["bios_settings", "driver_versions", "summary"] {
            if matches!(obj.get(key), None | Some(surrealdb::types::Value::None)) {
                obj.insert(
                    key.to_string(),
                    surrealdb::types::Value::Object(Default::default()),
                );
            }
        }
    }
}

impl StressTestRun {
    pub async fn create(run: &Self) -> anyhow::Result<RecordId> {
        let mut content = surreal_create_content(run, true);
        ensure_run_content_objects(&mut content, run);
        super::utilities::spawn_embedding_backfill();
        // Embedding is optional on stress_test_run; NONE on embed failure.
        let embedding = match super::utilities::embed_text(&run.embed_source()).await {
            Ok(v) => Some(v),
            Err(e) => {
                log::warn!("embed_text failed for stress_test_run {:?}: {e:?}", run.id);
                None
            }
        };

        let mut response = db()
            .query(stress_test_sql::STRESS_RUN_CREATE)
            .bind(("id", run.id.clone()))
            .bind(("content", content))
            .bind(("embedding", embedding))
            .await?;

        let created: Vec<RecordId> = response.take(0).map_err(|e| {
            anyhow::anyhow!("stress_test_run CREATE for {:?} rejected: {e}", run.id)
        })?;
        if created.is_empty() {
            anyhow::bail!(
                "stress_test_run CREATE for {:?} returned no row (table missing or permissions?)",
                run.id
            );
        }

        if !Self::exists(&run.id).await? {
            anyhow::bail!(
                "stress_test_run row {:?} not readable after CREATE",
                run.id
            );
        }

        Ok(run.id.clone())
    }

    /// True when the run row is present in SurrealDB.
    pub async fn exists(id: &RecordId) -> anyhow::Result<bool> {
        Ok(matches!(record_exists(id.clone()).await, Ok(Some(true))))
    }

    /// Insert a completed run and linked events (backfill / hung-run recovery).
    pub async fn create_completed(
        run: &Self,
        events: &[StressTestEvent],
    ) -> anyhow::Result<RecordId> {
        let id = Self::create(run).await?;
        for event in events {
            let mut e = event.clone();
            e.run_ref = id.clone();
            StressTestEvent::create(&e).await?;
        }
        Ok(id)
    }

    /// Finalize: write summary, verdict, finish reason, stage breakdown,
    /// and ended_at in one transaction. `failure_mode == None` +
    /// `result == Pass` is the "clean run" path.
    pub async fn finalize(
        run_id: &RecordId,
        result: RunResult,
        finish_reason: FinishReason,
        failure_mode: FailureMode,
        summary: RunSummary,
        stages: Vec<ScenarioStageSummary>,
        ended_at: Option<Datetime>,
    ) -> anyhow::Result<()> {
        // `duration::secs(...)` returns an integer when the elapsed window
        // is a whole number of seconds; round-tripping through the Rust
        // `Option<f64>` field then fails with "Expected float, got number".
        // Force the cast at the write site so the column is always a float.
        let sql = "UPDATE $id SET \
                result = $result, \
                finish_reason = $finish, \
                failure_mode = $failure, \
                failure_kind = $failure_kind, \
                summary = $summary, \
                scenario_stages = $stages, \
                ended_at = $ended_at, \
                duration_actual_secs = <float> duration::secs(($ended_at ?? time::now()) - started_at)";
        let failure_kind = failure_mode.kind().to_string();
        db()
            .query(sql)
            .bind(("id", run_id.clone()))
            .bind(("result", result.as_str().to_string()))
            .bind(("finish", finish_reason))
            .bind(("failure", failure_mode))
            .bind(("failure_kind", failure_kind))
            .bind(("summary", summary))
            .bind(("stages", stages))
            .bind(("ended_at", ended_at.unwrap_or_else(|| chrono::Utc::now().into())))
            .await?;
        Ok(())
    }

    /// One run by id, with the float cast `list_for_computer` uses.
    pub async fn get(run_id: &RecordId) -> anyhow::Result<Option<Self>> {
        let run: Option<Self> = db()
            .query(
                "SELECT *, <float> duration_actual_secs AS duration_actual_secs \
                 FROM ONLY $id",
            )
            .bind(("id", run_id.clone()))
            .await?
            .take(0)?;
        Ok(run)
    }

    /// History for one machine, newest first.
    pub async fn list_for_computer(computer: &RecordId) -> anyhow::Result<Vec<Self>> {
        let runs: Vec<Self> = db()
            .query(
                "SELECT *, (IF duration_actual_secs != NONE THEN <float> duration_actual_secs END) AS duration_actual_secs \
                 FROM stress_test_run \
                 WHERE computer = $c ORDER BY started_at DESC LIMIT 200",
            )
            .bind(("c", computer.clone()))
            .await?
            .take(0)?;
        Ok(runs)
    }

    /// Every run that exercised this component (primary or touched).
    /// Drives "across all RTX 4070 SUPER tests, what shows up?".
    pub async fn list_for_component(component: &RecordId) -> anyhow::Result<Vec<Self>> {
        let runs: Vec<Self> = db()
            .query(
                "SELECT *, (IF duration_actual_secs != NONE THEN <float> duration_actual_secs END) AS duration_actual_secs \
                 FROM stress_test_run \
                 WHERE target_component = $c OR touched_components CONTAINS $c \
                 ORDER BY started_at DESC LIMIT 500",
            )
            .bind(("c", component.clone()))
            .await?
            .take(0)?;
        Ok(runs)
    }

    pub async fn list_for_session(session: &RecordId) -> anyhow::Result<Vec<Self>> {
        let runs: Vec<Self> = db()
            .query(
                "SELECT *, <float> duration_actual_secs AS duration_actual_secs \
                 FROM stress_test_run \
                 WHERE session_ref = $s ORDER BY started_at ASC",
            )
            .bind(("s", session.clone()))
            .await?
            .take(0)?;
        Ok(runs)
    }
}

// ============================================================
// Telemetry samples
// ============================================================

/// Per-core sample shape (matches stress-kit's `telemetry::core::CoreSample`).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, SurrealValue)]
pub struct CoreSampleRow {
    pub index: u32,
    pub brand: String,
    pub usage_pct: f32,
    pub freq_mhz: u64,
    pub temp_c: Option<f32>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, SurrealValue)]
pub struct DiskRateRow {
    pub name: String,
    pub read_mb_per_s: f64,
    pub write_mb_per_s: f64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, SurrealValue)]
pub struct NetworkRateRow {
    pub name: String,
    pub rx_mbps: f64,
    pub tx_mbps: f64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, SurrealValue)]
pub struct StressTestMetric {
    pub id: RecordId,
    pub run_ref: RecordId,
    pub captured_at: Datetime,
    /// Stage index for scenario runs (0-based). NONE for single-tool runs.
    pub stage_index: Option<u32>,
    pub stage_label: Option<String>,
    pub cores: Vec<CoreSampleRow>,
    pub memory_used_pct: Option<f32>,
    pub memory_used_mb: Option<u64>,
    pub page_file_used_pct: Option<f32>,
    pub disks: Vec<DiskRateRow>,
    pub networks: Vec<NetworkRateRow>,
    /// Stress-kit `Metrics.throughput` at this tick.
    pub throughput: Option<f64>,
    pub throughput_unit: Option<String>,
    /// WHEA delta from the most recent 5 s scan (refreshed every ~5 ticks).
    pub whea_delta_count: Option<u32>,
    /// Stress-kit `Metrics.last_error` from this tick.
    pub last_error: Option<String>,
    /// Max GPU temperature across cards at this tick. Missing on older rows.
    #[serde(default)]
    #[surreal(default)]
    pub gpu_temp_c: Option<f32>,
    /// Max CPU/package temperature from thermals at this tick. Missing on older rows.
    #[serde(default)]
    #[surreal(default)]
    pub cpu_temp_c: Option<f32>,
    #[serde(default)]
    #[surreal(default)]
    pub gpu_clock_mhz: Option<u32>,
    #[serde(default)]
    #[surreal(default)]
    pub gpu_power_w: Option<f32>,
    #[serde(default)]
    #[surreal(default)]
    pub gpu_usage_pct: Option<f32>,
    /// `TdrCounters.delta_since_program_start` at this tick.
    #[serde(default)]
    #[surreal(default)]
    pub tdr_delta_count: Option<u32>,
    /// Mean CPU utilization across logical cores at this tick. Missing on older rows.
    #[serde(default)]
    #[surreal(default)]
    pub cpu_usage_pct: Option<f32>,
    /// Mean CPU core clock (MHz) at this tick. Missing on older rows.
    #[serde(default)]
    #[surreal(default)]
    pub clock_mhz: Option<u32>,
    /// Aggregate board power (W) at this tick; GPU-sum proxy. Missing on older rows.
    #[serde(default)]
    #[surreal(default)]
    pub power_w: Option<f32>,
    /// SuperIO board rails at this tick (`winring0-thermal`, Windows only).
    /// Scaled with assumed nominal dividers, so these are uncalibrated and
    /// board-specific — read them as trend/droop, never as absolutes.
    #[serde(default)]
    #[surreal(default)]
    pub v12_v: Option<f32>,
    #[serde(default)]
    #[surreal(default)]
    pub v5_v: Option<f32>,
    /// Sensor chip's 3.3V supply, not the board's +3.3V PSU rail.
    #[serde(default)]
    #[surreal(default)]
    pub v3vcc_v: Option<f32>,
    #[serde(default)]
    #[surreal(default)]
    pub vcore_v: Option<f32>,
}

impl StressTestMetric {
    pub fn new(run_ref: RecordId, captured_at: Datetime) -> Self {
        Self {
            id: random_record_id(STRESS_TEST_METRIC_TABLE),
            run_ref,
            captured_at,
            stage_index: None,
            stage_label: None,
            cores: Vec::new(),
            memory_used_pct: None,
            memory_used_mb: None,
            page_file_used_pct: None,
            disks: Vec::new(),
            networks: Vec::new(),
            throughput: None,
            throughput_unit: None,
            whea_delta_count: None,
            last_error: None,
            gpu_temp_c: None,
            cpu_temp_c: None,
            gpu_clock_mhz: None,
            gpu_power_w: None,
            gpu_usage_pct: None,
            tdr_delta_count: None,
            cpu_usage_pct: None,
            clock_mhz: None,
            power_w: None,
            v12_v: None,
            v5_v: None,
            v3vcc_v: None,
            vcore_v: None,
        }
    }

    pub async fn create(metric: &Self) -> anyhow::Result<RecordId> {
        metric.validate_for_insert().await?;
        let value = surreal_create_content(metric, false);
        let created: Option<Self> = db()
            .create(metric.id.clone())
            .content(value)
            .await?;
        Ok(created.map(|c| c.id).unwrap_or_else(|| metric.id.clone()))
    }

    /// Reject default-shaped rows and orphan run_ref links before insert.
    pub fn validate_shape(&self) -> anyhow::Result<()> {
        if self.cores.is_empty() {
            anyhow::bail!("stress_test_metric has empty cores (default telemetry snapshot)");
        }
        let captured_ms = self.captured_at.timestamp_millis();
        if captured_ms <= 0 {
            anyhow::bail!(
                "stress_test_metric has invalid captured_at (epoch / unset: {captured_ms})"
            );
        }
        if self.memory_used_mb.unwrap_or(0) == 0
            && self.memory_used_pct.unwrap_or(0.0) <= 0.0
        {
            anyhow::bail!(
                "stress_test_metric has zero memory_used_mb and memory_used_pct (default snapshot)"
            );
        }
        Ok(())
    }

    async fn validate_for_insert(&self) -> anyhow::Result<()> {
        self.validate_shape()?;
        if !StressTestRun::exists(&self.run_ref).await? {
            anyhow::bail!(
                "stress_test_metric run_ref {:?} does not exist",
                self.run_ref
            );
        }
        Ok(())
    }

    /// Time-range scan for one run. Uses the (run_ref, captured_at) index.
    pub async fn list_for_run(
        run_ref: &RecordId,
        start: Option<Datetime>,
        end: Option<Datetime>,
    ) -> anyhow::Result<Vec<Self>> {
        let sql = "SELECT * FROM stress_test_metric \
                   WHERE run_ref = $r \
                     AND captured_at >= ($start ?? d'1970-01-01T00:00:00Z') \
                     AND captured_at <= ($end ?? time::now()) \
                   ORDER BY captured_at ASC";
        let rows: Vec<Self> = db()
            .query(sql)
            .bind(("r", run_ref.clone()))
            .bind(("start", start))
            .bind(("end", end))
            .await?
            .take(0)?;
        Ok(rows)
    }
}

// ============================================================
// Discrete events
// ============================================================

/// Discriminant for `StressTestEvent.kind`. Kept as a small enum so the
/// AI can filter on stable strings.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash, SurrealValue)]
#[serde(rename_all = "snake_case")]
#[surreal(untagged)]
pub enum EventKind {
    /// Stress-kit `ScenarioEvent::StageStarted`.
    #[surreal(value = "stage_started")]
    StageStarted,
    /// Stress-kit `ScenarioEvent::StageFinished`.
    #[surreal(value = "stage_finished")]
    StageFinished,
    /// Stress-kit `Metrics.last_error` populated (disk I/O fault).
    #[surreal(value = "disk_io_error")]
    DiskIoError,
    /// WHEA-Logger event observed since last tick.
    #[surreal(value = "whea_hit")]
    WheaHit,
    /// Display driver TDR (nvlddmkm event 4101 or similar).
    #[surreal(value = "tdr")]
    Tdr,
    /// Thermal throttle threshold crossed.
    #[surreal(value = "thermal_throttle")]
    ThermalThrottle,
    /// VRM / power-limit throttle observed.
    #[surreal(value = "vrm_throttle")]
    VrmThrottle,
    /// Memory test reported a value mismatch.
    #[surreal(value = "memory_error")]
    MemoryError,
    /// Windows kernel bugcheck observed during or shortly after the run.
    #[surreal(value = "bsod")]
    Bsod,
    /// Kernel-Power event 41 (unexpected shutdown / hard hang).
    #[surreal(value = "unexpected_shutdown")]
    UnexpectedShutdown,
    /// Operator-entered note attached to the run timeline.
    #[surreal(value = "operator_note")]
    OperatorNote,
    /// Free-form event with no dedicated variant.
    #[surreal(value = "custom")]
    Custom,
}

impl EventKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::StageStarted => "stage_started",
            Self::StageFinished => "stage_finished",
            Self::DiskIoError => "disk_io_error",
            Self::WheaHit => "whea_hit",
            Self::Tdr => "tdr",
            Self::ThermalThrottle => "thermal_throttle",
            Self::VrmThrottle => "vrm_throttle",
            Self::MemoryError => "memory_error",
            Self::Bsod => "bsod",
            Self::UnexpectedShutdown => "unexpected_shutdown",
            Self::OperatorNote => "operator_note",
            Self::Custom => "custom",
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, SurrealValue)]
pub struct StressTestEvent {
    pub id: RecordId,
    pub run_ref: RecordId,
    pub at: Datetime,
    pub kind: EventKind,
    /// Origin (e.g. "stress-kit", "hwsampler", "windows-event-log",
    /// "operator"). Lets us trust some sources more than others when
    /// reconciling conflicts.
    pub source: String,
    /// Optional vendor-specific code (BSOD bugcheck, WHEA error code,
    /// disk SMART error, etc.).
    pub code: Option<String>,
    pub detail: String,
    pub data: Option<serde_json::Value>,
}

impl StressTestEvent {
    pub fn new(run_ref: RecordId, kind: EventKind, source: impl Into<String>) -> Self {
        Self {
            id: random_record_id(STRESS_TEST_EVENT_TABLE),
            run_ref,
            at: chrono::Utc::now().into(),
            kind,
            source: source.into(),
            code: None,
            detail: String::new(),
            data: None,
        }
    }

    pub async fn create(event: &Self) -> anyhow::Result<RecordId> {
        if !StressTestRun::exists(&event.run_ref).await? {
            anyhow::bail!(
                "stress_test_event run_ref {:?} does not exist",
                event.run_ref
            );
        }
        let value = surreal_create_content(event, false);
        let created: Option<Self> = db()
            .create(event.id.clone())
            .content(value)
            .await?;
        Ok(created.map(|c| c.id).unwrap_or_else(|| event.id.clone()))
    }

    pub async fn list_for_run(run_ref: &RecordId) -> anyhow::Result<Vec<Self>> {
        let rows: Vec<Self> = db()
            .query(
                "SELECT * FROM stress_test_event \
                 WHERE run_ref = $r ORDER BY at ASC",
            )
            .bind(("r", run_ref.clone()))
            .await?
            .take(0)?;
        Ok(rows)
    }
}

// ============================================================
// Materialized baseline (read-only)
// ============================================================

/// One row of the `hardware_test_baseline` materialized view. SurrealDB
/// keeps this up to date automatically when `stress_test_run` rows are
/// written, so we only ever read from it.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, SurrealValue)]
pub struct HardwareTestBaseline {
    pub component: RecordId,
    pub tool: String,
    pub run_count: u64,
    pub pass_count: u64,
    pub fail_count: u64,
    pub abort_count: u64,
    pub avg_max_temp_c: Option<f32>,
    pub peak_max_temp_c: Option<f32>,
    pub avg_temp_c: Option<f32>,
    pub avg_max_clock_mhz: Option<f64>,
    pub avg_clock_mhz: Option<f64>,
    pub avg_max_power_w: Option<f64>,
    pub avg_peak_throughput: Option<f64>,
    pub avg_whea_delta: Option<f64>,
    pub total_whea_delta: Option<u64>,
    pub total_disk_io_errors: Option<u64>,
    pub throttle_count: Option<u64>,
}

impl HardwareTestBaseline {
    /// Population stats for one component across every tool we've run
    /// against it. Useful for "how does this CPU model usually behave?".
    pub async fn for_component(
        component: &RecordId,
    ) -> anyhow::Result<Vec<Self>> {
        let rows: Vec<Self> = db()
            .query(
                "SELECT * FROM hardware_test_baseline \
                 WHERE component = $c",
            )
            .bind(("c", component.clone()))
            .await?
            .take(0)?;
        Ok(rows)
    }

    /// Population stats for one (component, tool) pair — the most
    /// precise comparison for an in-progress run.
    pub async fn for_component_tool(
        component: &RecordId,
        tool_label: &str,
    ) -> anyhow::Result<Option<Self>> {
        let rows: Vec<Self> = db()
            .query(
                "SELECT * FROM hardware_test_baseline \
                 WHERE component = $c AND tool = $t LIMIT 1",
            )
            .bind(("c", component.clone()))
            .bind(("t", tool_label.to_string()))
            .await?
            .take(0)?;
        Ok(rows.into_iter().next())
    }
}
