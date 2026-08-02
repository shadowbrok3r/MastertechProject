//! Certification preset catalog: TOML-defined multi-stage scenarios with
//! attached [`VerdictRules`], mirroring the OCCT stability-certificate tiers.
//!
//! Files embed at compile time; `STRESS_PRESET_DIR` (or `<preset dir>/<name>.toml`)
//! overrides at load time for shop tuning without a rebuild.

use database::schema::{RecordId, TargetKind, TestTool};
use serde::Deserialize;
use stress_kit::Stressor;

use crate::rules::VerdictRules;
use crate::{RunPlan, RunSpec, RunStage};

/// Lookup keys for [`load_cert_preset`], display order.
pub const CERT_PRESET_NAMES: &[&str] = &["bronze", "silver", "gold", "platinum", "power-virus"];

const EMBEDDED: &[(&str, &str)] = &[
    ("bronze", include_str!("../presets/cert-bronze.toml")),
    ("silver", include_str!("../presets/cert-silver.toml")),
    ("gold", include_str!("../presets/cert-gold.toml")),
    ("platinum", include_str!("../presets/cert-platinum.toml")),
    ("power-virus", include_str!("../presets/power-virus.toml")),
];

/// Stage memory sizing: absolute, percent-of-pool, or the stressor default.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MemorySpec {
    Mb(u64),
    PctOfPool(f32),
    Default,
}

/// One resolved certification stage.
#[derive(Debug, Clone)]
pub struct CertStage {
    pub label: String,
    pub stressor: Stressor,
    pub threads: usize,
    pub duration_secs: u64,
    pub memory: MemorySpec,
    pub disk_file_mb: u64,
}

/// One parsed certification preset.
#[derive(Debug, Clone)]
pub struct CertPreset {
    pub name: String,
    pub label: String,
    pub description: String,
    pub target_kind: TargetKind,
    pub tags: Vec<String>,
    pub rules: VerdictRules,
    pub stages: Vec<CertStage>,
}

impl CertPreset {
    /// Planned wall-clock seconds at multiplier 1.0.
    pub fn total_secs(&self) -> u64 {
        self.stages.iter().map(|s| s.duration_secs).sum()
    }
}

#[derive(Deserialize)]
struct PresetFile {
    preset: PresetMeta,
    rules: VerdictRules,
    #[serde(default, rename = "stage")]
    stages: Vec<StageToml>,
}

#[derive(Deserialize)]
struct PresetMeta {
    name: String,
    label: String,
    #[serde(default)]
    description: String,
    target_kind: String,
    #[serde(default)]
    tags: Vec<String>,
}

#[derive(Deserialize)]
struct StageToml {
    label: String,
    stressor: Stressor,
    duration_secs: u64,
    #[serde(default)]
    threads: usize,
    memory_mb: Option<u64>,
    memory_pct: Option<f32>,
    disk_file_mb: Option<u64>,
}

fn parse_target_kind(s: &str) -> anyhow::Result<TargetKind> {
    Ok(match s {
        "cpu" => TargetKind::Cpu,
        "gpu" => TargetKind::Gpu,
        "memory" => TargetKind::Memory,
        "storage" => TargetKind::Storage,
        "psu" => TargetKind::Psu,
        "motherboard" => TargetKind::Motherboard,
        "system" => TargetKind::System,
        "mixed" => TargetKind::Mixed,
        other => anyhow::bail!("unknown target_kind '{other}'"),
    })
}

fn parse_preset(name: &str, raw: &str) -> anyhow::Result<CertPreset> {
    let file: PresetFile = toml::from_str(raw)
        .map_err(|e| anyhow::anyhow!("preset '{name}' failed to parse: {e}"))?;
    if file.stages.is_empty() {
        anyhow::bail!("preset '{name}' has no stages");
    }
    let stages = file
        .stages
        .into_iter()
        .map(|s| {
            let memory = match (s.memory_mb, s.memory_pct) {
                (Some(mb), _) => MemorySpec::Mb(mb),
                (None, Some(pct)) => MemorySpec::PctOfPool(pct),
                (None, None) => MemorySpec::Default,
            };
            CertStage {
                label: s.label,
                stressor: s.stressor,
                threads: s.threads,
                duration_secs: s.duration_secs,
                memory,
                disk_file_mb: s.disk_file_mb.unwrap_or(16),
            }
        })
        .collect();
    Ok(CertPreset {
        name: file.preset.name,
        label: file.preset.label,
        description: file.preset.description,
        target_kind: parse_target_kind(&file.preset.target_kind)?,
        tags: file.preset.tags,
        rules: file.rules,
        stages,
    })
}

