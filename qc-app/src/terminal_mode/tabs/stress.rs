//! Stress tab: single stressor, multi-stage scenario, QC benchmark, or TOML
//! certification. Mirrors the egui `StressPanel`; all runs go through
//! `stress_runner::RunController`.

use std::sync::{Arc, Mutex};

use mtech_tui::events::action_handler::{get_update_sender, ActionHandler, WidgetEvent, WidgetId};
use mtech_tui::styling::{Theme, APP_BACKGROUND, THEME};
use mtech_tui::widgets::{
    button::Button,
    click_zones::ClickZones,
    dropdown_menu::DropdownMenu,
    menu_item::MenuItem,
    ButtonType, HandleWidget, SHORTCUT_SET,
};
use ratatui::{
    crossterm::event::{KeyCode, KeyEvent, MouseEvent},
    layout::{Constraint, Direction, Layout, Rect},
    prelude::Backend,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Cell, Gauge, Paragraph, Row, Sparkline, Table, Widget, WidgetRef, Wrap,
    },
    Frame,
};
use stress_kit::telemetry::{TelemetrySnapshot, ThermalReading};
use stress_runner::{
    cert_spec, cert_spec_detected, load_cert_preset, CertPreset, RecordId, RunController, RunPlan,
    RunSpec, RunStage, RunUpdate, RunVerdict, Stressor, TelemetryAgent, TestTool,
};

use crate::stress_panel::{
    PanelMode, ScenarioStageConfig, StressPanelConfig, StressorChoice,
};
use crate::terminal_mode::charts::TuiChartBoard;
use crate::terminal_mode::context::QcContext;

const MODE_SINGLE: &str = "StressModeSingle";
const MODE_SCENARIO: &str = "StressModeScenario";
const MODE_QC: &str = "StressModeQc";
const MODE_CERT: &str = "StressModeCert";
const TELEMETRY_ID: &str = "StressTelemetry";
const START_ID: &str = "StressStart";
const ADD_STAGE_ID: &str = "StressAddStage";

#[derive(Default, Clone)]
struct LatestMetrics {
    elapsed_secs: f64,
    throughput: f64,
    last_error: Option<String>,
    throughput_unit: &'static str,
}

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

#[derive(Clone)]
struct StageVerdictRow {
    label: String,
    pass: bool,
    violations: Vec<String>,
    peak_throughput: Option<f64>,
}

#[derive(Clone, Copy)]
enum StageProg {
    Pending,
    Running(f32),
    Done { pass: Option<bool> },
}

/// Keyboard-focusable numeric field in the config form.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    Stressor,
    Threads,
    Memory,
    Disk,
    Timeout,
    TimeoutToggle,
    QcMult,
    CertPreset,
    CertMult,
    TotalToggle,
    TotalValue,
    RepeatToggle,
    Stage(usize),
}

pub struct StressTab<'a> {
    cfg: StressPanelConfig,
    run: Option<RunController>,
    latest: Option<LatestMetrics>,
    scenario_state: ScenarioState,
    history: Vec<f32>,
    stage_verdicts: Vec<StageVerdictRow>,
    last_run_id: Option<RecordId>,
    last_verdict: Option<RunVerdict>,
    show_verdict: bool,
    charts: TuiChartBoard,
    latest_thermals: Vec<ThermalReading>,
    latest_gpu_temps: Vec<(String, f32)>,
    cert_preview: Option<CertPreset>,
    cert_error: Option<String>,
    report_request: Option<RecordId>,
    temps_open: bool,
    pending_stage_pick: StressorChoice,
    focus: Focus,
    status: String,
    ctx: Arc<Mutex<QcContext>>,

    mode_single: Button<'a>,
    mode_scenario: Button<'a>,
    mode_qc: Button<'a>,
    mode_cert: Button<'a>,
    telemetry_btn: Button<'a>,
    start_btn: Button<'a>,
    add_stage_btn: Button<'a>,
    stressor_menu: DropdownMenu,
    stressor_menu_target: Option<MenuTarget>,
    stressor_anchor: Rect,
    zones: ClickZones,
}

/// What an open stressor dropdown is editing.
#[derive(Clone, Copy)]
enum MenuTarget {
    Single,
    Pending,
    Cert,
    Stage(usize),
}

impl<'a> StressTab<'a> {
    pub fn new(ctx: Arc<Mutex<QcContext>>) -> Self {
        Self {
            cfg: StressPanelConfig::default(),
            run: None,
            latest: None,
            scenario_state: ScenarioState::default(),
            history: Vec::new(),
            stage_verdicts: Vec::new(),
            last_run_id: None,
            last_verdict: None,
            show_verdict: false,
            charts: TuiChartBoard::default(),
            latest_thermals: Vec::new(),
            latest_gpu_temps: Vec::new(),
            cert_preview: None,
            cert_error: None,
            report_request: None,
            temps_open: false,
            pending_stage_pick: StressorChoice::Cpu,
            focus: Focus::Stressor,
            status: String::new(),
            ctx,
            mode_single: Button::new("Single", WidgetId(MODE_SINGLE.into())).theme(Theme::TERTIARY),
            mode_scenario: Button::new("Scenario", WidgetId(MODE_SCENARIO.into()))
                .theme(Theme::TERTIARY),
            mode_qc: Button::new("QC Bench", WidgetId(MODE_QC.into())).theme(Theme::TERTIARY),
            mode_cert: Button::new("Certify", WidgetId(MODE_CERT.into())).theme(Theme::TERTIARY),
            telemetry_btn: Button::new("Telemetry", WidgetId(TELEMETRY_ID.into()))
                .theme(Theme::NEUTRAL),
            start_btn: Button::new("Start", WidgetId(START_ID.into())).theme(Theme::ACCENT),
            add_stage_btn: Button::new("+ Add", WidgetId(ADD_STAGE_ID.into())).theme(Theme::TERTIARY),
            stressor_menu: DropdownMenu::new(),
            stressor_menu_target: None,
            stressor_anchor: Rect::default(),
            zones: ClickZones::default(),
        }
    }

    pub fn take_report_request(&mut self) -> Option<RecordId> {
        self.report_request.take()
    }

    /// Push the latest sampler snapshot into the charts + temp lists.
    pub fn push_telemetry(&mut self, snap: &TelemetrySnapshot) {
        self.charts.push(snap);
        self.latest_thermals = snap.thermals.clone();
        self.latest_gpu_temps = snap
            .gpus
            .iter()
            .filter_map(|g| g.temp_c.map(|t| (g.name.clone(), t)))
            .collect();
    }

    pub fn is_running(&self) -> bool {
        self.run.as_ref().map(|c| c.is_running()).unwrap_or(false)
    }

    fn has_run(&self) -> bool {
        self.run.is_some() || self.last_verdict.is_some()
    }

    /// Drain controller updates each tick; drop the controller when done.
    pub fn tick(&mut self) {
        if let Some(id) = self.zones.take() {
            self.on_zone_click(&id);
        }
        let Some(controller) = self.run.as_ref() else {
            return;
        };
        let running = controller.is_running();
        let updates = controller.poll();
        for update in updates {
            self.handle_update(update);
        }
        if !running {
            self.run = None;
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
            RunUpdate::Tick { metrics, throughput_unit, .. } => {
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
                if let Ok(mut ctx) = self.ctx.lock() {
                    ctx.last_verdict = Some(verdict.clone());
                }
                self.last_verdict = Some(verdict);
                self.show_verdict = true;
            }
            RunUpdate::Warning { message } => log::warn!("stress-runner: {message}"),
            RunUpdate::Error { message } => log::error!("stress-runner: {message}"),
        }
    }

