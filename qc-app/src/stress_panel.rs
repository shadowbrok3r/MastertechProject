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
use mtech_ui::stress_dashboard::{
    DashboardAction, LaneView, StageProgress, StageVerdictView, StressDashboard, StressLive,
    VerdictView,
};
use stress_kit::telemetry::{TelemetrySnapshot, ThermalReading};
use stress_runner::{
    RunController, RunPlan, RunSpec, RunStage, RunUpdate, RunVerdict, Stressor, TelemetryAgent,
    TestTool,
};
use stress_runner::RecordId;

use crate::charts::ChartBoard;

// Config, mode, and stressor types now live in `stress-runner` so every
// renderer (this egui panel, the terminal StressTab, Mastertech4.0) shares one
// source of truth. Re-exported here to keep `crate::stress_panel` paths working.
pub use stress_runner::{
    CertConfig, ConcurrentConfig, PanelMode, QcBenchmarkConfig, ScenarioConfig,
    ScenarioStageConfig, SingleConfig, StressPanelConfig, StressorChoice,
};

/// Latest stress-kit throughput tick (mirrors `stress_kit::Metrics` but stored
/// flat so the UI doesn't need to keep an Option around).
#[derive(Default, Clone)]
struct LatestMetrics {
    elapsed_secs: f64,
    throughput: f64,
    last_error: Option<String>,
    throughput_unit: &'static str,
}

/// Live state for one lane of a concurrent run, keyed by `stage_index`.
#[derive(Clone)]
struct LaneLive {
    index: u32,
    label: String,
    throughput: f64,
    unit: &'static str,
    errors: u64,
    last_error: Option<String>,
}

/// Scenario run progress.
#[derive(Default)]
struct ScenarioState {
    current_stage_index: usize,
    current_stage_label: String,
    stage_count: usize,
    finished: bool,
    finish_label: Option<String>,
    total_elapsed_secs: f64,
}

