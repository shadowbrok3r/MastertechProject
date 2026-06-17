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
//!   * **Temps**   — CPU (MSR/SMU via WinRing0) + NVMe/SATA disk temps.
//!
//! The widget is `Sync + Send`-free by design (egui ownership) but its
//! internal history buffer is updated in place from [`HwMonitor::update`],
//! which the host should call once per frame with the latest snapshot.

use eframe::egui::{self, Color32, RichText};
use stress_kit::telemetry::TelemetrySnapshot;

mod tables;

pub struct HwMonitor {
    snapshot: TelemetrySnapshot,
}

impl Default for HwMonitor {
    fn default() -> Self {
        Self {
            snapshot: TelemetrySnapshot::default(),
        }
    }
}

impl HwMonitor {
    /// Push the latest sampler snapshot. Cheap clone.
    pub fn update(&mut self, snapshot: TelemetrySnapshot) {
        self.snapshot = snapshot;
    }

    /// Draw every section at once: a global grid of tables, no view selector.
    /// Wide tables (cores, processes) span full width; the compact ones pair up.
    pub fn show(&mut self, ui: &mut egui::Ui) {
        let snap = &self.snapshot;

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Hardware monitor").strong().size(15.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(format!(
                        "captured @ {}",
                        fmt_unix_ms(snap.captured_at_unix_ms)
                    ))
                    .small()
                    .weak(),
                );
            });
        });
        ui.separator();
        // Drive a redraw so live data flows without user interaction.
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));

        egui::ScrollArea::vertical().show(ui, |ui| {
            section(ui, "CPU cores", |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("hw_all_cores")
                    .max_height(240.0)
                    .show(ui, |ui| tables::show_cores(ui, snap, ""));
            });
            ui.columns(2, |cols| {
                {
                    let ui = &mut cols[0];
                    section(ui, "Memory", |ui| tables::show_memory(ui, &snap.memory));
                    section(ui, "Disks", |ui| tables::show_disks(ui, &snap.disks, ""));
                    section(ui, "GPUs", |ui| tables::show_gpus(ui, &snap.gpus));
                }
                {
                    let ui = &mut cols[1];
                    section(ui, "Temperatures", |ui| {
                        tables::show_thermals(ui, &snap.thermals, "")
                    });
                    section(ui, "Networks", |ui| {
                        tables::show_networks(ui, &snap.networks, "")
                    });
                    section(ui, "WHEA", |ui| tables::show_whea(ui, &snap.whea));
                }
            });
            section(ui, "Processes", |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("hw_all_procs")
                    .max_height(260.0)
                    .show(ui, |ui| tables::show_processes(ui, &snap.processes, ""));
            });
        });
    }
}

/// One titled section box for the all-in-one grid.
fn section(ui: &mut egui::Ui, title: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.group(|ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(title).strong());
        ui.separator();
        add(ui);
    });
    ui.add_space(6.0);
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