    /// Read the shared telemetry + computer; error to status if missing.
    fn run_context(&mut self) -> Option<(Arc<TelemetryAgent>, RecordId)> {
        let guard = self.ctx.lock().ok()?;
        match (guard.telemetry.clone(), guard.computer.clone()) {
            (Some(t), Some(c)) => Some((t, c)),
            _ => {
                drop(guard);
                self.status = "Telemetry not ready yet - wait for the first sample.".into();
                None
            }
        }
    }

    fn start(&mut self) {
        if self.is_running() {
            return;
        }
        let Some((telemetry, computer)) = self.run_context() else {
            return;
        };
        match self.cfg.mode {
            PanelMode::Single => self.start_single(telemetry, computer),
            PanelMode::Scenario => self.start_scenario(telemetry, computer),
            PanelMode::QcBenchmark => self.start_qc_benchmark(telemetry, computer),
            PanelMode::Certification => self.start_certification(telemetry, computer),
        }
    }

    /// Stamp the open order's `(service_order, tech)` onto the run spec and
    /// publish its preset label, mirroring the egui panel.
    fn apply_order_context(&self, spec: &mut RunSpec) {
        if let Ok(ctx) = self.ctx.lock() {
            if let Some((service_order, tech)) = ctx.order_context.as_ref() {
                spec.service_order = Some(service_order.clone());
                if !tech.is_empty() {
                    spec.tech = Some(tech.clone());
                }
            }
        }
    }

    fn publish_preset(&self, spec: &RunSpec) {
        if let Ok(mut ctx) = self.ctx.lock() {
            ctx.last_preset = spec.preset_label.clone();
        }
    }

    fn reset_run_state(&mut self) {
        self.history.clear();
        self.latest = None;
        self.scenario_state = ScenarioState::default();
        self.show_verdict = false;
        self.status.clear();
    }

    fn start_single(&mut self, telemetry: Arc<TelemetryAgent>, computer: RecordId) {
        let cfg = self.cfg.single.clone();
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
        self.publish_preset(&spec);
        self.reset_run_state();
        self.run = Some(RunController::start(spec, telemetry));
    }

    fn start_scenario(&mut self, telemetry: Arc<TelemetryAgent>, computer: RecordId) {
        let scn = self.cfg.scenario.clone();
        let stages: Vec<RunStage> = scn.stages.iter().map(to_run_stage).collect();
        let plan = RunPlan::Scenario {
            stages: stages.clone(),
            total_wall_secs: if scn.use_total && scn.total_wall_secs > 0 {
                Some(scn.total_wall_secs)
            } else {
                None
            },
            repeat_until_total: scn.repeat_until_total,
        };
        let mut spec = RunSpec::single_stresskit(
            computer,
            stages.first().map(|s| s.stressor).unwrap_or(Stressor::Cpu),
            None,
        );
        spec.plan = plan;
        spec.tool = TestTool::StressKitScenario { name: Some("qc-app:scenario".to_string()) };
        spec.preset_label = Some("qc-app:scenario".to_string());
        self.apply_order_context(&mut spec);
        self.publish_preset(&spec);
        self.reset_run_state();
        self.run = Some(RunController::start(spec, telemetry));
    }

    fn start_qc_benchmark(&mut self, telemetry: Arc<TelemetryAgent>, computer: RecordId) {
        let mult = self.cfg.qc_benchmark.duration_multiplier.clamp(0.1, 10.0);
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
        spec.tags = vec!["origin:tui".into(), "preset:qc-benchmark".into()];
        self.apply_order_context(&mut spec);
        self.publish_preset(&spec);
        self.reset_run_state();
        self.run = Some(RunController::start(spec, telemetry));
    }

    fn start_certification(&mut self, telemetry: Arc<TelemetryAgent>, computer: RecordId) {
        let preset_name = self.cfg.certification.preset_name.clone();
        let mult = self.cfg.certification.duration_multiplier;
        if let Err(e) = self.start_certification_by_name(&preset_name, mult, telemetry, computer) {
            self.cert_error = Some(e);
        }
    }

    /// Start a certification preset by name; for fleet/MCP-driven runs.
    pub fn start_certification_by_name(
        &mut self,
        preset_name: &str,
        mult: f32,
        telemetry: Arc<TelemetryAgent>,
        computer: RecordId,
    ) -> Result<(), String> {
        if self.is_running() {
            return Err("a stress run is already active".to_string());
        }
        let preset = load_cert_preset(preset_name)
            .map_err(|e| format!("preset '{preset_name}' failed to load: {e:#}"))?;
        let mult = mult.clamp(0.001, 1.0);
        let snapshot = telemetry.snapshot();
        let mut spec = if snapshot.memory.total_mb > 0 {
            let gpu_vram_mb = snapshot.gpus.iter().filter_map(|g| g.memory_total_mb).max();
            cert_spec(&preset, computer, snapshot.memory.total_mb, gpu_vram_mb, mult)
        } else {
            cert_spec_detected(&preset, computer, mult)
        };
        spec.tags.push("origin:tui".into());
        self.apply_order_context(&mut spec);
        self.publish_preset(&spec);
        self.cert_error = None;
        self.reset_run_state();
        self.run = Some(RunController::start(spec, telemetry));
        Ok(())
    }

    fn stop(&mut self) {
        if let Some(c) = self.run.as_ref() {
            c.stop();
        }
    }

    /// Cancel the active run; public for fleet/host control.
    pub fn stop_active_run(&mut self) {
        self.stop();
    }

    fn set_mode(&mut self, mode: PanelMode) {
        if self.is_running() {
            return;
        }
        self.focus = match mode {
            PanelMode::Single => Focus::Stressor,
            PanelMode::Scenario => Focus::Stage(0),
            PanelMode::QcBenchmark => Focus::QcMult,
            PanelMode::Certification => Focus::CertPreset,
        };
        self.cfg.mode = mode;
        self.refresh_cert_preview();
    }

    fn refresh_cert_preview(&mut self) {
        if self.cert_preview.as_ref().map(|p| p.name.as_str())
            != Some(self.cfg.certification.preset_name.as_str())
        {
            self.cert_preview = load_cert_preset(&self.cfg.certification.preset_name).ok();
        }
    }

    /// Per-stage progress from the live scenario state + verdicts.
    fn stage_progress_for(&self, index: usize, stage_dur_secs: f64) -> StageProg {
        let ss = &self.scenario_state;
        if ss.finished || index < ss.current_stage_index {
            return StageProg::Done { pass: self.stage_verdicts.get(index).map(|v| v.pass) };
        }
        if self.is_running() && index == ss.current_stage_index && ss.stage_count > 0 {
            let elapsed = (ss.total_elapsed_secs - ss.stage_started_at_elapsed).max(0.0);
            let frac = (elapsed / stage_dur_secs.max(1.0)).clamp(0.0, 1.0) as f32;
            return StageProg::Running(frac);
        }
        StageProg::Pending
    }

    // ---- dropdown menu helpers ----

