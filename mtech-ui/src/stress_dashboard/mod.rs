//! Shared stress-test dashboard rendered by both Mastertech4.0 and qc-app.
//!
//! Presentation only: the host owns the run lifecycle (controller, telemetry,
//! computer record) and reacts to the [`DashboardAction`] returned by
//! [`StressDashboard::show`]. Every technician-facing string comes from
//! `stress_runner::stressor_info`, so the two front ends can never disagree on
//! what a test does or when to reach for it.

mod configure;
mod help;
mod live;
mod results;

use eframe::egui::{self, Ui};
use stress_runner::{RunResult, StressPanelConfig, StressorChoice};

/// What the operator asked for this frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum DashboardAction {
    #[default]
    None,
    Start,
    Stop,
    OpenHistory,
}

/// One lane of a concurrent run, or one stage of a scenario.
#[derive(Clone, Debug)]
pub struct LaneView {
    pub index: u32,
    pub label: String,
    pub stressor: Option<StressorChoice>,
    pub throughput: f64,
    pub unit: &'static str,
    pub errors: u64,
    pub last_error: Option<String>,
}

/// Progress through a multi-stage run.
#[derive(Clone, Debug, Default)]
pub struct StageProgress {
    pub index: usize,
    pub label: String,
    pub count: usize,
}

/// One finished stage's rules verdict.
#[derive(Clone, Debug)]
pub struct StageVerdictView {
    pub label: String,
    pub pass: bool,
    pub violations: Vec<String>,
    pub peak_throughput: Option<f64>,
}

/// Final verdict summary for the banner.
#[derive(Clone, Debug)]
pub struct VerdictView {
    pub result: RunResult,
    pub failure_kind: Option<String>,
    pub duration_secs: f64,
    pub max_temp_c: Option<f32>,
    pub whea_delta: u32,
    pub tdr_count: u32,
    pub run_id: Option<String>,
    /// Planned duration, when the run had one — drives the short-run warning.
    pub planned_secs: Option<u64>,
}

/// A previously persisted run, newest first.
#[derive(Clone, Debug)]
pub struct RecentRun {
    pub label: String,
    pub when: String,
    pub result: RunResult,
    pub duration_secs: f64,
}

/// Everything the dashboard renders about the current/last run.
#[derive(Clone, Debug, Default)]
pub struct StressLive {
    pub elapsed_secs: f64,
    pub throughput: f64,
    pub throughput_unit: &'static str,
    pub last_error: Option<String>,
    pub stage: Option<StageProgress>,
    pub lanes: Vec<LaneView>,
    pub stage_verdicts: Vec<StageVerdictView>,
    pub verdict: Option<VerdictView>,
    /// Recent throughput samples for the sparkline.
    pub history: Vec<f32>,
    pub recent_runs: Vec<RecentRun>,
}

/// Below this width the three columns stack instead of sitting side by side.
const STACK_BREAKPOINT: f32 = 1080.0;

/// Persistent view state (which groups are open, whether help is showing).
#[derive(Default)]
pub struct StressDashboard {
    help_open: bool,
    /// Explicit open/closed choices, by group label. Groups the operator has
    /// never touched fall back to "open only if it holds a selection", which
    /// keeps the column short instead of listing every stressor at once.
    group_overrides: Vec<(&'static str, bool)>,
}

impl StressDashboard {
    pub fn help_open(&self) -> bool {
        self.help_open
    }

    /// Show the help page, for a host that wants to link straight to it.
    pub fn open_help(&mut self) {
        self.help_open = true;
    }

    /// Whether a group's contents are shown; `has_selection` is the default
    /// when the operator has not explicitly toggled it.
    pub(crate) fn group_open(&self, group: &'static str, has_selection: bool) -> bool {
        self.group_overrides
            .iter()
            .find(|(g, _)| *g == group)
            .map(|(_, open)| *open)
            .unwrap_or(has_selection)
    }

    pub(crate) fn toggle_group(&mut self, group: &'static str, currently_open: bool) {
        match self.group_overrides.iter_mut().find(|(g, _)| *g == group) {
            Some((_, open)) => *open = !currently_open,
            None => self.group_overrides.push((group, !currently_open)),
        }
    }

