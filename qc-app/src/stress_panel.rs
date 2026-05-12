//! Stress panel: single stressor (optional timeout) or multi-stage scenario (per-stage
//! stressor, threads, duration; optional total wall time and repeat-until-total).

use eframe::egui;
use stress_kit::{
    Metrics, StressConfig, StressSession, Stressor,
    scenario::{
        FinishReason, ScenarioDefinition, ScenarioEvent, ScenarioRunner, ScenarioStage,
    },
};
use std::time::Duration;

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

/// Persisted stress tab state (mode + single/scenario fields).
#[derive(serde::Serialize, serde::Deserialize, Clone, Default)]
pub struct StressPanelConfig {
    pub mode: PanelMode,
    pub single: SingleConfig,
    pub scenario: ScenarioConfig,
}

/// Single-mode fields.
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

/// Serde-friendly mirror of [`Stressor`].
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum StressorChoice {
    Cpu,
    Memory,
    Disk,
}

impl StressorChoice {
    pub fn label(self) -> &'static str {
        match self {
            Self::Cpu => "CPU",
            Self::Memory => "Memory",
            Self::Disk => "Disk I/O",
        }
    }
    pub fn to_stressor(self) -> Stressor {
        match self {
            Self::Cpu => Stressor::Cpu,
            Self::Memory => Stressor::Memory,
            Self::Disk => Stressor::Disk,
        }
    }
    pub fn throughput_unit(self) -> &'static str {
        match self {
            Self::Cpu => "Mop/s",
            Self::Memory | Self::Disk => "MiB/s",
        }
    }
}

/// Scenario-mode fields.
#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct ScenarioConfig {
    pub stages: Vec<ScenarioStageConfig>,
    /// 0 means no limit.
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

/// One scenario stage in the editor list.
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

    fn to_stage(&self) -> ScenarioStage {
        ScenarioStage {
            label: self.label.clone(),
            config: StressConfig {
                stressor: self.stressor.to_stressor(),
                threads: self.threads,
                timeout: None, // `ScenarioRunner` owns per-stage duration
                memory_cap_mb: self.memory_cap_mb,
                disk_file_mb: self.disk_file_mb,
            },
            duration_secs: self.duration_secs,
        }
    }
}

enum ActiveRun {
    Single(StressSession),
    Scenario(ScenarioRunner),
}

/// Scenario run progress (stage index, labels, elapsed).
#[derive(Default)]
struct ScenarioState {
    current_stage_index: usize,
    current_stage_label: String,
    stage_count: usize,
    stage_started_at_elapsed: f64, // total elapsed when this stage began
    finished: bool,
    finish_reason: Option<FinishReason>,
    total_elapsed_secs: f64,
}

/// Non-persisted panel state.
pub struct StressPanel {
    run: Option<ActiveRun>,
    /// Last `Metrics` from the active run.
    latest: Option<Metrics>,
    /// Scenario progress (current stage, counts).
    scenario_state: ScenarioState,
    /// Throughput samples for the sparkline.
    history: Vec<f32>,
    /// Which scenario row has the detail block open (`None` if collapsed).
    editing_stage: Option<usize>,
}

impl Default for StressPanel {
    fn default() -> Self {
        Self {
            run: None,
            latest: None,
            scenario_state: ScenarioState::default(),
            history: Vec::new(),
            editing_stage: None,
        }
    }
}