    fn open_stressor_menu(&mut self, target: MenuTarget, anchor: Rect) {
        let current = match target {
            MenuTarget::Single => self.cfg.single.stressor,
            MenuTarget::Pending => self.pending_stage_pick,
            MenuTarget::Cert => return,
            MenuTarget::Stage(i) => self
                .cfg
                .scenario
                .stages
                .get(i)
                .map(|s| s.stressor)
                .unwrap_or(StressorChoice::Cpu),
        };
        let items: Vec<MenuItem> = StressorChoice::ALL
            .iter()
            .map(|c| MenuItem::new(c.label()).active(*c == current))
            .collect();
        self.stressor_anchor = anchor;
        self.stressor_menu.open_at(anchor, items, "Stressor");
        self.stressor_menu_target = Some(target);
    }

    fn open_cert_menu(&mut self, anchor: Rect) {
        let current = self.cfg.certification.preset_name.clone();
        let items: Vec<MenuItem> = stress_runner::CERT_PRESET_NAMES
            .iter()
            .map(|n| MenuItem::new(*n).active(*n == current))
            .collect();
        self.stressor_anchor = anchor;
        self.stressor_menu.open_at(anchor, items, "Preset");
        self.stressor_menu_target = Some(MenuTarget::Cert);
    }

    fn apply_menu_choice(&mut self, idx: usize) {
        match self.stressor_menu_target {
            Some(MenuTarget::Single) => {
                if let Some(c) = StressorChoice::ALL.get(idx) {
                    self.cfg.single.stressor = *c;
                }
            }
            Some(MenuTarget::Pending) => {
                if let Some(c) = StressorChoice::ALL.get(idx) {
                    self.pending_stage_pick = *c;
                }
            }
            Some(MenuTarget::Stage(i)) => {
                if let (Some(c), Some(stage)) =
                    (StressorChoice::ALL.get(idx), self.cfg.scenario.stages.get_mut(i))
                {
                    stage.stressor = *c;
                }
            }
            Some(MenuTarget::Cert) => {
                if let Some(n) = stress_runner::CERT_PRESET_NAMES.get(idx) {
                    self.cfg.certification.preset_name = n.to_string();
                    self.refresh_cert_preview();
                }
            }
            None => {}
        }
        self.stressor_menu.close();
        self.stressor_menu_target = None;
    }

    fn add_stage(&mut self) {
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
        self.cfg.scenario.stages.push(stage);
        let _ = get_update_sender().send(self.widget_id());
    }

    fn remove_stage(&mut self, i: usize) {
        if self.cfg.scenario.stages.len() > 1 && i < self.cfg.scenario.stages.len() {
            self.cfg.scenario.stages.remove(i);
            if let Focus::Stage(f) = self.focus {
                if f >= self.cfg.scenario.stages.len() {
                    self.focus = Focus::Stage(self.cfg.scenario.stages.len().saturating_sub(1));
                }
            }
            let _ = get_update_sender().send(self.widget_id());
        }
    }

    // ---- keyboard ----

    fn focus_order(&self) -> Vec<Focus> {
        match self.cfg.mode {
            PanelMode::Single => {
                let mut v = vec![Focus::Stressor, Focus::Threads];
                match self.cfg.single.stressor {
                    StressorChoice::Memory
                    | StressorChoice::Memcpy
                    | StressorChoice::Vm
                    | StressorChoice::MemTest
                    | StressorChoice::Linpack => v.push(Focus::Memory),
                    StressorChoice::Disk => v.push(Focus::Disk),
                    _ => {}
                }
                v.push(Focus::TimeoutToggle);
                if self.cfg.single.use_timeout {
                    v.push(Focus::Timeout);
                }
                v
            }
            PanelMode::Scenario => {
                let mut v: Vec<Focus> =
                    (0..self.cfg.scenario.stages.len()).map(Focus::Stage).collect();
                v.push(Focus::TotalToggle);
                if self.cfg.scenario.use_total {
                    v.push(Focus::TotalValue);
                    v.push(Focus::RepeatToggle);
                }
                v
            }
            PanelMode::QcBenchmark => vec![Focus::QcMult],
            PanelMode::Certification => vec![Focus::CertPreset, Focus::CertMult],
        }
    }

    fn move_focus(&mut self, delta: i32) {
        let order = self.focus_order();
        if order.is_empty() {
            return;
        }
        let cur = order.iter().position(|f| *f == self.focus).unwrap_or(0) as i32;
        let n = order.len() as i32;
        let next = ((cur + delta) % n + n) % n;
        self.focus = order[next as usize];
    }

    fn adjust(&mut self, dir: i32) {
        match self.focus {
            Focus::Stressor => {
                let anchor = self.stressor_anchor;
                self.open_stressor_menu(MenuTarget::Single, anchor);
            }
            Focus::Threads => {
                let v = &mut self.cfg.single.threads;
                *v = (*v as i64 + dir as i64).clamp(0, 64) as usize;
            }
            Focus::Memory => {
                let v = &mut self.cfg.single.memory_cap_mb;
                *v = (*v as i64 + dir as i64 * 16).clamp(16, 32768) as u64;
            }
            Focus::Disk => {
                let v = &mut self.cfg.single.disk_file_mb;
                *v = (*v as i64 + dir as i64).clamp(1, 512) as u64;
            }
            Focus::Timeout => {
                let v = &mut self.cfg.single.timeout_secs;
                *v = (*v as i64 + dir as i64).clamp(1, 3600) as u64;
            }
            Focus::TimeoutToggle => self.cfg.single.use_timeout = !self.cfg.single.use_timeout,
            Focus::QcMult => {
                let v = &mut self.cfg.qc_benchmark.duration_multiplier;
                *v = (*v + dir as f32 * 0.05).clamp(0.1, 4.0);
            }
            Focus::CertPreset => {
                let anchor = self.stressor_anchor;
                self.open_cert_menu(anchor);
            }
            Focus::CertMult => {
                let v = &mut self.cfg.certification.duration_multiplier;
                let factor = if dir > 0 { 1.5 } else { 1.0 / 1.5 };
                *v = (*v * factor).clamp(0.001, 1.0);
            }
            Focus::TotalToggle => self.cfg.scenario.use_total = !self.cfg.scenario.use_total,
            Focus::TotalValue => {
                let v = &mut self.cfg.scenario.total_wall_secs;
                *v = (*v as i64 + dir as i64 * 30).clamp(1, 86400) as u64;
            }
            Focus::RepeatToggle => {
                self.cfg.scenario.repeat_until_total = !self.cfg.scenario.repeat_until_total
            }
            Focus::Stage(i) => {
                if let Some(s) = self.cfg.scenario.stages.get_mut(i) {
                    s.duration_secs = (s.duration_secs as i64 + dir as i64).clamp(1, 3600) as u64;
                }
            }
        }
    }

