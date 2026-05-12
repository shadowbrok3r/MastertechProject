//! JSON DTOs for orchestrator + MCP. No UI types. `schema_version` on top-level values: bump minor for additive fields, major for breaks.

use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: &str = "1.0";

/// One logical CPU core's current state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoreSnapshot {
    pub index: usize,
    pub brand: String,
    pub name: String,
    pub usage_pct: f32,
    pub freq_mhz: u64,
    /// Often `None` on Windows without a sensor stack `sysinfo` can read.
    pub temp_c: Option<f32>,
}

/// Instantaneous hardware state of the machine.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HwSnapshot {
    /// Individual logical-core readings.
    pub cores: Vec<CoreSnapshot>,
    /// Mean usage across all cores (0–100).
    pub avg_usage_pct: f32,
    /// Peak usage across all cores (0–100).
    pub peak_usage_pct: f32,
    /// Mean over cores that reported a temperature.
    pub avg_temp_c: Option<f32>,
}

/// Which stress scenario was run.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StressScenario {
    Cpu,
    Memory,
    Disk,
    Combined,
    Custom(String),
}

/// Final outcome of a completed stress run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StressResult {
    pub scenario: StressScenario,
    pub duration_secs: u64,
    pub peak_usage_pct: f32,
    pub peak_temp_c: Option<f32>,
    /// Operator-set pass/fail.
    pub passed: Option<bool>,
    pub notes: Option<String>,
}

/// Full report payload for the orchestrator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QcReport {
    pub schema_version: String,
    /// Stable id (see `reporting::machine_id`).
    pub machine_id: String,
    /// `CARGO_PKG_VERSION`.
    pub agent_version: String,
    /// UTC `YYYY-MM-DDTHH:MM:SSZ` (no chrono dep).
    pub reported_at: String,
    /// Hardware snapshot at the time of the report.
    pub hw: HwSnapshot,
    /// Most recent completed stress result, if any.
    pub last_stress: Option<StressResult>,
    /// Arbitrary string tags.
    pub tags: std::collections::HashMap<String, String>,
}

impl QcReport {
    pub fn new(machine_id: impl Into<String>, hw: HwSnapshot) -> Self {
        Self {
            schema_version: SCHEMA_VERSION.to_string(),
            machine_id: machine_id.into(),
            agent_version: env!("CARGO_PKG_VERSION").to_string(),
            reported_at: chrono_now_utc(),
            hw,
            last_stress: None,
            tags: Default::default(),
        }
    }
}

/// Lightweight liveness; no full `HwSnapshot`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Heartbeat {
    pub schema_version: String,
    pub machine_id: String,
    pub agent_version: String,
    pub sent_at: String,
    /// Average CPU % at send time.
    pub cpu_avg_pct: f32,
}

impl Heartbeat {
    pub fn new(machine_id: impl Into<String>, cpu_avg_pct: f32) -> Self {
        Self {
            schema_version: SCHEMA_VERSION.to_string(),
            machine_id: machine_id.into(),
            agent_version: env!("CARGO_PKG_VERSION").to_string(),
            sent_at: chrono_now_utc(),
            cpu_avg_pct,
        }
    }
}

fn chrono_now_utc() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    // UTC wall time; no `chrono` dependency.
    let (y, mo, d, h, mi, s) = epoch_to_parts(secs);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

fn epoch_to_parts(mut secs: u64) -> (u64, u64, u64, u64, u64, u64) {
    let s = secs % 60; secs /= 60;
    let mi = secs % 60; secs /= 60;
    let h = secs % 24; secs /= 24;
    // Gregorian from Unix days; OK through 2099.
    let z = secs + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    (y, mo, d, h, mi, s)
}

impl From<&crate::hw_sampler::CoreRow> for CoreSnapshot {
    fn from(r: &crate::hw_sampler::CoreRow) -> Self {
        Self {
            index: r.index,
            brand: r.brand.clone(),
            name: r.name.clone(),
            usage_pct: r.usage_pct,
            freq_mhz: r.freq_mhz,
            temp_c: r.temp_c,
        }
    }
}

impl HwSnapshot {
    pub fn from_cores(cores: &[crate::hw_sampler::CoreRow]) -> Self {
        let snaps: Vec<CoreSnapshot> = cores.iter().map(CoreSnapshot::from).collect();
        let avg_usage_pct = if snaps.is_empty() {
            0.0
        } else {
            snaps.iter().map(|c| c.usage_pct).sum::<f32>() / snaps.len() as f32
        };
        let peak_usage_pct = snaps
            .iter()
            .map(|c| c.usage_pct)
            .fold(0.0f32, f32::max);
        let temps: Vec<f32> = snaps.iter().filter_map(|c| c.temp_c).collect();
        let avg_temp_c = if temps.is_empty() {
            None
        } else {
            Some(temps.iter().sum::<f32>() / temps.len() as f32)
        };
        Self { cores: snaps, avg_usage_pct, peak_usage_pct, avg_temp_c }
    }
}
