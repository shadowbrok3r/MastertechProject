//! Live telemetry chart grid

use std::collections::{HashMap, VecDeque};

use eframe::egui::{self, RichText, Ui};
use egui_plot::{Corner, Legend, Line, Plot, PlotPoints};
use stress_kit::telemetry::{CoreSample, TelemetrySnapshot, ThermalReading};
use web_time::Instant;

use crate::ui_tools::theme::{self, chart_palette};

const HISTORY_SECS: f64 = 120.0;
const MAX_SAMPLES: usize = 2048;

#[derive(Default)]
pub struct ChartBoard {
    started_at: Option<Instant>,
    cpu_usage_pct: MultiLineChart,
    cpu_freq_mhz: MultiLineChart,
    cpu_temp_c: MultiLineChart,
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

        for core in &snap.cores {
            let key = core_series_key(core);
            self.cpu_usage_pct
                .push_series(&key, t, core.usage_pct as f64);
            self.cpu_freq_mhz
                .push_series(&key, t, core.freq_mhz as f64);
            if let Some(temp) = core.temp_c {
                self.cpu_temp_c.push_series(&key, t, temp as f64);
            }
        }

        for reading in package_thermal_readings(snap) {
            self.cpu_temp_c
                .push_series(&reading.label, t, reading.temp_c as f64);
        }

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

