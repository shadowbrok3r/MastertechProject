//! Stress Lab — browse stress_test_run history and metric timelines by hardware_component.

mod data;
mod plots;

use crossbeam::channel::{Receiver, Sender, unbounded};
use database::schema::{HardwareKind, StressTestEvent, StressTestMetric};
use eframe::egui::{ComboBox, ScrollArea, Ui};

use crate::{ui_tools::theme, PlatformSpawner, Spawner};

#[derive(Clone, Default)]
pub struct ComponentRow {
    pub id: String,
    pub display_name: String,
    pub kind: String,
    pub run_count: u64,
    pub fail_count: u64,
}

#[derive(Clone, Default)]
pub struct RunRow {
    pub id: String,
    pub tool_label: String,
    pub result: String,
    pub failure_kind: String,
    pub hostname: Option<String>,
    pub started_at: String,
    pub duration_secs: Option<f64>,
    pub peak_throughput: Option<f64>,
    pub throughput_unit: Option<String>,
    pub max_temp_c: Option<f32>,
    pub target_component: Option<String>,
    pub preset_label: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum KindFilter {
    All,
    Cpu,
    Gpu,
}

impl KindFilter {
    fn label(self) -> &'static str {
        match self {
            Self::All => "All kinds",
            Self::Cpu => "CPU",
            Self::Gpu => "GPU",
        }
    }

    fn to_hardware_kind(self) -> Option<HardwareKind> {
        match self {
            Self::All => None,
            Self::Cpu => Some(HardwareKind::Cpu),
            Self::Gpu => Some(HardwareKind::Gpu),
        }
    }
}

pub struct StressLab {
    components_tx: Sender<Vec<ComponentRow>>,
    components_rx: Receiver<Vec<ComponentRow>>,
    runs_tx: Sender<Vec<RunRow>>,
    runs_rx: Receiver<Vec<RunRow>>,
    metrics_tx: Sender<(String, Vec<StressTestMetric>)>,
    metrics_rx: Receiver<(String, Vec<StressTestMetric>)>,
    events_tx: Sender<(String, Vec<StressTestEvent>)>,
    events_rx: Receiver<(String, Vec<StressTestEvent>)>,

    components: Vec<ComponentRow>,
    runs: Vec<RunRow>,
    metrics: Vec<StressTestMetric>,
    events: Vec<StressTestEvent>,

    kind_filter: KindFilter,
    selected_component: Option<String>,
    selected_run: Option<String>,
    loading_components: bool,
    loading_runs: bool,
    loading_detail: bool,
    error: Option<String>,
    show_recent_all: bool,
}

impl Default for StressLab {
    fn default() -> Self {
        let (components_tx, components_rx) = unbounded();
        let (runs_tx, runs_rx) = unbounded();
        let (metrics_tx, metrics_rx) = unbounded();
        let (events_tx, events_rx) = unbounded();
        Self {
            components_tx,
            components_rx,
            runs_tx,
            runs_rx,
            metrics_tx,
            metrics_rx,
            events_tx,
            events_rx,
            components: Vec::new(),
            runs: Vec::new(),
            metrics: Vec::new(),
            events: Vec::new(),
            kind_filter: KindFilter::All,
            selected_component: None,
            selected_run: None,
            loading_components: false,
            loading_runs: false,
            loading_detail: false,
            error: None,
            show_recent_all: true,
        }
    }
}

impl StressLab {
    pub fn ui(&mut self, ui: &mut Ui) {
        self.poll_channels();
        self.toolbar(ui);
        if let Some(err) = &self.error {
            ui.colored_label(theme::error(ui), err);
        }

        ui.horizontal_top(|ui| {
            let w = ui.available_width();
            let col_w = (w * 0.28).max(160.0);
            ui.vertical(|ui| {
                ui.set_width(col_w);
                self.components_panel(ui);
            });
            ui.separator();
            ui.vertical(|ui| {
                ui.set_width((w * 0.32).max(180.0));
                self.runs_panel(ui);
            });
            ui.separator();
            ui.vertical(|ui| {
                self.detail_panel(ui);
            });
        });
    }

    pub fn refresh_on_open(&mut self) {
        if self.components.is_empty() && !self.loading_components {
            self.reload_components();
        }
        if self.runs.is_empty() && !self.loading_runs {
            self.reload_runs();
        }
    }

    fn poll_channels(&mut self) {
        if let Ok(rows) = self.components_rx.try_recv() {
            self.components = rows;
            self.loading_components = false;
            self.error = None;
        }
        if let Ok(rows) = self.runs_rx.try_recv() {
            self.runs = rows;
            self.loading_runs = false;
        }
        if let Ok((run_id, metrics)) = self.metrics_rx.try_recv() {
            if self.selected_run.as_deref() == Some(run_id.as_str()) {
                self.metrics = metrics;
            }
            self.loading_detail = false;
        }
        if let Ok((run_id, events)) = self.events_rx.try_recv() {
            if self.selected_run.as_deref() == Some(run_id.as_str()) {
                self.events = events;
            }
        }
    }

