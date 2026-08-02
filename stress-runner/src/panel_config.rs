//! UI-agnostic stress configuration shared by every renderer.
//!
//! These types describe *what the operator wants to run* — the mode, the
//! per-mode knobs, and the stressor picker — independent of egui or ratatui.
//! Both qc-app and Mastertech4.0 (egui + terminal) build their `RunSpec` from
//! the same [`StressPanelConfig`] here so the two front ends can never disagree
//! on what a given mode actually executes.
//!
//! The `*_spec` builders turn a config into a [`RunSpec`]; a host stamps its
//! own [`StressRunContext`] (service order, tech, provenance) afterward.

use crate::qc_benchmark::{qc_benchmark_stages, QC_BENCHMARK_PRESET};
use crate::{
    cert_spec, cert_spec_detected, load_cert_preset, RecordId, RunPlan, RunSpec, RunStage,
    Stressor, TargetKind, TelemetrySnapshot, TestTool,
};

/// The five stress modes, mirrored 1:1 across every renderer.
#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq, Debug)]
pub enum PanelMode {
    Single,
    Scenario,
    /// Curated 8-stage burn-in shared with the MCP `run_qc_benchmark` tool.
    QcBenchmark,
    /// TOML certification presets (Bronze→Platinum, power virus) with
    /// per-stage verdict rules.
    Certification,
    /// Multiple stressors at once, each its own lane. Drives `RunPlan::Concurrent`.
    Concurrent,
}

impl Default for PanelMode {
    fn default() -> Self {
        Self::Single
    }
}

/// Persisted stress tab state.
#[derive(serde::Serialize, serde::Deserialize, Clone, Default)]
pub struct StressPanelConfig {
    pub mode: PanelMode,
    pub single: SingleConfig,
    pub scenario: ScenarioConfig,
    #[serde(default)]
    pub qc_benchmark: QcBenchmarkConfig,
    #[serde(default)]
    pub certification: CertConfig,
    #[serde(default)]
    pub concurrent: ConcurrentConfig,
}

/// Persisted state for the Concurrent mode: which lanes run at once + a shared duration.
#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct ConcurrentConfig {
    pub lanes: Vec<StressorChoice>,
    pub duration_secs: u64,
    pub use_timeout: bool,
    pub memory_cap_mb: u64,
    pub disk_file_mb: u64,
}

impl Default for ConcurrentConfig {
    fn default() -> Self {
        Self {
            lanes: vec![StressorChoice::Cpu, StressorChoice::Memory, StressorChoice::Gpu],
            duration_secs: 120,
            use_timeout: true,
            memory_cap_mb: 1024,
            disk_file_mb: 64,
        }
    }
}

/// Persisted state for the Certification mode.
#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct CertConfig {
    pub preset_name: String,
    /// 1.0 = full certification durations; tiny values are dev smoke runs.
    pub duration_multiplier: f32,
}

impl Default for CertConfig {
    fn default() -> Self {
        Self {
            preset_name: "bronze".to_string(),
            duration_multiplier: 1.0,
        }
    }
}

/// Persisted state for the QC Benchmark mode (just the duration multiplier).
#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct QcBenchmarkConfig {
    /// Multiplier applied to every stage's base duration (default 20 s).
    pub duration_multiplier: f32,
}

