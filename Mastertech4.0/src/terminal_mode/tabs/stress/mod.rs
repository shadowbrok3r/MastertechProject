//! Terminal-mode stress-test tab.
//!
//! A ratatui renderer over [`stress_runner::RunController`] with first-class
//! mouse support: every clickable control registers its `Rect` during `draw`,
//! and `handle_mouse_event` resolves a click against the previous frame's
//! rects (recorded into `pending`, applied at the top of the next `draw`) —
//! the same convention the rest of `terminal_mode` uses. All execution and the
//! strict `hardware_component` / `stress_test_run` / `stress_test_metric` /
//! `stress_test_event` persistence happen inside the controller worker.

use std::cell::RefCell;

use ratatui::{
    crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind},
    layout::{Constraint, Direction, Layout, Position, Rect},
    prelude::Backend,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Sparkline},
    Frame,
};
use stress_runner::{
    build_run_spec, is_stress_active, PanelMode, RecordId, RunController, RunResult, RunUpdate,
    RunVerdict, ScenarioStageConfig, StressPanelConfig, StressRunContext, StressorChoice,
    CERT_PRESET_NAMES,
};

use crate::filesystem::local_computer_record;
use crate::filesystem::system_info::shared_telemetry_agent;
use crate::terminal_mode::widgets::{HandleWidget, ShrinkArea, SHORTCUT_SET};

const ACCENT: Color = Color::Rgb(120, 200, 220);
const OK: Color = Color::Rgb(120, 220, 140);
const BAD: Color = Color::Rgb(255, 100, 100);
const WARN: Color = Color::Rgb(255, 180, 80);
const DIM: Color = Color::Rgb(150, 160, 170);

/// A clickable control, resolved against the previous frame's rects.
#[derive(Clone, PartialEq)]
enum Act {
    SetMode(PanelMode),
    StartStop,
    DismissVerdict,
    // single
    SingleStressor(i32),
    SingleThreads(i32),
    SingleToggleTimeout,
    SingleTimeout(i64),
    SingleMem(i64),
    SingleDisk(i64),
    // scenario
    ScnAddStage,
    ScnRemoveStage(usize),
    ScnStageStressor(usize, i32),
    ScnStageDur(usize, i64),
    ScnStageThreads(usize, i32),
    ScnToggleTotal,
    ScnTotal(i64),
    ScnToggleRepeat,
    // qc benchmark
    QcMult(i32),
    // certification
    CertPreset(i32),
    CertMult(i32),
    // concurrent
    ConcToggleLane(StressorChoice),
    ConcToggleTimeout,
    ConcDuration(i64),
    ConcMem(i64),
    ConcDisk(i64),
}

#[derive(Default, Clone)]
struct LatestMetrics {
    elapsed_secs: f64,
    throughput: f64,
    last_error: Option<String>,
    throughput_unit: &'static str,
}

#[derive(Clone)]
struct LaneLive {
    index: u32,
    label: String,
    throughput: f64,
    unit: &'static str,
    errors: u64,
}

#[derive(Default)]
struct ScenarioState {
    current_stage_index: usize,
    current_stage_label: String,
    stage_count: usize,
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

pub struct StressTab {
    cfg: StressPanelConfig,
    run: Option<RunController>,
    latest: Option<LatestMetrics>,
    scenario_state: ScenarioState,
    history: Vec<u64>,
    stage_verdicts: Vec<StageVerdictRow>,
    concurrent_lanes: Vec<LaneLive>,
    last_run_id: Option<RecordId>,
    last_verdict: Option<RunVerdict>,
    show_verdict: bool,
    start_error: Option<String>,
    zones: RefCell<Vec<(Rect, Act)>>,
    pending: RefCell<Vec<Act>>,
    hovered: RefCell<Option<Act>>,
}

impl StressTab {
    pub fn new() -> Self {
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
            zones: RefCell::new(Vec::new()),
            pending: RefCell::new(Vec::new()),
            hovered: RefCell::new(None),
        }
    }

    pub fn is_running(&self) -> bool {
        self.run.as_ref().map(|c| c.is_running()).unwrap_or(false)
    }

    // -- lifecycle ----------------------------------------------------------

