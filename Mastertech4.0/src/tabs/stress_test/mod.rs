//! Interactive stress-test tab (egui).
//!
//! A thin renderer over [`stress_runner::RunController`]: it builds a `RunSpec`
//! from the shared [`StressPanelConfig`] and drives start/poll/stop. All
//! execution and the strict `hardware_component` / `stress_test_run` /
//! `stress_test_metric` / `stress_test_event` persistence happen inside the
//! controller's worker thread — this tab only renders and forwards intent.

use std::time::Duration;

use displays::tabs::resource_monitor::chart_board::ChartBoard;
use displays::ui_tools::icons;
use eframe::egui::{self, Color32, RichText, Ui};
use stress_runner::{
    build_run_spec, is_stress_active, PanelMode, RecordId, RunController, RunResult, RunUpdate,
    RunVerdict, ScenarioStageConfig, StressPanelConfig, StressRunContext, StressorChoice,
    CERT_PRESET_NAMES,
};

use crate::app_state::MastertechContext;
use crate::filesystem::local_computer_record;
use crate::filesystem::system_info::{current_telemetry_snapshot, shared_telemetry_agent};

/// Flat mirror of the latest stress-kit tick for display.
#[derive(Default, Clone)]
struct LatestMetrics {
    elapsed_secs: f64,
    throughput: f64,
    last_error: Option<String>,
    throughput_unit: &'static str,
}

/// One lane of a concurrent run, keyed by `stage_index`.
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

/// One finished stage's rules verdict.
#[derive(Clone)]
struct StageVerdictRow {
    label: String,
    pass: bool,
    violations: Vec<String>,
    peak_throughput: Option<f64>,
}

pub struct StressRunner {
    cfg: StressPanelConfig,
    run: Option<RunController>,
    latest: Option<LatestMetrics>,
    scenario_state: ScenarioState,
    history: Vec<f32>,
    stage_verdicts: Vec<StageVerdictRow>,
    concurrent_lanes: Vec<LaneLive>,
    last_run_id: Option<RecordId>,
    last_verdict: Option<RunVerdict>,
    show_verdict: bool,
    start_error: Option<String>,
    charts: ChartBoard,
    /// Set when the operator clicks "View history"; drained by the host to
    /// open the read-only Stress Lab tab.
    open_history_requested: bool,
}

impl Default for StressRunner {
    fn default() -> Self {
        Self {
            cfg: StressPanelConfig::default(),
            run: None,
            latest: None,
            scenario_state: ScenarioState::default(),
            history: Vec::new(),
            stage_verdicts: Vec::new(),
            concurrent_lanes: Vec::new(),
            last_run_id: None,
            last_verdict: None,
            show_verdict: false,
            start_error: None,
            charts: ChartBoard::default(),
            open_history_requested: false,
        }
    }
}

impl StressRunner {
    pub fn is_running(&self) -> bool {
        self.run.as_ref().map(|c| c.is_running()).unwrap_or(false)
    }