        self.ram_used_pct.push(t, snap.memory.used_pct as f64);
        self.page_file_used_pct
            .push(t, snap.memory.page_file_used_pct as f64);
        self.top_disk_mb_s.push(t, top_disk as f64);
        self.top_net_mbps.push(t, top_net as f64);
        self.process_count.push(t, snap.processes.len() as f64);
    }

    pub fn show_compact(&self, ui: &mut Ui) {
        let palette = chart_palette(ui);
        const COMPACT_HEIGHT: f32 = 78.0;
        ui.add_space(2.0);
        ui.columns(4, |cols| {
            self.cpu_usage_pct.show_with(
                &mut cols[0],
                "cpu_usage_c",
                "CPU usage",
                "%",
                Some((0.0, 100.0)),
                COMPACT_HEIGHT,
                true,
                &palette,
            );
            self.cpu_freq_mhz.show_with(
                &mut cols[1],
                "cpu_freq_c",
                "CPU clock",
                "MHz",
                None,
                COMPACT_HEIGHT,
                true,
                &palette,
            );
            self.cpu_temp_c.show_with(
                &mut cols[2],
                "cpu_temp_c",
                "CPU temp",
                "°C",
                None,
                COMPACT_HEIGHT,
                true,
                &palette,
            );
            self.ram_used_pct.show_with(
                &mut cols[3],
                "ram_pct_c",
                "RAM used",
                "%",
                palette.memory,
                Some((0.0, 100.0)),
                COMPACT_HEIGHT,
                true,
            );
        });
        ui.add_space(4.0);
        ui.columns(4, |cols| {
            self.page_file_used_pct.show_with(
                &mut cols[0],
                "pf_pct_c",
                "Page file",
                "%",
                palette.page_file,
                Some((0.0, 100.0)),
                COMPACT_HEIGHT,
                true,
            );
            self.top_disk_mb_s.show_with(
                &mut cols[1],
                "top_disk_c",
                "Top disk",
                "MB/s",
                palette.disk,
                None,
                COMPACT_HEIGHT,
                true,
            );
            self.top_net_mbps.show_with(
                &mut cols[2],
                "top_net_c",
                "Top adapter",
                "Mbps",
                palette.network,
                None,
                COMPACT_HEIGHT,
                true,
            );
            self.process_count.show_with(
                &mut cols[3],
                "proc_c",
                "Processes",
                "count",
                palette.process_count,
                None,
                COMPACT_HEIGHT,
                true,
            );
        });
    }

    /// Every chart stacked in a single vertical column.
    pub fn show_compact_column(&self, ui: &mut Ui) {
        let palette = chart_palette(ui);
        const H: f32 = 88.0;
        self.cpu_usage_pct.show_with(
            ui, "cpu_usage_col", "CPU usage", "%", Some((0.0, 100.0)), H, true, &palette,
        );
        ui.add_space(4.0);
        self.cpu_freq_mhz.show_with(
            ui, "cpu_freq_col", "CPU clock", "MHz", None, H, true, &palette,
        );
        ui.add_space(4.0);
        self.cpu_temp_c.show_with(
            ui, "cpu_temp_col", "CPU temp", "°C", None, H, true, &palette,
        );
        ui.add_space(4.0);
        self.ram_used_pct.show_with(
            ui, "ram_pct_col", "RAM used", "%", palette.memory, Some((0.0, 100.0)), H, true,
        );
        ui.add_space(4.0);
        self.page_file_used_pct.show_with(
            ui, "pf_pct_col", "Page file", "%", palette.page_file, Some((0.0, 100.0)), H, true,
        );
        ui.add_space(4.0);
        self.top_disk_mb_s.show_with(
            ui, "top_disk_col", "Top disk", "MB/s", palette.disk, None, H, true,
        );
        ui.add_space(4.0);
        self.top_net_mbps.show_with(
            ui, "top_net_col", "Top adapter", "Mbps", palette.network, None, H, true,
        );
        ui.add_space(4.0);
        self.process_count.show_with(
            ui, "proc_col", "Processes", "count", palette.process_count, None, H, true,
        );
    }

    pub fn show(&self, ui: &mut Ui) {
        let palette = chart_palette(ui);
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(4.0);
            ui.columns(2, |cols| {
                self.cpu_usage_pct.show_with(
                    &mut cols[0],
                    "cpu_usage",
                    "CPU usage (per core)",
                    "%",
                    Some((0.0, 100.0)),
                    130.0,
                    false,
                    &palette,
                );
                self.cpu_temp_c.show_with(
                    &mut cols[1],
                    "cpu_temp",
                    "CPU temp (per core)",
                    "°C",
                    None,
                    130.0,
                    false,
                    &palette,
                );
            });
            ui.add_space(6.0);
            ui.columns(2, |cols| {
                self.cpu_freq_mhz.show_with(
                    &mut cols[0],
                    "cpu_freq",
                    "CPU clock (per core)",
                    "MHz",
                    None,
                    130.0,
                    false,
                    &palette,
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

fn core_series_key(core: &CoreSample) -> String {
    format!("C{}", core.index)
}

fn core_index_from_key(key: &str) -> Option<usize> {
    key.strip_prefix('C').and_then(|s| s.parse().ok())
}

fn is_package_thermal_label(label: &str) -> bool {
    let l = label.to_lowercase();
    l.contains("package")
        || l.contains("cpu")
        || l.contains("tctl")
        || l.contains("tdie")
        || l.starts_with("tz")
}

fn package_thermal_readings(snap: &TelemetrySnapshot) -> Vec<ThermalReading> {
    let mut out: Vec<ThermalReading> = snap
        .thermals
        .iter()
        .filter(|r| is_package_thermal_label(&r.label))
        .cloned()
        .collect();
    if out.is_empty() {
        return out;
    }
    out.sort_by(|a, b| a.label.cmp(&b.label));
    out.dedup_by(|a, b| a.label == b.label);
    out
}

fn series_color(key: &str, palette: &theme::ChartPalette) -> egui::Color32 {
    if let Some(idx) = core_index_from_key(key) {
        return theme::core_series_color(idx);
    }
    palette.temperature
}

#[derive(Default)]
struct MultiLineChart {
    series: HashMap<String, VecDeque<(f64, f64)>>,
}

impl MultiLineChart {
    fn push_series(&mut self, name: &str, t: f64, y: f64) {
        let samples = self.series.entry(name.to_string()).or_default();
        trim_samples(samples, t);
        samples.push_back((t, y));
    }

    fn latest_summary(&self) -> f64 {
        self.series
            .values()
            .filter_map(|s| s.back().map(|(_, y)| *y))
            .fold(f64::NAN, |acc, y| {
                if acc.is_nan() {
                    y
                } else {
                    acc.max(y)
                }
            })
    }

    fn show_with(
        &self,
        ui: &mut Ui,
        id: &str,
        title: &str,
        unit: &str,
        y_range: Option<(f64, f64)>,
        height: f32,
        compact: bool,
        palette: &theme::ChartPalette,
    ) {
        ui.vertical(|ui| {
            let latest = self.latest_summary();
            let latest_display = if latest.is_nan() {
                0.0
            } else {
                latest
            };
            ui.horizontal(|ui| {
                let title_rt = if compact {
                    RichText::new(title).strong().small()
                } else {
                    RichText::new(title).strong()
                };
                ui.label(title_rt);
                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        let val_rt =
                            RichText::new(format!("{latest_display:.1} {unit}")).monospace();
                        ui.colored_label(
                            palette.avg_cpu,
                            if compact { val_rt.small() } else { val_rt },
                        );
                    },
                );
            });

            let mut plot = Plot::new(id)
                .height(height)
                .allow_drag(false)
                .allow_zoom(false)
                .allow_scroll(false)
                .show_background(false)
                .show_axes([!compact, !compact])
                .show_grid([true, true]);
            if !compact {
                plot = plot
                    .x_axis_label("s")
                    .y_axis_label(unit)
                    .legend(
                        Legend::default()
                            .position(Corner::LeftTop)
                            .background_alpha(0.9),
                    );
            }
            if let Some((lo, hi)) = y_range {
                plot = plot.include_y(lo).include_y(hi);
            }

            plot.show(ui, |plot_ui| {
                let mut keys: Vec<_> = self.series.keys().cloned().collect();
                keys.sort_by(|a, b| {
                    match (core_index_from_key(a), core_index_from_key(b)) {
                        (Some(i), Some(j)) => i.cmp(&j),
                        (Some(_), None) => std::cmp::Ordering::Less,
                        (None, Some(_)) => std::cmp::Ordering::Greater,
                        (None, None) => a.cmp(b),
                    }
                });
                for key in keys {
                    let Some(samples) = self.series.get(&key) else {
                        continue;
                    };
                    if samples.is_empty() {
                        continue;
                    }
                    let points: PlotPoints = samples.iter().map(|(t, y)| [*t, *y]).collect();
                    let line_id = format!("{id}_{key}");
                    plot_ui.line(
                        Line::new(line_id, points)
                            .name(key.clone())
                            .color(series_color(&key, palette))
                            .width(if compact { 1.2 } else { 1.6 }),
                    );
                }
            });
        });
    }
}

fn trim_samples(samples: &mut VecDeque<(f64, f64)>, t: f64) {
    let cutoff = t - HISTORY_SECS;
    while let Some(&(ts, _)) = samples.front() {
        if ts < cutoff {
            samples.pop_front();
        } else {
            break;
        }
    }
    while samples.len() >= MAX_SAMPLES {
        samples.pop_front();
    }
}

#[derive(Default)]
struct LineChart {
    samples: VecDeque<(f64, f64)>,
}

impl LineChart {
    fn push(&mut self, t: f64, y: f64) {
        trim_samples(&mut self.samples, t);
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
        self.show_with(ui, id, title, unit, color, y_range, 130.0, false);
    }

    fn show_with(
        &self,
        ui: &mut Ui,
        id: &str,
        title: &str,
        unit: &str,
        color: egui::Color32,
        y_range: Option<(f64, f64)>,
        height: f32,
        compact: bool,
    ) {
        ui.vertical(|ui| {
            let latest = self.samples.back().map(|(_, y)| *y).unwrap_or(0.0);
            ui.horizontal(|ui| {
                let title_rt = if compact {
                    RichText::new(title).strong().small()
                } else {
                    RichText::new(title).strong()
                };
                ui.label(title_rt);
                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        let val_rt = RichText::new(format!("{latest:.1} {unit}")).monospace();
                        ui.colored_label(color, if compact { val_rt.small() } else { val_rt });
                    },
                );
            });

            let points: PlotPoints = self
                .samples
                .iter()
                .map(|(t, y)| [*t, *y])
                .collect();

            let mut plot = Plot::new(id)
                .height(height)
                .allow_drag(false)
                .allow_zoom(false)
                .allow_scroll(false)
                .show_background(false)
                .show_axes([!compact, !compact])
                .show_grid([true, true]);
            if !compact {
                plot = plot.x_axis_label("s").y_axis_label(unit);
            }
            if let Some((lo, hi)) = y_range {
                plot = plot.include_y(lo).include_y(hi);
            }

            plot.show(ui, |plot_ui| {
                plot_ui.line(Line::new(id, points).color(color).width(1.6));
            });
        });
    }
}