    fn poll_controller(&mut self) {
        let Some(controller) = self.run.as_ref() else {
            return;
        };
        let running = controller.is_running();
        for update in controller.poll() {
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
                self.concurrent_lanes.clear();
            }
            RunUpdate::StageStarted { index, label, stage_count } => {
                self.scenario_state = ScenarioState {
                    current_stage_index: index,
                    current_stage_label: label,
                    stage_count,
                    finished: false,
                    finish_label: None,
                    total_elapsed_secs: self.scenario_state.total_elapsed_secs,
                };
                self.history.clear();
            }
            RunUpdate::Tick { stage_index, stage_label, metrics, telemetry: _, throughput_unit } => {
                if let Some(idx) = stage_index {
                    self.upsert_lane(idx, stage_label, metrics.throughput, metrics.errors, throughput_unit);
                }
                self.history.push(metrics.throughput.max(0.0) as u64);
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
                self.stage_verdicts.push(StageVerdictRow { label, pass, violations, peak_throughput });
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
        unit: &'static str,
    ) {
        if let Some(lane) = self.concurrent_lanes.iter_mut().find(|l| l.index == index) {
            lane.throughput = throughput;
            lane.errors = errors;
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
        let ctx = StressRunContext::new("mtech", "tui");
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

    // -- mouse action application ------------------------------------------

    fn apply_pending(&mut self) {
        let actions: Vec<Act> = self.pending.borrow_mut().drain(..).collect();
        for a in actions {
            self.apply(a);
        }
    }

    fn apply(&mut self, act: Act) {
        let running = self.is_running();
        match act {
            Act::SetMode(m) => {
                if !running {
                    self.cfg.mode = m;
                }
            }
            Act::StartStop => {
                if running {
                    self.stop();
                } else {
                    self.start();
                }
            }
            Act::DismissVerdict => self.show_verdict = false,
            _ if running => {}
            // single
            Act::SingleStressor(d) => {
                self.cfg.single.stressor = cycle_stressor(self.cfg.single.stressor, d)
            }
            Act::SingleThreads(d) => {
                self.cfg.single.threads = adj_usize(self.cfg.single.threads, d, 0, 1024)
            }
            Act::SingleToggleTimeout => self.cfg.single.use_timeout = !self.cfg.single.use_timeout,
            Act::SingleTimeout(d) => {
                self.cfg.single.timeout_secs = adj_u64(self.cfg.single.timeout_secs, d, 1, 86_400)
            }
            Act::SingleMem(d) => {
                self.cfg.single.memory_cap_mb = adj_u64(self.cfg.single.memory_cap_mb, d, 16, 1_048_576)
            }
            Act::SingleDisk(d) => {
                self.cfg.single.disk_file_mb = adj_u64(self.cfg.single.disk_file_mb, d, 1, 1_048_576)
            }
            // scenario
            Act::ScnAddStage => self.cfg.scenario.stages.push(ScenarioStageConfig::default_cpu()),
            Act::ScnRemoveStage(i) => {
                if self.cfg.scenario.stages.len() > 1 && i < self.cfg.scenario.stages.len() {
                    self.cfg.scenario.stages.remove(i);
                }
            }
            Act::ScnStageStressor(i, d) => {
                if let Some(s) = self.cfg.scenario.stages.get_mut(i) {
                    s.stressor = cycle_stressor(s.stressor, d);
                }
            }
            Act::ScnStageDur(i, d) => {
                if let Some(s) = self.cfg.scenario.stages.get_mut(i) {
                    s.duration_secs = adj_u64(s.duration_secs, d, 1, 86_400);
                }
            }
            Act::ScnStageThreads(i, d) => {
                if let Some(s) = self.cfg.scenario.stages.get_mut(i) {
                    s.threads = adj_usize(s.threads, d, 0, 1024);
                }
            }
            Act::ScnToggleTotal => self.cfg.scenario.use_total = !self.cfg.scenario.use_total,
            Act::ScnTotal(d) => {
                self.cfg.scenario.total_wall_secs = adj_u64(self.cfg.scenario.total_wall_secs, d, 1, 604_800)
            }
            Act::ScnToggleRepeat => {
                self.cfg.scenario.repeat_until_total = !self.cfg.scenario.repeat_until_total
            }
            // qc
            Act::QcMult(d) => {
                let v = (self.cfg.qc_benchmark.duration_multiplier * 10.0).round() + d as f32;
                self.cfg.qc_benchmark.duration_multiplier = (v / 10.0).clamp(0.1, 10.0);
            }
            // cert
            Act::CertPreset(d) => {
                self.cfg.certification.preset_name = cycle_cert(&self.cfg.certification.preset_name, d)
            }
            Act::CertMult(d) => {
                let v = (self.cfg.certification.duration_multiplier * 100.0).round() + (d * 5) as f32;
                self.cfg.certification.duration_multiplier = (v / 100.0).clamp(0.01, 1.0);
            }
            // concurrent
            Act::ConcToggleLane(c) => {
                if self.cfg.concurrent.lanes.contains(&c) {
                    self.cfg.concurrent.lanes.retain(|l| *l != c);
                } else {
                    self.cfg.concurrent.lanes.push(c);
                }
            }
            Act::ConcToggleTimeout => {
                self.cfg.concurrent.use_timeout = !self.cfg.concurrent.use_timeout
            }
            Act::ConcDuration(d) => {
                self.cfg.concurrent.duration_secs = adj_u64(self.cfg.concurrent.duration_secs, d, 1, 86_400)
            }
            Act::ConcMem(d) => {
                self.cfg.concurrent.memory_cap_mb = adj_u64(self.cfg.concurrent.memory_cap_mb, d, 16, 1_048_576)
            }
            Act::ConcDisk(d) => {
                self.cfg.concurrent.disk_file_mb = adj_u64(self.cfg.concurrent.disk_file_mb, d, 1, 1_048_576)
            }
        }
    }

    // -- zone/button helpers -----------------------------------------------

    fn register(&self, rect: Rect, act: Act) {
        self.zones.borrow_mut().push((rect, act));
    }

    fn is_hovered(&self, act: &Act) -> bool {
        self.hovered.borrow().as_ref() == Some(act)
    }

    /// Render a bracketed clickable button at the cursor; advances `x`.
    fn button(&self, f: &mut Frame, x: &mut u16, y: u16, max_x: u16, label: &str, act: Act, active: bool) {
        let text = format!("[{label}]");
        let w = text.chars().count() as u16;
        if *x + w > max_x {
            return;
        }
        let rect = Rect { x: *x, y, width: w, height: 1 };
        let mut style = Style::default();
        if active {
            style = style.fg(Color::Black).bg(ACCENT).add_modifier(Modifier::BOLD);
        } else if self.is_hovered(&act) {
            style = style.fg(ACCENT).add_modifier(Modifier::BOLD);
        } else {
            style = style.fg(DIM);
        }
        f.render_widget(Paragraph::new(text).style(style), rect);
        self.register(rect, act);
        *x += w + 1;
    }

    fn label_at(&self, f: &mut Frame, x: &mut u16, y: u16, max_x: u16, text: &str, color: Color) {
        let w = text.chars().count() as u16;
        if *x + w > max_x {
            return;
        }
        f.render_widget(
            Paragraph::new(text.to_string()).style(Style::default().fg(color)),
            Rect { x: *x, y, width: w, height: 1 },
        );
        *x += w + 1;
    }

    // -- per-mode config rendering -----------------------------------------

    fn draw_config(&self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(SHORTCUT_SET)
            .title(" Configuration ")
            .style(Style::default().fg(DIM));
        let inner = block.inner(area);
        f.render_widget(block, area);
        match self.cfg.mode {
            PanelMode::Single => self.cfg_single(f, inner),
            PanelMode::Scenario => self.cfg_scenario(f, inner),
            PanelMode::QcBenchmark => self.cfg_qc(f, inner),
            PanelMode::Certification => self.cfg_cert(f, inner),
            PanelMode::Concurrent => self.cfg_concurrent(f, inner),
        }
    }

    fn cfg_single(&self, f: &mut Frame, area: Rect) {
        let c = &self.cfg.single;
        let max_x = area.x + area.width;
        let mut y = area.y;
        let stepper = |s: &Self, f: &mut Frame, y: u16, name: &str, val: String, dec: Act, inc: Act| {
            let mut x = area.x;
            s.label_at(f, &mut x, y, max_x, &format!("{name}:"), DIM);
            s.button(f, &mut x, y, max_x, "-", dec, false);
            s.label_at(f, &mut x, y, max_x, &val, Color::White);
            s.button(f, &mut x, y, max_x, "+", inc, false);
        };
        let mut x = area.x;
        self.label_at(f, &mut x, y, max_x, "Stressor:", DIM);
        self.button(f, &mut x, y, max_x, "<", Act::SingleStressor(-1), false);
        self.label_at(f, &mut x, y, max_x, c.stressor.label(), ACCENT);
        self.button(f, &mut x, y, max_x, ">", Act::SingleStressor(1), false);
        y += 1;
        stepper(self, f, y, "Threads (0=auto)", c.threads.to_string(), Act::SingleThreads(-1), Act::SingleThreads(1));
        y += 1;
        let mut x = area.x;
        self.label_at(f, &mut x, y, max_x, "Timeout:", DIM);
        self.button(f, &mut x, y, max_x, if c.use_timeout { "on" } else { "off" }, Act::SingleToggleTimeout, c.use_timeout);
        if c.use_timeout {
            self.button(f, &mut x, y, max_x, "-", Act::SingleTimeout(-10), false);
            self.label_at(f, &mut x, y, max_x, &format!("{}s", c.timeout_secs), Color::White);
            self.button(f, &mut x, y, max_x, "+", Act::SingleTimeout(10), false);
        }
        y += 1;
        stepper(self, f, y, "Mem cap (MB)", c.memory_cap_mb.to_string(), Act::SingleMem(-64), Act::SingleMem(64));
        y += 1;
        stepper(self, f, y, "Disk file (MB)", c.disk_file_mb.to_string(), Act::SingleDisk(-16), Act::SingleDisk(16));
    }

    fn cfg_scenario(&self, f: &mut Frame, area: Rect) {
        let max_x = area.x + area.width;
        let mut y = area.y;
        let mut x = area.x;
        self.label_at(f, &mut x, y, max_x, "Stages:", DIM);
        self.button(f, &mut x, y, max_x, "+ add", Act::ScnAddStage, false);
        y += 1;
        for (i, s) in self.cfg.scenario.stages.iter().enumerate() {
            if y >= area.y + area.height {
                break;
            }
            let mut x = area.x;
            self.label_at(f, &mut x, y, max_x, &format!("{}.", i + 1), DIM);
            self.button(f, &mut x, y, max_x, "<", Act::ScnStageStressor(i, -1), false);
            self.label_at(f, &mut x, y, max_x, s.stressor.label(), ACCENT);
            self.button(f, &mut x, y, max_x, ">", Act::ScnStageStressor(i, 1), false);
            self.button(f, &mut x, y, max_x, "-", Act::ScnStageDur(i, -10), false);
            self.label_at(f, &mut x, y, max_x, &format!("{}s", s.duration_secs), Color::White);
            self.button(f, &mut x, y, max_x, "+", Act::ScnStageDur(i, 10), false);
            self.button(f, &mut x, y, max_x, "thr-", Act::ScnStageThreads(i, -1), false);
            self.label_at(f, &mut x, y, max_x, &format!("t{}", s.threads), DIM);
            self.button(f, &mut x, y, max_x, "thr+", Act::ScnStageThreads(i, 1), false);
            self.button(f, &mut x, y, max_x, "x", Act::ScnRemoveStage(i), false);
            y += 1;
        }
        let sc = &self.cfg.scenario;
        let mut x = area.x;
        self.label_at(f, &mut x, y, max_x, "Total cap:", DIM);
        self.button(f, &mut x, y, max_x, if sc.use_total { "on" } else { "off" }, Act::ScnToggleTotal, sc.use_total);
        if sc.use_total {
            self.button(f, &mut x, y, max_x, "-", Act::ScnTotal(-30), false);
            self.label_at(f, &mut x, y, max_x, &format!("{}s", sc.total_wall_secs), Color::White);
            self.button(f, &mut x, y, max_x, "+", Act::ScnTotal(30), false);
            self.button(f, &mut x, y, max_x, if sc.repeat_until_total { "repeat:on" } else { "repeat:off" }, Act::ScnToggleRepeat, sc.repeat_until_total);
        }
    }

    fn cfg_qc(&self, f: &mut Frame, area: Rect) {
        let max_x = area.x + area.width;
        let mut y = area.y;
        f.render_widget(
            Paragraph::new("8-stage burn-in: cpu, matrix, fp, stream, cache, branch, memory, vm")
                .style(Style::default().fg(DIM)),
            Rect { x: area.x, y, width: area.width, height: 1 },
        );
        y += 2;
        let mut x = area.x;
        self.label_at(f, &mut x, y, max_x, "Duration x:", DIM);
        self.button(f, &mut x, y, max_x, "-", Act::QcMult(-1), false);
        self.label_at(f, &mut x, y, max_x, &format!("{:.1}", self.cfg.qc_benchmark.duration_multiplier), Color::White);
        self.button(f, &mut x, y, max_x, "+", Act::QcMult(1), false);
        let per = (20.0 * self.cfg.qc_benchmark.duration_multiplier).round();
        self.label_at(f, &mut x, y, max_x, &format!("(~{per:.0}s/stage, {:.0}s total)", per * 8.0), DIM);
    }

    fn cfg_cert(&self, f: &mut Frame, area: Rect) {
        let max_x = area.x + area.width;
        let mut y = area.y;
        let mut x = area.x;
        self.label_at(f, &mut x, y, max_x, "Preset:", DIM);
        self.button(f, &mut x, y, max_x, "<", Act::CertPreset(-1), false);
        self.label_at(f, &mut x, y, max_x, &self.cfg.certification.preset_name, ACCENT);
        self.button(f, &mut x, y, max_x, ">", Act::CertPreset(1), false);
        y += 1;
        let mut x = area.x;
        self.label_at(f, &mut x, y, max_x, "Duration x:", DIM);
        self.button(f, &mut x, y, max_x, "-", Act::CertMult(-1), false);
        self.label_at(f, &mut x, y, max_x, &format!("{:.2}", self.cfg.certification.duration_multiplier), Color::White);
        self.button(f, &mut x, y, max_x, "+", Act::CertMult(1), false);
        y += 2;
        f.render_widget(
            Paragraph::new("Presets carry per-stage verdict rules (temp, WHEA/TDR, throughput).")
                .style(Style::default().fg(DIM)),
            Rect { x: area.x, y, width: area.width, height: 1 },
        );
    }

    fn cfg_concurrent(&self, f: &mut Frame, area: Rect) {
        let max_x = area.x + area.width;
        let mut y = area.y;
        let mut lx = area.x;
        self.label_at(f, &mut lx, y, max_x, "Lanes (click to toggle):", DIM);
        y += 1;
        let mut x = area.x;
        for choice in StressorChoice::ALL {
            let on = self.cfg.concurrent.lanes.contains(&choice);
            if x + choice.label().len() as u16 + 3 > max_x {
                x = area.x;
                y += 1;
                if y >= area.y + area.height.saturating_sub(2) {
                    break;
                }
            }
            self.button(f, &mut x, y, max_x, choice.label(), Act::ConcToggleLane(choice), on);
        }
        y += 2;
        let c = &self.cfg.concurrent;
        let mut x = area.x;
        self.label_at(f, &mut x, y, max_x, "Timeout:", DIM);
        self.button(f, &mut x, y, max_x, if c.use_timeout { "on" } else { "off" }, Act::ConcToggleTimeout, c.use_timeout);
        if c.use_timeout {
            self.button(f, &mut x, y, max_x, "-", Act::ConcDuration(-10), false);
            self.label_at(f, &mut x, y, max_x, &format!("{}s", c.duration_secs), Color::White);
            self.button(f, &mut x, y, max_x, "+", Act::ConcDuration(10), false);
        }
        y += 1;
        let mut x = area.x;
        self.label_at(f, &mut x, y, max_x, "Mem/lane:", DIM);
        self.button(f, &mut x, y, max_x, "-", Act::ConcMem(-64), false);
        self.label_at(f, &mut x, y, max_x, &format!("{}MB", c.memory_cap_mb), Color::White);
        self.button(f, &mut x, y, max_x, "+", Act::ConcMem(64), false);
        y += 1;
        let mut x = area.x;
        self.label_at(f, &mut x, y, max_x, "Disk/lane:", DIM);
        self.button(f, &mut x, y, max_x, "-", Act::ConcDisk(-16), false);
        self.label_at(f, &mut x, y, max_x, &format!("{}MB", c.disk_file_mb), Color::White);
        self.button(f, &mut x, y, max_x, "+", Act::ConcDisk(16), false);
    }

    // -- live column --------------------------------------------------------

    fn draw_live(&self, f: &mut Frame, area: Rect) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_set(SHORTCUT_SET)
            .title(" Live ")
            .style(Style::default().fg(DIM));
        let inner = block.inner(area);
        f.render_widget(block, area);
        if inner.height == 0 {
            return;
        }

        let mut lines: Vec<Line> = Vec::new();
        if let Some(m) = &self.latest {
            lines.push(Line::from(vec![
                Span::styled("elapsed ", Style::default().fg(DIM)),
                Span::styled(format!("{:.0}s", m.elapsed_secs), Style::default().fg(Color::White)),
                Span::styled("  thru ", Style::default().fg(DIM)),
                Span::styled(format!("{:.1} {}", m.throughput, m.throughput_unit), Style::default().fg(ACCENT)),
            ]));
            if let Some(err) = &m.last_error {
                lines.push(Line::styled(format!("err: {err}"), Style::default().fg(WARN)));
            }
        } else if self.is_running() {
            lines.push(Line::styled("starting…", Style::default().fg(DIM)));
        } else {
            lines.push(Line::styled("idle — configure and Start", Style::default().fg(DIM)));
        }

        if self.scenario_state.stage_count > 0 {
            lines.push(Line::from(vec![
                Span::styled("stage ", Style::default().fg(DIM)),
                Span::styled(
                    format!(
                        "{}/{} {}",
                        self.scenario_state.current_stage_index + 1,
                        self.scenario_state.stage_count,
                        self.scenario_state.current_stage_label
                    ),
                    Style::default().fg(Color::White),
                ),
            ]));
        }

        for lane in &self.concurrent_lanes {
            let col = if lane.errors > 0 { BAD } else { Color::White };
            lines.push(Line::from(vec![
                Span::styled(format!("{:<10}", lane.label), Style::default().fg(DIM)),
                Span::styled(format!("{:.1}{} ", lane.throughput, lane.unit), Style::default().fg(col)),
                Span::styled(format!("e{}", lane.errors), Style::default().fg(if lane.errors > 0 { BAD } else { DIM })),
            ]));
        }

        for row in &self.stage_verdicts {
            let (mark, col) = if row.pass { ("PASS", OK) } else { ("FAIL", BAD) };
            lines.push(Line::from(vec![
                Span::styled(format!("{mark} "), Style::default().fg(col)),
                Span::styled(row.label.clone(), Style::default().fg(Color::White)),
                Span::styled(
                    row.peak_throughput.map(|p| format!(" peak {p:.1}")).unwrap_or_default(),
                    Style::default().fg(DIM),
                ),
            ]));
            for v in &row.violations {
                lines.push(Line::styled(format!("  • {v}"), Style::default().fg(WARN)));
            }
        }

        // Reserve the last rows for verdict banner + sparkline.
        let banner_rows = if self.show_verdict && self.last_verdict.is_some() { 2 } else { 0 };
        let spark_rows = if self.history.len() > 1 { 3 } else { 0 };
        let text_h = inner.height.saturating_sub(banner_rows + spark_rows);
        let text_area = Rect { height: text_h, ..inner };
        f.render_widget(Paragraph::new(lines), text_area);

        let mut cursor_y = inner.y + text_h;
        if spark_rows > 0 {
            let spark_area = Rect { x: inner.x, y: cursor_y, width: inner.width, height: spark_rows };
            f.render_widget(
                Sparkline::default()
                    .block(Block::default().title(Span::styled("throughput", Style::default().fg(DIM))))
                    .data(&self.history)
                    .style(Style::default().fg(ACCENT)),
                spark_area,
            );
            cursor_y += spark_rows;
        }
        if banner_rows > 0 {
            if let Some(v) = &self.last_verdict {
                let (col, text) = verdict_banner(v);
                let mut extra = format!("{text}  {:.0}s", v.duration_secs);
                if let Some(t) = v.summary.max_temp_c {
                    extra.push_str(&format!("  max {t:.0}C"));
                }
                if v.summary.whea_delta_count > 0 {
                    extra.push_str(&format!("  WHEA+{}", v.summary.whea_delta_count));
                }
                let banner_area = Rect { x: inner.x, y: cursor_y, width: inner.width, height: 1 };
                f.render_widget(
                    Paragraph::new(extra).style(Style::default().fg(col).add_modifier(Modifier::BOLD)),
                    banner_area,
                );
                let mut x = inner.x;
                self.button(f, &mut x, cursor_y + 1, inner.x + inner.width, "dismiss", Act::DismissVerdict, false);
            }
        }
    }
}

impl<'a> HandleWidget<'a> for StressTab {
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {
        self.apply_pending();
        self.poll_controller();
        self.zones.borrow_mut().clear();

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(1), Constraint::Length(1), Constraint::Min(3)])
            .split(area);

        // Mode + Start row.
        let running = self.is_running();
        let max_x = rows[0].x + rows[0].width;
        let mut x = rows[0].x;
        for (m, label) in [
            (PanelMode::Single, "Single"),
            (PanelMode::Scenario, "Scenario"),
            (PanelMode::QcBenchmark, "QC Bench"),
            (PanelMode::Certification, "Cert"),
            (PanelMode::Concurrent, "Concurrent"),
        ] {
            let active = self.cfg.mode == m;
            self.button(f, &mut x, rows[0].y, max_x, label, Act::SetMode(m), active);
        }
        x += 2;
        self.button(f, &mut x, rows[0].y, max_x, if running { "STOP" } else { "START" }, Act::StartStop, running);

        // Status line.
        let status = if let Some(e) = &self.start_error {
            Span::styled(e.clone(), Style::default().fg(BAD))
        } else if running {
            Span::styled("running…", Style::default().fg(OK))
        } else {
            Span::styled("mouse: click controls · keys: ←/→ mode, space start/stop", Style::default().fg(DIM))
        };
        f.render_widget(Paragraph::new(Line::from(status)), rows[1]);

        // Body: config | live.
        let body = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(58), Constraint::Percentage(42)])
            .split(rows[2].shrink_symmetric(0, 0));
        self.draw_config(f, body[0]);
        self.draw_live(f, body[1]);
    }

    fn handle_mouse_event(&self, ev: &MouseEvent) {
        let pos = Position::new(ev.column, ev.row);
        match ev.kind {
            MouseEventKind::Moved => {
                let hit = self
                    .zones
                    .borrow()
                    .iter()
                    .find(|(r, _)| r.contains(pos))
                    .map(|(_, a)| a.clone());
                *self.hovered.borrow_mut() = hit;
            }
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some((_, act)) = self.zones.borrow().iter().find(|(r, _)| r.contains(pos)) {
                    self.pending.borrow_mut().push(act.clone());
                }
            }
            _ => {}
        }
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> bool {
        let modes = [
            PanelMode::Single,
            PanelMode::Scenario,
            PanelMode::QcBenchmark,
            PanelMode::Certification,
            PanelMode::Concurrent,
        ];
        let idx = modes.iter().position(|m| *m == self.cfg.mode).unwrap_or(0);
        match key.code {
            KeyCode::Left if !self.is_running() => {
                self.cfg.mode = modes[(idx + modes.len() - 1) % modes.len()].clone();
            }
            KeyCode::Right if !self.is_running() => {
                self.cfg.mode = modes[(idx + 1) % modes.len()].clone();
            }
            KeyCode::Char(' ') | KeyCode::Enter => {
                if self.is_running() {
                    self.stop();
                } else {
                    self.start();
                }
            }
            _ => {}
        }
        true
    }
}