impl Default for QcBenchmarkConfig {
    fn default() -> Self {
        Self {
            duration_multiplier: 1.0,
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct SingleConfig {
    pub stressor: StressorChoice,
    pub threads: usize,
    pub timeout_secs: u64,
    pub memory_cap_mb: u64,
    pub disk_file_mb: u64,
    pub use_timeout: bool,
}

impl Default for SingleConfig {
    fn default() -> Self {
        Self {
            stressor: StressorChoice::Cpu,
            threads: 0,
            timeout_secs: 60,
            memory_cap_mb: 256,
            disk_file_mb: 16,
            use_timeout: false,
        }
    }
}

/// Serde-friendly mirror of stress-kit's [`Stressor`]. All stress-kit stressors
/// are exposed here; the panel picks sensible defaults per kind.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum StressorChoice {
    Cpu,
    Memory,
    Disk,
    Matrix,
    Memcpy,
    Bitops,
    Cache,
    Vm,
    Stream,
    Branch,
    Atomic,
    Mutex,
    Switch,
    Prime,
    Fp,
    Hash,
    Prefetch,
    Icache,
    Tsc,
    MemTest,
    CpuVerify,
    Linpack,
    Psu,
    PsuTransient,
    Combined,
    Gpu,
    GpuMatmul,
    GpuVram,
    GpuPcie,
    GpuDisplay,
}

impl StressorChoice {
    pub const ALL: [Self; 30] = [
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
        Self::PsuTransient,
        Self::Combined,
        Self::Gpu,
        Self::GpuMatmul,
        Self::GpuVram,
        Self::GpuPcie,
        Self::GpuDisplay,
    ];

    pub fn label(self) -> &'static str {
        self.to_stressor().label()
    }

    pub fn to_stressor(self) -> Stressor {
        match self {
            Self::Cpu => Stressor::Cpu,
            Self::Memory => Stressor::Memory,
            Self::Disk => Stressor::Disk,
            Self::Matrix => Stressor::Matrix,
            Self::Memcpy => Stressor::Memcpy,
            Self::Bitops => Stressor::Bitops,
            Self::Cache => Stressor::Cache,
            Self::Vm => Stressor::Vm,
            Self::Stream => Stressor::Stream,
            Self::Branch => Stressor::Branch,
            Self::Atomic => Stressor::Atomic,
            Self::Mutex => Stressor::Mutex,
            Self::Switch => Stressor::Switch,
            Self::Prime => Stressor::Prime,
            Self::Fp => Stressor::Fp,
            Self::Hash => Stressor::Hash,
            Self::Prefetch => Stressor::Prefetch,
            Self::Icache => Stressor::Icache,
            Self::Tsc => Stressor::Tsc,
            Self::MemTest => Stressor::MemTest,
            Self::CpuVerify => Stressor::CpuVerify,
            Self::Linpack => Stressor::Linpack,
            Self::Psu => Stressor::Psu,
            Self::PsuTransient => Stressor::PsuTransient,
            Self::Combined => Stressor::Combined,
            Self::Gpu => Stressor::Gpu,
            Self::GpuMatmul => Stressor::GpuMatmul,
            Self::GpuVram => Stressor::GpuVram,
            Self::GpuPcie => Stressor::GpuPcie,
            Self::GpuDisplay => Stressor::GpuDisplay,
        }
    }

    pub fn to_db(self) -> String {
        self.to_stressor().as_str().to_string()
    }

    pub fn throughput_unit(self) -> &'static str {
        self.to_stressor().throughput_unit()
    }

    /// `true` for GPU-backed stressors (gated behind stress-kit's `gpu` feature).
    pub fn is_gpu(self) -> bool {
        self.to_stressor().is_gpu()
    }
}

/// Scenario-mode fields.
#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct ScenarioConfig {
    pub stages: Vec<ScenarioStageConfig>,
    pub total_wall_secs: u64,
    pub use_total: bool,
    pub repeat_until_total: bool,
}

