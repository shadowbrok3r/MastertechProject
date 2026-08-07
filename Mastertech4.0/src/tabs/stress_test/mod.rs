//! Interactive stress-test tab (egui).
//!
//! A thin host over [`stress_runner::RunController`] and the shared
//! [`mtech_ui::stress_dashboard`]: it builds a `RunSpec` from the shared
//! [`StressPanelConfig`], drives start/poll/stop, and mirrors controller
//! updates into a [`StressLive`] for the dashboard to render. All execution and
//! the strict `hardware_component` / `stress_test_run` / `stress_test_metric` /
//! `stress_test_event` persistence happen inside the controller's worker thread.

use std::time::{Duration, Instant};

use displays::tabs::resource_monitor::chart_board::ChartBoard;
use eframe::egui::Ui;
use mtech_ui::stress_dashboard::{
    DashboardAction, LaneView, StageProgress, StageVerdictView, StressDashboard, StressLive,
    VerdictView,
};
use stress_runner::{
    build_run_spec, is_stress_active, planned_duration_secs, RecordId, RunController, RunUpdate,
    StressPanelConfig, StressRunContext,
};

use crate::app_state::MastertechContext;
use crate::filesystem::local_computer_record;
use crate::filesystem::system_info::{current_telemetry_snapshot, shared_telemetry_agent};

/// Throughput samples retained for the dashboard sparkline.
const HISTORY_LEN: usize = 120;

/// Chart sampling and redraw period, independent of input-driven frames.
const FRAME_PERIOD: Duration = Duration::from_millis(33);

pub struct StressRunner {
    cfg: StressPanelConfig,
    run: Option<RunController>,
    live: StressLive,
    dashboard: StressDashboard,
    last_run_id: Option<RecordId>,
    /// Planned wall-clock of the active run, so a short run is flagged.
    planned_secs: Option<u64>,
    start_error: Option<String>,
    charts: ChartBoard,
    /// Last chart sample; gates sampling to `FRAME_PERIOD` so the retained
    /// window does not depend on how fast input drives repaints.
    last_sample: Option<Instant>,
    /// Set when the operator clicks "History"; drained by the host to open the
    /// read-only Stress Lab tab.
    open_history_requested: bool,
}

impl Default for StressRunner {
    fn default() -> Self {
        Self {
            cfg: StressPanelConfig::default(),
            run: None,
            live: StressLive::default(),
            dashboard: StressDashboard::default(),
            last_run_id: None,
            planned_secs: None,
            start_error: None,
            charts: ChartBoard::default(),
            last_sample: None,
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
        if self.last_sample.is_none_or(|t| t.elapsed() >= FRAME_PERIOD) {
            self.last_sample = Some(Instant::now());
            self.charts.push(&current_telemetry_snapshot());
        }
        self.tick();
        ui.ctx().request_repaint_after(FRAME_PERIOD);

        let running = self.is_running();
        let charts = &mut self.charts;
        let action = self.dashboard.show(
            ui,
            &mut self.cfg,
            &self.live,
            running,
            self.start_error.as_deref(),
            |ui| charts.show(ui),
        );

        match action {
            DashboardAction::Start => self.start(),
            DashboardAction::Stop => self.stop(),
            DashboardAction::OpenHistory => self.open_history_requested = true,
            DashboardAction::None => {}
        }
    }

    // -- lifecycle ----------------------------------------------------------

    fn tick(&mut self) {
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
                self.reset_live();
            }
            RunUpdate::StageStarted {
                index,
                label,
                stage_count,
            } => {
                self.live.stage = Some(StageProgress {
                    index,
                    label,
                    count: stage_count,
                });
                self.live.history.clear();
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
                self.live.history.push(metrics.throughput as f32);
                if self.live.history.len() > HISTORY_LEN {
                    self.live.history.remove(0);
                }
                self.live.elapsed_secs = metrics.elapsed_secs;
                self.live.throughput = metrics.throughput;
                self.live.throughput_unit = throughput_unit;
                self.live.last_error = metrics.last_error;
            }
            RunUpdate::StageFinished { .. } => {}
            RunUpdate::StageVerdict {
                label,
                pass,
                violations,
                peak_throughput,
                ..
            } => {
                self.live.stage_verdicts.push(StageVerdictView {
                    label,
                    pass,
                    violations,
                    peak_throughput,
                });
            }
            RunUpdate::Finished(verdict) => {
                self.live.elapsed_secs = verdict.duration_secs;
                self.live.verdict = Some(VerdictView {
                    result: verdict.result,
                    failure_kind: Some(verdict.failure_mode.kind().to_string()),
                    duration_secs: verdict.duration_secs,
                    max_temp_c: verdict.summary.max_temp_c,
                    whea_delta: verdict.summary.whea_delta_count,
                    tdr_count: verdict.summary.tdr_count,
                    run_id: self.last_run_id.as_ref().map(|id| format!("{id:?}")),
                    planned_secs: self.planned_secs,
                });
            }
            RunUpdate::Warning { message } => log::warn!("stress-runner: {message}"),
            RunUpdate::Error { message } => {
                log::error!("stress-runner: {message}");
                self.start_error = Some(message);
            }
        }
    }

    fn reset_live(&mut self) {
        let recent = std::mem::take(&mut self.live.recent_runs);
        self.live = StressLive {
            recent_runs: recent,
            ..StressLive::default()
        };
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
        if let Some(lane) = self.live.lanes.iter_mut().find(|l| l.index == index) {
            lane.throughput = throughput;
            lane.errors = errors;
            lane.last_error = last_error;
            lane.unit = unit;
            if let Some(l) = label {
                lane.label = l;
            }
        } else {
            let label = label.unwrap_or_else(|| format!("lane {index}"));
            let stressor = self
                .cfg
                .concurrent
                .lanes
                .iter()
                .copied()
                .find(|c| c.label() == label);
            self.live.lanes.push(LaneView {
                index,
                label,
                stressor,
                throughput,
                unit,
                errors,
                last_error,
            });
            self.live.lanes.sort_by_key(|l| l.index);
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
                self.planned_secs = planned_duration_secs(&spec.plan);
                self.reset_live();
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
}

impl MastertechContext {
    pub fn show_stress_test(&mut self, ui: &mut Ui) {
        self.stress_test.ui(ui);
        if self.stress_test.take_open_history() {
            self.pending_tab_opens.push(displays::tabs::TabId::StressLab);
        }
    }
}