fn cycle_stressor(cur: StressorChoice, delta: i32) -> StressorChoice {
    let all = StressorChoice::ALL;
    let idx = all.iter().position(|s| *s == cur).unwrap_or(0) as i32;
    let n = all.len() as i32;
    let next = ((idx + delta) % n + n) % n;
    all[next as usize]
}

fn cycle_cert(cur: &str, delta: i32) -> String {
    let idx = CERT_PRESET_NAMES.iter().position(|s| *s == cur).unwrap_or(0) as i32;
    let n = CERT_PRESET_NAMES.len() as i32;
    let next = ((idx + delta) % n + n) % n;
    CERT_PRESET_NAMES[next as usize].to_string()
}

fn adj_usize(v: usize, delta: i32, min: usize, max: usize) -> usize {
    let nv = v as i64 + delta as i64;
    nv.clamp(min as i64, max as i64) as usize
}

fn adj_u64(v: u64, delta: i64, min: u64, max: u64) -> u64 {
    let nv = v as i64 + delta;
    nv.clamp(min as i64, max as i64) as u64
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

fn verdict_banner(verdict: &RunVerdict) -> (Color, String) {
    let col = match verdict.result {
        RunResult::Pass => OK,
        RunResult::Fail => BAD,
        RunResult::Aborted => WARN,
        _ => DIM,
    };
    (col, verdict_label(verdict))
}