impl Default for ScenarioConfig {
    fn default() -> Self {
        Self {
            stages: vec![
                ScenarioStageConfig::default_cpu(),
                ScenarioStageConfig::default_memory(),
            ],
            total_wall_secs: 300,
            use_total: false,
            repeat_until_total: false,
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct ScenarioStageConfig {
    pub label: String,
    pub stressor: StressorChoice,
    pub threads: usize,
    pub duration_secs: u64,
    pub memory_cap_mb: u64,
    pub disk_file_mb: u64,
}

impl ScenarioStageConfig {
    pub fn default_cpu() -> Self {
        Self {
            label: "CPU".into(),
            stressor: StressorChoice::Cpu,
            threads: 0,
            duration_secs: 60,
            memory_cap_mb: 256,
            disk_file_mb: 16,
        }
    }
    pub fn default_memory() -> Self {
        Self {
            label: "Memory".into(),
            stressor: StressorChoice::Memory,
            threads: 0,
            duration_secs: 60,
            memory_cap_mb: 512,
            disk_file_mb: 16,
        }
    }
    pub fn default_disk() -> Self {
        Self {
            label: "Disk I/O".into(),
            stressor: StressorChoice::Disk,
            threads: 2,
            duration_secs: 30,
            memory_cap_mb: 256,
            disk_file_mb: 32,
        }
    }

    pub fn to_run_stage(&self) -> RunStage {
        RunStage {
            label: self.label.clone(),
            stressor: self.stressor.to_stressor(),
            threads: self.threads,
            duration_secs: self.duration_secs,
            memory_cap_mb: self.memory_cap_mb,
            disk_file_mb: self.disk_file_mb,
        }
    }
}

// ---------------------------------------------------------------------------
// RunSpec builders
// ---------------------------------------------------------------------------

/// Host-supplied provenance stamped onto a freshly built [`RunSpec`].
///
/// `source` prefixes the `preset_label` (e.g. `"qc-app"` / `"mtech"`) so runs
/// keep their originating app's namespace; `origin` becomes an `origin:<x>` tag
/// (`"gui"` / `"tui"`). Order/session context links the run to open work.
#[derive(Debug, Clone, Default)]
pub struct StressRunContext {
    pub source: String,
    pub origin: String,
    /// `(service_order, tech)` applied while an order session is open.
    pub order_context: Option<(RecordId, String)>,
    pub session_ref: Option<RecordId>,
    pub task_ref: Option<RecordId>,
}

impl StressRunContext {
    pub fn new(source: impl Into<String>, origin: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            origin: origin.into(),
            ..Default::default()
        }
    }

    /// Stamp order/session/tech context and an `origin:<x>` tag onto a spec.
    pub fn apply(&self, spec: &mut RunSpec) {
        if let Some((service_order, tech)) = &self.order_context {
            spec.service_order = Some(service_order.clone());
            spec.tech = Some(tech.clone());
        }
        if let Some(session_ref) = &self.session_ref {
            spec.session_ref = Some(session_ref.clone());
        }
        if let Some(task_ref) = &self.task_ref {
            spec.task_ref = Some(task_ref.clone());
        }
        if !self.origin.is_empty() {
            let tag = format!("origin:{}", self.origin);
            if !spec.tags.contains(&tag) {
                spec.tags.push(tag);
            }
        }
    }

    fn source_or(&self, fallback: &str) -> String {
        if self.source.is_empty() {
            fallback.to_string()
        } else {
            self.source.clone()
        }
    }
}

/// Single-stressor run spec (no context applied).
pub fn single_spec(cfg: &SingleConfig, computer: RecordId, source: &str) -> RunSpec {
    let stressor = cfg.stressor.to_stressor();
    let plan = RunPlan::Single {
        stressor,
        threads: cfg.threads,
        duration_secs: if cfg.use_timeout && cfg.timeout_secs > 0 {
            Some(cfg.timeout_secs)
        } else {
            None
        },
        memory_cap_mb: cfg.memory_cap_mb,
        disk_file_mb: cfg.disk_file_mb,
    };
    let mut spec = RunSpec::single_stresskit(computer, stressor, None);
    spec.plan = plan;
    spec.tool = TestTool::StressKit {
        stressor: cfg.stressor.to_db(),
    };
    spec.preset_label = Some(format!("{source}:single:{}", cfg.stressor.label()));
    spec
}

/// Multi-stage scenario run spec (no context applied).
pub fn scenario_spec(cfg: &ScenarioConfig, computer: RecordId, source: &str) -> RunSpec {
    let stages: Vec<RunStage> = cfg.stages.iter().map(|s| s.to_run_stage()).collect();
    let plan = RunPlan::Scenario {
        stages: stages.clone(),
        total_wall_secs: if cfg.use_total && cfg.total_wall_secs > 0 {
            Some(cfg.total_wall_secs)
        } else {
            None
        },
        repeat_until_total: cfg.repeat_until_total,
    };
    let mut spec = RunSpec::single_stresskit(
        computer,
        stages.first().map(|s| s.stressor).unwrap_or(Stressor::Cpu),
        None,
    );
    spec.plan = plan;
    spec.tool = TestTool::StressKitScenario {
        name: Some(format!("{source}:scenario")),
    };
    spec.preset_label = Some(format!("{source}:scenario"));
    spec
}

/// Curated QC benchmark run spec (no context applied).
pub fn qc_benchmark_spec(cfg: &QcBenchmarkConfig, computer: RecordId) -> RunSpec {
    let mult = cfg.duration_multiplier.clamp(0.1, 10.0);
    let stages = qc_benchmark_stages(mult);
    let plan = RunPlan::Scenario {
        stages: stages.clone(),
        total_wall_secs: None,
        repeat_until_total: false,
    };
    let mut spec = RunSpec::single_stresskit(
        computer,
        stages.first().map(|s| s.stressor).unwrap_or(Stressor::Cpu),
        None,
    );
    spec.plan = plan;
    spec.tool = TestTool::StressKitScenario {
        name: Some(QC_BENCHMARK_PRESET.to_string()),
    };
    spec.preset_label = Some(QC_BENCHMARK_PRESET.to_string());
    spec.tags = vec!["preset:qc-benchmark".into()];
    spec
}

/// Certification preset run spec (no context applied). Resolves percent-of-pool
/// memory against `snapshot` when present, else detects the machine's pools.
pub fn certification_spec(
    preset_name: &str,
    duration_multiplier: f32,
    computer: RecordId,
    snapshot: Option<&TelemetrySnapshot>,
) -> Result<RunSpec, String> {
    let preset = load_cert_preset(preset_name)
        .map_err(|e| format!("preset '{preset_name}' failed to load: {e:#}"))?;
    let mult = duration_multiplier.clamp(0.001, 1.0);
    let spec = match snapshot {
        Some(s) if s.memory.total_mb > 0 => {
            let gpu_vram_mb = s.gpus.iter().filter_map(|g| g.memory_total_mb).max();
            cert_spec(&preset, computer, s.memory.total_mb, gpu_vram_mb, mult)
        }
        _ => cert_spec_detected(&preset, computer, mult),
    };
    Ok(spec)
}

/// Concurrent multi-lane run spec (no context applied). `None` if no lanes.
pub fn concurrent_spec(cfg: &ConcurrentConfig, computer: RecordId, source: &str) -> Option<RunSpec> {
    if cfg.lanes.is_empty() {
        return None;
    }
    let duration = if cfg.use_timeout && cfg.duration_secs > 0 {
        Some(cfg.duration_secs)
    } else {
        None
    };
    let lanes: Vec<RunStage> = cfg
        .lanes
        .iter()
        .map(|c| RunStage {
            label: c.label().to_string(),
            stressor: c.to_stressor(),
            threads: 0,
            duration_secs: cfg.duration_secs,
            memory_cap_mb: cfg.memory_cap_mb,
            disk_file_mb: cfg.disk_file_mb,
        })
        .collect();
    // Seed off Combined so the run's target_kind is System (whole-system).
    let mut spec = RunSpec::single_stresskit(computer, Stressor::Combined, None);
    spec.plan = RunPlan::Concurrent { lanes, duration_secs: duration };
    spec.tool = TestTool::StressKitScenario {
        name: Some(format!("{source}:concurrent")),
    };
    spec.preset_label = Some(format!("{source}:concurrent"));
    spec.tags = vec!["preset:concurrent".into()];
    Some(spec)
}

/// Build the `RunSpec` for the config's active mode and stamp `ctx` onto it.
///
/// `snapshot` supplies live RAM/VRAM pools for certification memory scaling.
/// Returns an operator-facing message on the modes that can refuse (empty
/// concurrent lanes, unloadable certification preset).
pub fn build_run_spec(
    cfg: &StressPanelConfig,
    computer: RecordId,
    snapshot: Option<&TelemetrySnapshot>,
    ctx: &StressRunContext,
) -> Result<RunSpec, String> {
    let source = ctx.source_or("stress");
    let mut spec = match cfg.mode {
        PanelMode::Single => single_spec(&cfg.single, computer, &source),
        PanelMode::Scenario => scenario_spec(&cfg.scenario, computer, &source),
        PanelMode::QcBenchmark => qc_benchmark_spec(&cfg.qc_benchmark, computer),
        PanelMode::Certification => certification_spec(
            &cfg.certification.preset_name,
            cfg.certification.duration_multiplier,
            computer,
            snapshot,
        )?,
        PanelMode::Concurrent => concurrent_spec(&cfg.concurrent, computer, &source)
            .ok_or_else(|| "no lanes selected for concurrent run".to_string())?,
    };
    ctx.apply(&mut spec);
    Ok(spec)
}

/// Which stressor `TargetKind::default` a choice maps to, exposed for UI hints.
pub fn target_kind_for(choice: StressorChoice) -> TargetKind {
    crate::default_target_kind(choice.to_stressor())
}
