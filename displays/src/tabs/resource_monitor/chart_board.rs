//! Live telemetry chart grid 

use std::collections::VecDeque;

use eframe::egui::{self, RichText, Ui};
use egui_plot::{Line, Plot, PlotPoints};
use stress_kit::telemetry::TelemetrySnapshot;
use web_time::Instant;

use crate::ui_tools::theme::chart_palette;

const HISTORY_SECS: f64 = 120.0;
const MAX_SAMPLES: usize = 2048;

#[derive(Default)]
pub struct ChartBoard {
    started_at: Option<Instant>,
    avg_cpu_pct: LineChart,
    peak_cpu_pct: LineChart,
    avg_freq_mhz: LineChart,
    ram_used_pct: LineChart,
    page_file_used_pct: LineChart,
    top_disk_mb_s: LineChart,
    top_net_mbps: LineChart,
    process_count: LineChart,
}

impl ChartBoard {
    pub fn push(&mut self, snap: &TelemetrySnapshot) {
        let t = match self.started_at {
            Some(start) => start.elapsed().as_secs_f64(),
            None => {
                self.started_at = Some(Instant::now());
                0.0
            }
        };

        let n = snap.cores.len() as f32;
        let avg_pct = if n > 0.0 {
            snap.cores.iter().map(|c| c.usage_pct).sum::<f32>() / n
        } else {
            0.0
        };
        let peak_pct = snap
            .cores
            .iter()
            .map(|c| c.usage_pct)
            .fold(0.0_f32, f32::max);
        let avg_mhz = if n > 0.0 {
            snap.cores.iter().map(|c| c.freq_mhz as f32).sum::<f32>() / n
        } else {
            0.0
        };
        let top_disk = snap
            .disks
            .iter()
            .map(|d| d.read_mb_per_s + d.write_mb_per_s)
            .fold(0.0_f32, f32::max);
        let top_net = snap
            .networks
            .iter()
            .map(|n| n.rx_mbps + n.tx_mbps)
            .fold(0.0_f32, f32::max);

        self.avg_cpu_pct.push(t, avg_pct as f64);
        self.peak_cpu_pct.push(t, peak_pct as f64);
        self.avg_freq_mhz.push(t, avg_mhz as f64);
        self.ram_used_pct.push(t, snap.memory.used_pct as f64);
        self.page_file_used_pct
            .push(t, snap.memory.page_file_used_pct as f64);
        self.top_disk_mb_s.push(t, top_disk as f64);
        self.top_net_mbps.push(t, top_net as f64);
        self.process_count.push(t, snap.processes.len() as f64);
    }

    pub fn show(&self, ui: &mut Ui) {
        let palette = chart_palette(ui);
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(4.0);
            ui.columns(2, |cols| {
                self.avg_cpu_pct.show(
                    &mut cols[0],
                    "avg_cpu",
                    "Avg CPU %",
                    "%",
                    palette.avg_cpu,
                    Some((0.0, 100.0)),
                );
                self.peak_cpu_pct.show(
                    &mut cols[1],
                    "peak_cpu",
                    "Peak CPU %",
                    "%",
                    palette.peak_cpu,
                    Some((0.0, 100.0)),
                );
            });
            ui.add_space(6.0);
            ui.columns(2, |cols| {
                self.avg_freq_mhz.show(
                    &mut cols[0],
                    "avg_mhz",
                    "Avg core clock",
                    "MHz",
                    palette.clock,
                    None,
                );
                self.ram_used_pct.show(
                    &mut cols[1],
                    "ram_pct",
                    "RAM used",
                    "%",
                    palette.memory,
                    Some((0.0, 100.0)),
                );
            });
            ui.add_space(6.0);
            ui.columns(2, |cols| {
                self.page_file_used_pct.show(
                    &mut cols[0],
                    "pf_pct",
                    "Page file used",
                    "%",
                    palette.page_file,
                    Some((0.0, 100.0)),
                );
                self.top_disk_mb_s.show(
                    &mut cols[1],
                    "top_disk",
                    "Top disk throughput",
                    "MB/s",
                    palette.disk,
                    None,
                );
            });
            ui.add_space(6.0);
            ui.columns(2, |cols| {
                self.top_net_mbps.show(
                    &mut cols[0],
                    "top_net",
                    "Top adapter throughput",
                    "Mbps",
                    palette.network,
                    None,
                );
                self.process_count.show(
                    &mut cols[1],
                    "proc_count",
                    "Tracked processes",
                    "count",
                    palette.process_count,
                    None,
                );
            });
            ui.add_space(6.0);
            ui.label(
                RichText::new(
                    "Raw samples, linear segments — no spline interpolation. \
                     History capped at 120 s; older points are dropped.",
                )
                .small()
                .weak(),
            );
        });
    }
}

#[derive(Default)]
struct LineChart {
    samples: VecDeque<(f64, f64)>,
}

impl LineChart {
    fn push(&mut self, t: f64, y: f64) {
        let cutoff = t - HISTORY_SECS;
        while let Some(&(ts, _)) = self.samples.front() {
            if ts < cutoff {
                self.samples.pop_front();
            } else {
                break;
            }
        }
        while self.samples.len() >= MAX_SAMPLES {
            self.samples.pop_front();
        }
        self.samples.push_back((t, y));
    }

    fn show(
        &self,
        ui: &mut Ui,
        id: &str,
        title: &str,
        unit: &str,
        color: egui::Color32,
        y_range: Option<(f64, f64)>,
    ) {
        ui.vertical(|ui| {
            let latest = self.samples.back().map(|(_, y)| *y).unwrap_or(0.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(title).strong());
                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        ui.colored_label(
                            color,
                            RichText::new(format!("{latest:.1} {unit}")).monospace(),
                        );
                    },
                );
            });

            let points: PlotPoints = self
                .samples
                .iter()
                .map(|(t, y)| [*t, *y])
                .collect();

            let mut plot = Plot::new(id)
                .height(130.0)
                .allow_drag(false)
                .allow_zoom(false)
                .allow_scroll(false)
                .show_background(false)
                .show_axes([true, true])
                .show_grid([true, true])
                .x_axis_label("s")
                .y_axis_label(unit);
            if let Some((lo, hi)) = y_range {
                plot = plot.include_y(lo).include_y(hi);
            }

            plot.show(ui, |plot_ui| {
                plot_ui.line(Line::new(id, points).color(color).width(1.6));
            });
        });
    }
}