    fn toolbar(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            if ui.button("Refresh").clicked() {
                self.reload_components();
                self.reload_runs();
                if let Some(run_id) = self.selected_run.clone() {
                    self.load_run_detail(run_id);
                }
            }
            let prev_kind = self.kind_filter;
            ComboBox::from_id_salt("stress_lab_kind_filter")
                .selected_text(self.kind_filter.label())
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.kind_filter, KindFilter::All, "All kinds");
                    ui.selectable_value(&mut self.kind_filter, KindFilter::Cpu, "CPU");
                    ui.selectable_value(&mut self.kind_filter, KindFilter::Gpu, "GPU");
                });
            if self.kind_filter != prev_kind {
                self.reload_components();
            }
            if ui
                .checkbox(&mut self.show_recent_all, "Show recent runs (all hardware)")
                .changed()
            {
                self.reload_runs();
            }
            if self.loading_components || self.loading_runs || self.loading_detail {
                ui.spinner();
            }
        });
    }

    fn reload_components(&mut self) {
        self.loading_components = true;
        let tx = self.components_tx.clone();
        let kind = self.kind_filter.to_hardware_kind();
        PlatformSpawner::spawn(async move {
            let result = data::fetch_components(kind).await;
            match result {
                Ok(rows) => {
                    let _ = tx.send(rows);
                }
                Err(e) => {
                    log::warn!("stress_lab components fetch: {e}");
                }
            }
        });
    }

    fn reload_runs(&mut self) {
        self.loading_runs = true;
        let tx = self.runs_tx.clone();
        let component = self.selected_component.clone();
        let recent_all = self.show_recent_all;
        PlatformSpawner::spawn(async move {
            let result = if recent_all || component.is_none() {
                data::fetch_recent_runs(80).await
            } else if let Some(cid) = component {
                data::fetch_runs_for_component(&cid).await
            } else {
                data::fetch_recent_runs(80).await
            };
            match result {
                Ok(rows) => {
                    let _ = tx.send(rows);
                }
                Err(e) => {
                    log::warn!("stress_lab runs fetch: {e}");
                }
            }
        });
    }

    fn load_run_detail(&mut self, run_id: String) {
        self.loading_detail = true;
        self.metrics.clear();
        self.events.clear();
        let mtx = self.metrics_tx.clone();
        let etx = self.events_tx.clone();
        let rid = run_id.clone();
        PlatformSpawner::spawn(async move {
            let metrics = data::fetch_metrics(&rid).await.unwrap_or_default();
            let events = data::fetch_events(&rid).await.unwrap_or_default();
            let _ = mtx.send((rid.clone(), metrics));
            let _ = etx.send((rid, events));
        });
    }

    fn components_panel(&mut self, ui: &mut Ui) {
        ui.heading("Hardware");
        ui.label(format!("{} components", self.components.len()));
        ScrollArea::vertical().show(ui, |ui| {
            for row in self.components.clone() {
                let selected = self.selected_component.as_deref() == Some(row.id.as_str());
                let label = format!(
                    "{}  ({} runs, {} fail)",
                    row.display_name, row.run_count, row.fail_count
                );
                if ui.selectable_label(selected, label).clicked() {
                    self.selected_component = Some(row.id.clone());
                    self.show_recent_all = false;
                    self.selected_run = None;
                    self.reload_runs();
                }
                ui.label(format!("  {} · {}", row.kind, row.id));
                ui.separator();
            }
        });
    }

    fn runs_panel(&mut self, ui: &mut Ui) {
        ui.heading("Runs");
        if let Some(cid) = &self.selected_component {
            ui.label(format!("Filtered: {cid}"));
        } else if self.show_recent_all {
            ui.label("Recent across all hardware");
        }
        ScrollArea::vertical().show(ui, |ui| {
            for row in self.runs.clone() {
                let selected = self.selected_run.as_deref() == Some(row.id.as_str());
                let result_color = match row.result.as_str() {
                    "pass" => theme::result_pass(ui),
                    "fail" => theme::result_fail(ui),
                    "aborted" => theme::result_aborted(ui),
                    _ => theme::result_unknown(ui),
                };
                ui.horizontal(|ui| {
                    ui.colored_label(result_color, &row.result);
                    if ui.selectable_label(selected, &row.started_at).clicked() {
                        self.selected_run = Some(row.id.clone());
                        self.load_run_detail(row.id.clone());
                    }
                });
                ui.label(format!(
                    "{} · {}",
                    row.tool_label,
                    row.preset_label.as_deref().unwrap_or("—")
                ));
                if let Some(h) = &row.hostname {
                    ui.label(format!("  {h}"));
                }
                if let (Some(tp), Some(unit)) = (row.peak_throughput, &row.throughput_unit) {
                    ui.label(format!("  peak {tp:.1} {unit}"));
                }
                if let Some(t) = row.max_temp_c {
                    ui.label(format!("  max temp {t:.0}°C"));
                }
                ui.separator();
            }
        });
    }

    fn detail_panel(&mut self, ui: &mut Ui) {
        ui.heading("Run detail");
        let Some(run_id) = self.selected_run.clone() else {
            ui.label("Select a run to view metrics and events.");
            return;
        };
        let run = self.runs.iter().find(|r| r.id == run_id);
        if let Some(r) = run {
            ui.label(format!("ID: {}", r.id));
            ui.label(format!(
                "Tool: {} · Result: {} · Failure: {}",
                r.tool_label, r.result, r.failure_kind
            ));
            if let Some(d) = r.duration_secs {
                ui.label(format!("Duration: {d:.1}s"));
            }
            if let Some(tc) = &r.target_component {
                ui.label(format!("Target: {tc}"));
            }
        }
        ui.separator();
        plots::render_metric_plots(ui, &self.metrics);
        ui.separator();
        plots::render_events(ui, &self.events);
    }
}
