//! Run report window: renders a [`stress_runner::RunReportModel`] for the
//! just-finished run or any past run on this machine.

use std::sync::mpsc;

use eframe::egui;
use egui_phosphor::regular as p;
use egui_plot::{Line, Plot, PlotPoints, VLine};
use stress_runner::{RecordId, RunReportModel, StressTestRun};

enum ViewMsg {
    Model(Box<RunReportModel>),
    Error(String),
    Runs(Vec<StressTestRun>),
}

pub struct ReportView {
    open: bool,
    loading: bool,
    error: Option<String>,
    model: Option<RunReportModel>,
    runs: Vec<StressTestRun>,
    runs_loading: bool,
    tx: mpsc::Sender<ViewMsg>,
    rx: mpsc::Receiver<ViewMsg>,
}

impl Default for ReportView {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            open: false,
            loading: false,
            error: None,
            model: None,
            runs: Vec::new(),
            runs_loading: false,
            tx,
            rx,
        }
    }
}

impl ReportView {
    /// Open the window and load one run's report.
    pub fn open_run(&mut self, run_id: RecordId, ctx: &egui::Context) {
        self.open = true;
        self.loading = true;
        self.error = None;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        tokio::spawn(async move {
            let msg = match stress_runner::fetch_report_data(&run_id).await {
                Ok(data) => ViewMsg::Model(Box::new(RunReportModel::from_data(&data))),
                Err(e) => ViewMsg::Error(format!("report fetch failed: {e:#}")),
            };
            let _ = tx.send(msg);
            ctx.request_repaint();
        });
    }

    /// Refresh the past-run picker for this machine.
    fn refresh_runs(&mut self, computer: RecordId, ctx: &egui::Context) {
        self.runs_loading = true;
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        tokio::spawn(async move {
            let msg = match StressTestRun::list_for_computer(&computer).await {
                Ok(runs) => ViewMsg::Runs(runs),
                Err(e) => ViewMsg::Error(format!("run list failed: {e:#}")),
            };
            let _ = tx.send(msg);
            ctx.request_repaint();
        });
    }

    /// Render the window; call every frame from the host app.
    pub fn show(&mut self, ctx: &egui::Context, computer: &RecordId) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                ViewMsg::Model(model) => {
                    self.model = Some(*model);
                    self.loading = false;
                }
                ViewMsg::Error(err) => {
                    self.error = Some(err);
                    self.loading = false;
                    self.runs_loading = false;
                }
                ViewMsg::Runs(runs) => {
                    self.runs = runs;
                    self.runs_loading = false;
                }
            }
        }

        if !self.open {
            return;
        }

        let mut open = self.open;
        let mut load_run: Option<RecordId> = None;
        let mut refresh = false;

        egui::Window::new("Run Report")
            .open(&mut open)
            .default_size([900.0, 640.0])
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui.button(format!("{} Past runs", p::CLOCK_COUNTER_CLOCKWISE)).clicked() {
                        refresh = true;
                    }
                    if self.runs_loading {
                        ui.add(egui::Spinner::new());
                    }
                    if !self.runs.is_empty() {
                        egui::ComboBox::from_id_salt("report_run_picker")
                            .width(420.0)
                            .selected_text(
                                self.model
                                    .as_ref()
                                    .map(|m| m.run_id.clone())
                                    .unwrap_or_else(|| "pick a run".into()),
                            )
                            .show_ui(ui, |ui| {
                                for run in &self.runs {
                                    let label = format!(
                                        "{}  {}  {}",
                                        run.started_at_label(),
                                        run.preset_label.as_deref().unwrap_or(&run.tool_label),
                                        run.result.as_str(),
                                    );
                                    if ui.selectable_label(false, label).clicked() {
                                        load_run = Some(run.id.clone());
                                    }
                                }
                            });
                    }
                    if self.loading {
                        ui.add(egui::Spinner::new());
                        ui.label("loading…");
                    }
                });

                if let Some(err) = &self.error {
                    ui.colored_label(egui::Color32::from_rgb(200, 60, 60), err);
                }

                ui.separator();

                egui::ScrollArea::vertical().show(ui, |ui| {
                    if let Some(model) = &self.model {
                        render_report(ui, model);
                    } else if !self.loading {
                        ui.label(
                            egui::RichText::new(
                                "No report loaded — pick a past run or finish a stress run.",
                            )
                            .weak(),
                        );
                    }
                });
            });

        self.open = open;
        if refresh {
            self.refresh_runs(computer.clone(), ctx);
        }
        if let Some(id) = load_run {
            self.open_run(id, ctx);
        }
    }
}