pub struct StressPanel {
    run: Option<RunController>,
    latest: Option<LatestMetrics>,
    scenario_state: ScenarioState,
    history: Vec<f32>,
    last_run_id: Option<RecordId>,
    last_verdict: Option<RunVerdict>,
    /// Live system telemetry charts (relocated from the hardware monitor).
    /// Fed every frame from the shared `HwSampler` via [`StressPanel::push_telemetry`].
    charts: ChartBoard,
    /// `(service_order, tech)` applied to new runs while an order session is open.
    order_context: Option<(RecordId, String)>,
    /// Preset label of the most recently started run.
    last_preset: Option<String>,
    /// Per-stage rules verdicts for the current/last run, in finish order.
    stage_verdicts: Vec<StageVerdictRow>,
    /// Live per-lane throughput for a concurrent run, keyed by `stage_index`.
    concurrent_lanes: Vec<LaneLive>,
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
    /// Shared 3-column dashboard view state.
    dashboard: StressDashboard,
    /// Planned wall-clock of the active run, so a short run is flagged.
    planned_secs: Option<u64>,
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
            last_run_id: None,
            last_verdict: None,
            charts: ChartBoard::default(),
            order_context: None,
            last_preset: None,
            stage_verdicts: Vec::new(),
            concurrent_lanes: Vec::new(),
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
            dashboard: StressDashboard::default(),
            planned_secs: None,
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
                self.concurrent_lanes.clear();
            }
            RunUpdate::StageStarted { index, label, stage_count } => {
                let elapsed = self.latest.as_ref().map_or(0.0, |m| m.elapsed_secs);
                self.scenario_state = ScenarioState {
                    current_stage_index: index,
                    current_stage_label: label,
                    stage_count,
                    finished: false,
                    finish_label: None,
                    total_elapsed_secs: elapsed,
                };
                self.history.clear();
            }
            RunUpdate::Tick {
                stage_index,
                stage_label,
                metrics,
                telemetry: _,
                throughput_unit,
            } => {
                if let Some(idx) = stage_index {
                    self.upsert_lane(
                        idx,
                        stage_label,
                        metrics.throughput,
                        metrics.errors,
                        metrics.last_error.clone(),
                        throughput_unit,
                    );
                }
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

    /// Mirror the panel's run state into the shared dashboard's view model.
    fn build_live(&self) -> StressLive {
        let m = self.latest.clone().unwrap_or_default();
        let stage = (self.scenario_state.stage_count > 0).then(|| StageProgress {
            index: self.scenario_state.current_stage_index,
            label: self.scenario_state.current_stage_label.clone(),
            count: self.scenario_state.stage_count,
        });
        let lanes = self
            .concurrent_lanes
            .iter()
            .map(|l| LaneView {
                index: l.index,
                label: l.label.clone(),
                stressor: StressorChoice::ALL
                    .into_iter()
                    .find(|c| c.label() == l.label),
                throughput: l.throughput,
                unit: l.unit,
                errors: l.errors,
                last_error: l.last_error.clone(),
            })
            .collect();
        let stage_verdicts = self
            .stage_verdicts
            .iter()
            .map(|s| StageVerdictView {
                label: s.label.clone(),
                pass: s.pass,
                violations: s.violations.clone(),
                peak_throughput: s.peak_throughput,
            })
            .collect();
        let verdict = self.last_verdict.as_ref().map(|v| VerdictView {
            result: v.result,
            failure_kind: Some(v.failure_mode.kind().to_string()),
            duration_secs: v.duration_secs,
            max_temp_c: v.summary.max_temp_c,
            whea_delta: v.summary.whea_delta_count,
            tdr_count: v.summary.tdr_count,
            run_id: self.last_run_id.as_ref().map(|id| id.key_string_pretty()),
            planned_secs: self.planned_secs,
        });

        StressLive {
            elapsed_secs: m.elapsed_secs,
            throughput: m.throughput,
            throughput_unit: m.throughput_unit,
            last_error: m.last_error,
            stage,
            lanes,
            stage_verdicts,
            verdict,
            history: self.history.clone(),
            recent_runs: Vec::new(),
        }
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
        self.run = Some(RunController::start(spec, telemetry));
    }

    /// Spin up a concurrent run: every selected lane runs at once via
    /// `RunPlan::Concurrent`. Same `RunController` + persistence path as the
    /// other modes; the worker budgets threads across lanes at launch.
    fn start_concurrent(
        &mut self,
        cfg: &ConcurrentConfig,
        telemetry: Arc<TelemetryAgent>,
        computer: RecordId,
    ) {
        if cfg.lanes.is_empty() {
            return;
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
            name: Some("qc-app:concurrent".to_string()),
        };
        spec.preset_label = Some("qc-app:concurrent".to_string());
        spec.tags = vec!["origin:gui".into(), "preset:concurrent".into()];
        self.apply_order_context(&mut spec);
        self.last_preset = spec.preset_label.clone();
        self.history.clear();
        self.latest = None;
        self.scenario_state = ScenarioState::default();
        self.concurrent_lanes.clear();
        self.run = Some(RunController::start(spec, telemetry));
    }

    /// Upsert a concurrent lane's latest throughput, keyed by `stage_index`.
    fn upsert_lane(
        &mut self,
        index: u32,
        label: Option<String>,
        throughput: f64,
        errors: u64,
        last_error: Option<String>,
        unit: &'static str,
    ) {
        if let Some(lane) = self.concurrent_lanes.iter_mut().find(|l| l.index == index) {
            lane.throughput = throughput;
            lane.errors = errors;
            lane.last_error = last_error;
            lane.unit = unit;
            if let Some(l) = label {
                lane.label = l;
            }
        } else {
            self.concurrent_lanes.push(LaneLive {
                index,
                label: label.unwrap_or_else(|| format!("lane {index}")),
                throughput,
                unit,
                errors,
                last_error,
            });
            self.concurrent_lanes.sort_by_key(|l| l.index);
        }
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
        // Read the plan's intended length up front so a run that stops short is
        // reported as incomplete rather than as a clean result.
        self.planned_secs = stress_runner::build_run_spec(
            cfg,
            computer.clone(),
            None,
            &stress_runner::StressRunContext::new("qc-app", "gui"),
        )
        .ok()
        .and_then(|s| stress_runner::planned_duration_secs(&s.plan));
        match cfg.mode {
            PanelMode::Single => self.start_single(&cfg.single, telemetry, computer),
            PanelMode::Scenario => self.start_scenario(&cfg.scenario, telemetry, computer),
            PanelMode::QcBenchmark => self.start_qc_benchmark(&cfg.qc_benchmark, telemetry, computer),
            PanelMode::Certification => self.start_certification(&cfg.certification, telemetry, computer),
            PanelMode::Concurrent => self.start_concurrent(&cfg.concurrent, telemetry, computer),
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
                    if ui
                        .add(
                            egui::Button::new(format!("{}  Combined Torture", p::CPU))
                                .fill(egui::Color32::from_rgb(150, 70, 160)),
                        )
                        .on_hover_text("Run CPU + RAM + GPU stressors at the same time")
                        .clicked()
                    {
                        cfg.mode = PanelMode::Single;
                        cfg.single.stressor = StressorChoice::Combined;
                        self.start_requested = true;
                    }
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
            });
            ui.add_space(4.0);
        });

        egui::Panel::right("stress_panel_side")
            .resizable(true)
            .default_size(440.0)
            .show_animated_inside(ui, self.temps_open, |ui| {
                self.ui_side_panel(ui);
            });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            let live = self.build_live();
            let charts = &mut self.charts;
            let action = self.dashboard.show(
                ui,
                cfg,
                &live,
                running,
                self.cert_error.as_deref(),
                |ui| charts.show(ui),
            );
            match action {
                DashboardAction::Start => self.start_requested = true,
                DashboardAction::Stop => self.stop(),
                DashboardAction::OpenHistory => {
                    self.report_request = self.last_run_id.clone();
                }
                DashboardAction::None => {}
            }
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
                    "No sensors yet. CPU/board temps need a kernel-mode sensor backend and an \
                     elevated run. NVMe/SATA need no driver. See the Logs tab.",
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
            FailureMode::RailDroop { .. } => "rail droop",
            FailureMode::StressorHang { .. } => "stressor hung (tool)",
        }
    }
}