    /// Dispatch a clicked zone id to the same internal method the keyboard uses.
    fn on_zone_click(&mut self, id: &str) {
        // Dropdown is modal: clicks pick a row, anything else closes it.
        if self.stressor_menu.is_open() {
            if let Some(rest) = id.strip_prefix("menu:") {
                if let Ok(idx) = rest.parse::<usize>() {
                    self.apply_menu_choice(idx);
                    return;
                }
            }
            self.stressor_menu.close();
            self.stressor_menu_target = None;
            return;
        }
        match id {
            "mode:single" => self.set_mode(PanelMode::Single),
            "mode:scenario" => self.set_mode(PanelMode::Scenario),
            "mode:qc" => self.set_mode(PanelMode::QcBenchmark),
            "mode:cert" => self.set_mode(PanelMode::Certification),
            "telemetry" => self.temps_open = !self.temps_open,
            "start" => {
                if self.is_running() {
                    self.stop();
                } else {
                    self.start();
                }
            }
            "single:stressor" => {
                let anchor = self.stressor_anchor;
                self.open_stressor_menu(MenuTarget::Single, anchor);
            }
            "single:threads:dec" => self.adjust_at(Focus::Threads, -1),
            "single:threads:inc" => self.adjust_at(Focus::Threads, 1),
            "single:mem:dec" => self.adjust_at(Focus::Memory, -1),
            "single:mem:inc" => self.adjust_at(Focus::Memory, 1),
            "single:disk:dec" => self.adjust_at(Focus::Disk, -1),
            "single:disk:inc" => self.adjust_at(Focus::Disk, 1),
            "single:timeout" => self.adjust_at(Focus::TimeoutToggle, 0),
            "single:timeout:dec" => self.adjust_at(Focus::Timeout, -1),
            "single:timeout:inc" => self.adjust_at(Focus::Timeout, 1),
            "qc:mult:dec" => self.adjust_at(Focus::QcMult, -1),
            "qc:mult:inc" => self.adjust_at(Focus::QcMult, 1),
            "cert:preset" => {
                let anchor = self.stressor_anchor;
                self.open_cert_menu(anchor);
            }
            "cert:mult:dec" => self.adjust_at(Focus::CertMult, -1),
            "cert:mult:inc" => self.adjust_at(Focus::CertMult, 1),
            "scn:add" => {
                if !self.is_running() {
                    self.add_stage();
                }
            }
            "scn:pick" => {
                if !self.is_running() {
                    let anchor = self.stressor_anchor;
                    self.open_stressor_menu(MenuTarget::Pending, anchor);
                }
            }
            "scn:total" => self.adjust_at(Focus::TotalToggle, 0),
            "scn:total:dec" => self.adjust_at(Focus::TotalValue, -1),
            "scn:total:inc" => self.adjust_at(Focus::TotalValue, 1),
            "scn:repeat" => self.adjust_at(Focus::RepeatToggle, 0),
            "verdict:report" => {
                if let Some(v) = &self.last_verdict {
                    self.report_request = Some(v.run_id.clone());
                }
            }
            "verdict:dismiss" => self.show_verdict = false,
            _ => {
                if let Some(rest) = id.strip_prefix("scn:stressor:") {
                    if let Ok(i) = rest.parse::<usize>() {
                        if !self.is_running() {
                            let anchor = self.stressor_anchor;
                            self.open_stressor_menu(MenuTarget::Stage(i), anchor);
                        }
                    }
                } else if let Some(rest) = id.strip_prefix("scn:dur:dec:") {
                    if let Ok(i) = rest.parse::<usize>() {
                        self.adjust_at(Focus::Stage(i), -1);
                    }
                } else if let Some(rest) = id.strip_prefix("scn:dur:inc:") {
                    if let Ok(i) = rest.parse::<usize>() {
                        self.adjust_at(Focus::Stage(i), 1);
                    }
                } else if let Some(rest) = id.strip_prefix("scn:up:") {
                    if let Ok(i) = rest.parse::<usize>() {
                        if !self.is_running() && i > 0 {
                            self.cfg.scenario.stages.swap(i - 1, i);
                        }
                    }
                } else if let Some(rest) = id.strip_prefix("scn:down:") {
                    if let Ok(i) = rest.parse::<usize>() {
                        if !self.is_running() && i + 1 < self.cfg.scenario.stages.len() {
                            self.cfg.scenario.stages.swap(i, i + 1);
                        }
                    }
                } else if let Some(rest) = id.strip_prefix("scn:del:") {
                    if let Ok(i) = rest.parse::<usize>() {
                        if !self.is_running() {
                            self.remove_stage(i);
                        }
                    }
                }
            }
        }
    }

    /// Focus a field then apply the keyboard adjustment to it; `dir == 0` flips a toggle.
    fn adjust_at(&mut self, focus: Focus, dir: i32) {
        if self.is_running() {
            return;
        }
        self.focus = focus;
        self.adjust(dir);
    }
}

fn to_run_stage(s: &ScenarioStageConfig) -> RunStage {
    RunStage {
        label: s.label.clone(),
        stressor: s.stressor.to_stressor(),
        threads: s.threads,
        duration_secs: s.duration_secs,
        memory_cap_mb: s.memory_cap_mb,
        disk_file_mb: s.disk_file_mb,
    }
}

fn verdict_label(v: &RunVerdict) -> String {
    use stress_runner::RunResult;
    match v.result {
        RunResult::Pass => "Pass".to_string(),
        RunResult::Fail => format!("Fail ({})", failure_mode_label(v)),
        RunResult::Aborted => "Aborted".to_string(),
        RunResult::Inconclusive => "Inconclusive".to_string(),
        RunResult::InProgress => "In progress".to_string(),
    }
}

