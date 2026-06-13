//! Stress panel: single stressor (optional timeout) or multi-stage scenario.
//!
//! As of the stress-runner integration, all stress runs go through
//! [`stress_runner::RunController`] which owns the stress-kit session, samples
//! the shared `TelemetryAgent`, and persists `stress_test_run` /
//! `stress_test_metric` / `stress_test_event` rows.  The panel just renders
//! and forwards user intent.

use std::sync::Arc;
use std::time::Duration;

use eframe::egui;
use stress_kit::telemetry::TelemetrySnapshot;
use stress_runner::{
    RunController, RunPlan, RunSpec, RunStage, RunUpdate, RunVerdict, Stressor, TelemetryAgent,
    TestTool,
};
use stress_runner::{RecordId, StressKitStressor};

use crate::charts::ChartBoard;

#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq)]
pub enum PanelMode {
    Single,
    Scenario,
    /// Curated 8-stage burn-in shared with the MCP `run_qc_benchmark` tool.
    /// One knob: a duration multiplier. Same `RunController` + persistence
    /// path as the other modes.
    QcBenchmark,
    /// TOML certification presets (Bronze→Platinum, power virus) with
    /// per-stage verdict rules. Shared with the MCP `run_certification` tool.
    Certification,
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
    /// Stored as f32 so the egui slider is happy.
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

/// Serde-friendly mirror of stress-kit's [`Stressor`].  All stress-kit stressors
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
}

