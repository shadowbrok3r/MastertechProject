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
use egui_phosphor::regular as p;
use stress_kit::telemetry::{TelemetrySnapshot, ThermalReading};
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
    /// Collapsible right-hand temperature panel toggle.
    temps_open: bool,
    /// Latest device temps for the side panel (CPU/board + storage).
    latest_thermals: Vec<ThermalReading>,
    /// Latest per-GPU temps `(name, °C)` for the side panel.
    latest_gpu_temps: Vec<(String, f32)>,
    /// Pre-run connectivity gate state.
    start_requested: bool,
    pending_start: bool,
    conn_prompt: bool,
    run_offline: bool,
    conn_probe: Option<crossbeam::channel::Receiver<bool>>,
}

/// One row of the per-stage verdict table.
#[derive(Clone)]
pub struct StageVerdictRow {
    pub label: String,
    pub pass: bool,
    pub violations: Vec<String>,
    pub peak_throughput: Option<f64>,
}

/// Per-stage progress state for the live stage grid.
#[derive(Clone, Copy)]
enum StageProg {
    Pending,
    Running(f32),
    Done { pass: Option<bool> },
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
            temps_open: true,
            latest_thermals: Vec::new(),
            latest_gpu_temps: Vec::new(),
            start_requested: false,
            pending_start: false,
            conn_prompt: false,
            run_offline: false,
            conn_probe: None,
        }
    }
}