fn failure_mode_label(v: &RunVerdict) -> &'static str {
    use stress_runner::FailureMode;
    match &v.failure_mode {
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

fn temp_color(c: f32) -> Color {
    if c < 60.0 {
        THEME.success
    } else if c < 85.0 {
        THEME.warning
    } else {
        THEME.error
    }
}

impl<'a> ActionHandler for StressTab<'a> {
    fn widget_id(&self) -> WidgetId {
        WidgetId("StressTab".to_string())
    }

    fn managed_widget_ids(&self) -> Vec<WidgetId> {
        let mut ids = vec![
            WidgetId(MODE_SINGLE.into()),
            WidgetId(MODE_SCENARIO.into()),
            WidgetId(MODE_QC.into()),
            WidgetId(MODE_CERT.into()),
            WidgetId(TELEMETRY_ID.into()),
            WidgetId(START_ID.into()),
            WidgetId(ADD_STAGE_ID.into()),
        ];
        for i in 0..self.cfg.scenario.stages.len() {
            ids.push(WidgetId(format!("StressStageUp{i}")));
            ids.push(WidgetId(format!("StressStageDown{i}")));
            ids.push(WidgetId(format!("StressStageDel{i}")));
        }
        ids
    }

    fn handle_event(&mut self, event: &WidgetEvent) {
        if let WidgetEvent::ButtonClick { widget_id, .. } = event {
            let id = widget_id.0.as_str();
            match id {
                MODE_SINGLE => self.set_mode(PanelMode::Single),
                MODE_SCENARIO => self.set_mode(PanelMode::Scenario),
                MODE_QC => self.set_mode(PanelMode::QcBenchmark),
                MODE_CERT => self.set_mode(PanelMode::Certification),
                TELEMETRY_ID => self.temps_open = !self.temps_open,
                START_ID => {
                    if self.is_running() {
                        self.stop();
                    } else {
                        self.start();
                    }
                }
                ADD_STAGE_ID => {
                    if !self.is_running() {
                        self.add_stage();
                    }
                }
                other => {
                    if let Some(rest) = other.strip_prefix("StressStageUp") {
                        if let Ok(i) = rest.parse::<usize>() {
                            if !self.is_running() && i > 0 {
                                self.cfg.scenario.stages.swap(i - 1, i);
                            }
                        }
                    } else if let Some(rest) = other.strip_prefix("StressStageDown") {
                        if let Ok(i) = rest.parse::<usize>() {
                            if !self.is_running() && i + 1 < self.cfg.scenario.stages.len() {
                                self.cfg.scenario.stages.swap(i, i + 1);
                            }
                        }
                    } else if let Some(rest) = other.strip_prefix("StressStageDel") {
                        if let Ok(i) = rest.parse::<usize>() {
                            if !self.is_running() {
                                self.remove_stage(i);
                            }
                        }
                    }
                }
            }
        }
    }
}

impl<'a> StressTab<'a> {
    fn draw_top_bar(&mut self, f: &mut Frame, area: Rect) {
        let running = self.is_running();
        self.mode_single.set_disabled(running);
        self.mode_scenario.set_disabled(running);
        self.mode_qc.set_disabled(running);
        self.mode_cert.set_disabled(running);
        self.mode_single.set_selected(matches!(self.cfg.mode, PanelMode::Single));
        self.mode_scenario.set_selected(matches!(self.cfg.mode, PanelMode::Scenario));
        self.mode_qc.set_selected(matches!(self.cfg.mode, PanelMode::QcBenchmark));
        self.mode_cert.set_selected(matches!(self.cfg.mode, PanelMode::Certification));
        self.start_btn.set_label(if running { "Stop".into() } else { "Start".into() });

        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(12),
                Constraint::Length(12),
                Constraint::Length(12),
                Constraint::Length(12),
                Constraint::Length(14),
                Constraint::Min(10),
                Constraint::Length(12),
            ])
            .split(area);
        self.mode_single.render_ref(cols[0], f.buffer_mut());
        self.mode_scenario.render_ref(cols[1], f.buffer_mut());
        self.mode_qc.render_ref(cols[2], f.buffer_mut());
        self.mode_cert.render_ref(cols[3], f.buffer_mut());
        self.telemetry_btn.render_ref(cols[4], f.buffer_mut());

        let run_text = match &self.last_run_id {
            Some(id) => {
                use database::schema::RecordIdExt;
                let key = id.key_string();
                let short = if key.len() > 14 { format!("{}..", &key[..14]) } else { key };
                let state = if running {
                    "running"
                } else if self.has_run() {
                    "stopped"
                } else {
                    "ready"
                };
                format!("run: {short}  [{state}]")
            }
            None => String::new(),
        };
        f.render_widget(
            Paragraph::new(Line::from(run_text).style(Style::default().fg(THEME.text_muted)))
                .wrap(Wrap { trim: true }),
            cols[5].inner(ratatui::layout::Margin { horizontal: 1, vertical: 1 }),
        );
        self.start_btn.render_ref(cols[6], f.buffer_mut());
    }

    fn draw_config(&mut self, f: &mut Frame, area: Rect) {
        match self.cfg.mode {
            PanelMode::Single => self.draw_single(f, area),
            PanelMode::Scenario => self.draw_scenario(f, area),
            PanelMode::QcBenchmark => self.draw_qc(f, area),
            PanelMode::Certification => self.draw_cert(f, area),
        }
    }

    fn focus_style(&self, focus: Focus) -> Style {
        if self.focus == focus {
            Style::default().fg(THEME.accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(THEME.text)
        }
    }

    fn draw_single(&mut self, f: &mut Frame, area: Rect) {
        let inner = config_block_inner(f, area, "Single stressor");
        let s = self.cfg.single.clone();
        let mut y = inner.y;

        self.draw_field_row(f, inner, &mut y, "Stressor", s.stressor.label(),
            self.focus_style(Focus::Stressor), Some("single:stressor"), None);
        let thr = if s.threads == 0 { "auto".to_string() } else { s.threads.to_string() };
        self.draw_field_row(f, inner, &mut y, "Threads", &thr,
            self.focus_style(Focus::Threads), None, Some("single:threads"));
        match s.stressor {
            StressorChoice::Memory
            | StressorChoice::Memcpy
            | StressorChoice::Vm
            | StressorChoice::MemTest
            | StressorChoice::Linpack => self.draw_field_row(f, inner, &mut y, "Memory cap",
                &format!("{} MiB", s.memory_cap_mb), self.focus_style(Focus::Memory),
                None, Some("single:mem")),
            StressorChoice::Disk => self.draw_field_row(f, inner, &mut y, "File size",
                &format!("{} MiB", s.disk_file_mb), self.focus_style(Focus::Disk),
                None, Some("single:disk")),
            _ => {}
        }
        self.draw_field_row(f, inner, &mut y, "Timeout", if s.use_timeout { "on" } else { "off" },
            self.focus_style(Focus::TimeoutToggle), Some("single:timeout"), None);
        if s.use_timeout {
            self.draw_field_row(f, inner, &mut y, "Timeout secs", &format!("{} s", s.timeout_secs),
                self.focus_style(Focus::Timeout), None, Some("single:timeout"));
        }
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "Up/Down focus - Left/Right adjust - Enter/click open select",
                Style::default().fg(THEME.text_muted),
            ))),
            Rect { x: inner.x, y, width: inner.width, height: 1 },
        );
    }

    /// One config row: label + value (+ optional steppers), registering zones.
    /// `toggle_zone` makes the value clickable; `stepper_prefix` adds `[-] [+]`.
    #[allow(clippy::too_many_arguments)]
    fn draw_field_row(
        &self,
        f: &mut Frame,
        inner: Rect,
        y: &mut u16,
        label: &str,
        value: &str,
        value_style: Style,
        toggle_zone: Option<&str>,
        stepper_prefix: Option<&str>,
    ) {
        if *y >= inner.bottom() {
            return;
        }
        let row = Rect { x: inner.x, y: *y, width: inner.width, height: 1 };
        f.render_widget(Paragraph::new(field_line(label, value, value_style)), row);
        if let Some(zone) = toggle_zone {
            let w = (label.len() + value.len() + 2) as u16;
            self.zones.add(Rect { x: row.x, y: row.y, width: w.min(row.width), height: 1 }, zone.to_string());
        }
        if let Some(prefix) = stepper_prefix {
            let dec_x = inner.right().saturating_sub(8);
            let inc_x = inner.right().saturating_sub(4);
            f.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled("[-]", Style::default().fg(THEME.tertiary)),
                    Span::raw(" "),
                    Span::styled("[+]", Style::default().fg(THEME.tertiary)),
                ])),
                Rect { x: dec_x, y: *y, width: 7, height: 1 },
            );
            self.zones.add(Rect { x: dec_x, y: *y, width: 3, height: 1 }, format!("{prefix}:dec"));
            self.zones.add(Rect { x: inc_x, y: *y, width: 3, height: 1 }, format!("{prefix}:inc"));
        }
        *y += 1;
    }

    fn draw_scenario(&mut self, f: &mut Frame, area: Rect) {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(6), Constraint::Length(3), Constraint::Length(3)])
            .split(area);

        // Stage list.
        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(SHORTCUT_SET)
            .border_style(THEME.border(false))
            .title("Stages  (click stressor/dur [-]/[+], up/dn/x; a add, p pick)");
        let inner = block.inner(rows[0]);
        f.render_widget(block, rows[0]);

        let header = Row::new(vec!["#", "Stressor", "Dur", "", "", "Prog", "Move"])
            .style(Style::default().fg(THEME.text_muted).add_modifier(Modifier::BOLD));
        let col = [
            Constraint::Length(3),
            Constraint::Length(12),
            Constraint::Length(8),
            Constraint::Length(4),
            Constraint::Length(4),
            Constraint::Length(8),
            Constraint::Min(12),
        ];
        f.render_widget(Table::new(Vec::<Row>::new(), col).header(header), inner);

        let stage_count = self.cfg.scenario.stages.len();
        let cols = Layout::default().direction(Direction::Horizontal).constraints(col).split(inner);
        for i in 0..stage_count {
            let y = inner.y + 1 + i as u16;
            if y >= inner.bottom() {
                break;
            }
            let st = self.cfg.scenario.stages[i].clone();
            let focused = self.focus == Focus::Stage(i);
            let dur_style = if focused {
                Style::default().fg(THEME.accent).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(THEME.text)
            };
            let prog = self.stage_progress_for(i, st.duration_secs as f64);
            let cell = |c: usize| Rect { x: cols[c].x, y, width: cols[c].width, height: 1 };
            f.render_widget(Paragraph::new((i + 1).to_string()), cell(0));
            f.render_widget(
                Paragraph::new(Span::styled(st.stressor.label(), Style::default().fg(THEME.text))),
                cell(1),
            );
            self.zones.add(cell(1), format!("scn:stressor:{i}"));
            f.render_widget(Paragraph::new(Span::styled(format!("{} s", st.duration_secs), dur_style)), cell(2));
            f.render_widget(Paragraph::new(Span::styled("[-]", Style::default().fg(THEME.tertiary))), cell(3));
            self.zones.add(cell(3), format!("scn:dur:dec:{i}"));
            f.render_widget(Paragraph::new(Span::styled("[+]", Style::default().fg(THEME.tertiary))), cell(4));
            self.zones.add(cell(4), format!("scn:dur:inc:{i}"));
            f.render_widget(Paragraph::new(stage_prog_span(prog)), cell(5));
            let move_cells = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Length(4), Constraint::Length(4), Constraint::Length(4)])
                .split(cell(6));
            f.render_widget(Paragraph::new(Span::styled("[up]", Style::default().fg(THEME.tertiary))), move_cells[0]);
            self.zones.add(move_cells[0], format!("scn:up:{i}"));
            f.render_widget(Paragraph::new(Span::styled("[dn]", Style::default().fg(THEME.tertiary))), move_cells[1]);
            self.zones.add(move_cells[1], format!("scn:down:{i}"));
            f.render_widget(Paragraph::new(Span::styled("[x]", Style::default().fg(THEME.error))), move_cells[2]);
            self.zones.add(move_cells[2], format!("scn:del:{i}"));
        }

        // Add-stage row.
        let add_cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(20), Constraint::Length(12), Constraint::Min(0)])
            .split(rows[1]);
        let pick_block = Block::default().borders(Borders::ALL).border_style(THEME.border(false));
        let pick_inner = pick_block.inner(add_cols[0]);
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("Pick: ", Style::default().fg(THEME.text_muted)),
                Span::styled(self.pending_stage_pick.label(), Style::default().fg(THEME.tertiary)),
            ]))
            .block(pick_block),
            add_cols[0],
        );
        self.zones.add(pick_inner, "scn:pick");
        self.add_stage_btn.render_ref(add_cols[1], f.buffer_mut());

        // Total wall + repeat.
        let total_block = Block::default().borders(Borders::ALL).border_style(THEME.border(false));
        let total_inner = total_block.inner(rows[2]);
        let total_style = if matches!(
            self.focus,
            Focus::TotalToggle | Focus::TotalValue | Focus::RepeatToggle
        ) {
            Style::default().fg(THEME.accent)
        } else {
            Style::default().fg(THEME.text)
        };
        f.render_widget(total_block, rows[2]);
        if total_inner.height > 0 {
            let tcols = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Length(14),
                    Constraint::Length(8),
                    Constraint::Length(4),
                    Constraint::Length(4),
                    Constraint::Length(12),
                ])
                .split(total_inner);
            f.render_widget(
                Paragraph::new(Span::styled(
                    format!("Total {}", if self.cfg.scenario.use_total { "on" } else { "off" }),
                    total_style,
                )),
                tcols[0],
            );
            self.zones.add(tcols[0], "scn:total");
            f.render_widget(
                Paragraph::new(Span::styled(format!("{} s", self.cfg.scenario.total_wall_secs), total_style)),
                tcols[1],
            );
            f.render_widget(Paragraph::new(Span::styled("[-]", Style::default().fg(THEME.tertiary))), tcols[2]);
            self.zones.add(tcols[2], "scn:total:dec");
            f.render_widget(Paragraph::new(Span::styled("[+]", Style::default().fg(THEME.tertiary))), tcols[3]);
            self.zones.add(tcols[3], "scn:total:inc");
            f.render_widget(
                Paragraph::new(Span::styled(
                    format!("repeat {}", if self.cfg.scenario.repeat_until_total { "on" } else { "off" }),
                    total_style,
                )),
                tcols[4],
            );
            self.zones.add(tcols[4], "scn:repeat");
        }
    }

    fn draw_qc(&mut self, f: &mut Frame, area: Rect) {
        let mult = self.cfg.qc_benchmark.duration_multiplier.clamp(0.1, 10.0);
        let total_secs = (mult * 20.0 * 8.0).round() as u64;
        let stages = crate::qc_benchmark::qc_benchmark_stages(mult);

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(4), Constraint::Min(4)])
            .split(area);

        let inner = config_block_inner(f, rows[0], "QC Benchmark");
        let mut y = inner.y;
        self.draw_field_row(f, inner, &mut y, "Duration mult", &format!("{mult:.2} x"),
            self.focus_style(Focus::QcMult), None, Some("qc:mult"));
        f.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    format!("QC Benchmark v1  ~ {total_secs} s total"),
                    Style::default().fg(THEME.text_muted),
                )),
                Line::from(Span::styled(
                    "Left/Right or click adjust multiplier",
                    Style::default().fg(THEME.text_muted),
                )),
            ]),
            Rect { x: inner.x, y, width: inner.width, height: inner.bottom().saturating_sub(y) },
        );

        let stage_rows: Vec<(String, &'static str, f64)> = stages
            .iter()
            .map(|s| (s.label.clone(), s.stressor.label(), (s.duration_secs as f64).max(1.0)))
            .collect();
        self.draw_readonly_stage_grid(f, rows[1], &stage_rows);
    }

    fn draw_cert(&mut self, f: &mut Frame, area: Rect) {
        self.refresh_cert_preview();
        let mult = self.cfg.certification.duration_multiplier.clamp(0.001, 1.0);
        let preset_name = self.cfg.certification.preset_name.clone();
        let description = self.cert_preview.as_ref().map(|p| p.description.clone());
        let total_secs = self
            .cert_preview
            .as_ref()
            .map(|p| (p.total_secs() as f64 * mult as f64).round() as u64)
            .unwrap_or(0);
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

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(6), Constraint::Min(4)])
            .split(area);

        let inner = config_block_inner(f, rows[0], "Certification");
        let mut y = inner.y;
        self.draw_field_row(f, inner, &mut y, "Preset", &preset_name,
            self.focus_style(Focus::CertPreset), Some("cert:preset"), None);
        self.draw_field_row(f, inner, &mut y, "Duration mult",
            &format!("{mult:.3} x  (~ {:.1} min)", total_secs as f64 / 60.0),
            self.focus_style(Focus::CertMult), None, Some("cert:mult"));
        let mut tail: Vec<Line> = Vec::new();
        if let Some(desc) = &description {
            tail.push(Line::from(Span::styled(desc.clone(), Style::default().fg(THEME.text_muted))));
        }
        if let Some(err) = &self.cert_error {
            tail.push(Line::from(Span::styled(err.clone(), Style::default().fg(THEME.error))));
        }
        tail.push(Line::from(Span::styled(
            "Up/Down focus - Left/Right adjust - Enter/click pick preset",
            Style::default().fg(THEME.text_muted),
        )));
        if y < inner.bottom() {
            f.render_widget(
                Paragraph::new(tail).style(Style::default().fg(THEME.text)),
                Rect { x: inner.x, y, width: inner.width, height: inner.bottom().saturating_sub(y) },
            );
        }

        if stage_rows.is_empty() {
            f.render_widget(
                Paragraph::new(format!("preset '{preset_name}' failed to load"))
                    .style(Style::default().fg(THEME.error)),
                rows[1],
            );
        } else {
            self.draw_readonly_stage_grid(f, rows[1], &stage_rows);
        }
    }

    fn draw_readonly_stage_grid(
        &self,
        f: &mut Frame,
        area: Rect,
        stages: &[(String, &'static str, f64)],
    ) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(THEME.border(false))
            .title("Stages");
        let inner = block.inner(area);
        f.render_widget(block, area);

        let header = Row::new(vec!["Stage", "Stressor", "Duration", "Progress", "Peak"])
            .style(Style::default().fg(THEME.text_muted).add_modifier(Modifier::BOLD));
        let rows: Vec<Row> = stages
            .iter()
            .enumerate()
            .map(|(i, (label, stressor, dur))| {
                let prog = self.stage_progress_for(i, *dur);
                let peak = self
                    .stage_verdicts
                    .get(i)
                    .and_then(|v| v.peak_throughput)
                    .map(|p| format!("{p:.1}"))
                    .unwrap_or_else(|| "-".into());
                Row::new(vec![
                    Cell::from(label.clone()),
                    Cell::from(*stressor),
                    Cell::from(format!("{:.1} min", dur / 60.0)),
                    stage_prog_cell(prog),
                    Cell::from(peak),
                ])
            })
            .collect();
        let table = Table::new(
            rows,
            [
                Constraint::Length(14),
                Constraint::Length(12),
                Constraint::Length(10),
                Constraint::Min(10),
                Constraint::Length(10),
            ],
        )
        .header(header)
        .style(Style::default().fg(THEME.text));
        f.render_widget(table, inner);
    }

    fn draw_status(&mut self, f: &mut Frame, area: Rect) {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2), // overall gauge / stage label
                Constraint::Length(4), // metrics
                Constraint::Length(3), // sparkline
                Constraint::Min(3),    // verdict / status
            ])
            .split(area);

        let ss = &self.scenario_state;
        if self.is_running() && self.cfg.scenario.use_total && self.cfg.scenario.total_wall_secs > 0
        {
            let frac = (ss.total_elapsed_secs / self.cfg.scenario.total_wall_secs as f64)
                .clamp(0.0, 1.0);
            let gauge = Gauge::default()
                .gauge_style(Style::default().fg(THEME.accent))
                .ratio(frac)
                .label(format!(
                    "Overall {:.0}/{} s",
                    ss.total_elapsed_secs, self.cfg.scenario.total_wall_secs
                ));
            f.render_widget(gauge, rows[0]);
        } else if ss.finished {
            if let Some(label) = &ss.finish_label {
                f.render_widget(
                    Paragraph::new(format!("{label} - {:.1} s total", ss.total_elapsed_secs))
                        .style(Style::default().fg(THEME.text).add_modifier(Modifier::BOLD)),
                    rows[0],
                );
            }
        } else if self.is_running() && ss.stage_count > 0 {
            f.render_widget(
                Paragraph::new(format!(
                    "Stage {}/{}: {}",
                    ss.current_stage_index + 1,
                    ss.stage_count,
                    ss.current_stage_label
                ))
                .style(Style::default().fg(THEME.text).add_modifier(Modifier::BOLD)),
                rows[0],
            );
        }

        if let Some(m) = &self.latest {
            let mut lines = vec![
                Line::from(format!("Elapsed     {:.1} s", m.elapsed_secs)),
                Line::from(Span::styled(
                    format!("Throughput  {:.2} {}", m.throughput, m.throughput_unit),
                    Style::default().fg(THEME.text).add_modifier(Modifier::BOLD),
                )),
            ];
            if let Some(e) = &m.last_error {
                lines.push(Line::from(Span::styled(
                    format!("Warning     {e}"),
                    Style::default().fg(THEME.warning),
                )));
            }
            f.render_widget(
                Paragraph::new(lines).style(Style::default().fg(THEME.text)),
                rows[1],
            );
        }

        if self.history.len() > 1 {
            let data: Vec<u64> = self.history.iter().map(|v| v.max(0.0) as u64).collect();
            let spark = Sparkline::default()
                .block(Block::default().borders(Borders::ALL).border_style(THEME.border(false)))
                .data(&data)
                .style(Style::default().fg(THEME.success));
            f.render_widget(spark, rows[2]);
        }

        if self.show_verdict {
            if let Some(v) = self.last_verdict.clone() {
                self.draw_verdict(f, rows[3], &v);
            }
        } else if !self.status.is_empty() {
            f.render_widget(
                Paragraph::new(self.status.as_str())
                    .wrap(Wrap { trim: true })
                    .style(Style::default().fg(THEME.warning).bg(APP_BACKGROUND)),
                rows[3],
            );
        }
    }

    fn draw_verdict(&self, f: &mut Frame, area: Rect, v: &RunVerdict) {
        use stress_runner::RunResult;
        let (text, color) = match v.result {
            RunResult::Pass => (format!("PASS - {:.1} s", v.duration_secs), THEME.success),
            RunResult::Fail => (
                format!("FAIL ({}) - {:.1} s", failure_mode_label(v), v.duration_secs),
                THEME.error,
            ),
            RunResult::Aborted => (format!("ABORTED - {:.1} s", v.duration_secs), THEME.warning),
            RunResult::Inconclusive => {
                (format!("INCONCLUSIVE - {:.1} s", v.duration_secs), THEME.text_muted)
            }
            RunResult::InProgress => ("IN PROGRESS".to_string(), THEME.tertiary),
        };
        let mut lines = vec![
            Line::from(Span::styled(
                text,
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                "[R] View report   [D] Dismiss",
                Style::default().fg(THEME.text_muted),
            )),
        ];
        for row in self.stage_verdicts.iter().filter(|r| !r.pass) {
            for violation in &row.violations {
                lines.push(Line::from(Span::styled(
                    format!("{}: {}", row.label, violation),
                    Style::default().fg(THEME.error),
                )));
            }
        }
        f.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: true })
                .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(color))),
            area,
        );
        // "[R] View report" / "[D] Dismiss" sit on the block's second inner line.
        let ctrl_y = area.y + 2;
        if ctrl_y < area.bottom() {
            self.zones.add(Rect { x: area.x + 1, y: ctrl_y, width: 16, height: 1 }, "verdict:report");
            self.zones.add(Rect { x: area.x + 18, y: ctrl_y, width: 11, height: 1 }, "verdict:dismiss");
        }
    }

    fn draw_telemetry_pane(&self, f: &mut Frame, area: Rect) {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(8), Constraint::Min(8)])
            .split(area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(THEME.border(false))
            .title("Temperatures");
        let inner = block.inner(rows[0]);
        f.render_widget(block, rows[0]);

        if self.latest_thermals.is_empty() && self.latest_gpu_temps.is_empty() {
            f.render_widget(
                Paragraph::new("No sensors yet. CPU/board temps need WinRing0 (run elevated).")
                    .wrap(Wrap { trim: true })
                    .style(Style::default().fg(THEME.text_muted)),
                inner,
            );
        } else {
            let mut lines: Vec<Line> = Vec::new();
            for t in &self.latest_thermals {
                lines.push(temp_line(&t.label, t.temp_c));
            }
            for (name, t) in &self.latest_gpu_temps {
                lines.push(temp_line(name, *t));
            }
            f.render_widget(Paragraph::new(lines), inner);
        }

        self.charts.render(f, rows[1]);
    }
}