    /// Render the dashboard. `chart` paints the host's own telemetry widget
    /// into the Live column, which keeps `displays`-only chart types out of here.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        cfg: &mut StressPanelConfig,
        live: &StressLive,
        running: bool,
        start_error: Option<&str>,
        chart: impl FnOnce(&mut Ui),
    ) -> DashboardAction {
        let mut action = DashboardAction::None;

        self.header(ui, cfg, running, &mut action);
        if let Some(err) = start_error {
            let col = crate::theme::error(ui);
            ui.colored_label(col, err);
        }
        ui.separator();

        if self.help_open {
            help::show(ui, &mut self.help_open);
            return action;
        }

        let stacked = ui.available_width() < STACK_BREAKPOINT;
        if stacked {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    configure::show(self, ui, cfg, running, &mut action);
                    ui.separator();
                    live::show(ui, cfg, live, running, chart);
                    ui.separator();
                    results::show(ui, live);
                });
            return action;
        }

        let total = ui.available_width();
        let spacing = ui.spacing().item_spacing.x * 2.0;
        let usable = (total - spacing).max(0.0);
        let col_configure = (usable * 0.28).clamp(260.0, 420.0);
        let col_results = (usable * 0.28).clamp(260.0, 460.0);
        let col_live = (usable - col_configure - col_results).max(240.0);

        ui.horizontal_top(|ui| {
            column(ui, col_configure, "stress_col_configure", |ui| {
                configure::show(self, ui, cfg, running, &mut action);
            });
            ui.separator();
            column(ui, col_live, "stress_col_live", |ui| {
                live::show(ui, cfg, live, running, chart);
            });
            ui.separator();
            column(ui, col_results, "stress_col_results", |ui| {
                results::show(ui, live);
            });
        });

        action
    }

    fn header(
        &mut self,
        ui: &mut Ui,
        cfg: &mut StressPanelConfig,
        running: bool,
        action: &mut DashboardAction,
    ) {
        use crate::icons;

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!("{}  Stress Test", icons::FLASK))
                    .heading(),
            );
            ui.separator();
            configure::mode_selector(ui, cfg, running);

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .selectable_label(self.help_open, format!("{}  Help", icons::INFO))
                    .on_hover_text("What each test does and when to run it")
                    .clicked()
                {
                    self.help_open = !self.help_open;
                }
                if ui
                    .button(format!("{}  History", icons::EYE))
                    .on_hover_text("Browse previously persisted runs")
                    .clicked()
                {
                    *action = DashboardAction::OpenHistory;
                }
                ui.separator();
                if running {
                    ui.spinner();
                    if ui
                        .button(egui::RichText::new(format!("{}  Stop", icons::STOP)))
                        .on_hover_text("Cancel the run — it is recorded as aborted")
                        .clicked()
                    {
                        *action = DashboardAction::Stop;
                    }
                } else if ui
                    .button(egui::RichText::new(format!("{}  Start", icons::PLAY)).strong())
                    .on_hover_text("Run the configured test on this machine")
                    .clicked()
                {
                    *action = DashboardAction::Start;
                }
            });
        });
        ui.add_space(2.0);
    }
}

/// A fixed-width, independently scrolling dashboard column.
fn column(ui: &mut Ui, width: f32, id: &str, add: impl FnOnce(&mut Ui)) {
    let height = ui.available_height();
    ui.allocate_ui_with_layout(
        egui::vec2(width, height),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.set_min_width(width);
            ui.set_max_width(width);
            egui::ScrollArea::vertical()
                .id_salt(id)
                .auto_shrink([false, false])
                .show(ui, add);
        },
    );
}

/// Colour for a run result, used by the live and results columns.
pub(crate) fn result_color(ui: &Ui, result: RunResult) -> egui::Color32 {
    use crate::theme;
    match result {
        RunResult::Pass => theme::result_pass(ui),
        RunResult::Fail => theme::result_fail(ui),
        RunResult::Aborted | RunResult::Inconclusive => theme::result_aborted(ui),
        RunResult::InProgress => theme::result_unknown(ui),
    }
}

pub(crate) fn result_label(result: RunResult, failure_kind: Option<&str>) -> String {
    match result {
        RunResult::Pass => "PASS".to_string(),
        RunResult::Fail => match failure_kind {
            Some(k) if !k.is_empty() && k != "none" => format!("FAIL ({k})"),
            _ => "FAIL".to_string(),
        },
        RunResult::Aborted => "ABORTED".to_string(),
        RunResult::Inconclusive => match failure_kind {
            Some(k) if !k.is_empty() && k != "none" => format!("INCONCLUSIVE ({k})"),
            _ => "INCONCLUSIVE".to_string(),
        },
        RunResult::InProgress => "RUNNING".to_string(),
    }
}