    /// True (once) if the operator asked to open the Stress Lab history tab.
    pub fn take_open_history(&mut self) -> bool {
        std::mem::take(&mut self.open_history_requested)
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        // Feed the live telemetry charts from the shared background sampler.
        let snapshot = current_telemetry_snapshot();
        self.charts.push(&snapshot);

        self.tick(ui.ctx());

        self.top_bar(ui);
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            self.config_ui(ui);
            ui.separator();
            self.live_ui(ui);
            ui.separator();
            ui.label(RichText::new(format!("{}  Live telemetry", icons::CHART)).strong());
            self.charts.show(ui);
        });
    }

    // -- lifecycle ----------------------------------------------------------

    fn tick(&mut self, ctx: &egui::Context) {
        let Some(controller) = self.run.as_ref() else {
            return;
        };
        let running = controller.is_running();
        for update in controller.poll() {
            self.handle_update(update);
        }
        if !running {
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
            RunUpdate::StageVerdict { label, pass, violations, peak_throughput, .. } => {
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
            RunUpdate::Warning { message } => log::warn!("stress-runner: {message}"),
            RunUpdate::Error { message } => {
                log::error!("stress-runner: {message}");
                self.start_error = Some(message);
            }
        }
    }

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

    fn start(&mut self) {
        if self.is_running() {
            return;
        }
        if is_stress_active() {
            self.start_error = Some("a stress run is already active on this machine".into());
            return;
        }
        let computer = local_computer_record();
        let telemetry = shared_telemetry_agent();
        let snapshot = telemetry.snapshot();
        let ctx = StressRunContext::new("mtech", "gui");
        match build_run_spec(&self.cfg, computer, Some(&snapshot), &ctx) {
            Ok(spec) => {
                self.start_error = None;
                self.history.clear();
                self.latest = None;
                self.scenario_state = ScenarioState::default();
                self.stage_verdicts.clear();
                self.concurrent_lanes.clear();
                self.show_verdict = false;
                self.run = Some(RunController::start(spec, telemetry));
            }
            Err(e) => self.start_error = Some(e),
        }
    }

    fn stop(&mut self) {
        if let Some(c) = self.run.as_ref() {
            c.stop();
        }
    }

    // -- top bar ------------------------------------------------------------

    fn top_bar(&mut self, ui: &mut Ui) {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{}  Stress Test", icons::FLASK)).heading());
            ui.separator();
            ui.selectable_value(&mut self.cfg.mode, PanelMode::Single, "Single");
            ui.selectable_value(&mut self.cfg.mode, PanelMode::Scenario, "Scenario");
            ui.selectable_value(&mut self.cfg.mode, PanelMode::QcBenchmark, "QC Benchmark");
            ui.selectable_value(&mut self.cfg.mode, PanelMode::Certification, "Certification");
            ui.selectable_value(&mut self.cfg.mode, PanelMode::Concurrent, "Concurrent");

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let running = self.is_running();
                if running {
                    if ui
                        .button(RichText::new(format!("{}  Stop", icons::STOP)))
                        .clicked()
                    {
                        self.stop();
                    }
                    ui.spinner();
                } else if ui
                    .button(RichText::new(format!("{}  Start", icons::PLAY)))
                    .clicked()
                {
                    self.start();
                }
                if ui
                    .button(RichText::new(format!("{}  History", icons::EYE)))
                    .on_hover_text("Open the Stress Lab history browser")
                    .clicked()
                {
                    self.open_history_requested = true;
                }
            });
        });
        if let Some(err) = &self.start_error {
            ui.colored_label(Color32::from_rgb(255, 90, 90), err);
        }
        ui.add_space(4.0);
    }

    // -- per-mode config ----------------------------------------------------

    fn config_ui(&mut self, ui: &mut Ui) {
        match self.cfg.mode {
            PanelMode::Single => self.single_ui(ui),
            PanelMode::Scenario => self.scenario_ui(ui),
            PanelMode::QcBenchmark => self.qc_benchmark_ui(ui),
            PanelMode::Certification => self.certification_ui(ui),
            PanelMode::Concurrent => self.concurrent_ui(ui),
        }
    }

    fn single_ui(&mut self, ui: &mut Ui) {
        let running = self.is_running();
        ui.add_enabled_ui(!running, |ui| {
            let c = &mut self.cfg.single;
            egui::Grid::new("stress_single_grid")
                .num_columns(2)
                .spacing([12.0, 6.0])
                .show(ui, |ui| {
                    ui.label("Stressor");
                    stressor_combo(ui, "single_stressor", &mut c.stressor);
                    ui.end_row();

                    ui.label("Threads (0 = auto)");
                    ui.add(egui::DragValue::new(&mut c.threads).range(0..=1024));
                    ui.end_row();

                    ui.label("Timeout");
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut c.use_timeout, "Stop after");
                        ui.add_enabled(
                            c.use_timeout,
                            egui::DragValue::new(&mut c.timeout_secs).suffix(" s").range(1..=86_400),
                        );
                    });
                    ui.end_row();

                    ui.label("Memory cap");
                    ui.add(egui::DragValue::new(&mut c.memory_cap_mb).suffix(" MB").range(16..=1_048_576));
                    ui.end_row();

                    ui.label("Disk file size");
                    ui.add(egui::DragValue::new(&mut c.disk_file_mb).suffix(" MB").range(1..=1_048_576));
                    ui.end_row();
                });
        });
    }

    fn scenario_ui(&mut self, ui: &mut Ui) {
        let running = self.is_running();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Scenario stages").strong());
            if !running && ui.button(format!("{}  Add stage", icons::PLUS)).clicked() {
                self.cfg.scenario.stages.push(ScenarioStageConfig::default_cpu());
            }
        });
        let mut remove: Option<usize> = None;
        let count = self.cfg.scenario.stages.len();
        for i in 0..count {
            ui.push_id(i, |ui| {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.add_enabled_ui(!running, |ui| {
                        let s = &mut self.cfg.scenario.stages[i];
                        ui.horizontal(|ui| {
                            ui.label(format!("#{}", i + 1));
                            ui.text_edit_singleline(&mut s.label);
                            if !running && ui.button(icons::TRASH).clicked() {
                                remove = Some(i);
                            }
                        });
                        egui::Grid::new("stage_grid")
                            .num_columns(2)
                            .spacing([12.0, 4.0])
                            .show(ui, |ui| {
                                ui.label("Stressor");
                                stressor_combo(ui, "stage_stressor", &mut s.stressor);
                                ui.end_row();
                                ui.label("Threads");
                                ui.add(egui::DragValue::new(&mut s.threads).range(0..=1024));
                                ui.end_row();
                                ui.label("Duration");
                                ui.add(egui::DragValue::new(&mut s.duration_secs).suffix(" s").range(1..=86_400));
                                ui.end_row();
                                ui.label("Memory cap");
                                ui.add(egui::DragValue::new(&mut s.memory_cap_mb).suffix(" MB").range(16..=1_048_576));
                                ui.end_row();
                                ui.label("Disk file");
                                ui.add(egui::DragValue::new(&mut s.disk_file_mb).suffix(" MB").range(1..=1_048_576));
                                ui.end_row();
                            });
                    });
                });
            });
        }
        if let Some(i) = remove {
            if self.cfg.scenario.stages.len() > 1 {
                self.cfg.scenario.stages.remove(i);
            }
        }
        ui.add_enabled_ui(!running, |ui| {
            let sc = &mut self.cfg.scenario;
            ui.horizontal(|ui| {
                ui.checkbox(&mut sc.use_total, "Total wall-clock cap");
                ui.add_enabled(
                    sc.use_total,
                    egui::DragValue::new(&mut sc.total_wall_secs).suffix(" s").range(1..=604_800),
                );
                ui.add_enabled(
                    sc.use_total,
                    egui::Checkbox::new(&mut sc.repeat_until_total, "Repeat until total"),
                );
            });
        });
    }

    fn qc_benchmark_ui(&mut self, ui: &mut Ui) {
        let running = self.is_running();
        ui.add_enabled_ui(!running, |ui| {
            ui.label("Curated 8-stage burn-in: cpu, matrix, fp, stream, cache, branch, memory, vm.");
            ui.horizontal(|ui| {
                ui.label("Duration multiplier");
                ui.add(
                    egui::Slider::new(&mut self.cfg.qc_benchmark.duration_multiplier, 0.1..=10.0)
                        .text("×"),
                );
            });
            let per_stage = (20.0 * self.cfg.qc_benchmark.duration_multiplier).round();
            ui.label(format!("≈ {per_stage:.0}s/stage, {:.0}s total", per_stage * 8.0));
        });
    }

    fn certification_ui(&mut self, ui: &mut Ui) {
        let running = self.is_running();
        ui.add_enabled_ui(!running, |ui| {
            let c = &mut self.cfg.certification;
            egui::Grid::new("cert_grid")
                .num_columns(2)
                .spacing([12.0, 6.0])
                .show(ui, |ui| {
                    ui.label("Preset");
                    egui::ComboBox::from_id_salt("cert_preset")
                        .selected_text(c.preset_name.clone())
                        .show_ui(ui, |ui| {
                            for name in CERT_PRESET_NAMES {
                                ui.selectable_value(&mut c.preset_name, name.to_string(), *name);
                            }
                        });
                    ui.end_row();
                    ui.label("Duration multiplier");
                    ui.add(egui::Slider::new(&mut c.duration_multiplier, 0.001..=1.0).text("×"));
                    ui.end_row();
                });
            ui.label("Certification presets carry per-stage verdict rules (temp, WHEA/TDR, throughput).");
        });
    }

    fn concurrent_ui(&mut self, ui: &mut Ui) {
        let running = self.is_running();
        ui.add_enabled_ui(!running, |ui| {
            ui.label(RichText::new("Lanes (run simultaneously)").strong());
            ui.horizontal_wrapped(|ui| {
                for choice in StressorChoice::ALL {
                    let mut on = self.cfg.concurrent.lanes.contains(&choice);
                    if ui.selectable_label(on, choice.label()).clicked() {
                        on = !on;
                        if on {
                            if !self.cfg.concurrent.lanes.contains(&choice) {
                                self.cfg.concurrent.lanes.push(choice);
                            }
                        } else {
                            self.cfg.concurrent.lanes.retain(|c| *c != choice);
                        }
                    }
                }
            });
            let c = &mut self.cfg.concurrent;
            egui::Grid::new("concurrent_grid")
                .num_columns(2)
                .spacing([12.0, 6.0])
                .show(ui, |ui| {
                    ui.label("Duration");
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut c.use_timeout, "Stop after");
                        ui.add_enabled(
                            c.use_timeout,
                            egui::DragValue::new(&mut c.duration_secs).suffix(" s").range(1..=86_400),
                        );
                    });
                    ui.end_row();
                    ui.label("Memory cap / lane");
                    ui.add(egui::DragValue::new(&mut c.memory_cap_mb).suffix(" MB").range(16..=1_048_576));
                    ui.end_row();
                    ui.label("Disk file / lane");
                    ui.add(egui::DragValue::new(&mut c.disk_file_mb).suffix(" MB").range(1..=1_048_576));
                    ui.end_row();
                });
        });
    }

    // -- live metrics + verdict --------------------------------------------

    fn live_ui(&mut self, ui: &mut Ui) {
        if let Some(m) = &self.latest {
            ui.horizontal(|ui| {
                ui.label(format!("Elapsed: {:.0}s", m.elapsed_secs));
                ui.separator();
                ui.label(format!("Throughput: {:.1} {}", m.throughput, m.throughput_unit));
                if let Some(err) = &m.last_error {
                    ui.separator();
                    ui.colored_label(Color32::from_rgb(255, 160, 60), format!("last error: {err}"));
                }
            });
        }

        if self.scenario_state.stage_count > 0 {
            let frac = if self.scenario_state.stage_count == 0 {
                0.0
            } else {
                (self.scenario_state.current_stage_index as f32 + 1.0)
                    / self.scenario_state.stage_count as f32
            };
            ui.add(
                egui::ProgressBar::new(frac).text(format!(
                    "stage {}/{}: {}",
                    self.scenario_state.current_stage_index + 1,
                    self.scenario_state.stage_count,
                    self.scenario_state.current_stage_label
                )),
            );
        }

        if !self.concurrent_lanes.is_empty() {
            ui.separator();
            ui.label(RichText::new("Lanes").strong());
            egui::Grid::new("lane_live_grid")
                .num_columns(3)
                .striped(true)
                .show(ui, |ui| {
                    ui.label("Lane");
                    ui.label("Throughput");
                    ui.label("Errors");
                    ui.end_row();
                    for lane in &self.concurrent_lanes {
                        ui.label(&lane.label);
                        ui.label(format!("{:.1} {}", lane.throughput, lane.unit));
                        let col = if lane.errors > 0 {
                            Color32::from_rgb(255, 90, 90)
                        } else {
                            ui.visuals().text_color()
                        };
                        ui.colored_label(col, lane.errors.to_string());
                        ui.end_row();
                    }
                });
        }

        if !self.stage_verdicts.is_empty() {
            ui.separator();
            ui.label(RichText::new("Stage verdicts").strong());
            for row in &self.stage_verdicts {
                let (icon, col) = if row.pass {
                    (icons::STATUS_ON, Color32::from_rgb(90, 220, 120))
                } else {
                    (icons::STATUS_ERR, Color32::from_rgb(255, 90, 90))
                };
                ui.horizontal(|ui| {
                    ui.colored_label(col, icon);
                    ui.label(&row.label);
                    if let Some(pt) = row.peak_throughput {
                        ui.label(format!("(peak {pt:.1})"));
                    }
                });
                for v in &row.violations {
                    ui.colored_label(Color32::from_rgb(255, 160, 60), format!("    • {v}"));
                }
            }
        }

        if self.show_verdict {
            if let Some(verdict) = &self.last_verdict {
                ui.separator();
                let (col, text) = verdict_banner(verdict);
                egui::Frame::group(ui.style())
                    .fill(col.linear_multiply(0.15))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.colored_label(col, RichText::new(text).strong());
                            ui.label(format!("· {:.0}s", verdict.duration_secs));
                            if let Some(t) = verdict.summary.max_temp_c {
                                ui.label(format!("· max {t:.0}°C"));
                            }
                            if verdict.summary.whea_delta_count > 0 {
                                ui.colored_label(
                                    Color32::from_rgb(255, 90, 90),
                                    format!("· WHEA +{}", verdict.summary.whea_delta_count),
                                );
                            }
                            if verdict.summary.tdr_count > 0 {
                                ui.colored_label(
                                    Color32::from_rgb(255, 90, 90),
                                    format!("· TDR {}", verdict.summary.tdr_count),
                                );
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button("Dismiss").clicked() {
                                    self.show_verdict = false;
                                }
                            });
                        });
                        if let Some(id) = &self.last_run_id {
                            ui.label(RichText::new(format!("run: {id:?}")).weak());
                        }
                    });
            }
        }
    }
}

