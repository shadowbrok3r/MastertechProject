use eframe::egui::{
    Align, Button, CentralPanel, ComboBox, FontId, Layout, RichText, ScrollArea, Ui, Vec2, Widget,
};
use crate::ui_tools::theme;
use process_table::ProcessTableViewer;
use crate::channel_manager::ChannelManager;
use crossbeam::channel::{Receiver, Sender};
use database::schema::SystemInformation;

pub mod machine_info;
pub mod process_table;
#[cfg(feature = "native-telemetry")]
pub mod chart_board;
#[cfg(feature = "native-telemetry")]
pub mod hw_tables;
// `sysinfo_convert` is always exposed because its `sysinfo_to_machine_info`
// helper doesn't depend on `stress_kit` — the `native-telemetry`-gated bits
// are the per-function imports + the `sysinfo_to_telemetry` body.
pub mod sysinfo_convert;

pub use machine_info::{MachineDriveRow, MachineInfo};

#[cfg(feature = "native-telemetry")]
use stress_kit::telemetry::TelemetrySnapshot;

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
    Machine,
}

impl HwView {
    pub const ALL: [Self; 7] = [
        Self::Cores,
        Self::Memory,
        Self::Disks,
        Self::Networks,
        Self::Whea,
        Self::Gpus,
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
    chart_board: chart_board::ChartBoard,
    machine_info: Option<MachineInfo>,
    pub process_table_viewer: ProcessTableViewer,
    pub latest_sysinfo: Option<SystemInformation>,
    /// When true, new sysinfo payloads still land in `latest_sysinfo`
    /// (so tables stay current) but are NOT pushed into `chart_board`
    /// — the charts freeze at whatever values they had when the
    /// operator clicked Pause. Toggle from the chart header button on
    /// the Home page; resume snaps charts back to live with the next
    /// arriving sample.
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
            chart_board: chart_board::ChartBoard::default(),
            machine_info: None,
            process_table_viewer: ProcessTableViewer::new(),
            latest_sysinfo: None,
            charts_paused: false,
        }
    }
}

impl ResourceMonitor {
    #[cfg(feature = "native-telemetry")]
    pub fn set_telemetry(&mut self, snapshot: TelemetrySnapshot) {
        self.telemetry = snapshot;
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

        // Build the static-fact Machine panel from this same payload.
        // Previously `set_machine_info` was never called from anywhere
        // in the displays crate, so the Home page's Machine section
        // always read "Machine info not available yet."
        self.machine_info = Some(sysinfo_convert::sysinfo_to_machine_info(&sysinfo));

        #[cfg(feature = "native-telemetry")]
        {
            self.telemetry = sysinfo_convert::sysinfo_to_telemetry(&sysinfo);
            // Skip chart_board.push while paused so the line plots
            // freeze. Telemetry tables (machine, gpus, whea, etc.)
            // still update from `self.telemetry` so static facts
            // stay fresh.
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
            .exact_size(25.)
            .show_inside(ui, |ui| {
                eframe::egui::MenuBar::new().ui(ui, |ui| {
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        let button_stroke = ui.style().visuals.window_stroke;
                        let button_size = Vec2::new(120.0, 15.0);

                        let all_charts_selected = matches!(
                            self.state,
                            ResourceMonitorState::AllCharts | ResourceMonitorState::RequestingData
                        );

                        if Button::new("📊 Live telemetry")
                            .min_size(button_size)
                            .frame(true)
                            .stroke(button_stroke)
                            .fill(if all_charts_selected {
                                ui.style().visuals.selection.bg_fill
                            } else {
                                eframe::egui::Color32::TRANSPARENT
                            })
                            .ui(ui)
                            .clicked()
                        {
                            self.state = ResourceMonitorState::AllCharts;
                        }

                        ui.add_space(10.);

                        if Button::new("📋 Processes")
                            .min_size(button_size)
                            .stroke(button_stroke)
                            .fill(if matches!(self.state, ResourceMonitorState::Processes) {
                                ui.style().visuals.selection.bg_fill
                            } else {
                                eframe::egui::Color32::TRANSPARENT
                            })
                            .ui(ui)
                            .clicked()
                        {
                            self.state = ResourceMonitorState::Processes;
                        }
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_space(2.);
                        let button_stroke = ui.style().visuals.window_stroke;
                        let button_size = Vec2::new(60.0, 15.0);
                        if Button::new("Refresh")
                            .min_size(button_size)
                            .stroke(button_stroke)
                            .ui(ui)
                            .clicked()
                        {
                            self.state = ResourceMonitorState::RequestingData;
                        }

                        ui.add_space(5.);
                        if !matches!(self.state, ResourceMonitorState::Stop) {
                            if Button::new("Stop")
                                .min_size(button_size)
                                .stroke(button_stroke)
                                .ui(ui)
                                .clicked()
                            {
                                self.state = ResourceMonitorState::Stop;
                            }
                        }

                        ui.add_space(ui.available_width() / 1.5);

                        ui.label(
                            RichText::new("Resource monitor")
                                .color(theme::accent(ui))
                                .heading()
                                .font(FontId::monospace(20.)),
                        );
                    });
                });
            });