/// Load a certification preset by name. A readable
/// `$STRESS_PRESET_DIR/<file>.toml` wins over the embedded copy.
pub fn load_cert_preset(name: &str) -> anyhow::Result<CertPreset> {
    let (key, embedded) = EMBEDDED
        .iter()
        .find(|(k, _)| *k == name)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "unknown cert preset '{name}' (expected one of {})",
                CERT_PRESET_NAMES.join(", ")
            )
        })?;

    if let Ok(dir) = std::env::var("STRESS_PRESET_DIR") {
        let file_name = if *key == "power-virus" {
            "power-virus.toml".to_string()
        } else {
            format!("cert-{key}.toml")
        };
        let path = std::path::Path::new(&dir).join(file_name);
        if let Ok(raw) = std::fs::read_to_string(&path) {
            log::info!("presets: loading '{key}' from override {}", path.display());
            return parse_preset(key, &raw);
        }
    }

    parse_preset(key, embedded)
}

/// MemTest cap: percent of system RAM, leaving a 2 GiB OS floor, min 1 GiB.
fn resolve_ram_pct(pct: f32, total_ram_mb: u64) -> u64 {
    let want = (total_ram_mb as f64 * (pct as f64 / 100.0)) as u64;
    want.min(total_ram_mb.saturating_sub(2048)).max(1024)
}

/// VRAM cap: percent of the largest card's VRAM, 4 GiB fallback when unknown.
fn resolve_vram_pct(pct: f32, gpu_vram_mb: Option<u64>) -> u64 {
    let total = gpu_vram_mb.unwrap_or(4096);
    ((total as f64 * (pct as f64 / 100.0)) as u64).max(256)
}

/// Build the runnable spec: resolves percent memory against the machine's
/// pools and scales stage durations by `mult` (min 1 s, for smoke runs).
pub fn cert_spec(
    preset: &CertPreset,
    computer: RecordId,
    total_ram_mb: u64,
    gpu_vram_mb: Option<u64>,
    mult: f32,
) -> RunSpec {
    let stages = preset
        .stages
        .iter()
        .map(|s| {
            let memory_cap_mb = match s.memory {
                MemorySpec::Mb(mb) => mb,
                MemorySpec::PctOfPool(pct) if s.stressor.is_gpu() => {
                    resolve_vram_pct(pct, gpu_vram_mb)
                }
                MemorySpec::PctOfPool(pct) => resolve_ram_pct(pct, total_ram_mb),
                MemorySpec::Default => 1024,
            };
            RunStage {
                label: s.label.clone(),
                stressor: s.stressor,
                threads: s.threads,
                duration_secs: ((s.duration_secs as f64 * mult as f64).round() as u64).max(1),
                memory_cap_mb,
                disk_file_mb: s.disk_file_mb,
            }
        })
        .collect();

    RunSpec {
        computer,
        tool: TestTool::StressKitScenario {
            name: Some(preset.label.clone()),
        },
        target_kind: preset.target_kind,
        target_component: None,
        touched_components: Vec::new(),
        service_order: None,
        session_ref: None,
        task_ref: None,
        tech: None,
        hostname: None,
        machine_id: None,
        bios_settings: Default::default(),
        driver_versions: Default::default(),
        notes: None,
        preset_label: Some(preset.label.clone()),
        tags: preset.tags.clone(),
        plan: RunPlan::Scenario {
            stages,
            total_wall_secs: None,
            repeat_until_total: false,
        },
        rules: Some(preset.rules.clone()),
    }
}