impl StressorChoice {
    pub const ALL: [Self; 23] = [
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
        }
    }

    pub fn to_db(self) -> StressKitStressor {
        stress_runner::stressor_to_db(self.to_stressor())
    }

    pub fn throughput_unit(self) -> &'static str {
        self.to_stressor().throughput_unit()
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

    fn to_run_stage(&self) -> RunStage {
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

/// Latest stress-kit throughput tick (mirrors `stress_kit::Metrics` but stored
/// flat so the UI doesn't need to keep an Option around).
#[derive(Default, Clone)]
struct LatestMetrics {
    elapsed_secs: f64,
    throughput: f64,
    last_error: Option<String>,
    throughput_unit: &'static str,
}

/// Scenario run progress.
#[derive(Default)]
struct ScenarioState {
    current_stage_index: usize,
    current_stage_label: String,
    stage_count: usize,
    stage_started_at_elapsed: f64,
    finished: bool,
    finish_label: Option<String>,
    total_elapsed_secs: f64,
}

pub struct StressPanel {
    run: Option<RunController>,
    latest: Option<LatestMetrics>,
    scenario_state: ScenarioState,
    history: Vec<f32>,
    editing_stage: Option<usize>,
    last_run_id: Option<RecordId>,
    last_verdict: Option<RunVerdict>,
    /// True until the user dismisses the last verdict banner.
    show_verdict: bool,
    /// Stressor currently selected in the scenario "add stage" combobox.
    pending_stage_pick: StressorChoice,
    /// Live system telemetry charts (relocated from the hardware monitor).
    /// Fed every frame from the shared `HwSampler` via [`StressPanel::push_telemetry`].
    charts: ChartBoard,
    /// `(service_order, tech)` applied to new runs while an order session is open.
    order_context: Option<(RecordId, String)>,
    /// Preset label of the most recently started run.
    last_preset: Option<String>,
    /// Per-stage rules verdicts for the current/last run, in finish order.
    stage_verdicts: Vec<StageVerdictRow>,
    /// Parsed preset cached for the Certification mode preview.
    cert_preview: Option<stress_runner::CertPreset>,
    /// Last certification start error, shown in the Certification UI.
    cert_error: Option<String>,
    /// Run the operator asked to open in the report view.
    report_request: Option<RecordId>,
}

/// One row of the per-stage verdict table.
#[derive(Clone)]
pub struct StageVerdictRow {
    pub label: String,
    pub pass: bool,
    pub violations: Vec<String>,
    pub peak_throughput: Option<f64>,
}

impl Default for StressPanel {
    fn default() -> Self {
        Self {
            run: None,
            latest: None,
            scenario_state: ScenarioState::default(),
            history: Vec::new(),
            editing_stage: None,
            last_run_id: None,
            last_verdict: None,
            show_verdict: false,
            pending_stage_pick: StressorChoice::Cpu,
            charts: ChartBoard::default(),
            order_context: None,
            last_preset: None,
            stage_verdicts: Vec::new(),
            cert_preview: None,
            cert_error: None,
            report_request: None,
        }
    }
}

impl StressPanel {
    /// Push the latest hardware sampler snapshot into the chart history.
    /// Call once per frame from the host so the bottom-panel charts stay
    /// populated even when the user hasn't started a stress run.
    pub fn push_telemetry(&mut self, snapshot: &TelemetrySnapshot) {
        self.charts.push(snapshot);
    }

    /// Bind/unbind the order context stamped onto new runs.
    pub fn set_order_context(&mut self, ctx: Option<(RecordId, String)>) {
        self.order_context = ctx;
    }

    pub fn last_verdict_ref(&self) -> Option<&RunVerdict> {
        self.last_verdict.as_ref()
    }

    pub fn last_preset(&self) -> Option<String> {
        self.last_preset.clone()
    }

    /// Stamp order linkage onto a run spec before start.
    fn apply_order_context(&self, spec: &mut RunSpec) {
        if let Some((service_order, tech)) = self.order_context.as_ref() {
            spec.service_order = Some(service_order.clone());
            if !tech.is_empty() {
                spec.tech = Some(tech.clone());
            }
        }
    }

    /// Drain controller updates.  Call from the host `update` loop each frame.
    pub fn tick(&mut self, ctx: &egui::Context) {
        let Some(controller) = self.run.as_ref() else {
            return;
        };
        let running = controller.is_running();
        let updates = controller.poll();

        for update in updates {
            self.handle_update(update);
        }

        if !running {
            // Controller marked itself done — drop it so `is_running` reads false.
            // The verdict was already captured on the `Finished` update above.
            self.run = None;
        } else {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn handle_update(&mut self, update: RunUpdate) {
        match update {
            RunUpdate::Started { run_id } => {
                self.last_run_id = Some(run_id);
                self.history.clear();
                self.latest = None;
                self.scenario_state = ScenarioState::default();
                self.stage_verdicts.clear();
            }
            RunUpdate::StageStarted { index, label, stage_count } => {
                let elapsed = self.latest.as_ref().map_or(0.0, |m| m.elapsed_secs);
                self.scenario_state = ScenarioState {
                    current_stage_index: index,
                    current_stage_label: label,
                    stage_count,
                    stage_started_at_elapsed: elapsed,
                    finished: false,
                    finish_label: None,
                    total_elapsed_secs: elapsed,
                };
                self.history.clear();
            }
            RunUpdate::Tick {
                stage_index: _,
                stage_label: _,
                metrics,
                telemetry: _,
                throughput_unit,
            } => {
                self.history.push(metrics.throughput as f32);
                if self.history.len() > 120 {
                    self.history.remove(0);
                }
                self.scenario_state.total_elapsed_secs = metrics.elapsed_secs;
                self.latest = Some(LatestMetrics {
                    elapsed_secs: metrics.elapsed_secs,
                    throughput: metrics.throughput,
                    last_error: metrics.last_error,
                    throughput_unit,
                });
            }
            RunUpdate::StageFinished { .. } => {}
            RunUpdate::StageVerdict {
                label,
                pass,
                violations,
                peak_throughput,
                ..
            } => {
                self.stage_verdicts.push(StageVerdictRow {
                    label,
                    pass,
                    violations,
                    peak_throughput,
                });
            }
            RunUpdate::Finished(verdict) => {
                self.scenario_state.finished = true;
                self.scenario_state.finish_label = Some(verdict_label(&verdict));
                self.scenario_state.total_elapsed_secs = verdict.duration_secs;
                self.last_verdict = Some(verdict);
                self.show_verdict = true;
            }
            RunUpdate::Warning { message } => {
                log::warn!("stress-runner: {message}");
            }
            RunUpdate::Error { message } => {
                log::error!("stress-runner: {message}");
            }
        }
    }

    pub fn is_running(&self) -> bool {
        self.run.as_ref().map(|c| c.is_running()).unwrap_or(false)
    }

    pub fn has_run(&self) -> bool {
        self.run.is_some() || self.last_verdict.is_some()
    }

    fn start_single(
        &mut self,
        cfg: &SingleConfig,
        telemetry: Arc<TelemetryAgent>,
        computer: RecordId,
    ) {
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
        spec.tool = TestTool::StressKit { stressor: cfg.stressor.to_db() };
        spec.preset_label = Some(format!("qc-app:single:{}", cfg.stressor.label()));
        self.apply_order_context(&mut spec);
        self.last_preset = spec.preset_label.clone();
        self.history.clear();
        self.latest = None;
        self.scenario_state = ScenarioState::default();
        self.show_verdict = false;
        self.run = Some(RunController::start(spec, telemetry));
    }

    fn start_scenario(
        &mut self,
        cfg: &ScenarioConfig,
        telemetry: Arc<TelemetryAgent>,
        computer: RecordId,
    ) {
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
            name: Some("qc-app:scenario".to_string()),
        };
        spec.preset_label = Some("qc-app:scenario".to_string());
        self.apply_order_context(&mut spec);
        self.last_preset = spec.preset_label.clone();
        self.history.clear();
        self.latest = None;
        self.scenario_state = ScenarioState::default();
        self.show_verdict = false;
        self.run = Some(RunController::start(spec, telemetry));
    }

    fn stop(&mut self) {
        if let Some(c) = self.run.as_ref() {
            c.stop();
        }
    }

    /// Cancel the active run, if any. Public for fleet/host control.
    pub fn stop_active_run(&mut self) {
        self.stop();
    }

    /// Spin up the curated QC benchmark. Same `RunController` + persistence
    /// path as `start_scenario`; uses the shared `qc_benchmark` recipe so the
    /// MCP and GUI paths can never disagree on what the preset runs.
    fn start_qc_benchmark(
        &mut self,
        cfg: &QcBenchmarkConfig,
        telemetry: Arc<TelemetryAgent>,
        computer: RecordId,
    ) {
        let mult = cfg.duration_multiplier.clamp(0.1, 10.0);
        let stages = crate::qc_benchmark::qc_benchmark_stages(mult);
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
            name: Some(crate::qc_benchmark::QC_BENCHMARK_PRESET.to_string()),
        };
        spec.preset_label = Some(crate::qc_benchmark::QC_BENCHMARK_PRESET.to_string());
        spec.tags = vec!["origin:gui".into(), "preset:qc-benchmark".into()];
        self.apply_order_context(&mut spec);
        self.last_preset = spec.preset_label.clone();
        self.history.clear();
        self.latest = None;
        self.scenario_state = ScenarioState::default();
        self.show_verdict = false;
        self.run = Some(RunController::start(spec, telemetry));
    }

    /// Spin up a certification preset run. Resolves percent-of-pool memory
    /// against the live telemetry snapshot; same `RunController` path as the
    /// other modes.
    fn start_certification(
        &mut self,
        cfg: &CertConfig,
        telemetry: Arc<TelemetryAgent>,
        computer: RecordId,
    ) {
        if let Err(err) =
            self.start_certification_by_name(&cfg.preset_name, cfg.duration_multiplier, telemetry, computer)
        {
            self.cert_error = Some(err);
        }
    }

    /// Start a certification preset by name. Public so fleet commands and the
    /// host app can trigger runs without going through the panel UI.
    pub fn start_certification_by_name(
        &mut self,
        preset_name: &str,
        duration_multiplier: f32,
        telemetry: Arc<TelemetryAgent>,
        computer: RecordId,
    ) -> Result<(), String> {
        if self.is_running() {
            return Err("a stress run is already active".to_string());
        }
        let preset = stress_runner::load_cert_preset(preset_name)
            .map_err(|e| format!("preset '{preset_name}' failed to load: {e:#}"))?;
        let mult = duration_multiplier.clamp(0.001, 1.0);
        let snapshot = telemetry.snapshot();
        let mut spec = if snapshot.memory.total_mb > 0 {
            let gpu_vram_mb = snapshot.gpus.iter().filter_map(|g| g.memory_total_mb).max();
            stress_runner::cert_spec(&preset, computer, snapshot.memory.total_mb, gpu_vram_mb, mult)
        } else {
            stress_runner::cert_spec_detected(&preset, computer, mult)
        };
        spec.tags.push("origin:gui".into());
        self.apply_order_context(&mut spec);
        self.last_preset = spec.preset_label.clone();
        self.cert_error = None;
        self.history.clear();
        self.latest = None;
        self.scenario_state = ScenarioState::default();
        self.show_verdict = false;
        self.run = Some(RunController::start(spec, telemetry));
        Ok(())
    }

    /// Take the operator's pending "view report" request, if any.
    pub fn take_report_request(&mut self) -> Option<RecordId> {
        self.report_request.take()
    }

    /// Stress tab UI.
    ///
    /// Lays out as:
    ///   * `Panel::top`    — mode selector (Single / Scenario / QC Benchmark)
    ///                       plus the Hardware Monitor button.
    ///   * `Panel::bottom` — live system telemetry charts (relocated from
    ///                       the hardware monitor's old `Charts` view).
    ///   * `CentralPanel`  — the mode-specific configuration + run controls
    ///                       + metrics + verdict banner.
    ///
    /// `telemetry` is the shared `TelemetryAgent` (typically from `HwSampler::agent()`).
    /// `computer` is the `RecordId` the run will be persisted against — qc-app
    /// computes this from `reporting::machine_id()`.
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        cfg: &mut StressPanelConfig,
        open_hw_monitor: &mut bool,
        telemetry: Arc<TelemetryAgent>,
        computer: RecordId,
    ) {
        let running = self.is_running();

        egui::Panel::top("stress_panel_top").show_inside(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add_enabled_ui(!running, |ui| {
                    ui.selectable_value(&mut cfg.mode, PanelMode::Single, "Single stressor");
                    ui.selectable_value(&mut cfg.mode, PanelMode::Scenario, "Scenario");
                    ui.selectable_value(&mut cfg.mode, PanelMode::QcBenchmark, "QC Benchmark");
                    ui.selectable_value(&mut cfg.mode, PanelMode::Certification, "Certification");
                });
                ui.separator();
                if ui.button("Hardware Monitor").clicked() {
                    *open_hw_monitor = true;
                }
                if let Some(id) = &self.last_run_id {
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(format!("run: {}", id.key_string_pretty()))
                            .weak()
                            .small()
                            .monospace(),
                    );
                }
            });
            ui.add_space(4.0);
        });

        egui::Panel::bottom("stress_panel_bottom")
            .resizable(true)
            .default_size(300.0)
            .show_inside(ui, |ui| {
                self.charts.show(ui);
            });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                match cfg.mode {
                    PanelMode::Single => {
                        self.ui_single(ui, cfg, running, telemetry.clone(), computer.clone())
                    }
                    PanelMode::Scenario => {
                        self.ui_scenario(ui, cfg, running, telemetry.clone(), computer.clone())
                    }
                    PanelMode::QcBenchmark => {
                        self.ui_qc_benchmark(ui, cfg, running, telemetry, computer)
                    }
                    PanelMode::Certification => {
                        self.ui_certification(ui, cfg, running, telemetry, computer)
                    }
                }

                if matches!(
                    cfg.mode,
                    PanelMode::Scenario | PanelMode::QcBenchmark | PanelMode::Certification
                ) {
                    self.ui_stage_verdicts(ui);
                }

                if self.show_verdict {
                    if let Some(v) = self.last_verdict.clone() {
                        self.ui_verdict_banner(ui, &v);
                    }
                }
            });
        });
    }

    fn ui_single(
        &mut self,
        ui: &mut egui::Ui,
        cfg: &mut StressPanelConfig,
        running: bool,
        telemetry: Arc<TelemetryAgent>,
        computer: RecordId,
    ) {
        let s = &mut cfg.single;

        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.label("Stressor");
                ui.add_enabled_ui(!running, |ui| {
                    egui::ComboBox::from_id_salt("single_stressor")
                        .selected_text(s.stressor.label())
                        .show_ui(ui, |ui| {
                            for choice in StressorChoice::ALL {
                                ui.selectable_value(&mut s.stressor, choice, choice.label());
                            }
                        });
                });
            });

            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.label("Worker threads");
                let suffix = if s.threads == 0 { " (auto)" } else { "" };
                ui.add_enabled(
                    !running,
                    egui::DragValue::new(&mut s.threads).range(0..=64).suffix(suffix),
                );
                ui.label(egui::RichText::new("0 = logical CPU count").weak().small());
            });

            match s.stressor {
                StressorChoice::Memory
                | StressorChoice::Memcpy
                | StressorChoice::Vm
                | StressorChoice::MemTest
                | StressorChoice::Linpack => {
                    ui.horizontal(|ui| {
                        ui.label("Memory cap (MiB)");
                        ui.add_enabled(
                            !running,
                            egui::DragValue::new(&mut s.memory_cap_mb).range(16..=32768),
                        );
                    });
                }
                StressorChoice::Disk => {
                    ui.horizontal(|ui| {
                        ui.label("File size (MiB)");
                        ui.add_enabled(
                            !running,
                            egui::DragValue::new(&mut s.disk_file_mb).range(1..=512),
                        );
                    });
                }
                // Pure CPU/cache/memory-bandwidth stressors have no extra knobs.
                StressorChoice::Cpu
                | StressorChoice::Matrix
                | StressorChoice::Bitops
                | StressorChoice::Cache
                | StressorChoice::Stream
                | StressorChoice::Branch
                | StressorChoice::Atomic
                | StressorChoice::Mutex
                | StressorChoice::Switch
                | StressorChoice::Prime
                | StressorChoice::Fp
                | StressorChoice::Hash
                | StressorChoice::Prefetch
                | StressorChoice::Icache
                | StressorChoice::Tsc
                | StressorChoice::CpuVerify
                | StressorChoice::Psu => {}
            }

            ui.horizontal(|ui| {
                ui.add_enabled(!running, |ui: &mut egui::Ui| {
                    ui.checkbox(&mut s.use_timeout, "Timeout")
                });
                if s.use_timeout {
                    ui.add_enabled(
                        !running,
                        egui::DragValue::new(&mut s.timeout_secs).range(1..=3600).suffix(" s"),
                    );
                }
            });
        });

        // Clone before moving into the start closure so we can still use
        // `cfg` afterwards for the metric header label.
        let single_clone = cfg.single.clone();
        let computer_clone = computer.clone();
        self.ui_start_stop(ui, running, move |panel| {
            panel.start_single(&single_clone, telemetry, computer_clone);
        });

        self.ui_metrics(ui, cfg.single.stressor.throughput_unit());
    }

    fn ui_scenario(
        &mut self,
        ui: &mut egui::Ui,
        cfg: &mut StressPanelConfig,
        running: bool,
        telemetry: Arc<TelemetryAgent>,
        computer: RecordId,
    ) {
        ui.group(|ui| {
            ui.label(egui::RichText::new("Stages (run in order)").strong());
            ui.add_space(4.0);

            let mut swap: Option<(usize, usize)> = None;
            let mut remove: Option<usize> = None;
            let n = cfg.scenario.stages.len();

            for i in 0..n {
                let is_editing = self.editing_stage == Some(i);
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(!running && i > 0, |ui| {
                        if ui.small_button("▲").clicked() {
                            swap = Some((i - 1, i));
                        }
                    });
                    ui.add_enabled_ui(!running && i + 1 < n, |ui| {
                        if ui.small_button("▼").clicked() {
                            swap = Some((i, i + 1));
                        }
                    });

                    ui.add_space(4.0);

                    ui.add_enabled_ui(!running, |ui| {
                        let selected = cfg.scenario.stages[i].stressor.label();
                        egui::ComboBox::from_id_salt(format!("stage_stressor_{i}"))
                            .selected_text(selected)
                            .width(90.0)
                            .show_ui(ui, |ui| {
                                for choice in StressorChoice::ALL {
                                    ui.selectable_value(
                                        &mut cfg.scenario.stages[i].stressor,
                                        choice,
                                        choice.label(),
                                    );
                                }
                            });
                    });

                    ui.add_enabled(
                        !running,
                        egui::TextEdit::singleline(&mut cfg.scenario.stages[i].label)
                            .desired_width(100.0),
                    );

                    ui.label("for");
                    ui.add_enabled(
                        !running,
                        egui::DragValue::new(&mut cfg.scenario.stages[i].duration_secs)
                            .range(1..=3600)
                            .suffix(" s"),
                    );

                    let btn_label = if is_editing { "▾" } else { "▸" };
                    if ui.small_button(btn_label).clicked() {
                        self.editing_stage = if is_editing { None } else { Some(i) };
                    }

                    ui.add_enabled_ui(!running && n > 1, |ui| {
                        if ui.small_button("✕").on_hover_text("Remove stage").clicked() {
                            remove = Some(i);
                        }
                    });
                });

                if is_editing {
                    ui.indent(format!("stage_opts_{i}"), |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Threads");
                            let suffix = if cfg.scenario.stages[i].threads == 0 { " (auto)" } else { "" };
                            ui.add_enabled(
                                !running,
                                egui::DragValue::new(&mut cfg.scenario.stages[i].threads)
                                    .range(0..=64)
                                    .suffix(suffix),
                            );
                        });
                        match cfg.scenario.stages[i].stressor {
                            StressorChoice::Memory | StressorChoice::Memcpy | StressorChoice::Vm => {
                                ui.horizontal(|ui| {
                                    ui.label("Memory cap (MiB)");
                                    ui.add_enabled(
                                        !running,
                                        egui::DragValue::new(
                                            &mut cfg.scenario.stages[i].memory_cap_mb,
                                        )
                                        .range(16..=32768),
                                    );
                                });
                            }
                            StressorChoice::Disk => {
                                ui.horizontal(|ui| {
                                    ui.label("File size (MiB)");
                                    ui.add_enabled(
                                        !running,
                                        egui::DragValue::new(
                                            &mut cfg.scenario.stages[i].disk_file_mb,
                                        )
                                        .range(1..=512),
                                    );
                                });
                            }
                            _ => {}
                        }
                    });
                }
            }

            if let Some((a, b)) = swap {
                cfg.scenario.stages.swap(a, b);
            }
            if let Some(idx) = remove {
                cfg.scenario.stages.remove(idx);
                if self.editing_stage == Some(idx) {
                    self.editing_stage = None;
                }
            }

            ui.add_space(4.0);
            if !running {
                ui.horizontal(|ui| {
                    ui.label("Add stage");
                    egui::ComboBox::from_id_salt("scenario_add_stage")
                        .selected_text(self.pending_stage_pick.label())
                        .show_ui(ui, |ui| {
                            for choice in StressorChoice::ALL {
                                ui.selectable_value(
                                    &mut self.pending_stage_pick,
                                    choice,
                                    choice.label(),
                                );
                            }
                        });
                    if ui.button("+ Add").clicked() {
                        let stage = match self.pending_stage_pick {
                            StressorChoice::Cpu => ScenarioStageConfig::default_cpu(),
                            StressorChoice::Memory => ScenarioStageConfig::default_memory(),
                            StressorChoice::Disk => ScenarioStageConfig::default_disk(),
                            other => ScenarioStageConfig {
                                label: other.label().into(),
                                stressor: other,
                                threads: 0,
                                duration_secs: 60,
                                memory_cap_mb: 256,
                                disk_file_mb: 16,
                            },
                        };
                        cfg.scenario.stages.push(stage);
                    }
                });
            }
        });

        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.add_enabled(!running, |ui: &mut egui::Ui| {
                    ui.checkbox(&mut cfg.scenario.use_total, "Total wall time")
                });
                if cfg.scenario.use_total {
                    ui.add_enabled(
                        !running,
                        egui::DragValue::new(&mut cfg.scenario.total_wall_secs)
                            .range(1..=86400)
                            .suffix(" s"),
                    );
                    ui.add_space(8.0);
                    ui.add_enabled(!running, |ui: &mut egui::Ui| {
                        ui.checkbox(&mut cfg.scenario.repeat_until_total, "Repeat until total")
                    });
                }
            });
        });

        // Clone before moving into the start closure so we can still read
        // `cfg` afterwards for the progress + metrics rendering.
        let scenario_clone = cfg.scenario.clone();
        let computer_clone = computer.clone();
        self.ui_start_stop(ui, running, move |panel| {
            panel.start_scenario(&scenario_clone, telemetry, computer_clone);
        });

        if running || (self.has_run() && cfg.mode == PanelMode::Scenario) {
            self.ui_scenario_progress(ui, cfg);
        }

        let stage_idx = self.scenario_state.current_stage_index;
        let unit = cfg
            .scenario
            .stages
            .get(stage_idx)
            .map(|s| s.stressor.throughput_unit())
            .unwrap_or("ops/s");
        self.ui_metrics(ui, unit);
    }

    /// QC Benchmark mode. Just the duration multiplier slider + a Start
    /// button — every stage is hard-coded by the shared `qc_benchmark` recipe.
    fn ui_qc_benchmark(
        &mut self,
        ui: &mut egui::Ui,
        cfg: &mut StressPanelConfig,
        running: bool,
        telemetry: Arc<TelemetryAgent>,
        computer: RecordId,
    ) {
        let mult = cfg.qc_benchmark.duration_multiplier.clamp(0.1, 10.0);
        let total_secs = (mult * 20.0 * 8.0).round() as u64;

        ui.group(|ui| {
            ui.label(egui::RichText::new("QC Benchmark v1").strong());
            ui.label(
                egui::RichText::new(
                    "8-stage burn-in: cpu, matrix, fp, stream, cache, branch, memory, vm. \
                     Shared with the MCP `run_qc_benchmark` tool — same recipe, same persistence.",
                )
                .small()
                .weak(),
            );
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("Duration multiplier");
                ui.add_enabled(
                    !running,
                    egui::Slider::new(&mut cfg.qc_benchmark.duration_multiplier, 0.1..=4.0)
                        .step_by(0.05)
                        .suffix("×"),
                );
                ui.label(
                    egui::RichText::new(format!("≈ {total_secs} s total"))
                        .small()
                        .weak(),
                );
            });
            ui.label(
                egui::RichText::new(
                    "1.0× ≈ 20 s/stage (~2.7 min). 0.25× ≈ 40 s smoke. 4.0× ≈ 11 min.",
                )
                .small()
                .weak(),
            );
        });

        let bench_clone = cfg.qc_benchmark.clone();
        let computer_clone = computer.clone();
        self.ui_start_stop(ui, running, move |panel| {
            panel.start_qc_benchmark(&bench_clone, telemetry, computer_clone);
        });

        if running || (self.has_run() && cfg.mode == PanelMode::QcBenchmark) {
            self.ui_scenario_progress(ui, cfg);
        }

        let stage_idx = self.scenario_state.current_stage_index;
        let unit = crate::qc_benchmark::qc_benchmark_stages(mult)
            .get(stage_idx)
            .map(|s| s.stressor.throughput_unit())
            .unwrap_or("ops/s");
        self.ui_metrics(ui, unit);
    }

    fn ui_certification(
        &mut self,
        ui: &mut egui::Ui,
        cfg: &mut StressPanelConfig,
        running: bool,
        telemetry: Arc<TelemetryAgent>,
        computer: RecordId,
    ) {
        let cert = &mut cfg.certification;
        let mult = cert.duration_multiplier.clamp(0.001, 1.0);

        // Re-parse the preview only when the selection changes.
        if self.cert_preview.as_ref().map(|p| p.name.as_str()) != Some(cert.preset_name.as_str()) {
            self.cert_preview = stress_runner::load_cert_preset(&cert.preset_name).ok();
        }

        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.label("Preset");
                ui.add_enabled_ui(!running, |ui| {
                    egui::ComboBox::from_id_salt("cert_preset")
                        .selected_text(&cert.preset_name)
                        .show_ui(ui, |ui| {
                            for name in stress_runner::CERT_PRESET_NAMES {
                                ui.selectable_value(
                                    &mut cert.preset_name,
                                    name.to_string(),
                                    *name,
                                );
                            }
                        });
                });
            });

            if let Some(preset) = &self.cert_preview {
                ui.label(egui::RichText::new(&preset.description).small().weak());
                let total = (preset.total_secs() as f64 * mult as f64).round() as u64;
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label("Duration multiplier");
                    ui.add_enabled(
                        !running,
                        egui::Slider::new(&mut cert.duration_multiplier, 0.001..=1.0)
                            .logarithmic(true)
                            .suffix("×"),
                    );
                    ui.label(
                        egui::RichText::new(format!(
                            "≈ {:.1} min total{}",
                            total as f64 / 60.0,
                            if mult < 1.0 { "  (dev smoke below 1.0×)" } else { "" }
                        ))
                        .small()
                        .weak(),
                    );
                });

                ui.add_space(6.0);
                egui::Grid::new("cert_stage_preview")
                    .num_columns(3)
                    .spacing([16.0, 2.0])
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("Stage").small().strong());
                        ui.label(egui::RichText::new("Stressor").small().strong());
                        ui.label(egui::RichText::new("Duration").small().strong());
                        ui.end_row();
                        for s in &preset.stages {
                            let secs =
                                ((s.duration_secs as f64 * mult as f64).round() as u64).max(1);
                            ui.label(egui::RichText::new(&s.label).small());
                            ui.label(egui::RichText::new(s.stressor.label()).small());
                            ui.label(
                                egui::RichText::new(format!("{:.1} min", secs as f64 / 60.0))
                                    .small()
                                    .monospace(),
                            );
                            ui.end_row();
                        }
                    });
            } else {
                ui.colored_label(
                    egui::Color32::from_rgb(200, 60, 60),
                    format!("preset '{}' failed to load", cert.preset_name),
                );
            }
        });

        if let Some(err) = self.cert_error.clone() {
            ui.colored_label(egui::Color32::from_rgb(200, 60, 60), err);
        }

        let cert_clone = cfg.certification.clone();
        let computer_clone = computer.clone();
        self.ui_start_stop(ui, running, move |panel| {
            panel.start_certification(&cert_clone, telemetry, computer_clone);
        });

        if running || (self.has_run() && cfg.mode == PanelMode::Certification) {
            self.ui_cert_progress(ui, mult);
        }

        let unit = self
            .cert_preview
            .as_ref()
            .and_then(|p| p.stages.get(self.scenario_state.current_stage_index))
            .map(|s| s.stressor.throughput_unit())
            .unwrap_or("ops/s");
        self.ui_metrics(ui, unit);
    }

    /// Stage progress bar against the preset's scaled stage durations.
    fn ui_cert_progress(&self, ui: &mut egui::Ui, mult: f32) {
        let ss = &self.scenario_state;
        if ss.finished {
            let label = ss.finish_label.clone().unwrap_or_else(|| "done".to_string());
            ui.label(
                egui::RichText::new(format!("{label}  —  {:.1} s total", ss.total_elapsed_secs))
                    .strong(),
            );
            return;
        }
        if ss.stage_count == 0 {
            return;
        }
        let stage_elapsed = ss.total_elapsed_secs - ss.stage_started_at_elapsed;
        let stage_dur = self
            .cert_preview
            .as_ref()
            .and_then(|p| p.stages.get(ss.current_stage_index))
            .map(|s| (s.duration_secs as f64 * mult as f64).max(1.0))
            .unwrap_or(1.0);
        let progress = (stage_elapsed / stage_dur).clamp(0.0, 1.0) as f32;
        ui.add(
            egui::ProgressBar::new(progress)
                .text(format!(
                    "Stage {}/{}: {}  ({:.0} / {:.0} s)",
                    ss.current_stage_index + 1,
                    ss.stage_count,
                    ss.current_stage_label,
                    stage_elapsed,
                    stage_dur,
                ))
                .animate(true),
        );
    }

    /// Per-stage rules verdict table; empty until a rules-carrying run
    /// finishes its first stage.
    fn ui_stage_verdicts(&self, ui: &mut egui::Ui) {
        if self.stage_verdicts.is_empty() {
            return;
        }
        use egui_phosphor::regular as p;
        ui.add_space(6.0);
        ui.group(|ui| {
            ui.label(egui::RichText::new("Stage verdicts").strong());
            egui::Grid::new("stage_verdicts_grid")
                .num_columns(3)
                .spacing([14.0, 2.0])
                .striped(true)
                .show(ui, |ui| {
                    for row in &self.stage_verdicts {
                        if row.pass {
                            ui.colored_label(
                                egui::Color32::from_rgb(50, 160, 90),
                                format!("{} pass", p::CHECK_CIRCLE),
                            );
                        } else {
                            ui.colored_label(
                                egui::Color32::from_rgb(200, 60, 60),
                                format!("{} fail", p::X_CIRCLE),
                            );
                        }
                        ui.label(&row.label);
                        let detail = if row.violations.is_empty() {
                            row.peak_throughput
                                .map(|t| format!("peak {t:.1}"))
                                .unwrap_or_default()
                        } else {
                            row.violations.join("; ")
                        };
                        ui.label(egui::RichText::new(detail).small().weak());
                        ui.end_row();
                    }
                });
        });
    }

    fn ui_scenario_progress(&self, ui: &mut egui::Ui, cfg: &StressPanelConfig) {
        let ss = &self.scenario_state;
        if ss.finished {
            let label = ss.finish_label.clone().unwrap_or_else(|| "done".to_string());
            ui.label(
                egui::RichText::new(format!(
                    "{label}  —  {:.1} s total",
                    ss.total_elapsed_secs
                ))
                .strong(),
            );
            return;
        }

        if ss.stage_count == 0 {
            return;
        }

        let stage_elapsed = ss.total_elapsed_secs - ss.stage_started_at_elapsed;
        let stage_dur = cfg
            .scenario
            .stages
            .get(ss.current_stage_index)
            .map(|s| s.duration_secs as f64)
            .unwrap_or(1.0)
            .max(1.0);
        let stage_progress = (stage_elapsed / stage_dur).clamp(0.0, 1.0) as f32;

        let label = format!(
            "Stage {}/{}: {}  ({:.0} / {:.0} s)",
            ss.current_stage_index + 1,
            ss.stage_count,
            ss.current_stage_label,
            stage_elapsed,
            stage_dur,
        );

        ui.add(
            egui::ProgressBar::new(stage_progress)
                .text(label)
                .animate(true),
        );

        if cfg.scenario.use_total && cfg.scenario.total_wall_secs > 0 {
            let overall = (ss.total_elapsed_secs / cfg.scenario.total_wall_secs as f64)
                .clamp(0.0, 1.0) as f32;
            ui.add(
                egui::ProgressBar::new(overall)
                    .text(format!(
                        "Overall  {:.0}/{} s",
                        ss.total_elapsed_secs, cfg.scenario.total_wall_secs
                    ))
                    .desired_width(ui.available_width()),
            );
        }
    }

    fn ui_start_stop(&mut self, ui: &mut egui::Ui, running: bool, start_fn: impl FnOnce(&mut Self)) {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if running {
                if ui
                    .add(egui::Button::new("⏹  Stop").fill(egui::Color32::from_rgb(180, 60, 60)))
                    .clicked()
                {
                    self.stop();
                }
                ui.add(egui::Spinner::new());
                ui.label("Running…");
            } else {
                if ui
                    .add(egui::Button::new("▶  Start").fill(egui::Color32::from_rgb(50, 140, 80)))
                    .clicked()
                {
                    start_fn(self);
                }
                if self.has_run() && !running {
                    ui.label(egui::RichText::new("Stopped").weak());
                }
            }
        });
        ui.add_space(4.0);
    }

    fn ui_metrics(&self, ui: &mut egui::Ui, unit: &str) {
        let Some(ref m) = self.latest else { return };

        egui::Grid::new("stress_metrics")
            .num_columns(2)
            .spacing([16.0, 4.0])
            .show(ui, |ui| {
                ui.label("Elapsed");
                ui.label(egui::RichText::new(format!("{:.1} s", m.elapsed_secs)).monospace());
                ui.end_row();

                ui.label("Throughput");
                ui.label(
                    egui::RichText::new(format!("{:.2} {}", m.throughput, m.throughput_unit))
                        .monospace()
                        .strong(),
                );
                ui.end_row();

                if let Some(ref e) = m.last_error {
                    ui.label("Warning");
                    ui.colored_label(egui::Color32::YELLOW, e);
                    ui.end_row();
                }
            });

        if self.history.len() > 1 {
            ui.add_space(6.0);
            let max = self
                .history
                .iter()
                .cloned()
                .fold(f32::NEG_INFINITY, f32::max)
                .max(1.0);
            let width = ui.available_width().min(480.0);
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(width, 40.0), egui::Sense::empty());
            if ui.is_rect_visible(rect) {
                let painter = ui.painter_at(rect);
                painter.rect_filled(rect, 2.0, egui::Color32::from_gray(30));
                let n = self.history.len();
                let step = rect.width() / (n - 1) as f32;
                let points: Vec<egui::Pos2> = self
                    .history
                    .iter()
                    .enumerate()
                    .map(|(i, &v)| {
                        egui::pos2(
                            rect.left() + i as f32 * step,
                            rect.bottom() - (v / max) * rect.height(),
                        )
                    })
                    .collect();
                for w in points.windows(2) {
                    painter.line_segment(
                        [w[0], w[1]],
                        egui::Stroke::new(1.5, egui::Color32::from_rgb(100, 200, 100)),
                    );
                }
            }
            ui.label(
                egui::RichText::new(format!("peak {:.2} {unit}", max))
                    .small()
                    .weak(),
            );
        }
    }

    fn ui_verdict_banner(&mut self, ui: &mut egui::Ui, v: &RunVerdict) {
        ui.add_space(8.0);
        let (text, color) = match v.result {
            stress_runner::RunResult::Pass => (
                format!("PASS  —  {:.1} s", v.duration_secs),
                egui::Color32::from_rgb(50, 160, 90),
            ),
            stress_runner::RunResult::Fail => (
                format!("FAIL  ({})  —  {:.1} s", v.failure_mode_label(), v.duration_secs),
                egui::Color32::from_rgb(200, 60, 60),
            ),
            stress_runner::RunResult::Aborted => (
                format!("ABORTED  —  {:.1} s", v.duration_secs),
                egui::Color32::from_rgb(180, 140, 50),
            ),
            stress_runner::RunResult::Inconclusive => (
                format!("INCONCLUSIVE  —  {:.1} s", v.duration_secs),
                egui::Color32::from_rgb(160, 160, 160),
            ),
            stress_runner::RunResult::InProgress => (
                "IN PROGRESS".to_string(),
                egui::Color32::from_rgb(120, 160, 220),
            ),
        };
        ui.horizontal(|ui| {
            ui.colored_label(color, egui::RichText::new(text).strong());
            if ui.small_button("View report").clicked() {
                self.report_request = Some(v.run_id.clone());
            }
            if ui.small_button("Dismiss").clicked() {
                self.show_verdict = false;
            }
        });
    }
}