        CentralPanel::default().show_inside(ui, |ui| {
            match self.state {
                ResourceMonitorState::Stop => {}
                ResourceMonitorState::RequestingData => {
                    ui.vertical_centered(|ui| {
                        ui.label(
                            RichText::new("Loading system data...")
                                .color(ui.style().visuals.warn_fg_color)
                                .heading(),
                        );
                    });
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

    /// Drain the sysinfo channel so the chart_board / process_table / etc.
    /// pick up any frames that arrived while the parent wasn't calling
    /// `display()` directly. Used by the Home page, which renders our
    /// internals via the helper methods below instead of going through
    /// the full `display()` chrome (combobox, top toolbar, etc.).
    pub fn pump_telemetry(&mut self) {
        self.receive();
    }

    /// Combobox-free overview suitable for the Home page: live chart
    /// grid (the standard `chart_board` 2-col layout) followed by
    /// CollapsingHeaders for every per-section table — cores, memory,
    /// disks, networks, GPUs, WHEA/TDR counters, machine info.
    ///
    /// The headers default to collapsed for the verbose ones (cores,
    /// disks, networks) and expanded for the high-signal small-volume
    /// ones (machine info, WHEA/TDR, GPUs) so the page opens with
    /// useful detail visible without scrolling forever.
    pub fn show_compact_overview(&mut self, ui: &mut Ui) {
        self.receive();
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));

        #[cfg(feature = "native-telemetry")]
        {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Live telemetry")
                        .color(theme::accent(ui))
                        .strong(),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let (icon, hover) = if self.charts_paused {
                        (crate::ui_tools::icons::PLAY, "Resume live chart updates")
                    } else {
                        (crate::ui_tools::icons::PAUSE, "Pause live chart updates (latest_sysinfo + tables keep refreshing)")
                    };
                    if ui
                        .small_button(icon)
                        .on_hover_text(hover)
                        .clicked()
                    {
                        self.charts_paused = !self.charts_paused;
                    }
                    if self.charts_paused {
                        ui.colored_label(theme::warn(ui), "charts paused");
                    }
                });
            });
            // 4 × 2 compact grid, no axis labels — fits the RMM-style
            // "see everything at a glance" goal of the Home page.
            self.chart_board.show_compact(ui);
            ui.add_space(8.0);

            ui.collapsing(HwView::Machine.label(), |ui| {
                if let Some(info) = self.machine_info.clone() {
                    info.show(ui);
                } else {
                    ui.colored_label(theme::weak_text(ui), "Machine info not available yet.");
                }
            });
            ui.collapsing(HwView::Gpus.label(), |ui| {
                hw_tables::show_gpus(ui, &self.telemetry.gpus);
            });
            ui.collapsing(HwView::Whea.label(), |ui| {
                hw_tables::show_whea(ui, &self.telemetry.whea);
            });
            ui.collapsing(HwView::Memory.label(), |ui| {
                hw_tables::show_memory(ui, &self.telemetry.memory);
            });
            ui.collapsing(HwView::Cores.label(), |ui| {
                hw_tables::show_cores(ui, &self.telemetry, "");
            });
            ui.collapsing(HwView::Disks.label(), |ui| {
                hw_tables::show_disks(ui, &self.telemetry.disks, "");
            });
            ui.collapsing(HwView::Networks.label(), |ui| {
                hw_tables::show_networks(ui, &self.telemetry.networks, "");
            });
        }

        #[cfg(not(feature = "native-telemetry"))]
        {
            ui.colored_label(
                theme::weak_text(ui),
                "Live telemetry requires the native build with stress-kit.",
            );
        }
    }

    fn show_all_charts(&mut self, ui: &mut Ui) {
        #[allow(deprecated)]
        eframe::egui::Panel::top("resource_monitor_live_view")
            .exact_size(34.0)
            .show_inside(ui, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ComboBox::from_id_salt("resource_monitor_live_view")
                        .selected_text(self.live_view.label())
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.live_view,
                                LiveTelemetryView::Charts,
                                "Charts",
                            );
                            ui.selectable_value(
                                &mut self.live_view,
                                LiveTelemetryView::Tables,
                                "Tables",
                            );
                        });

                    if self.live_view == LiveTelemetryView::Tables {
                        ui.add_space(8.0);
                        ComboBox::from_id_salt("resource_monitor_hw_view")
                            .selected_text(self.hw_view.label())
                            .show_ui(ui, |ui| {
                                for view in HwView::ALL {
                                    ui.selectable_value(&mut self.hw_view, view, view.label());
                                }
                            });

                        ui.add_space(8.0);

                        if matches!(
                            self.hw_view,
                            HwView::Cores | HwView::Disks | HwView::Networks
                        ) {
                            let _ = ui.add(
                                eframe::egui::TextEdit::singleline(&mut self.filter)
                                    .hint_text("Filter…")
                                    .desired_width(200.0),
                            );
                        }
                    }

                    #[cfg(feature = "native-telemetry")]
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!(
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
                    ui.colored_label(theme::weak_text(ui), "Machine info not available.");
                }
                return;
            }

            #[cfg(feature = "native-telemetry")]
            match self.hw_view {
                HwView::Cores => hw_tables::show_cores(ui, &self.telemetry, &self.filter),
                HwView::Memory => hw_tables::show_memory(ui, &self.telemetry.memory),
                HwView::Disks => hw_tables::show_disks(ui, &self.telemetry.disks, &self.filter),
                HwView::Networks => {
                    hw_tables::show_networks(ui, &self.telemetry.networks, &self.filter)
                }
                HwView::Whea => hw_tables::show_whea(ui, &self.telemetry.whea),
                HwView::Gpus => hw_tables::show_gpus(ui, &self.telemetry.gpus),
                HwView::Machine => {}
            }

            #[cfg(not(feature = "native-telemetry"))]
            {
                ui.colored_label(
                    theme::weak_text(ui),
                    "Live telemetry requires the native build with stress-kit.",
                );
            }
        });
    }
}
