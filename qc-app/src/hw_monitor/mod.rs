//! Multi-view hardware monitor.
//!
//! One widget, one combobox, many views. All views read the same
//! [`stress_kit::telemetry::TelemetrySnapshot`] that the existing
//! `HwSampler` already publishes — there's no second sampling path.
//!
//! Views:
//!   * **Cores**   — per-logical-core usage / MHz / temp.
//!   * **Memory**  — RAM, page file, vmmem with usage bars.
//!   * **Disks**   — per-volume read/write MB/s.
//!   * **Networks**— per-adapter Rx/Tx Mbps.
//!   * **WHEA**    — Windows machine-check counters (when readable).
//!   * **Processes** — top-N by CPU% then RAM.
//!   * **GPUs**    — vendor / name / temp; vendor-specific live usage TBD.
//!   * **Charts**  — clean live plots (raw samples, linear segments). The
//!     antidote to the wavy `displays::resource_monitor` interpolation.
//!
//! The widget is `Sync + Send`-free by design (egui ownership) but its
//! internal history buffer is updated in place from [`HwMonitor::update`],
//! which the host should call once per frame with the latest snapshot.

use eframe::egui::{self, Color32, RichText};
use stress_kit::telemetry::TelemetrySnapshot;

mod charts;
mod tables;

use self::charts::ChartBoard;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum HwView {
    Cores,
    Memory,
    Disks,
    Networks,
    Whea,
    Processes,
    Gpus,
    Charts,
}

impl Default for HwView {
    fn default() -> Self {
        Self::Cores
    }
}

impl HwView {
    pub const ALL: [Self; 8] = [
        Self::Cores,
        Self::Memory,
        Self::Disks,
        Self::Networks,
        Self::Whea,
        Self::Processes,
        Self::Gpus,
        Self::Charts,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Cores => "CPU cores",
            Self::Memory => "Memory",
            Self::Disks => "Disks",
            Self::Networks => "Networks",
            Self::Whea => "WHEA",
            Self::Processes => "Processes",
            Self::Gpus => "GPUs",
            Self::Charts => "Charts",
        }
    }
}

pub struct HwMonitor {
    view: HwView,
    filter: String,
    snapshot: TelemetrySnapshot,
    charts: ChartBoard,
}

impl Default for HwMonitor {
    fn default() -> Self {
        Self {
            view: HwView::default(),
            filter: String::new(),
            snapshot: TelemetrySnapshot::default(),
            charts: ChartBoard::default(),
        }
    }
}

impl HwMonitor {
    /// Push the latest sampler snapshot. Cheap clone. The chart history is
    /// extended unconditionally so users get a populated graph even before
    /// they switch to the Charts view.
    pub fn update(&mut self, snapshot: TelemetrySnapshot) {
        self.charts.push(&snapshot);
        self.snapshot = snapshot;
    }

    /// Draw the full monitor UI inside the caller's `ui`.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        #[allow(deprecated)]
        egui::TopBottomPanel::top("hw_monitor_top")
            .exact_size(34.0)
            .show_inside(ui, |ui| {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("hw_monitor_view")
                        .selected_text(self.view.label())
                        .show_ui(ui, |ui| {
                            for v in HwView::ALL {
                                ui.selectable_value(&mut self.view, v, v.label());
                            }
                        });

                    ui.add_space(8.0);

                    if matches!(
                        self.view,
                        HwView::Cores | HwView::Processes | HwView::Disks | HwView::Networks
                    ) {
                        let _ = ui.add(
                            egui::TextEdit::singleline(&mut self.filter)
                                .hint_text("Filter…")
                                .desired_width(200.0),
                        );
                    }

                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            ui.label(
                                RichText::new(format!(
                                    "captured @ {}",
                                    fmt_unix_ms(self.snapshot.captured_at_unix_ms)
                                ))
                                .small()
                                .weak(),
                            );
                        },
                    );
                });
            });

        egui::CentralPanel::default().show_inside(ui, |ui| {
            // Drive a redraw so live data flows without user interaction.
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));

            match self.view {
                HwView::Cores => tables::show_cores(ui, &self.snapshot, &self.filter),
                HwView::Memory => tables::show_memory(ui, &self.snapshot.memory),
                HwView::Disks => tables::show_disks(ui, &self.snapshot.disks, &self.filter),
                HwView::Networks => {
                    tables::show_networks(ui, &self.snapshot.networks, &self.filter)
                }
                HwView::Whea => tables::show_whea(ui, &self.snapshot.whea),
                HwView::Processes => {
                    tables::show_processes(ui, &self.snapshot.processes, &self.filter)
                }
                HwView::Gpus => tables::show_gpus(ui, &self.snapshot.gpus),
                HwView::Charts => self.charts.show(ui),
            }
        });
    }
}

pub(crate) fn usage_color(pct: f32) -> Color32 {
    if pct >= 90.0 {
        Color32::from_rgb(220, 80, 60)
    } else if pct >= 70.0 {
        Color32::from_rgb(230, 180, 60)
    } else {
        Color32::from_rgb(100, 200, 100)
    }
}

pub(crate) fn temp_color(temp: f32) -> Color32 {
    if temp >= 90.0 {
        Color32::from_rgb(220, 80, 60)
    } else if temp >= 75.0 {
        Color32::from_rgb(230, 180, 60)
    } else {
        Color32::from_rgb(100, 200, 100)
    }
}

fn fmt_unix_ms(ms: u64) -> String {
    if ms == 0 {
        return "—".into();
    }
    let secs = (ms / 1000) as i64;
    let frac = (ms % 1000) as u32;
    // Format HH:MM:SS.mmm in UTC without pulling chrono just for this.
    let (_y, _mo, _d, h, mi, s) = epoch_to_parts(secs as u64);
    format!("{h:02}:{mi:02}:{s:02}.{frac:03}Z")
}

fn epoch_to_parts(mut secs: u64) -> (u64, u64, u64, u64, u64, u64) {
    let s = secs % 60;
    secs /= 60;
    let mi = secs % 60;
    secs /= 60;
    let h = secs % 24;
    secs /= 24;
    let z = secs + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };
    (y, mo, d, h, mi, s)
}