/// [`cert_spec`] with RAM/VRAM totals sampled from this machine.
pub fn cert_spec_detected(preset: &CertPreset, computer: RecordId, mult: f32) -> RunSpec {
    let snapshot = stress_kit::telemetry::TelemetryAgent::capture_now();
    let total_ram_mb = snapshot.memory.total_mb;
    let gpu_vram_mb = snapshot
        .gpus
        .iter()
        .filter_map(|g| g.memory_total_mb)
        .max();
    cert_spec(preset, computer, total_ram_mb, gpu_vram_mb, mult)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_embedded_presets_parse() {
        for name in CERT_PRESET_NAMES {
            let preset = load_cert_preset(name).expect(name);
            assert_eq!(&preset.name, name);
            assert!(preset.label.starts_with("cert:"), "{}", preset.label);
            assert!(!preset.stages.is_empty());
            assert!(preset.rules.tdr_fails, "cert presets fail on TDR");
        }
    }

    #[test]
    fn tier_durations_match_bands() {
        let hours = |name: &str| load_cert_preset(name).unwrap().total_secs() as f64 / 3600.0;
        let bronze = hours("bronze");
        assert!((1.0..=2.0).contains(&bronze), "bronze {bronze}h");
        let silver = hours("silver");
        assert!((3.0..=4.0).contains(&silver), "silver {silver}h");
        let gold = hours("gold");
        assert!((6.0..=8.5).contains(&gold), "gold {gold}h");
        let platinum = hours("platinum");
        assert!((10.0..=12.5).contains(&platinum), "platinum {platinum}h");
    }

    #[test]
    fn stressor_toml_names_round_trip() {
        for &s in Stressor::all() {
            let name = serde_json::to_value(s).unwrap();
            let raw = format!(
                "label = \"x\"\nstressor = {}\nduration_secs = 1\n",
                serde_json::to_string(&name).unwrap()
            );
            let parsed: StageToml = toml::from_str(&raw).unwrap_or_else(|e| {
                panic!("stressor {s:?} TOML name {name} failed: {e}")
            });
            assert_eq!(parsed.stressor, s);
        }
    }

    #[test]
    fn memory_resolution_math() {
        // 80% of 32 GiB = ~26.2 GiB, under the 30 GiB OS-floor ceiling.
        assert_eq!(resolve_ram_pct(80.0, 32768), 26214);
        // 95% of 4 GiB would exceed total - 2 GiB; clamps to the floor ceiling.
        assert_eq!(resolve_ram_pct(95.0, 4096), 2048);
        // Tiny pool clamps up to the 1 GiB minimum.
        assert_eq!(resolve_ram_pct(10.0, 2048), 1024);
        // VRAM: 80% of 16 GiB; unknown falls back to 4 GiB.
        assert_eq!(resolve_vram_pct(80.0, Some(16384)), 13107);
        assert_eq!(resolve_vram_pct(80.0, None), 3276);
    }

    #[test]
    fn cert_spec_resolves_and_scales() {
        let preset = load_cert_preset("gold").unwrap();
        let spec = cert_spec(
            &preset,
            RecordId::new("computer", "test"),
            32768,
            Some(16384),
            0.01,
        );
        let RunPlan::Scenario { stages, .. } = &spec.plan else {
            panic!("expected scenario plan");
        };
        assert_eq!(stages.len(), preset.stages.len());
        let memtest = stages.iter().find(|s| s.stressor == Stressor::MemTest).unwrap();
        assert_eq!(memtest.memory_cap_mb, 26214);
        assert_eq!(memtest.duration_secs, 54);
        let vram = stages.iter().find(|s| s.stressor == Stressor::GpuVram).unwrap();
        assert_eq!(vram.memory_cap_mb, 13107);
        let linpack = stages.iter().find(|s| s.stressor == Stressor::Linpack).unwrap();
        assert_eq!(linpack.memory_cap_mb, 2048);
        assert!(spec.rules.is_some());
        assert_eq!(spec.preset_label.as_deref(), Some("cert:gold-v1"));
    }
}
