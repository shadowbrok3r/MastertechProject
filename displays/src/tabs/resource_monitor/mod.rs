use eframe::egui::{Align, CentralPanel, Layout, ScrollArea, Ui};
use crate::ui_tools::{
    controls::{self, BarKind, Segment},
    icons::svg,
    theme,
};
use process_table::ProcessTableViewer;
use crate::channel_manager::ChannelManager;
use crossbeam::channel::{Receiver, Sender};
use database::schema::SystemInformation;
#[cfg(feature = "native-telemetry")]
use std::collections::HashMap;

pub mod machine_info;
pub mod process_table;
// Ungated: widgets.rs depends only on egui, theme and icons.
pub mod widgets;
#[cfg(feature = "native-telemetry")]
pub mod chart_board;
#[cfg(feature = "native-telemetry")]
pub mod hw_tables;
// Exposed unconditionally: `sysinfo_to_machine_info` needs no `stress_kit`.
pub mod sysinfo_convert;

pub use machine_info::{MachineDriveRow, MachineInfo};

#[cfg(feature = "native-telemetry")]
use stress_kit::telemetry::TelemetrySnapshot;

/// Rendered in place of a value nothing measured.
pub use widgets::ABSENT_TEXT as ABSENT;

/// Width at or above which the dashboard lays panels out two per row.
#[cfg(feature = "native-telemetry")]
const TWO_COL_MIN_WIDTH: f32 = 760.0;

/// Width at or above which a panel splits its own meters into two columns.
#[cfg(feature = "native-telemetry")]
const INNER_TWO_COL_MIN_WIDTH: f32 = 430.0;

/// Path the snapshot came from; the wire payload carries no rates, page file or rails.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TelemetrySource {
    Local,
    #[default]
    Wire,
}