fn stressor_combo(ui: &mut Ui, salt: &str, current: &mut StressorChoice) {
    egui::ComboBox::from_id_salt(salt)
        .selected_text(current.label())
        .show_ui(ui, |ui| {
            for choice in StressorChoice::ALL {
                ui.selectable_value(current, choice, choice.label());
            }
        });
}

fn verdict_label(verdict: &RunVerdict) -> String {
    match verdict.result {
        RunResult::Pass => "PASS".to_string(),
        RunResult::Fail => format!("FAIL ({})", verdict.failure_mode.kind()),
        RunResult::Aborted => "ABORTED".to_string(),
        RunResult::Inconclusive => "INCONCLUSIVE".to_string(),
        RunResult::InProgress => "IN PROGRESS".to_string(),
    }
}

fn verdict_banner(verdict: &RunVerdict) -> (Color32, String) {
    let col = match verdict.result {
        RunResult::Pass => Color32::from_rgb(90, 220, 120),
        RunResult::Fail => Color32::from_rgb(255, 90, 90),
        RunResult::Aborted => Color32::from_rgb(255, 190, 70),
        _ => Color32::from_rgb(150, 170, 190),
    };
    (col, verdict_label(verdict))
}

impl MastertechContext {
    pub fn show_stress_test(&mut self, ui: &mut Ui) {
        self.stress_test.ui(ui);
        if self.stress_test.take_open_history() {
            self.pending_tab_opens.push(displays::tabs::TabId::StressLab);
        }
    }
}
