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
use stress_runner::{
    RunController, RunPlan, RunSpec, RunStage, RunUpdate, RunVerdict, Stressor, TelemetryAgent,
    TestTool,
};
use stress_runner::{RecordId, StressKitStressor};

#[derive(serde::Serialize, serde::Deserialize, Clone, PartialEq, Eq)]
pub enum PanelMode {
    Single,
    Scenario,
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
}

impl StressorChoice {
    pub const ALL: [Self; 19] = [
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
        }
    }
}

impl StressPanel {
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

    /// Stress tab UI.
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

        ui.horizontal(|ui| {
            ui.heading("Stress Test");
            ui.add_space(8.0);
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
        ui.add_space(6.0);

        ui.horizontal(|ui| {
            ui.add_enabled_ui(!running, |ui| {
                ui.selectable_value(&mut cfg.mode, PanelMode::Single, "Single stressor");
                ui.selectable_value(&mut cfg.mode, PanelMode::Scenario, "Scenario");
            });
        });
        ui.separator();

        match cfg.mode {
            PanelMode::Single => self.ui_single(ui, cfg, running, telemetry.clone(), computer.clone()),
            PanelMode::Scenario => self.ui_scenario(ui, cfg, running, telemetry, computer),
        }

        if self.show_verdict {
            if let Some(v) = self.last_verdict.clone() {
                self.ui_verdict_banner(ui, &v);
            }
        }
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
                StressorChoice::Memory | StressorChoice::Memcpy | StressorChoice::Vm => {
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
                | StressorChoice::Tsc => {}
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
            FailureMode::WheaError { .. } => "WHEA",
            FailureMode::ThermalThrottle { .. } => "thermal throttle",
            FailureMode::DiskIoError { .. } => "disk I/O",
            FailureMode::DataMismatch { .. } => "data mismatch",
            FailureMode::Reboot => "reboot",
            FailureMode::Timeout => "timeout",
            FailureMode::OperatorOverride { .. } => "operator override",
        }
    }
}