impl TelemetrySource {
    /// True when the disk and adapter figures are per-second rates.
    pub fn io_rates_measured(self) -> bool {
        matches!(self, Self::Local)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ResourceMonitorState {
    #[default]
    AllCharts,
    Processes,
    RequestingData,
    Stop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LiveTelemetryView {
    #[default]
    Charts,
    Tables,
}

impl LiveTelemetryView {
    fn label(self) -> &'static str {
        match self {
            Self::Charts => "Charts",
            Self::Tables => "Tables",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum HwView {
    #[default]
    Cores,
    Memory,
    Disks,
    Networks,
    Whea,
    Gpus,
    Rails,
    Machine,
}

impl HwView {
    pub const ALL: [Self; 8] = [
        Self::Cores,
        Self::Memory,
        Self::Disks,
        Self::Networks,
        Self::Whea,
        Self::Gpus,
        Self::Rails,
        Self::Machine,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Cores => "CPU cores",
            Self::Memory => "Memory",
            Self::Disks => "Disks",
            Self::Networks => "Networks",
            Self::Whea => "WHEA",
            Self::Gpus => "GPUs",
            Self::Rails => "Board rails",
            Self::Machine => "Machine",
        }
    }
}

pub struct ResourceMonitor {
    pub state: ResourceMonitorState,
    pub sysinfo_channel: (Sender<SystemInformation>, Receiver<SystemInformation>),
    live_view: LiveTelemetryView,
    hw_view: HwView,
    filter: String,
    #[cfg(feature = "native-telemetry")]
    telemetry: TelemetrySnapshot,
    #[cfg(feature = "native-telemetry")]
    source: TelemetrySource,
    /// Latched once a local snapshot arrives, so the wire payload stops replacing it.
    #[cfg(feature = "native-telemetry")]
    prefer_local: bool,
    /// Lowest volts seen per rail label since telemetry started.
    #[cfg(feature = "native-telemetry")]
    rail_minimums: HashMap<String, f32>,
    #[cfg(feature = "native-telemetry")]
    chart_board: chart_board::ChartBoard,
    machine_info: Option<MachineInfo>,
    pub process_table_viewer: ProcessTableViewer,
    pub latest_sysinfo: Option<SystemInformation>,
    /// When true, arriving payloads still refresh the panels but stop feeding `chart_board`.
    pub charts_paused: bool,
}

impl Default for ResourceMonitor {
    fn default() -> Self {
        let sysinfo_channel = SystemInformation::create_unbounded_channel();
        Self {
            sysinfo_channel,
            state: ResourceMonitorState::default(),
            live_view: LiveTelemetryView::default(),
            hw_view: HwView::default(),
            filter: String::new(),
            #[cfg(feature = "native-telemetry")]
            telemetry: TelemetrySnapshot::default(),
            #[cfg(feature = "native-telemetry")]
            source: TelemetrySource::default(),
            #[cfg(feature = "native-telemetry")]
            prefer_local: false,
            #[cfg(feature = "native-telemetry")]
            rail_minimums: HashMap::new(),
            #[cfg(feature = "native-telemetry")]
            chart_board: chart_board::ChartBoard::default(),
            machine_info: None,
            process_table_viewer: ProcessTableViewer::new(),
            latest_sysinfo: None,
            charts_paused: false,
        }
    }
}

impl ResourceMonitor {
    /// Local agent snapshot: I/O rates, page-file figures and board rails are real.
    #[cfg(feature = "native-telemetry")]
    pub fn set_telemetry(&mut self, snapshot: TelemetrySnapshot) {
        self.telemetry = snapshot;
        self.source = TelemetrySource::Local;
        self.prefer_local = true;
        self.track_rail_minimums();
    }

    /// Folds this tick's rails into the per-rail minimum seen.
    #[cfg(feature = "native-telemetry")]
    fn track_rail_minimums(&mut self) {
        let rails: Vec<(String, f32)> = self
            .telemetry
            .rails()
            .iter()
            .map(|v| (v.label.clone(), v.volts))
            .collect();
        for (label, volts) in rails {
            let seen = self.rail_minimums.entry(label).or_insert(volts);
            *seen = seen.min(volts);
        }
    }

    pub fn set_machine_info(&mut self, info: MachineInfo) {
        self.machine_info = Some(info);
    }

    fn receive(&mut self) {
        if let Ok(sysinfo) = self.sysinfo_channel.1.try_recv() {
            self.set_sysinfo(sysinfo);
        }
    }

    pub fn set_sysinfo(&mut self, sysinfo: SystemInformation) {
        self.latest_sysinfo = Some(sysinfo.clone());

        if matches!(self.state, ResourceMonitorState::RequestingData) {
            self.state = ResourceMonitorState::AllCharts;
        }

        // Machine facts come from this same payload.
        self.machine_info = Some(sysinfo_convert::sysinfo_to_machine_info(&sysinfo));

        #[cfg(feature = "native-telemetry")]
        {
            // A local snapshot carries I/O rates, page-file figures and board rails
            // that this payload never does, so once one has arrived it stands.
            if !self.prefer_local {
                self.telemetry = sysinfo_convert::sysinfo_to_telemetry(&sysinfo);
                self.source = TelemetrySource::Wire;
            }
            self.track_rail_minimums();
            // Paused charts stop taking samples; the panels keep refreshing.
            if !self.charts_paused {
                self.chart_board.push(&self.telemetry);
            }
        }

        if !matches!(self.state, ResourceMonitorState::Stop) {
            self.process_table_viewer.set_data(sysinfo.processes);
        }
    }

    pub fn display(&mut self, ui: &mut Ui) {
        self.receive();

        ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));

        eframe::egui::Panel::top("Resource Monitor Top Panel")
            .show(ui, |ui| {
                controls::bar(ui, BarKind::Tool, |ui| {
                    controls::heading(ui, svg::MT_HEALTH, "Resource monitor");
                    crate::tabs::bar_rule(ui);

                    let all_charts_selected = matches!(
                        self.state,
                        ResourceMonitorState::AllCharts | ResourceMonitorState::RequestingData
                    );
                    if controls::pill(ui, svg::CHART_LINE, "Live telemetry", all_charts_selected)
                        .clicked()
                    {
                        self.state = ResourceMonitorState::AllCharts;
                    }

                    let processes_selected = matches!(self.state, ResourceMonitorState::Processes);
                    if controls::pill(ui, svg::LIST, "Processes", processes_selected).clicked() {
                        self.state = ResourceMonitorState::Processes;
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if controls::icon_text_button(ui, svg::ARROW_CLOCKWISE, "Refresh").clicked()
                        {
                            self.state = ResourceMonitorState::RequestingData;
                        }

                        if !matches!(self.state, ResourceMonitorState::Stop)
                            && controls::icon_text_button(ui, svg::STOP, "Stop").clicked()
                        {
                            self.state = ResourceMonitorState::Stop;
                        }
                    });
                });
            });

        CentralPanel::default().show(ui, |ui| {
            match self.state {
                ResourceMonitorState::Stop => {}
                ResourceMonitorState::RequestingData => {
                    controls::loading_state(ui, "Loading system data…");
                }
                ResourceMonitorState::Processes => {
                    self.process_table_viewer.show(ui);
                }
                ResourceMonitorState::AllCharts => {
                    self.show_all_charts(ui);
                }
            }
        });
    }

    /// Drains the sysinfo channel for callers that skip `display()`.
    pub fn pump_telemetry(&mut self) {
        self.receive();
    }

    /// Home dashboard: machine line, KPI row, trend charts, per-item meter panels.
    pub fn show_compact_overview(&mut self, ui: &mut Ui) {
        self.receive();
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));