/// Date label helper on the run row.
trait RunLabel {
    fn started_at_label(&self) -> String;
}

impl RunLabel for StressTestRun {
    fn started_at_label(&self) -> String {
        chrono::DateTime::<chrono::Utc>::from_timestamp_millis(self.started_at.timestamp_millis())
            .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_default()
    }
}

fn render_report(ui: &mut egui::Ui, m: &RunReportModel) {
    let (verdict_text, verdict_color) = match m.result.as_str() {
        "pass" => ("PASS".to_string(), egui::Color32::from_rgb(50, 160, 90)),
        "fail" => (
            format!(
                "FAIL — {}",
                m.failure_detail
                    .clone()
                    .or_else(|| m.failure_kind.clone())
                    .unwrap_or_default()
            ),
            egui::Color32::from_rgb(200, 60, 60),
        ),
        "aborted" => ("ABORTED".to_string(), egui::Color32::from_rgb(180, 140, 50)),
        other => (other.to_uppercase(), egui::Color32::from_rgb(160, 160, 160)),
    };
    ui.colored_label(verdict_color, egui::RichText::new(verdict_text).strong().size(16.0));

    egui::Grid::new("report_header")
        .num_columns(4)
        .spacing([18.0, 2.0])
        .show(ui, |ui| {
            let kv = |ui: &mut egui::Ui, k: &str, v: String| {
                ui.label(egui::RichText::new(k).small().weak());
                ui.label(egui::RichText::new(v).small().monospace());
            };
            kv(ui, "Run", m.run_id.clone());
            kv(ui, "Preset", m.preset_label.clone().unwrap_or_else(|| m.tool_label.clone()));
            ui.end_row();
            kv(ui, "Machine", m.hostname.clone().or(m.machine_id.clone()).unwrap_or_default());
            kv(ui, "Tech", m.tech.clone().unwrap_or_default());
            ui.end_row();
            kv(ui, "Started", m.started_at.clone());
            kv(
                ui,
                "Duration",
                m.duration_actual_secs
                    .map(|s| format!("{:.1} min", s / 60.0))
                    .unwrap_or_default(),
            );
            ui.end_row();
            if let Some(so) = &m.service_order {
                kv(ui, "Order", so.clone());
                ui.end_row();
            }
        });

    ui.add_space(4.0);
    ui.group(|ui| {
        egui::Grid::new("report_summary")
            .num_columns(8)
            .spacing([14.0, 2.0])
            .show(ui, |ui| {
                let cell = |ui: &mut egui::Ui, k: &str, v: String| {
                    ui.label(egui::RichText::new(k).small().weak());
                    ui.label(egui::RichText::new(v).small().monospace());
                };
                cell(ui, "CPU max", fmt_opt_f32(m.max_temp_c, "°C"));
                cell(ui, "CPU avg", fmt_opt_f32(m.avg_temp_c, "°C"));
                cell(ui, "GPU max", fmt_opt_f32(m.max_gpu_temp_c, "°C"));
                cell(
                    ui,
                    "Max clock",
                    m.max_clock_mhz.map(|c| format!("{c} MHz")).unwrap_or_else(|| "—".into()),
                );
                ui.end_row();
                cell(ui, "WHEA", m.whea_delta_count.to_string());
                cell(ui, "TDR", m.tdr_count.to_string());
                cell(ui, "Test errors", m.test_errors.to_string());
                cell(ui, "Disk errors", m.disk_io_errors.to_string());
                ui.end_row();
            });
        if m.thermal_throttle_detected || m.vrm_throttle_detected {
            ui.colored_label(
                egui::Color32::from_rgb(180, 140, 50),
                format!(
                    "{} {}",
                    p::WARNING,
                    if m.thermal_throttle_detected {
                        "thermal throttle observed"
                    } else {
                        "clock collapse without temp breach (VRM/power suspect)"
                    }
                ),
            );
        }
    });

    if !m.stages.is_empty() {
        ui.add_space(4.0);
        ui.label(egui::RichText::new("Stages").strong());
        egui::Grid::new("report_stages")
            .num_columns(8)
            .spacing([12.0, 2.0])
            .striped(true)
            .show(ui, |ui| {
                for h in ["", "Stage", "Stressor", "Time", "Avg throughput", "Errors", "WHEA/TDR", "Notes"] {
                    ui.label(egui::RichText::new(h).small().strong());
                }
                ui.end_row();
                for s in &m.stages {
                    match s.result.as_deref() {
                        Some("pass") => {
                            ui.colored_label(egui::Color32::from_rgb(50, 160, 90), p::CHECK_CIRCLE)
                        }
                        Some("fail") => {
                            ui.colored_label(egui::Color32::from_rgb(200, 60, 60), p::X_CIRCLE)
                        }
                        _ => ui.label(egui::RichText::new(p::MINUS_CIRCLE).weak()),
                    };
                    ui.label(egui::RichText::new(&s.label).small());
                    ui.label(egui::RichText::new(&s.stressor).small().weak());
                    ui.label(
                        egui::RichText::new(format!("{:.1}m", s.duration_actual_secs / 60.0))
                            .small()
                            .monospace(),
                    );
                    ui.label(
                        egui::RichText::new(
                            s.avg_throughput
                                .map(|t| format!("{t:.1} {}", s.throughput_unit))
                                .unwrap_or_else(|| "—".into()),
                        )
                        .small()
                        .monospace(),
                    );
                    ui.label(egui::RichText::new(s.errors.to_string()).small().monospace());
                    ui.label(
                        egui::RichText::new(format!("{}/{}", s.whea_delta, s.tdr_delta))
                            .small()
                            .monospace(),
                    );
                    let notes = if s.violations.is_empty() {
                        s.throughput_cv.map(|cv| format!("CV {cv:.3}")).unwrap_or_default()
                    } else {
                        s.violations.join("; ")
                    };
                    ui.label(egui::RichText::new(notes).small().weak());
                    ui.end_row();
                }
            });
    }

    ui.add_space(6.0);
    report_plot(ui, "report_temp", "°C", &[
        (&m.cpu_temp, egui::Color32::from_rgb(230, 130, 70)),
        (&m.gpu_temp, egui::Color32::from_rgb(120, 180, 240)),
    ], m);
    report_plot(ui, "report_clock", "MHz", &[
        (&m.avg_clock, egui::Color32::from_rgb(150, 220, 150)),
    ], m);
    report_plot(ui, "report_tp", &m.throughput.unit.clone(), &[
        (&m.throughput, egui::Color32::from_rgb(200, 160, 240)),
    ], m);

    if !m.timeline.is_empty() {
        ui.add_space(6.0);
        ui.label(egui::RichText::new("Events").strong());
        egui::Grid::new("report_timeline")
            .num_columns(4)
            .spacing([12.0, 2.0])
            .striped(true)
            .show(ui, |ui| {
                for e in m.timeline.iter().take(200) {
                    ui.label(
                        egui::RichText::new(format!("{:.0}s", e.at_secs)).small().monospace(),
                    );
                    ui.label(egui::RichText::new(&e.kind).small());
                    ui.label(egui::RichText::new(e.code.clone().unwrap_or_default()).small().weak());
                    ui.label(egui::RichText::new(&e.detail).small().weak());
                    ui.end_row();
                }
            });
        if m.timeline.len() > 200 {
            ui.label(
                egui::RichText::new(format!("… {} more events", m.timeline.len() - 200))
                    .small()
                    .weak(),
            );
        }
    }
}