impl StressPanel {
    /// Push the latest hardware sampler snapshot into the chart history.
    /// Call once per frame from the host so the bottom-panel charts stay
    /// populated even when the user hasn't started a stress run.
    pub fn push_telemetry(&mut self, snapshot: &TelemetrySnapshot) {
        self.charts.push(snapshot);
        self.latest_thermals = snapshot.thermals.clone();
        self.latest_gpu_temps = snapshot
            .gpus
            .iter()
            .filter_map(|g| g.temp_c.map(|t| (g.name.clone(), t)))
            .collect();
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
                if self.run_offline {
                    self.queue_offline_result(&verdict);
                }
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

    /// Probe orchestrator reachability; result drives the pre-run gate.
    fn start_probe(&mut self) {
        let (tx, rx) = crossbeam::channel::bounded::<bool>(1);
        self.conn_probe = Some(rx);
        let url = database::orchestrator_url().to_string();
        tokio::spawn(async move {
            let ok = if url.is_empty() {
                false
            } else {
                match reqwest::Client::builder().timeout(Duration::from_secs(3)).build() {
                    Ok(c) => c.get(&url).send().await.is_ok(),
                    Err(_) => false,
                }
            };
            let _ = tx.send(ok);
        });
    }

    /// Start the run for the active mode.
    fn dispatch_start(
        &mut self,
        cfg: &StressPanelConfig,
        telemetry: Arc<TelemetryAgent>,
        computer: RecordId,
    ) {
        if self.is_running() {
            return;
        }
        match cfg.mode {
            PanelMode::Single => self.start_single(&cfg.single, telemetry, computer),
            PanelMode::Scenario => self.start_scenario(&cfg.scenario, telemetry, computer),
            PanelMode::QcBenchmark => self.start_qc_benchmark(&cfg.qc_benchmark, telemetry, computer),
            PanelMode::Certification => self.start_certification(&cfg.certification, telemetry, computer),
        }
    }

    /// Poll the connectivity probe and render the offline prompt; dispatches a
    /// pending start once the gate resolves.
    fn drive_conn_gate(
        &mut self,
        ui: &mut egui::Ui,
        cfg: &StressPanelConfig,
        telemetry: &Arc<TelemetryAgent>,
        computer: &RecordId,
    ) {
        if let Some(rx) = self.conn_probe.as_ref() {
            if let Ok(online) = rx.try_recv() {
                self.conn_probe = None;
                if online {
                    self.run_offline = false;
                    if self.pending_start {
                        self.pending_start = false;
                        self.dispatch_start(cfg, telemetry.clone(), computer.clone());
                    }
                } else {
                    self.conn_prompt = true;
                }
            }
        }
        if !self.conn_prompt {
            return;
        }
        let ctx = ui.ctx().clone();
        let mut decision: Option<u8> = None;
        egui::Window::new("Not connected")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(&ctx, |ui| {
                ui.label("The orchestrator/database is unreachable. Results can't be uploaded right now.");
                ui.horizontal(|ui| {
                    if ui.button("Connect to Wi-Fi").clicked() {
                        decision = Some(0);
                    }
                    if ui.button("Continue offline (save to disk)").clicked() {
                        decision = Some(1);
                    }
                    if ui.button("Cancel").clicked() {
                        decision = Some(2);
                    }
                });
            });
        match decision {
            Some(0) => {
                let _ = crate::provisioning::osconfig::open_wifi_settings();
                self.conn_prompt = false;
                self.start_probe();
            }
            Some(1) => {
                self.run_offline = true;
                self.conn_prompt = false;
                if self.pending_start {
                    self.pending_start = false;
                    self.dispatch_start(cfg, telemetry.clone(), computer.clone());
                }
            }
            Some(2) => {
                self.conn_prompt = false;
                self.pending_start = false;
            }
            _ => {}
        }
    }

    /// Queue a completed run's result to disk for later upload.
    fn queue_offline_result(&self, verdict: &RunVerdict) {
        let result = crate::telemetry::StressResult {
            scenario: crate::telemetry::StressScenario::Custom(
                self.last_preset.clone().unwrap_or_else(|| "stress".into()),
            ),
            duration_secs: verdict.duration_secs as u64,
            peak_usage_pct: 0.0,
            peak_temp_c: verdict.summary.max_temp_c,
            passed: Some(matches!(verdict.result, stress_runner::RunResult::Pass)),
            notes: Some(format!("offline-queued; mode={}", verdict.failure_mode_label())),
        };
        let mut report = crate::telemetry::QcReport::new(
            crate::reporting::machine_id(),
            crate::telemetry::HwSnapshot::default(),
        );
        report.last_stress = Some(result);
        match serde_json::to_value(&report) {
            Ok(v) => match crate::pending_results::save(&v) {
                Ok(path) => log::info!("stress: offline result queued at {}", path.display()),
                Err(e) => log::error!("stress: failed to queue offline result: {e}"),
            },
            Err(e) => log::error!("stress: serialize offline result: {e}"),
        }
    }

    /// Stress tab UI.
    ///
    /// Lays out as:
    ///   * `Panel::top`       — mode selector + Hardware Monitor + Temps toggle,
    ///                          with the Start/Stop control right-aligned.
    ///   * `Panel::bottom`    — live system telemetry charts (incl. temps).
    ///   * `SidePanel::right` — collapsible, scrollable device-temperature list.
    ///   * `CentralPanel`     — mode-specific config + stage grid + metrics.
    ///
    /// `telemetry` is the shared `TelemetryAgent`; `computer` is the run's owner.
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
                ui.toggle_value(&mut self.temps_open, "Telemetry");
                if let Some(id) = &self.last_run_id {
                    ui.label(
                        egui::RichText::new(format!("run: {}", id.key_string_pretty()))
                            .weak()
                            .small()
                            .monospace(),
                    );
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    self.ui_top_start_stop(ui, running);
                });
            });
            ui.add_space(4.0);
        });

        egui::SidePanel::right("stress_panel_side")
            .resizable(true)
            .default_width(440.0)
            .show_animated_inside(ui, self.temps_open, |ui| {
                self.ui_side_panel(ui);
            });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                match cfg.mode {
                    PanelMode::Single => self.ui_single(ui, cfg, running),
                    PanelMode::Scenario => self.ui_scenario(ui, cfg, running),
                    PanelMode::QcBenchmark => self.ui_qc_benchmark(ui, cfg, running),
                    PanelMode::Certification => self.ui_certification(ui, cfg, running),
                }

                if self.show_verdict {
                    if let Some(v) = self.last_verdict.clone() {
                        self.ui_verdict_banner(ui, &v);
                    }
                }
            });
        });

        // Pre-run connectivity gate (probe → prompt → dispatch).
        if self.start_requested {
            self.start_requested = false;
            if database::orchestrator_url().is_empty() {
                self.pending_start = true;
                self.conn_prompt = true;
            } else if self.conn_probe.is_none() {
                self.pending_start = true;
                self.start_probe();
            }
        }
        self.drive_conn_gate(ui, cfg, &telemetry, &computer);
    }

    fn ui_single(&mut self, ui: &mut egui::Ui, cfg: &mut StressPanelConfig, running: bool) {
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

        self.ui_metrics(ui, cfg.single.stressor.throughput_unit());
    }

    fn ui_scenario(&mut self, ui: &mut egui::Ui, cfg: &mut StressPanelConfig, running: bool) {
        let stage_idx = self.scenario_state.current_stage_index;
        let unit = cfg
            .scenario
            .stages
            .get(stage_idx)
            .map(|s| s.stressor.throughput_unit())
            .unwrap_or("ops/s");

        ui.columns(2, |cols| {
            {
                let ui = &mut cols[0];
                ui.group(|ui| {
                    ui.label(egui::RichText::new("Stages (run in order)").strong());
                    ui.add_space(4.0);

                    let mut swap: Option<(usize, usize)> = None;
                    let mut remove: Option<usize> = None;
                    let n = cfg.scenario.stages.len();

                    for i in 0..n {
                        let is_editing = self.editing_stage == Some(i);
                        let prog =
                            self.stage_progress_for(i, cfg.scenario.stages[i].duration_secs as f64);
                        ui.horizontal(|ui| {
                            ui.add_enabled_ui(!running && i > 0, |ui| {
                                if ui.small_button(p::CARET_UP).clicked() {
                                    swap = Some((i - 1, i));
                                }
                            });
                            ui.add_enabled_ui(!running && i + 1 < n, |ui| {
                                if ui.small_button(p::CARET_DOWN).clicked() {
                                    swap = Some((i, i + 1));
                                }
                            });

                            ui.add_enabled_ui(!running, |ui| {
                                let selected = cfg.scenario.stages[i].stressor.label();
                                egui::ComboBox::from_id_salt(format!("stage_stressor_{i}"))
                                    .selected_text(selected)
                                    .width(80.0)
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
                                    .desired_width(80.0),
                            );

                            ui.add_enabled(
                                !running,
                                egui::DragValue::new(&mut cfg.scenario.stages[i].duration_secs)
                                    .range(1..=3600)
                                    .suffix(" s"),
                            );

                            let btn_label =
                                if is_editing { p::CARET_DOWN } else { p::CARET_RIGHT };
                            if ui.small_button(btn_label).clicked() {
                                self.editing_stage = if is_editing { None } else { Some(i) };
                            }

                            ui.add_enabled_ui(!running && n > 1, |ui| {
                                if ui.small_button(p::X).on_hover_text("Remove stage").clicked() {
                                    remove = Some(i);
                                }
                            });

                            Self::stage_progress_cell(ui, prog);
                        });

                        if is_editing {
                            ui.indent(format!("stage_opts_{i}"), |ui| {
                                ui.horizontal(|ui| {
                                    ui.label("Threads");
                                    let suffix = if cfg.scenario.stages[i].threads == 0 {
                                        " (auto)"
                                    } else {
                                        ""
                                    };
                                    ui.add_enabled(
                                        !running,
                                        egui::DragValue::new(&mut cfg.scenario.stages[i].threads)
                                            .range(0..=64)
                                            .suffix(suffix),
                                    );
                                });
                                match cfg.scenario.stages[i].stressor {
                                    StressorChoice::Memory
                                    | StressorChoice::Memcpy
                                    | StressorChoice::Vm => {
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
                                ui.checkbox(
                                    &mut cfg.scenario.repeat_until_total,
                                    "Repeat until total",
                                )
                            });
                        }
                    });
                });

                if running && cfg.scenario.use_total && cfg.scenario.total_wall_secs > 0 {
                    let overall = (self.scenario_state.total_elapsed_secs
                        / cfg.scenario.total_wall_secs as f64)
                        .clamp(0.0, 1.0) as f32;
                    ui.add(
                        egui::ProgressBar::new(overall)
                            .text(format!(
                                "Overall  {:.0}/{} s",
                                self.scenario_state.total_elapsed_secs,
                                cfg.scenario.total_wall_secs
                            ))
                            .animate(true),
                    );
                }
            }
            {
                let ui = &mut cols[1];
                self.ui_run_status_column(ui, unit);
            }
        });
    }

    /// QC Benchmark mode. Just the duration multiplier slider + a Start
    /// button — every stage is hard-coded by the shared `qc_benchmark` recipe.
    fn ui_qc_benchmark(&mut self, ui: &mut egui::Ui, cfg: &mut StressPanelConfig, running: bool) {
        let mult = cfg.qc_benchmark.duration_multiplier.clamp(0.1, 10.0);
        let total_secs = (mult * 20.0 * 8.0).round() as u64;

        let stages = crate::qc_benchmark::qc_benchmark_stages(mult);
        let stage_rows: Vec<(String, &'static str, f64)> = stages
            .iter()
            .map(|s| (s.label.clone(), s.stressor.label(), (s.duration_secs as f64).max(1.0)))
            .collect();
        let unit = stages
            .get(self.scenario_state.current_stage_index)
            .map(|s| s.stressor.throughput_unit())
            .unwrap_or("ops/s");

        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("QC Benchmark v1").strong());
                ui.label(
                    egui::RichText::new("cpu · matrix · fp · stream · cache · branch · memory · vm")
                        .small()
                        .weak(),
                )
                .on_hover_text("8-stage burn-in shared with the MCP `run_qc_benchmark` tool.");
            });
            ui.add_space(4.0);
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
        });
        ui.add_space(6.0);
        self.ui_stage_grid(ui, "qc_bench_stage_grid", &stage_rows);
        ui.add_space(6.0);
        self.ui_run_status_column(ui, unit);
    }

    fn ui_certification(&mut self, ui: &mut egui::Ui, cfg: &mut StressPanelConfig, running: bool) {
        let mult = cfg.certification.duration_multiplier.clamp(0.001, 1.0);

        // Re-parse the preview only when the selection changes.
        if self.cert_preview.as_ref().map(|p| p.name.as_str())
            != Some(cfg.certification.preset_name.as_str())
        {
            self.cert_preview = stress_runner::load_cert_preset(&cfg.certification.preset_name).ok();
        }

        if let Some(err) = self.cert_error.clone() {
            ui.colored_label(egui::Color32::from_rgb(200, 60, 60), err);
        }

        let stage_rows: Vec<(String, &'static str, f64)> = self
            .cert_preview
            .as_ref()
            .map(|p| {
                p.stages
                    .iter()
                    .map(|s| {
                        (
                            s.label.clone(),
                            s.stressor.label(),
                            (s.duration_secs as f64 * mult as f64).max(1.0),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let description = self.cert_preview.as_ref().map(|p| p.description.clone());
        let total_secs = self
            .cert_preview
            .as_ref()
            .map(|p| (p.total_secs() as f64 * mult as f64).round() as u64)
            .unwrap_or(0);
        let unit = self
            .cert_preview
            .as_ref()
            .and_then(|p| p.stages.get(self.scenario_state.current_stage_index))
            .map(|s| s.stressor.throughput_unit())
            .unwrap_or("ops/s");

        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.label("Preset");
                ui.add_enabled_ui(!running, |ui| {
                    let combo = egui::ComboBox::from_id_salt("cert_preset")
                        .selected_text(&cfg.certification.preset_name)
                        .show_ui(ui, |ui| {
                            for name in stress_runner::CERT_PRESET_NAMES {
                                ui.selectable_value(
                                    &mut cfg.certification.preset_name,
                                    name.to_string(),
                                    *name,
                                );
                            }
                        });
                    if let Some(desc) = &description {
                        combo.response.on_hover_text(desc.as_str());
                    }
                });
                ui.separator();
                ui.label("Duration");
                ui.add_enabled(
                    !running,
                    egui::Slider::new(&mut cfg.certification.duration_multiplier, 0.001..=1.0)
                        .logarithmic(true)
                        .suffix("×"),
                );
                ui.label(
                    egui::RichText::new(format!("≈ {:.1} min", total_secs as f64 / 60.0))
                        .small()
                        .weak(),
                );
            });
        });

        if stage_rows.is_empty() {
            ui.colored_label(
                egui::Color32::from_rgb(200, 60, 60),
                format!("preset '{}' failed to load", cfg.certification.preset_name),
            );
        } else {
            ui.add_space(6.0);
            self.ui_stage_grid(ui, "cert_stage_grid", &stage_rows);
            ui.add_space(6.0);
            self.ui_run_status_column(ui, unit);
        }
    }

    /// Per-stage progress derived from the live scenario state + verdicts.
    fn stage_progress_for(&self, index: usize, stage_dur_secs: f64) -> StageProg {
        let ss = &self.scenario_state;
        if ss.finished || index < ss.current_stage_index {
            return StageProg::Done {
                pass: self.stage_verdicts.get(index).map(|v| v.pass),
            };
        }
        if self.is_running() && index == ss.current_stage_index && ss.stage_count > 0 {
            let elapsed = (ss.total_elapsed_secs - ss.stage_started_at_elapsed).max(0.0);
            let frac = (elapsed / stage_dur_secs.max(1.0)).clamp(0.0, 1.0) as f32;
            return StageProg::Running(frac);
        }
        StageProg::Pending
    }

    /// Render one stage's progress cell.
    fn stage_progress_cell(ui: &mut egui::Ui, prog: StageProg) {
        match prog {
            StageProg::Done { pass } => match pass {
                Some(true) => {
                    ui.colored_label(
                        egui::Color32::from_rgb(50, 160, 90),
                        format!("{} pass", p::CHECK_CIRCLE),
                    );
                }
                Some(false) => {
                    ui.colored_label(
                        egui::Color32::from_rgb(200, 60, 60),
                        format!("{} fail", p::X_CIRCLE),
                    );
                }
                None => {
                    ui.colored_label(
                        egui::Color32::from_rgb(120, 160, 120),
                        format!("{} done", p::CHECK),
                    );
                }
            },
            StageProg::Running(frac) => {
                ui.add(
                    egui::ProgressBar::new(frac)
                        .desired_width(130.0)
                        .animate(true)
                        .text(format!("{:.0}%", frac * 100.0)),
                );
            }
            StageProg::Pending => {
                ui.label(egui::RichText::new("—").weak());
            }
        }
    }

    /// Read-only stage list with a per-stage progress column.
    fn ui_stage_grid(
        &self,
        ui: &mut egui::Ui,
        grid_id: &str,
        stages: &[(String, &'static str, f64)],
    ) {
        egui::Grid::new(grid_id)
            .num_columns(5)
            .spacing([14.0, 3.0])
            .striped(true)
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Stage").small().strong());
                ui.label(egui::RichText::new("Stressor").small().strong());
                ui.label(egui::RichText::new("Duration").small().strong());
                ui.label(egui::RichText::new("Progress").small().strong());
                ui.label(egui::RichText::new("Peak").small().strong());
                ui.end_row();
                for (i, (label, stressor, dur)) in stages.iter().enumerate() {
                    ui.label(egui::RichText::new(label).small());
                    ui.label(egui::RichText::new(*stressor).small());
                    ui.label(
                        egui::RichText::new(format!("{:.1} min", dur / 60.0))
                            .small()
                            .monospace(),
                    );
                    let prog = self.stage_progress_for(i, *dur);
                    let resp = ui.scope(|ui| Self::stage_progress_cell(ui, prog)).response;
                    if let Some(v) = self.stage_verdicts.get(i) {
                        if !v.pass && !v.violations.is_empty() {
                            resp.on_hover_text(v.violations.join("\n"));
                        }
                    }
                    match self.stage_verdicts.get(i).and_then(|v| v.peak_throughput) {
                        Some(peak) => {
                            ui.label(egui::RichText::new(format!("{peak:.1}")).small().monospace());
                        }
                        None => {
                            ui.label(egui::RichText::new("—").small().weak());
                        }
                    }
                    ui.end_row();
                }
            });
    }

    /// Right column: run status, live throughput metrics, per-stage verdicts.
    fn ui_run_status_column(&self, ui: &mut egui::Ui, unit: &str) {
        let ss = &self.scenario_state;
        if ss.finished {
            if let Some(label) = &ss.finish_label {
                ui.label(
                    egui::RichText::new(format!("{label}  —  {:.1} s total", ss.total_elapsed_secs))
                        .strong(),
                );
            }
        } else if self.is_running() && ss.stage_count > 0 {
            ui.label(
                egui::RichText::new(format!(
                    "Stage {}/{}: {}",
                    ss.current_stage_index + 1,
                    ss.stage_count,
                    ss.current_stage_label
                ))
                .strong(),
            );
        }
        self.ui_metrics(ui, unit);
    }

    /// Start/Stop control rendered right-aligned in the top bar; dispatches the
    /// start by the active mode.
    fn ui_top_start_stop(&mut self, ui: &mut egui::Ui, running: bool) {
        if running {
            if ui
                .add(egui::Button::new(format!("{}  Stop", p::STOP)).fill(egui::Color32::from_rgb(180, 60, 60)))
                .clicked()
            {
                self.stop();
            }
            ui.add(egui::Spinner::new());
            ui.label("Running…");
        } else {
            let probing = self.conn_probe.is_some();
            if ui
                .add_enabled(
                    !probing,
                    egui::Button::new(format!("{}  Start", p::PLAY)).fill(egui::Color32::from_rgb(50, 140, 80)),
                )
                .clicked()
            {
                self.start_requested = true;
            }
            if probing {
                ui.add(egui::Spinner::new());
                ui.label("Checking connection…");
            } else {
                let queued = crate::pending_results::pending_count();
                if queued > 0 {
                    ui.label(egui::RichText::new(format!("{queued} result(s) queued offline")).weak());
                } else if self.has_run() {
                    ui.label(egui::RichText::new("Stopped").weak());
                }
            }
        }
    }

    /// Collapsible right panel: live device temps + telemetry charts, stacked
    /// in one scroll area.
    fn ui_side_panel(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(4.0);
            ui.label(egui::RichText::new("Temperatures").strong());
            ui.separator();
            if self.latest_thermals.is_empty() && self.latest_gpu_temps.is_empty() {
                ui.add_space(2.0);
                ui.colored_label(
                    egui::Color32::GRAY,
                    "No sensors yet. CPU/board temps need WinRing0 loaded (run elevated; \
                     lower Memory Integrity + the vulnerable-driver blocklist). NVMe/SATA \
                     need no driver. See the Logs tab.",
                );
            } else {
                for t in &self.latest_thermals {
                    Self::temp_row(ui, &t.label, t.temp_c);
                }
                for (name, t) in &self.latest_gpu_temps {
                    Self::temp_row(ui, name, *t);
                }
            }
            ui.add_space(10.0);
            ui.label(egui::RichText::new("Live charts").strong());
            ui.separator();
            self.charts.show(ui);
        });
    }

    /// One stacked temp row: label left, colored value right.
    fn temp_row(ui: &mut egui::Ui, label: &str, temp_c: f32) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(label).small());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.colored_label(
                    crate::hw_monitor::temp_color(temp_c),
                    egui::RichText::new(format!("{temp_c:.1} °C")).monospace(),
                );
            });
        });
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