fn temp_line(label: &str, c: f32) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label:<22}"), Style::default().fg(THEME.text)),
        Span::styled(format!("{c:.1} C"), Style::default().fg(temp_color(c))),
    ])
}

fn field_line<'b>(label: &str, value: &str, value_style: Style) -> Line<'b> {
    Line::from(vec![
        Span::styled(format!("{label:<14}"), Style::default().fg(THEME.text_muted)),
        Span::styled(value.to_string(), value_style),
    ])
}

/// Render the titled config block and return its inner rect.
fn config_block_inner(f: &mut Frame, area: Rect, title: &str) -> Rect {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_set(SHORTCUT_SET)
        .border_style(THEME.border(false))
        .title_style(THEME.title())
        .title(title.to_string());
    let inner = block.inner(area);
    f.render_widget(block, area);
    inner
}

fn stage_prog_cell(prog: StageProg) -> Cell<'static> {
    let (text, color) = stage_prog_text(prog);
    Cell::from(text).style(Style::default().fg(color))
}

fn stage_prog_span(prog: StageProg) -> Span<'static> {
    let (text, color) = stage_prog_text(prog);
    Span::styled(text, Style::default().fg(color))
}

fn stage_prog_text(prog: StageProg) -> (String, Color) {
    match prog {
        StageProg::Done { pass } => match pass {
            Some(true) => ("pass OK".into(), THEME.success),
            Some(false) => ("fail X".into(), THEME.error),
            None => ("done".into(), THEME.text_muted),
        },
        StageProg::Running(frac) => (format!("{:.0}%", frac * 100.0), THEME.accent),
        StageProg::Pending => ("-".into(), THEME.text_muted),
    }
}

