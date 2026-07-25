//! Live telemetry chart grid

use std::collections::{BTreeMap, HashMap, VecDeque};

use eframe::egui::{
    self, Align2, Color32, FontId, Rect, RichText, Sense, Stroke, StrokeKind, Ui, pos2, vec2,
};
use egui_plot::{Line, Plot, PlotPoints};
use stress_kit::telemetry::{CpuTempSource, GpuSample, TelemetrySnapshot, is_cpu_thermal_label};
use web_time::Instant;

use crate::ui_tools::{icons, theme};

const HISTORY_SECS: f64 = 120.0;
const MAX_SAMPLES: usize = 2048;
/// Narrowest time window a heatmap maps across.
const MIN_WINDOW_SECS: f64 = 10.0;
/// Width below which the charts stack in one column.
const TWO_COL_MIN_WIDTH: f32 = 520.0;
const COMPACT_HEIGHT: f32 = 88.0;
const FULL_HEIGHT: f32 = 130.0;
const TEMP_MIN_C: f32 = 0.5;
const TEMP_MAX_C: f32 = 150.0;
/// Series key that absorbs temperature devices past the palette's capacity.
const OTHER_TEMPS: &str = "Other";
/// CPU usage, CPU clock, temperatures, RAM.
const CELL_COUNT: usize = 4;

#[derive(Default)]
pub struct ChartBoard {
    started_at: Option<Instant>,
    cpu_usage_pct: MultiLineChart,
    cpu_usage_heat: CoreHeatmap,
    cpu_clock_heat: CoreHeatmap,
    temps: TempChart,
    ram_used_pct: LineChart,
    page_file_pct: Option<f64>,
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

        // Per-core usage stays lines only while the core count fits the palette.
        let usage_as_lines = snap.cores.len() <= theme::SERIES_LEN;
        for core in &snap.cores {
            if usage_as_lines {
                self.cpu_usage_pct
                    .push_series(&core_series_key(core.index), t, core.usage_pct as f64);
            } else {
                self.cpu_usage_heat
                    .push(core.index, t, core.usage_pct as f64);
            }
            self.cpu_clock_heat.push(core.index, t, core.freq_mhz as f64);
        }

        self.temps.push(snap, t);
        self.ram_used_pct.push(t, snap.memory.used_pct as f64);
        self.page_file_pct = Some(snap.memory.page_file_used_pct as f64);
    }

    /// Latest page-file utilization; `None` before the first sample. No chart plots it.
    pub fn page_file_used_pct(&self) -> Option<f64> {
        self.page_file_pct
    }

    /// Two charts per row, falling back to one column when the width can't hold two.
    pub fn show_compact(&self, ui: &mut Ui) {
        ui.add_space(2.0);
        let two_col = ui.available_width() >= TWO_COL_MIN_WIDTH;
        self.grid(ui, "chart_c", COMPACT_HEIGHT, true, two_col);
    }

    /// Every chart stacked in a single column.
    pub fn show_compact_column(&self, ui: &mut Ui) {
        self.grid(ui, "chart_col", COMPACT_HEIGHT, true, false);
    }

    pub fn show(&self, ui: &mut Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(4.0);
            let two_col = ui.available_width() >= TWO_COL_MIN_WIDTH;
            self.grid(ui, "chart_full", FULL_HEIGHT, false, two_col);
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

    fn grid(&self, ui: &mut Ui, id: &str, height: f32, compact: bool, two_col: bool) {
        if two_col {
            for row in 0..CELL_COUNT / 2 {
                if row > 0 {
                    ui.add_space(6.0);
                }
                ui.columns(2, |cols| {
                    self.cell(&mut cols[0], row * 2, id, height, compact);
                    self.cell(&mut cols[1], row * 2 + 1, id, height, compact);
                });
            }
        } else {
            for index in 0..CELL_COUNT {
                if index > 0 {
                    ui.add_space(6.0);
                }
                self.cell(ui, index, id, height, compact);
            }
        }
    }

    fn cell(&self, ui: &mut Ui, index: usize, id: &str, height: f32, compact: bool) {
        let ram_color = theme::series_color(0).unwrap_or_else(|| theme::other_series(ui));
        match index {
            0 => self.show_cpu_usage(ui, id, height, compact),
            1 => self
                .cpu_clock_heat
                .show(ui, "CPU clock (per core)", "MHz", None, height, compact),
            2 => self.temps.show(ui, &format!("{id}_temp"), height, compact),
            _ => self.ram_used_pct.show(
                ui,
                &format!("{id}_ram"),
                "RAM used",
                "%",
                ram_color,
                Some((0.0, 100.0)),
                height,
                compact,
            ),
        }
    }

    fn show_cpu_usage(&self, ui: &mut Ui, id: &str, height: f32, compact: bool) {
        if self.cpu_usage_heat.has_samples() {
            self.cpu_usage_heat.show(
                ui,
                "CPU usage (per core)",
                "%",
                Some((0.0, 100.0)),
                height,
                compact,
            );
        } else {
            self.cpu_usage_pct.show(
                ui,
                &format!("{id}_usage"),
                "CPU usage (per core)",
                "%",
                Some((0.0, 100.0)),
                height,
                compact,
            );
        }
    }
}