fn verdict_label(v: &RunVerdict) -> String {
    match v.result {
        stress_runner::RunResult::Pass => "Pass".to_string(),
        stress_runner::RunResult::Fail => format!("Fail ({})", v.failure_mode_label()),
        stress_runner::RunResult::Aborted => "Aborted".to_string(),
        stress_runner::RunResult::Inconclusive => "Inconclusive".to_string(),
        stress_runner::RunResult::InProgress => "In progress".to_string(),
    }
}

/// Small helper trait so we can pretty-print RecordIds for the header label.
trait RecordIdPretty {
    fn key_string_pretty(&self) -> String;
}

impl RecordIdPretty for RecordId {
    fn key_string_pretty(&self) -> String {
        // We don't have a `Display` impl on `RecordId` to extract the table
        // prefix, but every run id is on the `stress_test_run` table — just
        // shorten the key portion.
        use database::schema::RecordIdExt;
        short(&self.key_string(), 12)
    }
}

fn short(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

/// Tiny extension on RunVerdict so the panel can ask "what's the failure mode?"
/// without owning the FailureMode enum directly.
trait VerdictPretty {
    fn failure_mode_label(&self) -> &'static str;
}

impl VerdictPretty for RunVerdict {
    fn failure_mode_label(&self) -> &'static str {
        use stress_runner::FailureMode;
        match &self.failure_mode {
            FailureMode::None => "none",
            FailureMode::AppError { .. } => "app error",
            FailureMode::Bsod { .. } => "BSOD",
            FailureMode::Tdr { .. } => "TDR",
            FailureMode::GpuDeviceLost { .. } => "GPU device lost",
            FailureMode::WheaError { .. } => "WHEA",
            FailureMode::ThermalThrottle { .. } => "thermal throttle",
            FailureMode::DiskIoError { .. } => "disk I/O",
            FailureMode::DataMismatch { .. } => "data mismatch",
            FailureMode::ClockCollapse { .. } => "clock collapse",
            FailureMode::ThroughputUnstable { .. } => "unstable throughput",
            FailureMode::Reboot => "reboot",
            FailureMode::Timeout => "timeout",
            FailureMode::OperatorOverride { .. } => "operator override",
        }
    }
}