impl StressPanel {
    /// Poll active run; call from app `logic` each frame.
    pub fn tick(&mut self, ctx: &egui::Context) {
        // Read run state here so `push_metrics` / `handle_scenario_event` can take `&mut self`.
        let update = match &self.run {
            None => return,
            Some(ActiveRun::Single(s)) => {
                let m = s.try_recv();
                let stopping = s.is_stopping();
                (m, vec![], stopping)
            }
            Some(ActiveRun::Scenario(runner)) => {
                let events = runner.try_recv_all();
                let stopping = runner.is_stopping();
                (None, events, stopping)
            }
        };

        let (metrics, events, stopping) = update;
        if let Some(m) = metrics {
            self.push_metrics(m);
        }
        for event in events {
            self.handle_scenario_event(event);
        }
        if !stopping {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    fn push_metrics(&mut self, m: Metrics) {
        self.history.push(m.throughput as f32);
        if self.history.len() > 120 {
            self.history.remove(0);
        }
        self.latest = Some(m);
    }

    fn handle_scenario_event(&mut self, event: ScenarioEvent) {
        match event {
            ScenarioEvent::StageStarted { index, label, stage_count } => {
                let elapsed = self.run.as_ref().map_or(0.0, |r| {
                    if let ActiveRun::Scenario(r) = r {
                        r.elapsed().as_secs_f64()
                    } else {
                        0.0
                    }
                });
                self.scenario_state = ScenarioState {
                    current_stage_index: index,
                    current_stage_label: label,
                    stage_count,
                    stage_started_at_elapsed: elapsed,
                    finished: false,
                    finish_reason: None,
                    total_elapsed_secs: elapsed,
                };
                self.history.clear();
                self.latest = None;
            }
            ScenarioEvent::Tick { metrics, .. } => {
                self.scenario_state.total_elapsed_secs = metrics.elapsed_secs;
                self.push_metrics(metrics);
            }
            ScenarioEvent::StageFinished { .. } => {}
            ScenarioEvent::Finished { reason, total_elapsed_secs } => {
                self.scenario_state.finished = true;
                self.scenario_state.finish_reason = Some(reason);
                self.scenario_state.total_elapsed_secs = total_elapsed_secs;
            }
        }
    }

    pub fn is_running(&self) -> bool {
        match &self.run {
            None => false,
            Some(ActiveRun::Single(s)) => !s.is_stopping(),
            Some(ActiveRun::Scenario(r)) => !r.is_stopping(),
        }
    }

    pub fn has_run(&self) -> bool {
        self.run.is_some()
    }

    fn start_single(&mut self, cfg: &SingleConfig) {
        self.history.clear();
        self.latest = None;
        self.scenario_state = ScenarioState::default();
        let config = StressConfig {
            stressor: cfg.stressor.to_stressor(),
            threads: cfg.threads,
            timeout: if cfg.use_timeout && cfg.timeout_secs > 0 {
                Some(Duration::from_secs(cfg.timeout_secs))
            } else {
                None
            },
            memory_cap_mb: cfg.memory_cap_mb,
            disk_file_mb: cfg.disk_file_mb,
        };
        self.run = Some(ActiveRun::Single(StressSession::start(config)));
    }

    fn start_scenario(&mut self, cfg: &ScenarioConfig) {
        self.history.clear();
        self.latest = None;
        self.scenario_state = ScenarioState::default();
        let def = ScenarioDefinition {
            stages: cfg.stages.iter().map(|s| s.to_stage()).collect(),
            total_wall_secs: if cfg.use_total && cfg.total_wall_secs > 0 {
                Some(cfg.total_wall_secs)
            } else {
                None
            },
            repeat_until_total: cfg.repeat_until_total,
        };
        self.run = Some(ActiveRun::Scenario(ScenarioRunner::start(def)));
    }

    fn stop(&mut self) {
        match &self.run {
            Some(ActiveRun::Single(s)) => s.stop(),
            Some(ActiveRun::Scenario(r)) => r.stop(),
            None => {}
        }
    }

    /// Stress tab UI; sets `open_hw_monitor` if the user opens the monitor.
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        cfg: &mut StressPanelConfig,
        open_hw_monitor: &mut bool,
    ) {
        let running = self.is_running();

        ui.horizontal(|ui| {
            ui.heading("Stress Test");
            ui.add_space(8.0);
            if ui.button("Hardware Monitor").clicked() {
                *open_hw_monitor = true;
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
            PanelMode::Single => self.ui_single(ui, cfg, running),
            PanelMode::Scenario => self.ui_scenario(ui, cfg, running),
        }
    }

    fn ui_single(&mut self, ui: &mut egui::Ui, cfg: &mut StressPanelConfig, running: bool) {
        let s = &mut cfg.single;

        ui.group(|ui| {
            ui.label("Stressor");
            ui.horizontal(|ui| {
                for choice in [StressorChoice::Cpu, StressorChoice::Memory, StressorChoice::Disk] {
                    ui.add_enabled_ui(!running, |ui| {
                        ui.selectable_value(&mut s.stressor, choice, choice.label());
                    });
                }
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
                StressorChoice::Memory => {
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
                StressorChoice::Cpu => {}
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

        self.ui_start_stop(ui, running, |panel| {
            panel.start_single(&cfg.single);
        });

        self.ui_metrics(ui, cfg.single.stressor.throughput_unit());
    }

    fn ui_scenario(&mut self, ui: &mut egui::Ui, cfg: &mut StressPanelConfig, running: bool) {
        ui.group(|ui| {
            ui.label(egui::RichText::new("Stages (run in order)").strong());
            ui.add_space(4.0);

            let mut swap: Option<(usize, usize)> = None;
            let mut remove: Option<usize> = None;
            let n = cfg.scenario.stages.len();

            for i in 0..n {
                let is_editing = self.editing_stage == Some(i);
                ui.horizontal(|ui| {
                    // Reorder
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

                    // Stressor combo (index into `cfg.scenario.stages`)
                    ui.add_enabled_ui(!running, |ui| {
                        let selected = cfg.scenario.stages[i].stressor.label();
                        egui::ComboBox::from_id_salt(format!("stage_stressor_{i}"))
                            .selected_text(selected)
                            .width(90.0)
                            .show_ui(ui, |ui| {
                                for choice in [
                                    StressorChoice::Cpu,
                                    StressorChoice::Memory,
                                    StressorChoice::Disk,
                                ] {
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

                // Per-stage threads / caps when expanded
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
                            StressorChoice::Memory => {
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
                            StressorChoice::Cpu => {}
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
                    if ui.small_button("+ CPU stage").clicked() {
                        cfg.scenario.stages.push(ScenarioStageConfig::default_cpu());
                    }
                    if ui.small_button("+ Memory stage").clicked() {
                        cfg.scenario.stages.push(ScenarioStageConfig::default_memory());
                    }
                    if ui.small_button("+ Disk stage").clicked() {
                        cfg.scenario.stages.push(ScenarioStageConfig::default_disk());
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

        self.ui_start_stop(ui, running, |panel| {
            panel.start_scenario(&cfg.scenario);
        });

        if running || (self.has_run() && cfg.mode == PanelMode::Scenario) {
            self.ui_scenario_progress(ui, cfg);
        }

        let stage_idx = self.scenario_state.current_stage_index;
        let unit = cfg.scenario
            .stages
            .get(stage_idx)
            .map(|s| s.stressor.throughput_unit())
            .unwrap_or("ops/s");
        self.ui_metrics(ui, unit);
    }

    fn ui_scenario_progress(&self, ui: &mut egui::Ui, cfg: &StressPanelConfig) {
        let ss = &self.scenario_state;
        if ss.finished {
            let reason = ss.finish_reason.map(|r| r.label()).unwrap_or("done");
            ui.label(
                egui::RichText::new(format!(
                    "{reason}  —  {:.1} s total",
                    ss.total_elapsed_secs
                ))
                .strong(),
            );
            return;
        }

        if ss.stage_count == 0 {
            return;
        }

        // Elapsed in current stage vs `duration_secs`
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

        // Optional overall bar when `use_total` is set
        if cfg.scenario.use_total && cfg.scenario.total_wall_secs > 0 {
            let overall = (ss.total_elapsed_secs / cfg.scenario.total_wall_secs as f64)
                .clamp(0.0, 1.0) as f32;
            ui.add(
                egui::ProgressBar::new(overall)
                    .text(format!("Overall  {:.0}/{} s", ss.total_elapsed_secs, cfg.scenario.total_wall_secs))
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
                    egui::RichText::new(format!("{:.2} {unit}", m.throughput))
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
}