        #[cfg(feature = "native-telemetry")]
        {
            self.dashboard_header(ui);
            self.machine_line(ui);
            self.headline_panel(ui);
            let two_col = ui.available_width() >= TWO_COL_MIN_WIDTH;
            hw_tables::panel(ui, svg::CHART_LINE, "Trends", |ui| {
                if two_col {
                    self.chart_board.show_compact(ui);
                } else {
                    self.chart_board.show_compact_column(ui);
                }
            });
            self.meter_groups(ui, two_col);
        }

        #[cfg(not(feature = "native-telemetry"))]
        {
            controls::IconLabel::new(
                svg::PULSE,
                "Live telemetry requires the native build with stress-kit.",
            )
            .tone(theme::weak_text(ui))
            .show(ui);
        }
    }

    /// Title, capture time and the chart pause toggle.
    #[cfg(feature = "native-telemetry")]
    fn dashboard_header(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            controls::subheading(ui, svg::PULSE, "Live telemetry");
            ui.label(
                eframe::egui::RichText::new(format!(
                    "captured @ {}",
                    hw_tables::fmt_captured_at(self.telemetry.captured_at_unix_ms)
                ))
                .small()
                .weak(),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let (icon, hover) = if self.charts_paused {
                    (svg::PLAY, "Resume live chart updates")
                } else {
                    (
                        svg::PAUSE,
                        "Pause live chart updates (latest_sysinfo + panels keep refreshing)",
                    )
                };
                let toggle = controls::icon_button(
                    ui,
                    icon,
                    controls::IconBox::Sm,
                    self.charts_paused,
                    hover,
                );
                if toggle.clicked() {
                    self.charts_paused = !self.charts_paused;
                }
                if self.charts_paused {
                    controls::IconLabel::new(svg::PAUSE, "charts paused")
                        .tone(theme::warn(ui))
                        .show(ui);
                }
            });
        });
    }

    /// Host, CPU, RAM, GPU and volume count on one line.
    #[cfg(feature = "native-telemetry")]
    fn machine_line(&self, ui: &mut Ui) {
        match self.machine_info.as_ref() {
            Some(info) => info.show_header_line(ui),
            None => {
                controls::IconLabel::new(svg::DESKTOP, "Machine info not available yet.")
                    .tone(theme::weak_text(ui))
                    .show(ui);
            }
        }
    }

    /// KPI gauges, the CPU temperature and process tiles, and the WHEA pill.
    #[cfg(feature = "native-telemetry")]
    fn headline_panel(&self, ui: &mut Ui) {
        hw_tables::panel(ui, svg::GAUGE, "Headline", |ui| {
            let m = crate::ui_tools::metrics::Metrics::get(ui.ctx());
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.horizontal_wrapped(|ui| {
                        hw_tables::cpu_load_gauge(ui, &self.telemetry);
                        hw_tables::ram_gauge(ui, &self.telemetry.memory);
                        hw_tables::gpu_load_gauge(ui, &self.telemetry.gpus);
                        ui.add_space(m.gap.y);
                        hw_tables::cpu_temp_tile(ui, &self.telemetry);
                        ui.add_space(m.gap.y);
                        hw_tables::process_count_tile(ui, &self.telemetry);
                    });
                    ui.add_space(m.gap.y);
                    hw_tables::show_whea_pill(ui, &self.telemetry);
                });

                ui.add_space(m.gap.x * 2.0);

                // Identity and GPUs fill the width the gauge cluster leaves.
                ui.vertical(|ui| {
                    if let Some(info) = self.machine_info.as_ref() {
                        info.show_header_line(ui);
                        ui.add_space(m.gap.y);
                    }
                    self.gpu_panel(ui);
                });
            });
        });
    }

    /// Per-item meter panels: cores, memory, storage, adapters, rails.
    ///
    /// Rails only render when the snapshot carries them — a remote client's telemetry never does,
    /// and locally they need a kernel-mode SuperIO backend, so an ungated panel is an empty section
    /// on most machines.
    #[cfg(feature = "native-telemetry")]
    fn meter_groups(&self, ui: &mut Ui, two_col: bool) {
        if two_col {
            ui.columns(2, |cols| {
                self.cores_panel(&mut cols[0]);
                self.memory_panel(&mut cols[1]);
            });
            ui.columns(2, |cols| {
                self.storage_panel(&mut cols[0]);
                self.network_panel(&mut cols[1]);
            });
        } else {
            self.cores_panel(ui);
            self.memory_panel(ui);
            self.storage_panel(ui);
            self.network_panel(ui);
        }
        if !self.telemetry.rails().is_empty() {
            self.rails_panel(ui);
        }
    }

    #[cfg(feature = "native-telemetry")]
    fn cores_panel(&self, ui: &mut Ui) {
        hw_tables::panel(ui, svg::CPU, HwView::Cores.label(), |ui| {
            let columns = if ui.available_width() >= INNER_TWO_COL_MIN_WIDTH {
                2
            } else {
                1
            };
            hw_tables::show_core_meters(ui, &self.telemetry, columns);
        });
    }

    #[cfg(feature = "native-telemetry")]
    fn memory_panel(&self, ui: &mut Ui) {
        hw_tables::panel(ui, svg::MEMORY, HwView::Memory.label(), |ui| {
            hw_tables::show_memory_panel(ui, &self.telemetry.memory, self.source);
        });
    }

    /// Per-volume capacity meters carrying filesystem and throughput inline.
    #[cfg(feature = "native-telemetry")]
    fn storage_panel(&self, ui: &mut Ui) {
        hw_tables::panel(ui, svg::HARD_DRIVE, "Storage", |ui| {
            hw_tables::show_disk_meters(ui, &self.telemetry.disks, self.source);
        });
    }

    #[cfg(feature = "native-telemetry")]
    fn network_panel(&self, ui: &mut Ui) {
        hw_tables::panel(ui, svg::WIFI_HIGH, HwView::Networks.label(), |ui| {
            hw_tables::show_network_meters(ui, &self.telemetry.networks, self.source);
        });
    }

    #[cfg(feature = "native-telemetry")]
    fn gpu_panel(&self, ui: &mut Ui) {
        hw_tables::panel(ui, svg::GRAPHICS_CARD, HwView::Gpus.label(), |ui| {
            hw_tables::show_gpu_panel(ui, &self.telemetry.gpus);
        });
    }

    #[cfg(feature = "native-telemetry")]
    fn rails_panel(&self, ui: &mut Ui) {
        hw_tables::panel(ui, svg::LIGHTNING, HwView::Rails.label(), |ui| {
            hw_tables::show_rail_meters(ui, &self.telemetry, &self.rail_minimums);
        });
    }

    fn show_all_charts(&mut self, ui: &mut Ui) {
        eframe::egui::Panel::top("resource_monitor_live_view")
            .show(ui, |ui| {
                controls::bar(ui, BarKind::Tool, |ui| {
                    let views = [
                        Segment::new(LiveTelemetryView::Charts, LiveTelemetryView::Charts.label())
                            .icon(svg::CHART_LINE),
                        Segment::new(LiveTelemetryView::Tables, LiveTelemetryView::Tables.label())
                            .icon(svg::TABLE),
                    ];
                    controls::segmented(ui, &mut self.live_view, &views);

                    if self.live_view == LiveTelemetryView::Tables {
                        let hw_views: Vec<(HwView, &str)> =
                            HwView::ALL.iter().map(|v| (*v, v.label())).collect();
                        controls::dropdown(
                            ui,
                            "resource_monitor_hw_view",
                            &mut self.hw_view,
                            &hw_views,
                            130.0,
                        );

                        if matches!(
                            self.hw_view,
                            HwView::Cores | HwView::Disks | HwView::Networks
                        ) {
                            controls::TextField::new(&mut self.filter)
                                .hint("Filter…")
                                .icon(svg::FUNNEL_SIMPLE)
                                .clear_button(true)
                                .width(200.0)
                                .show(ui);
                        }
                    }

                    #[cfg(feature = "native-telemetry")]
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            eframe::egui::RichText::new(format!(
                                "captured @ {}",
                                hw_tables::fmt_captured_at(self.telemetry.captured_at_unix_ms)
                            ))
                            .small()
                            .weak(),
                        );
                    });
                });
            });

        ScrollArea::vertical().show(ui, |ui| {
            #[cfg(feature = "native-telemetry")]
            if self.live_view == LiveTelemetryView::Charts {
                self.chart_board.show(ui);
                return;
            }

            if self.hw_view == HwView::Machine {
                if let Some(info) = self.machine_info.clone() {
                    info.show(ui);
                } else {
                    controls::empty_state(
                        ui,
                        svg::DESKTOP,
                        "Machine info not available",
                        "",
                        &[],
                    );
                }
                return;
            }

            #[cfg(feature = "native-telemetry")]
            match self.hw_view {
                HwView::Cores => hw_tables::show_cores(ui, &self.telemetry, &self.filter),
                HwView::Memory => {
                    hw_tables::show_memory_panel(ui, &self.telemetry.memory, self.source)
                }
                HwView::Disks => hw_tables::show_disks(ui, &self.telemetry.disks, &self.filter),
                HwView::Networks => {
                    hw_tables::show_networks(ui, &self.telemetry.networks, &self.filter)
                }
                HwView::Whea => hw_tables::show_whea(ui, &self.telemetry),
                HwView::Gpus => hw_tables::show_gpus(ui, &self.telemetry.gpus),
                HwView::Rails => hw_tables::show_voltages(ui, &self.telemetry.voltages),
                HwView::Machine => {}
            }

            #[cfg(not(feature = "native-telemetry"))]
            {
                controls::empty_state(
                    ui,
                    svg::PULSE,
                    "Live telemetry unavailable",
                    "Live telemetry requires the native build with stress-kit.",
                    &[],
                );
            }
        });
    }
}