fn core_series_key(index: usize) -> String {
    format!("C{index}")
}

fn core_index_from_key(key: &str) -> Option<usize> {
    key.strip_prefix('C').and_then(|s| s.parse().ok())
}

/// Palette slot for a per-core series.
fn core_line_color(ui: &Ui, key: &str) -> Color32 {
    core_index_from_key(key)
        .and_then(theme::series_color)
        .unwrap_or_else(|| theme::other_series(ui))
}

/// True when a reading sits inside the plausible sensor window.
fn plausible(temp_c: f32) -> bool {
    // Zero is the wire schema's unknown-temperature fill, so the window starts above it.
    temp_c.is_finite() && (TEMP_MIN_C..=TEMP_MAX_C).contains(&temp_c)
}

/// A storage sensor label as published by the storage thermal reader.
fn is_drive_label(label: &str) -> bool {
    let l = label.trim().to_lowercase();
    l.starts_with("disk ") || l.starts_with("nvme disk ")
}

/// A firmware board zone (`TZnn`, `THRM`, `…zone…`); never a CPU or drive sensor.
fn is_board_zone_label(label: &str) -> bool {
    let l = label.trim().to_lowercase();
    !is_cpu_thermal_label(&l)
        && !is_drive_label(&l)
        && (l.starts_with("tz") || l.starts_with("thrm") || l.contains("zone"))
}

/// Trailing integer in a label (`NVMe Disk 3` → 3).
fn trailing_index(label: &str) -> usize {
    label
        .rsplit(|c: char| !c.is_ascii_digit())
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(usize::MAX)
}

fn gpu_label(gpu: &GpuSample) -> String {
    let name = gpu.name.trim();
    if name.is_empty() {
        format!("GPU {}", gpu.index)
    } else {
        name.to_string()
    }
}

/// Hottest per-core reading; the sysinfo path fills these when the thermal list is empty.
fn hottest_core_temp(snap: &TelemetrySnapshot) -> Option<f32> {
    snap.cores
        .iter()
        .filter_map(|c| c.temp_c)
        .filter(|t| plausible(*t))
        .fold(None, |acc, t| Some(acc.map_or(t, |m: f32| m.max(t))))
}

fn max_latest(series: &HashMap<String, VecDeque<(f64, f64)>>) -> Option<f64> {
    series
        .values()
        .filter_map(|s| s.back().map(|(_, y)| *y))
        .fold(None, |acc, y| Some(acc.map_or(y, |m: f64| m.max(y))))
}

/// Filled mark carrying a series' identity next to its label.
fn swatch(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
    ui.painter().rect_filled(rect, 1.0, color);
}

/// Legend row; the swatch carries identity and the label wears text ink.
fn legend_row(ui: &mut Ui, entries: &[(String, Color32)]) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (label, color) in entries {
            swatch(ui, *color);
            ui.label(RichText::new(label.as_str()).small());
            ui.add_space(4.0);
        }
    });
}

/// Title row with an optional identity mark; `value` of `None` renders as an explicit absence.
fn chart_header(
    ui: &mut Ui,
    title: &str,
    mark: Option<Color32>,
    value: Option<String>,
    compact: bool,
) {
    ui.horizontal(|ui| {
        if let Some(color) = mark {
            swatch(ui, color);
        }
        let title_rt = if compact {
            RichText::new(title).strong().small()
        } else {
            RichText::new(title).strong()
        };
        ui.label(title_rt);
        ui.with_layout(
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| match value {
                Some(text) => {
                    let rt = RichText::new(text).monospace();
                    ui.label(if compact { rt.small() } else { rt });
                }
                None => {
                    ui.label(RichText::new("— no sensor").small().weak());
                }
            },
        );
    });
}