impl<'a> HandleWidget<'a> for StressTab<'a> {
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {
        self.zones.begin();
        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(SHORTCUT_SET)
            .border_style(THEME.border(false))
            .title_style(THEME.title())
            .title("Stress test");
        (&block).render(area, f.buffer_mut());
        let inner = block.inner(area);

        let main = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(8)])
            .margin(1)
            .split(inner);
        self.draw_top_bar(f, main[0]);

        // Split content: config+status, optional telemetry pane.
        let body = if self.temps_open {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
                .split(main[1])
        } else {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(100)])
                .split(main[1])
        };

        let cfg_status = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(55), Constraint::Percentage(45)])
            .split(body[0]);
        self.stressor_anchor = Rect {
            x: cfg_status[0].x + 2,
            y: cfg_status[0].y + 1,
            width: 18,
            height: 1,
        };
        self.draw_config(f, cfg_status[0]);
        self.draw_status(f, cfg_status[1]);

        if self.temps_open {
            self.draw_telemetry_pane(f, body[1]);
        }

        // Dropdown overlay on top.
        if self.stressor_menu.is_open() {
            self.stressor_menu.render(f, f.area());
            for (rect, idx) in self.stressor_menu.item_rects() {
                self.zones.add(rect, format!("menu:{idx}"));
            }
        }
    }

    fn handle_mouse_event(&self, ev: &MouseEvent) {
        self.mode_single.handle_mouse_event(ev);
        self.mode_scenario.handle_mouse_event(ev);
        self.mode_qc.handle_mouse_event(ev);
        self.mode_cert.handle_mouse_event(ev);
        self.telemetry_btn.handle_mouse_event(ev);
        self.start_btn.handle_mouse_event(ev);
        self.add_stage_btn.handle_mouse_event(ev);
        self.zones.on_mouse(ev);
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> bool {
        // Dropdown captures keys while open.
        if self.stressor_menu.is_open() {
            match key.code {
                KeyCode::Esc => {
                    self.stressor_menu.close();
                    self.stressor_menu_target = None;
                }
                KeyCode::Down => self.stressor_menu.select_next(),
                KeyCode::Up => self.stressor_menu.select_prev(),
                KeyCode::Enter => {
                    if let Some(idx) = self.stressor_menu.selected() {
                        self.apply_menu_choice(idx);
                    }
                }
                _ => {}
            }
            return true;
        }

        match key.code {
            KeyCode::Char('1') => self.set_mode(PanelMode::Single),
            KeyCode::Char('2') => self.set_mode(PanelMode::Scenario),
            KeyCode::Char('3') => self.set_mode(PanelMode::QcBenchmark),
            KeyCode::Char('4') => self.set_mode(PanelMode::Certification),
            KeyCode::Char('t') | KeyCode::Char('T') => self.temps_open = !self.temps_open,
            KeyCode::Char('s') | KeyCode::Char('S') => {
                if self.is_running() {
                    self.stop();
                } else {
                    self.start();
                }
            }
            KeyCode::Char('a') | KeyCode::Char('A')
                if matches!(self.cfg.mode, PanelMode::Scenario) && !self.is_running() =>
            {
                self.add_stage();
            }
            KeyCode::Char('p') | KeyCode::Char('P')
                if matches!(self.cfg.mode, PanelMode::Scenario) && !self.is_running() =>
            {
                let anchor = self.stressor_anchor;
                self.open_stressor_menu(MenuTarget::Pending, anchor);
            }
            KeyCode::Char('r') | KeyCode::Char('R') if self.show_verdict => {
                if let Some(v) = &self.last_verdict {
                    self.report_request = Some(v.run_id.clone());
                }
            }
            KeyCode::Char('d') | KeyCode::Char('D') if self.show_verdict => {
                self.show_verdict = false;
            }
            KeyCode::Up if !self.is_running() => self.move_focus(-1),
            KeyCode::Down if !self.is_running() => self.move_focus(1),
            KeyCode::Left if !self.is_running() => self.adjust(-1),
            KeyCode::Right if !self.is_running() => self.adjust(1),
            KeyCode::Enter if !self.is_running() => {
                let anchor = self.stressor_anchor;
                match self.focus {
                    Focus::Stressor => self.open_stressor_menu(MenuTarget::Single, anchor),
                    Focus::CertPreset => self.open_cert_menu(anchor),
                    Focus::Stage(i) => self.open_stressor_menu(MenuTarget::Stage(i), anchor),
                    _ => {}
                }
            }
            _ => {}
        }
        true
    }
}