fn fmt_opt_f32(v: Option<f32>, unit: &str) -> String {
    v.map(|x| format!("{x:.1}{unit}")).unwrap_or_else(|| "—".into())
}

/// One plot with stage-boundary and event markers shared across charts.
fn report_plot(
    ui: &mut egui::Ui,
    id: &str,
    y_label: &str,
    series: &[(&stress_runner::ReportSeries, egui::Color32)],
    m: &RunReportModel,
) {
    if series.iter().all(|(s, _)| s.points.is_empty()) {
        return;
    }
    Plot::new(id)
        .height(160.0)
        .allow_drag(false)
        .allow_scroll(false)
        .show_background(false)
        .x_axis_label("s")
        .y_axis_label(y_label)
        .legend(egui_plot::Legend::default())
        .show(ui, |plot_ui| {
            for b in &m.stage_boundaries {
                plot_ui.vline(
                    VLine::new(b.label.clone(), b.at_secs)
                        .color(egui::Color32::from_gray(90))
                        .width(1.0),
                );
            }
            for e in &m.event_markers {
                plot_ui.vline(
                    VLine::new(e.kind.clone(), e.at_secs)
                        .color(egui::Color32::from_rgb(200, 60, 60))
                        .width(1.0),
                );
            }
            for (s, color) in series {
                if s.points.is_empty() {
                    continue;
                }
                let points: PlotPoints = s.points.iter().map(|(x, y)| [*x, *y]).collect();
                plot_ui.line(Line::new(s.label.clone(), points).color(*color).width(1.5));
            }
        });
}