fn empty_note(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(format!("{} {text}", icons::STATUS_OFF))
            .small()
            .weak(),
    );
}

/// Colour-scale key for the sequential magnitude ramp.
fn scale_key(ui: &mut Ui, lo: f64, hi: f64, unit: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label(RichText::new(format!("{lo:.0}")).small().weak());
        let (rect, _) = ui.allocate_exact_size(vec2(56.0, 6.0), Sense::hover());
        let painter = ui.painter().with_clip_rect(rect);
        const STEPS: usize = 24;
        let step_w = rect.width() / STEPS as f32;
        for i in 0..STEPS {
            let x = rect.left() + i as f32 * step_w;
            let cell = Rect::from_min_max(pos2(x, rect.top()), pos2(x + step_w + 0.5, rect.bottom()));
            let t = i as f32 / (STEPS - 1) as f32;
            painter.rect_filled(cell, 0.0, theme::sequential_contrast(ui, t));
        }
        ui.label(RichText::new(format!("{hi:.0} {unit}")).small().weak());
    });
}

fn plot_frame<'a>(id: &str, height: f32, compact: bool, unit: &str) -> Plot<'a> {
    let mut plot = Plot::new(id.to_owned())
        .height(height)
        .allow_drag(false)
        .allow_zoom(false)
        .allow_scroll(false)
        .show_background(false)
        .show_axes([!compact, !compact])
        .show_grid([true, true]);
    if !compact {
        plot = plot.x_axis_label("s").y_axis_label(unit.to_owned());
    }
    plot
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

/// One temperature series per device: the CPU package/die, each GPU, each drive
/// with a sensor, and at most one board zone.
#[derive(Default)]
struct TempChart {
    series: HashMap<String, VecDeque<(f64, f64)>>,
    /// Labels in first-seen order; the index is the palette slot.
    order: Vec<String>,
    /// Devices present this tick with no readable sensor.
    absent: Vec<String>,
    /// Devices folded into `OTHER_TEMPS`.
    folded: usize,
    /// The CPU value came from a firmware zone rather than the die sensor.
    cpu_zone_only: bool,
}

impl TempChart {
    fn push(&mut self, snap: &TelemetrySnapshot, t: f64) {
        let mut readings: Vec<(String, f32)> = Vec::new();
        self.absent.clear();

        let cpu = snap.cpu_temp_reading().filter(|(r, _)| plausible(r.temp_c));
        self.cpu_zone_only = matches!(cpu.as_ref(), Some((_, CpuTempSource::AcpiZone)));
        match cpu {
            Some((reading, CpuTempSource::AcpiZone)) => {
                readings.push((format!("CPU zone ({})", reading.label), reading.temp_c));
            }
            Some((reading, _)) => readings.push((reading.label.clone(), reading.temp_c)),
            None => match hottest_core_temp(snap) {
                Some(temp) => readings.push(("CPU (hottest core)".to_string(), temp)),
                None => self.absent.push("CPU".to_string()),
            },
        }

        for gpu in &snap.gpus {
            let label = gpu_label(gpu);
            match gpu.temp_c.filter(|t| plausible(*t)) {
                Some(temp) => {
                    let label = if readings.iter().any(|(l, _)| *l == label) {
                        format!("{label} #{}", gpu.index)
                    } else {
                        label
                    };
                    readings.push((label, temp));
                }
                None => self.absent.push(label),
            }
        }

        let mut drives: Vec<(&str, f32)> = snap
            .thermals
            .iter()
            .filter(|r| is_drive_label(&r.label) && plausible(r.temp_c))
            .map(|r| (r.label.as_str(), r.temp_c))
            .collect();
        drives.sort_by(|a, b| {
            trailing_index(a.0)
                .cmp(&trailing_index(b.0))
                .then_with(|| a.0.cmp(b.0))
        });
        drives.dedup_by(|a, b| a.0 == b.0);
        readings.extend(drives.into_iter().map(|(l, t)| (l.to_string(), t)));

        if let Some(zone) = snap
            .thermals
            .iter()
            .filter(|r| is_board_zone_label(&r.label) && plausible(r.temp_c))
            .max_by(|a, b| a.temp_c.total_cmp(&b.temp_c))
        {
            readings.push((format!("Board zone ({})", zone.label), zone.temp_c));
        }

        for (label, _) in &readings {
            if !self.order.iter().any(|l| l == label) {
                self.order.push(label.clone());
            }
        }

        // Beyond the palette's capacity the tail devices share one neutral series.
        let named = if self.order.len() > theme::SERIES_LEN {
            theme::SERIES_LEN - 1
        } else {
            theme::SERIES_LEN
        };
        self.folded = self.order.len().saturating_sub(named);
        if self.folded > 0 {
            let keep: Vec<String> = self.order.iter().take(named).cloned().collect();
            self.series
                .retain(|k, _| k == OTHER_TEMPS || keep.iter().any(|l| l == k));
        }

        let mut other: Option<f32> = None;
        for (label, temp) in &readings {
            if self.slot(label) < named {
                self.push_series(label, t, *temp as f64);
            } else {
                other = Some(other.map_or(*temp, |m: f32| m.max(*temp)));
            }
        }
        if let Some(temp) = other {
            self.push_series(OTHER_TEMPS, t, temp as f64);
        }
    }

    fn push_series(&mut self, label: &str, t: f64, temp_c: f64) {
        let samples = self.series.entry(label.to_string()).or_default();
        trim_samples(samples, t);
        samples.push_back((t, temp_c));
    }

    fn slot(&self, label: &str) -> usize {
        self.order
            .iter()
            .position(|l| l == label)
            .unwrap_or(usize::MAX)
    }

    /// Plotted series in palette-slot order, with the folded bucket last.
    fn series_list(&self, ui: &Ui) -> Vec<(String, String, Color32)> {
        let mut out: Vec<(String, String, Color32)> = self
            .order
            .iter()
            .filter(|l| self.series.contains_key(l.as_str()))
            .map(|l| {
                let color = theme::series_color(self.slot(l))
                    .unwrap_or_else(|| theme::other_series(ui));
                (l.clone(), l.clone(), color)
            })
            .collect();
        if self.series.contains_key(OTHER_TEMPS) {
            out.push((
                OTHER_TEMPS.to_string(),
                format!("Other (hottest of {})", self.folded),
                theme::other_series(ui),
            ));
        }
        out
    }

    fn has_samples(&self) -> bool {
        self.series.values().any(|s| !s.is_empty())
    }

    fn show(&self, ui: &mut Ui, id: &str, height: f32, compact: bool) {
        ui.vertical(|ui| {
            let peak = max_latest(&self.series).map(|v| format!("peak {v:.1} °C"));
            chart_header(ui, "Temperatures (per device)", None, peak, compact);

            if !self.has_samples() {
                empty_note(ui, "No temperature sensor readable on this platform.");
                self.notes(ui);
                return;
            }

            let series = self.series_list(ui);
            let legend: Vec<(String, Color32)> = series
                .iter()
                .map(|(_, label, color)| (label.clone(), *color))
                .collect();
            legend_row(ui, &legend);

            plot_frame(id, height, compact, "°C").show(ui, |plot_ui| {
                for (key, label, color) in &series {
                    let Some(samples) = self.series.get(key) else {
                        continue;
                    };
                    if samples.is_empty() {
                        continue;
                    }
                    let points: PlotPoints = samples.iter().map(|(t, y)| [*t, *y]).collect();
                    plot_ui.line(
                        Line::new(format!("{id}_{key}"), points)
                            .name(label.clone())
                            .color(*color)
                            .width(if compact { 1.2 } else { 1.6 }),
                    );
                }
            });

            self.notes(ui);
        });
    }

    fn notes(&self, ui: &mut Ui) {
        if self.cpu_zone_only {
            ui.label(
                RichText::new(format!(
                    "{} CPU value is a firmware zone, not the die sensor.",
                    icons::STATUS_WARN
                ))
                .small()
                .color(theme::warn(ui)),
            );
        }
        if !self.absent.is_empty() {
            ui.label(
                RichText::new(format!(
                    "{} Not measured: {}",
                    icons::STATUS_OFF,
                    self.absent.join(", ")
                ))
                .small()
                .weak(),
            );
        }
    }
}

/// Per-core magnitudes as a time-vs-core grid coloured from the sequential ramp.
#[derive(Default)]
struct CoreHeatmap {
    cores: BTreeMap<usize, VecDeque<(f64, f64)>>,
}

impl CoreHeatmap {
    fn push(&mut self, core: usize, t: f64, value: f64) {
        let samples = self.cores.entry(core).or_default();
        trim_samples(samples, t);
        samples.push_back((t, value));
    }

    fn has_samples(&self) -> bool {
        self.cores.values().any(|s| !s.is_empty())
    }

    /// min, mean and max across every sample in the window.
    fn stats(&self) -> Option<(f64, f64, f64)> {
        let mut min = f64::MAX;
        let mut max = f64::MIN;
        let mut sum = 0.0;
        let mut count = 0usize;
        for &(_, v) in self.cores.values().flatten() {
            min = min.min(v);
            max = max.max(v);
            sum += v;
            count += 1;
        }
        (count > 0).then(|| (min, sum / count as f64, max))
    }

    /// Sample time range, widened to `MIN_WINDOW_SECS` while the buffer is short.
    fn window(&self) -> Option<(f64, f64)> {
        let mut lo = f64::MAX;
        let mut hi = f64::MIN;
        for &(t, _) in self.cores.values().flatten() {
            lo = lo.min(t);
            hi = hi.max(t);
        }
        (lo <= hi).then(|| ((hi - MIN_WINDOW_SECS).min(lo), hi))
    }

    fn show(
        &self,
        ui: &mut Ui,
        title: &str,
        unit: &str,
        domain: Option<(f64, f64)>,
        height: f32,
        compact: bool,
    ) {
        ui.vertical(|ui| {
            let stats = self.stats();
            let readout = stats.map(|(min, mean, max)| format!("{min:.0} / {mean:.0} / {max:.0} {unit}"));
            chart_header(ui, title, None, readout, compact);
            if !compact {
                ui.label(
                    RichText::new("min / mean / max across cores in view")
                        .small()
                        .weak(),
                );
            }

            let (Some((t_lo, t_hi)), Some((v_min, _, v_max))) = (self.window(), stats) else {
                empty_note(ui, "No samples yet.");
                return;
            };

            let (lo, raw_hi) = domain.unwrap_or((v_min, v_max));
            let hi = if raw_hi - lo < 1.0 { lo + 1.0 } else { raw_hi };
            let rows = self.cores.len().max(1);
            let gutter = if compact { 18.0 } else { 26.0 };
            let axis_h = if compact { 0.0 } else { 12.0 };
            let desired_h = height.max((rows as f32 * 5.0).min(200.0));
            let (rect, response) =
                ui.allocate_exact_size(vec2(ui.available_width(), desired_h), Sense::hover());
            let grid = Rect::from_min_max(
                pos2(rect.left() + gutter, rect.top()),
                pos2(rect.right(), rect.bottom() - axis_h),
            );
            let painter = ui.painter().with_clip_rect(rect);
            let row_h = grid.height() / rows as f32;
            let inset = if row_h >= 6.0 { 0.5 } else { 0.0 };
            let label_step = ((10.0 / row_h).ceil() as usize).max(1);
            let x_of = |t: f64| {
                grid.left() + ((t - t_lo) / (t_hi - t_lo)) as f32 * grid.width()
            };

            for (row, (core, samples)) in self.cores.iter().enumerate() {
                let y0 = grid.top() + row as f32 * row_h;
                let y1 = y0 + row_h - inset;
                for (i, &(t, v)) in samples.iter().enumerate() {
                    let x0 = x_of(t).clamp(grid.left(), grid.right());
                    let next = samples
                        .get(i + 1)
                        .map_or(grid.right(), |&(next_t, _)| x_of(next_t));
                    let x1 = next.clamp(x0 + 1.0, (x0 + 1.0).max(grid.right()));
                    let norm = ((v - lo) / (hi - lo)).clamp(0.0, 1.0) as f32;
                    painter.rect_filled(
                        Rect::from_min_max(pos2(x0, y0), pos2(x1, y1)),
                        0.0,
                        theme::sequential_contrast(ui, norm),
                    );
                }
                if row % label_step == 0 {
                    painter.text(
                        pos2(grid.left() - 3.0, y0 + row_h / 2.0),
                        Align2::RIGHT_CENTER,
                        format!("{core}"),
                        FontId::monospace(9.0),
                        theme::weak_text(ui),
                    );
                }
            }

            painter.rect_stroke(
                grid,
                0.0,
                Stroke::new(1.0, theme::border(ui)),
                StrokeKind::Inside,
            );
            if !compact {
                let span = t_hi - t_lo;
                painter.text(
                    pos2(grid.left(), grid.bottom() + 2.0),
                    Align2::LEFT_TOP,
                    format!("-{span:.0} s"),
                    FontId::monospace(9.0),
                    theme::weak_text(ui),
                );
                painter.text(
                    pos2(grid.right(), grid.bottom() + 2.0),
                    Align2::RIGHT_TOP,
                    "now",
                    FontId::monospace(9.0),
                    theme::weak_text(ui),
                );
            }

            let hovered = response.hover_pos().filter(|p| grid.contains(*p));
            let tip = hovered.and_then(|pos| {
                let row = (((pos.y - grid.top()) / row_h) as usize).min(rows - 1);
                let (core, samples) = self.cores.iter().nth(row)?;
                let t = t_lo + ((pos.x - grid.left()) / grid.width()) as f64 * (t_hi - t_lo);
                let &(ts, v) = samples
                    .iter()
                    .min_by(|a, b| (a.0 - t).abs().total_cmp(&(b.0 - t).abs()))?;
                Some(format!("Core {core} · {v:.0} {unit} · -{:.0} s", t_hi - ts))
            });
            if let Some(text) = tip {
                let _ = response.on_hover_text(text);
            }

            scale_key(ui, lo, hi, unit);
        });
    }
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

    /// Highest latest value across series; `None` when no series has a sample.
    fn latest_summary(&self) -> Option<f64> {
        max_latest(&self.series)
    }

    fn has_samples(&self) -> bool {
        self.series.values().any(|s| !s.is_empty())
    }

    #[allow(clippy::too_many_arguments)]
    fn show(
        &self,
        ui: &mut Ui,
        id: &str,
        title: &str,
        unit: &str,
        y_range: Option<(f64, f64)>,
        height: f32,
        compact: bool,
    ) {
        ui.vertical(|ui| {
            let latest = self.latest_summary().map(|v| format!("peak {v:.1} {unit}"));
            chart_header(ui, title, None, latest, compact);

            if !self.has_samples() {
                empty_note(ui, "No samples yet.");
                return;
            }
            let mut keys: Vec<String> = self.series.keys().cloned().collect();
            keys.sort_by_key(|k| core_index_from_key(k.as_str()).unwrap_or(usize::MAX));
            let series: Vec<(String, Color32)> = keys
                .into_iter()
                .map(|k| {
                    let color = core_line_color(ui, k.as_str());
                    (k, color)
                })
                .collect();
            if series.len() >= 2 {
                legend_row(ui, &series);
            }

            let mut plot = plot_frame(id, height, compact, unit);
            if let Some((lo, hi)) = y_range {
                plot = plot.include_y(lo).include_y(hi);
            }
            plot.show(ui, |plot_ui| {
                for (key, color) in &series {
                    let Some(samples) = self.series.get(key.as_str()) else {
                        continue;
                    };
                    if samples.is_empty() {
                        continue;
                    }
                    let points: PlotPoints = samples.iter().map(|(t, y)| [*t, *y]).collect();
                    plot_ui.line(
                        Line::new(format!("{id}_{key}"), points)
                            .name(key.clone())
                            .color(*color)
                            .width(if compact { 1.2 } else { 1.6 }),
                    );
                }
            });
        });
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

    #[allow(clippy::too_many_arguments)]
    fn show(
        &self,
        ui: &mut Ui,
        id: &str,
        title: &str,
        unit: &str,
        color: Color32,
        y_range: Option<(f64, f64)>,
        height: f32,
        compact: bool,
    ) {
        ui.vertical(|ui| {
            let latest = self
                .samples
                .back()
                .map(|(_, y)| format!("{y:.1} {unit}"));
            chart_header(ui, title, Some(color), latest, compact);

            if self.samples.is_empty() {
                empty_note(ui, "No samples yet.");
                return;
            }

            let points: PlotPoints = self.samples.iter().map(|(t, y)| [*t, *y]).collect();
            let mut plot = plot_frame(id, height, compact, unit);
            if let Some((lo, hi)) = y_range {
                plot = plot.include_y(lo).include_y(hi);
            }
            plot.show(ui, |plot_ui| {
                plot_ui.line(
                    Line::new(id.to_owned(), points)
                        .color(color)
                        .width(if compact { 1.2 } else { 1.6 }),
                );
            });
        });
    }
}
