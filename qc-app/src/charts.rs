//! Live chart board.
//!
//! Six small `LineChart` panels in a 3×2 grid:
//!   * Avg CPU % (cores mean)
//!   * Peak CPU % (cores max)
//!   * Avg core clock (MHz)
//!   * RAM used %
//!   * Top-disk total throughput (R+W MB/s)
//!   * Top-network total throughput (Rx+Tx Mbps)
//!
//! Every chart is **raw samples, linear segments**. No spline overshoot,
//! no invented peaks. The X axis is "seconds since first tick"; the
//! ring-buffer is capped at `HISTORY_SECS` so the panel stays at a
//! consistent visual cadence regardless of how long the app has been up.

use std::collections::VecDeque;

use eframe::egui::{self, Color32, RichText};
use egui_plot::{Line, Plot, PlotPoints};
use stress_kit::telemetry::TelemetrySnapshot;
use web_time::Instant;

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

        // Derived per-tick aggregates. Mean/peak across cores, plus the
        // single largest disk and network so we don't drown the chart in
        // 12+ lines of barely-used interfaces.
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

    pub fn show(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(4.0);
            ui.columns(2, |cols| {
                self.avg_cpu_pct.show(
                    &mut cols[0],
                    "avg_cpu",
                    "Avg CPU %",
                    "%",
                    Color32::from_rgb(120, 200, 255),
                    Some((0.0, 100.0)),
                );
                self.peak_cpu_pct.show(
                    &mut cols[1],
                    "peak_cpu",
                    "Peak CPU %",
                    "%",
                    Color32::from_rgb(220, 100, 100),
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
                    Color32::from_rgb(170, 230, 140),
                    None,
                );
                self.ram_used_pct.show(
                    &mut cols[1],
                    "ram_pct",
                    "RAM used",
                    "%",
                    Color32::from_rgb(220, 170, 90),
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
                    Color32::from_rgb(200, 120, 220),
                    Some((0.0, 100.0)),
                );
                self.top_disk_mb_s.show(
                    &mut cols[1],
                    "top_disk",
                    "Top disk throughput",
                    "MB/s",
                    Color32::from_rgb(140, 200, 200),
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
                    Color32::from_rgb(200, 200, 130),
                    None,
                );
                self.process_count.show(
                    &mut cols[1],
                    "proc_count",
                    "Tracked processes",
                    "count",
                    Color32::from_rgb(180, 180, 220),
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
        // Drop samples older than HISTORY_SECS so the chart window is
        // bounded even on long-running sessions.
        let cutoff = t - HISTORY_SECS;
        while let Some(&(ts, _)) = self.samples.front() {
            if ts < cutoff {
                self.samples.pop_front();
            } else {
                break;
            }
        }
        // Hard cap as a belt-and-braces against runaway tick rates.
        while self.samples.len() >= MAX_SAMPLES {
            self.samples.pop_front();
        }
        self.samples.push_back((t, y));
    }

    fn show(
        &self,
        ui: &mut egui::Ui,
        id: &str,
        title: &str,
        unit: &str,
        color: Color32,
        y_range: Option<(f64, f64)>,
    ) {
        ui.vertical(|ui| {
            let latest = self
                .samples
                .back()
                .map(|(_, y)| *y)
                .unwrap_or(0.0);
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