#[cfg(all(test, feature = "native-telemetry"))]
mod telemetry_source_tests {
    use super::*;

    /// The local agent snapshot is the only one carrying real I/O rates, page-file
    /// figures and board rails, so a wire payload arriving afterwards must not
    /// replace it. Both paths feed the same struct, which is what made this easy
    /// to get wrong.
    #[test]
    fn a_wire_payload_does_not_replace_the_local_snapshot() {
        let mut monitor = ResourceMonitor::default();

        // With nothing better available, the wire payload is the telemetry.
        monitor.set_sysinfo(SystemInformation::default());
        assert_eq!(monitor.source, TelemetrySource::Wire);
        assert!(!monitor.source.io_rates_measured());

        monitor.set_telemetry(TelemetrySnapshot::default());
        assert_eq!(monitor.source, TelemetrySource::Local);

        monitor.set_sysinfo(SystemInformation::default());
        assert_eq!(
            monitor.source,
            TelemetrySource::Local,
            "a wire tick clobbered the richer local snapshot"
        );
        assert!(
            monitor.source.io_rates_measured(),
            "the panels would silently fall back to unmeasured rates"
        );
    }

    /// The latch must only cover telemetry: the wire payload still owns the
    /// machine facts and the process list that other views read.
    #[test]
    fn a_wire_payload_still_fills_the_facts_it_owns() {
        let mut monitor = ResourceMonitor::default();
        monitor.set_telemetry(TelemetrySnapshot::default());
        monitor.set_sysinfo(SystemInformation::default());

        assert!(
            monitor.latest_sysinfo.is_some(),
            "tasks/mod.rs reads latest_sysinfo for the client cards"
        );
        assert!(monitor.machine_info.is_some(), "the machine line went blank");
    }
}
