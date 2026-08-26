//! Cross-hardware comparison: one stressor, every part that has run it.
//!
//! Everything here is derived from the run snapshot already in memory, except
//! the timeline, which is the only chart that needs telemetry rows.

use std::collections::BTreeMap;

use database::schema::{RecordId, RecordIdExt, RunResult};
use eframe::egui::{ComboBox, RichText, Ui};
use egui_plot::{Bar, BarChart, Legend, Line, MarkerShape, PlotPoints, Points};

use crate::ui_tools::{icons, plots as plot_tools, theme};
use crate::{PlatformSpawner, Spawner};

use super::data::{RunRecord, SeriesBucket};
use super::metrics::{RunMetric, SeriesMetric};
use super::{MAX_COMPARE_SERIES, StressLab, fmt_run_stamp, series_color};

/// Telemetry rows fetched for the timeline, over every part being compared.
const MAX_COMPARE_RUNS: usize = 24;

/// What the coloured series stand for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// One stressor, one series per hardware part that has run it.
    ByHardware,
    /// One hardware part, one series per stressor it has run.
    ByStressor,
}

impl Axis {
    const VALUES: [Self; 2] = [Self::ByHardware, Self::ByStressor];

    fn label(self) -> &'static str {
        match self {
            Self::ByHardware => "One stressor, every part",
            Self::ByStressor => "One part, every stressor",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum RunPick {
    Latest,
    Best,
    All,
}

impl RunPick {
    const VALUES: [Self; 3] = [Self::Latest, Self::Best, Self::All];

    fn label(self) -> &'static str {
        match self {
            Self::Latest => "Latest run per part",
            Self::Best => "Best run per part",
            Self::All => "Every run",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Agg {
    Max,
    Mean,
}

impl Agg {
    fn label(self) -> &'static str {
        match self {
            Self::Max => "Best",
            Self::Mean => "Average",
        }
    }

    fn apply(self, values: &[f64]) -> Option<f64> {
        if values.is_empty() {
            return None;
        }
        match self {
            Self::Max => values
                .iter()
                .copied()
                .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)),
            Self::Mean => Some(values.iter().sum::<f64>() / values.len() as f64),
        }
    }
}

pub struct CompareState {
    pub axis: Axis,
    pub component: Option<RecordId>,
    pub tool: Option<String>,
    pub unit: Option<String>,
    pub series_metric: SeriesMetric,
    pub run_pick: RunPick,
    pub scatter_x: RunMetric,
    pub scatter_y: RunMetric,
    pub bar_metric: RunMetric,
    pub bar_agg: Agg,
    pub buckets: Vec<SeriesBucket>,
    pub bucket_secs: u32,
    /// Selection that produced `buckets`; a mismatch is what triggers a refetch.
    pub loaded_key: Option<String>,
    pub dirty: bool,
}

impl Default for CompareState {
    fn default() -> Self {
        Self {
            axis: Axis::ByHardware,
            component: None,
            tool: None,
            unit: None,
            series_metric: SeriesMetric::Throughput,
            run_pick: RunPick::Latest,
            scatter_x: RunMetric::PeakTempC,
            scatter_y: RunMetric::PeakThroughput,
            bar_metric: RunMetric::PeakThroughput,
            bar_agg: Agg::Max,
            buckets: Vec::new(),
            bucket_secs: 1,
            loaded_key: None,
            dirty: true,
        }
    }
}

/// One part and the runs of the selected stressor that exercised it.
struct Series {
    name: String,
    runs: Vec<RunRecord>,
}

pub fn ui(lab: &mut StressLab, ui: &mut Ui) {
    controls(lab, ui);

    let (scope, series) = match lab.compare.axis {
        Axis::ByHardware => {
            let Some(tool) = lab.compare.tool.clone() else {
                ui.label(RichText::new("No stress runs loaded yet.").weak());
                return;
            };
            (tool.clone(), build_series_by_hardware(lab, &tool))
        }
        Axis::ByStressor => {
            let Some(component) = lab.compare.component.clone() else {
                ui.label(
                    RichText::new("Pick a hardware part to compare its stressors.").weak(),
                );
                return;
            };
            (
                lab.component_name(&component),
                build_series_by_stressor(lab, &component),
            )
        }
    };

    if series.is_empty() {
        ui.label(RichText::new(format!("No completed runs for {scope}.")).weak());
        return;
    }

    ui.label(
        RichText::new(format!(
            "{} series · {} runs · {scope}",
            series.len(),
            series.iter().map(|s| s.runs.len()).sum::<usize>()
        ))
        .weak()
        .small(),
    );
    ui.separator();

    ensure_timeline(lab, &scope, &series);
    timeline_chart(lab, ui, &series);
    ui.separator();
    scatter_chart(lab, ui, &series);
    ui.separator();
    bar_chart(lab, ui, &series);
    ui.separator();
    result_mix_chart(ui, &series, lab.interactive_charts);
}

fn controls(lab: &mut StressLab, ui: &mut Ui) {
    let tools = lab.tools.clone();
    ui.horizontal_wrapped(|ui| {
        let before_axis = lab.compare.axis;
        ComboBox::from_id_salt("stress_lab_compare_axis")
            .width(210.0)
            .selected_text(lab.compare.axis.label())
            .show_ui(ui, |ui| {
                for axis in Axis::VALUES {
                    ui.selectable_value(&mut lab.compare.axis, axis, axis.label());
                }
            });
        if lab.compare.axis != before_axis {
            lab.compare.unit = None;
            lab.compare.dirty = true;
            if lab.compare.axis == Axis::ByStressor && lab.compare.component.is_none() {
                lab.compare.component = lab
                    .selected_component
                    .clone()
                    .or_else(|| lab.tested_components().into_iter().next().map(|(id, _)| id));
            }
        }

        match lab.compare.axis {
            Axis::ByHardware => {
                ui.label(format!("{} Stressor", icons::FLASK));
                let before = lab.compare.tool.clone();
                ComboBox::from_id_salt("stress_lab_compare_tool")
                    .width(210.0)
                    .selected_text(lab.compare.tool.clone().unwrap_or_else(|| "—".into()))
                    .show_ui(ui, |ui| {
                        for tool in tools {
                            ui.selectable_value(&mut lab.compare.tool, Some(tool.clone()), tool);
                        }
                    });
                if lab.compare.tool != before {
                    lab.compare.unit = None;
                    lab.compare.dirty = true;
                }
            }
            Axis::ByStressor => {
                ui.label(format!("{} Hardware", icons::p::CPU));
                let parts = lab.tested_components();
                let before = lab.compare.component.clone();
                let selected = lab
                    .compare
                    .component
                    .as_ref()
                    .map(|c| lab.component_name(c))
                    .unwrap_or_else(|| "—".into());
                ComboBox::from_id_salt("stress_lab_compare_component")
                    .width(260.0)
                    .selected_text(selected)
                    .show_ui(ui, |ui| {
                        for (id, name) in parts {
                            ui.selectable_value(&mut lab.compare.component, Some(id), name);
                        }
                    });
                if lab.compare.component != before {
                    lab.compare.unit = None;
                    lab.compare.dirty = true;
                }
            }
        }

        let units = throughput_units(lab);
        if units.len() > 1 {
            let before = lab.compare.unit.clone();
            ComboBox::from_id_salt("stress_lab_compare_unit")
                .width(130.0)
                .selected_text(
                    lab.compare
                        .unit
                        .clone()
                        .unwrap_or_else(|| "All units".into()),
                )
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut lab.compare.unit, None, "All units");
                    for unit in units.clone() {
                        ui.selectable_value(&mut lab.compare.unit, Some(unit.clone()), unit);
                    }
                });
            if lab.compare.unit != before {
                lab.compare.dirty = true;
            }
        }

    });

    if throughput_units(lab).len() > 1 && lab.compare.unit.is_none() {
        ui.label(
            RichText::new(
                "This stressor reports more than one throughput unit; pick one before reading \
                 throughput across parts.",
            )
            .color(theme::warn(ui))
            .small(),
        );
    }
}

/// Distinct `throughput_unit` values among the selected tool's runs. Scenario
/// runs alone span several, and those numbers share no scale.
fn throughput_units(lab: &StressLab) -> Vec<String> {
    let mut units: Vec<String> = match lab.compare.axis {
        Axis::ByHardware => {
            let Some(tool) = &lab.compare.tool else {
                return Vec::new();
            };
            lab.runs
                .iter()
                .filter(|r| &r.tool_label == tool)
                .filter_map(|r| r.throughput_unit().map(str::to_owned))
                .collect()
        }
        Axis::ByStressor => {
            let Some(component) = &lab.compare.component else {
                return Vec::new();
            };
            lab.runs
                .iter()
                .filter(|r| r.involves(component))
                .filter_map(|r| r.throughput_unit().map(str::to_owned))
                .collect()
        }
    };
    units.sort();
    units.dedup();
    units
}

fn build_series_by_hardware(lab: &StressLab, tool: &str) -> Vec<Series> {
    let mut by_component: BTreeMap<String, (RecordId, Vec<RunRecord>)> = BTreeMap::new();
    for run in lab.runs.iter().filter(|r| {
        r.tool_label == tool
            && r.result != RunResult::InProgress
            && lab
                .compare
                .unit
                .as_deref()
                .is_none_or(|u| r.throughput_unit() == Some(u))
    }) {
        let Some(component) = run.target_component.clone() else {
            continue;
        };
        if lab
            .kind_filter
            .is_some_and(|kind| !lab.components.iter().any(|c| c.id == component && c.kind == kind))
        {
            continue;
        }
        by_component
            .entry(component.key_string())
            .or_insert_with(|| (component, Vec::new()))
            .1
            .push(run.clone());
    }

    let mut series: Vec<Series> = by_component
        .into_values()
        .map(|(component, mut runs)| {
            runs.sort_by_key(|r| std::cmp::Reverse(r.started_at.timestamp()));
            Series {
                name: lab.component_name(&component),
                runs,
            }
        })
        .collect();

    // Most-tested parts first, so the color budget goes where the data is.
    series.sort_by(|a, b| {
        b.runs
            .len()
            .cmp(&a.runs.len())
            .then_with(|| a.name.cmp(&b.name))
    });
    series.truncate(MAX_COMPARE_SERIES);
    series
}

/// One part's history, split by stressor. The counterpart to
/// [`build_series_by_hardware`]: same charts, the other axis.
fn build_series_by_stressor(lab: &StressLab, component: &RecordId) -> Vec<Series> {
    let mut by_tool: BTreeMap<String, Vec<RunRecord>> = BTreeMap::new();
    // Attributed by primary target, like `build_series_by_hardware`: a run that
    // merely touched this part while stressing another is not a measurement of
    // it. The Browse columns are the looser `involves` view.
    for run in lab.runs.iter().filter(|r| {
        r.result != RunResult::InProgress
            && r.target_component.as_ref() == Some(component)
            && lab
                .compare
                .unit
                .as_deref()
                .is_none_or(|u| r.throughput_unit() == Some(u))
    }) {
        by_tool
            .entry(run.tool_label.clone())
            .or_default()
            .push(run.clone());
    }

    let mut series: Vec<Series> = by_tool
        .into_iter()
        .map(|(name, mut runs)| {
            runs.sort_by_key(|r| std::cmp::Reverse(r.started_at.timestamp()));
            Series { name, runs }
        })
        .collect();
    series.sort_by(|a, b| {
        b.runs
            .len()
            .cmp(&a.runs.len())
            .then_with(|| a.name.cmp(&b.name))
    });
    series.truncate(MAX_COMPARE_SERIES);
    series
}

/// The runs whose telemetry the timeline needs, honouring the run-pick mode.
fn timeline_runs(state: &CompareState, series: &[Series]) -> Vec<RecordId> {
    // Budget per part rather than truncating the concatenation, which would
    // spend the whole allowance on whichever part happens to sort first.
    let per_series = (MAX_COMPARE_RUNS / series.len().max(1)).max(1);
    let mut out = Vec::new();
    for s in series {
        match state.run_pick {
            RunPick::Latest => out.extend(s.runs.first().map(|r| r.id.clone())),
            RunPick::Best => {
                let best = s.runs.iter().max_by(|a, b| {
                    let (a, b) = (
                        a.peak_throughput().unwrap_or(f64::MIN),
                        b.peak_throughput().unwrap_or(f64::MIN),
                    );
                    a.partial_cmp(&b).unwrap_or(std::cmp::Ordering::Equal)
                });
                out.extend(best.map(|r| r.id.clone()));
            }
            RunPick::All => out.extend(s.runs.iter().take(per_series).map(|r| r.id.clone())),
        }
    }
    out.truncate(MAX_COMPARE_RUNS);
    out
}

/// Rows the comparison fetch may pull across every charted run.
const MAX_SERIES_ROWS: f64 = 6000.0;

/// Points below which a run is not worth drawing as a line.
const MIN_POINTS_PER_RUN: f64 = 4.0;

/// Rows the fetch may never exceed, even to keep a very short run chartable.
const HARD_MAX_SERIES_ROWS: f64 = 20_000.0;

/// Bucket width for a set of run spans: wide enough to keep the whole fetch
/// bounded, but never so wide that the shortest run collapses below a line.
/// Sizing off the longest run alone silently dropped every short one.
fn compare_bucket(spans: &[f64]) -> u32 {
    let spans: Vec<f64> = spans.iter().copied().filter(|s| *s > 0.0).collect();
    if spans.is_empty() {
        return StressLab::bucket_for(None);
    }
    let total: f64 = spans.iter().sum();
    let shortest = spans.iter().copied().fold(f64::MAX, f64::min);
    let by_volume = (total / MAX_SERIES_ROWS).ceil().max(1.0);
    let by_shortest = (shortest / MIN_POINTS_PER_RUN).floor().max(1.0);
    let hard_floor = (total / HARD_MAX_SERIES_ROWS).ceil().max(1.0);
    // Narrowing for the shortest run is worth extra rows, but not unbounded
    // ones: a seconds-long run beside a multi-hour one would otherwise ask for
    // the whole metric table.
    by_volume.min(by_shortest).max(hard_floor) as u32
}

fn ensure_timeline(lab: &mut StressLab, tool: &str, series: &[Series]) {
    let runs = timeline_runs(&lab.compare, series);
    // Keyed on the run ids themselves: two different selections of the same
    // size are a different chart, and a filter change that swaps which parts
    // qualify has to refetch.
    let key = format!(
        "{tool}|{}|{}|{}",
        lab.compare.unit.clone().unwrap_or_default(),
        lab.compare.run_pick.label(),
        runs.iter().map(|r| r.key_string()).collect::<Vec<_>>().join(",")
    );
    if !lab.compare.dirty && lab.compare.loaded_key.as_deref() == Some(key.as_str()) {
        return;
    }
    if lab.loading_compare {
        return;
    }

    let spans: Vec<f64> = series
        .iter()
        .flat_map(|s| s.runs.iter())
        .filter(|r| runs.contains(&r.id))
        .filter_map(|r| r.span_secs())
        .collect();
    let bucket_secs = compare_bucket(&spans);

    lab.compare.dirty = false;
    lab.compare.loaded_key = Some(key);
    lab.loading_compare = true;
    lab.compare_request += 1;
    let request = lab.compare_request;
    let tx = lab.compare_tx.clone();
    let etx = lab.error_tx.clone();
    PlatformSpawner::spawn(async move {
        match super::data::fetch_series(&runs, bucket_secs).await {
            Ok(buckets) => {
                let _ = tx.send((request, bucket_secs, buckets));
            }
            Err(e) => {
                log::warn!("stress_lab compare series: {e}");
                let _ = etx.send((super::Source::Compare, format!("Comparison telemetry: {e}")));
            }
        }
    });
}

fn timeline_chart(lab: &mut StressLab, ui: &mut Ui, series: &[Series]) {
    ui.horizontal_wrapped(|ui| {
        ui.heading(format!("{} Over time", icons::p::CHART_LINE));
        ComboBox::from_id_salt("stress_lab_compare_series_metric")
            .width(170.0)
            .selected_text(lab.compare.series_metric.label())
            .show_ui(ui, |ui| {
                for metric in SeriesMetric::VALUES {
                    ui.selectable_value(&mut lab.compare.series_metric, metric, metric.label());
                }
            });
        let before = lab.compare.run_pick;
        ComboBox::from_id_salt("stress_lab_compare_pick")
            .width(170.0)
            .selected_text(lab.compare.run_pick.label())
            .show_ui(ui, |ui| {
                for pick in RunPick::VALUES {
                    ui.selectable_value(&mut lab.compare.run_pick, pick, pick.label());
                }
            });
        if lab.compare.run_pick != before {
            lab.compare.dirty = true;
        }
        if lab.loading_compare {
            ui.spinner();
        }
    });

    let metric = lab.compare.series_metric;
    if lab.compare.buckets.is_empty() {
        ui.label(
            RichText::new(if lab.loading_compare {
                "Loading telemetry…"
            } else {
                "No telemetry rows for the selected runs."
            })
            .weak(),
        );
        return;
    }

    // Elapsed time is per run: each run's own first bucket is its zero.
    let mut origins: BTreeMap<String, i64> = BTreeMap::new();
    for b in &lab.compare.buckets {
        let key = b.run.key_string();
        let entry = origins.entry(key).or_insert(b.bucket);
        *entry = (*entry).min(b.bucket);
    }

    let bucket_secs = lab.compare.bucket_secs as i64;
    let interactive = lab.interactive_charts;
    // One line per run, not per part: every run restarts elapsed time at zero,
    // so concatenating a part's runs would fold them on top of each other.
    let mut lines: Vec<(String, usize, Vec<[f64; 2]>)> = Vec::new();
    let mut dropped = 0usize;
    for (index, s) in series.iter().enumerate() {
        let charted: Vec<&RunRecord> = s
            .runs
            .iter()
            .filter(|r| origins.contains_key(&r.id.key_string()))
            .collect();
        for run in &charted {
            let Some(origin) = origins.get(&run.id.key_string()).copied() else {
                continue;
            };
            let mut points: Vec<[f64; 2]> = lab
                .compare
                .buckets
                .iter()
                .filter(|b| b.run == run.id)
                .filter_map(|b| {
                    metric
                        .value(b)
                        .map(|v| [((b.bucket - origin) * bucket_secs) as f64, v])
                })
                .collect();
            if points.len() < 2 {
                dropped += 1;
                continue;
            }
            points.sort_by(|a, b| a[0].partial_cmp(&b[0]).unwrap_or(std::cmp::Ordering::Equal));
            let name = if charted.len() > 1 {
                // Seconds included: egui_plot merges legend entries by name, and
                // back-to-back short runs share a minute.
                format!("{} · {}", s.name, fmt_run_stamp(&run.started_at))
            } else {
                s.name.clone()
            };
            lines.push((name, index, points));
        }
    }

    if lines.is_empty() {
        ui.label(
            RichText::new(format!("No part sampled {} on these runs.", metric.label())).weak(),
        );
        return;
    }

    if dropped > 0 {
        ui.label(
            RichText::new(format!(
                "{dropped} run(s) too short to chart at {}s buckets.",
                lab.compare.bucket_secs
            ))
            .weak()
            .small(),
        );
    }

    let y_label = axis_label(lab, metric.label(), metric.is_throughput());
    plot_tools::maybe_pinned("stress_lab_compare_timeline", interactive)
        .legend(Legend::default())
        .height(280.0)
        .x_axis_label("elapsed seconds")
        .y_axis_label(y_label)
        .show(ui, |plot| {
            if !interactive {
                plot.set_auto_bounds(true);
            }
            for (name, index, points) in &lines {
                plot.line(
                    Line::new(name.clone(), PlotPoints::new(points.clone()))
                        .color(series_color(*index)),
                );
            }
        });
}

fn scatter_chart(lab: &mut StressLab, ui: &mut Ui, series: &[Series]) {
    ui.horizontal_wrapped(|ui| {
        ui.heading(format!("{} Any measure vs any measure", icons::p::CHART_SCATTER));
        ui.label("X");
        metric_combo(ui, "stress_lab_scatter_x", &mut lab.compare.scatter_x);
        ui.label("Y");
        metric_combo(ui, "stress_lab_scatter_y", &mut lab.compare.scatter_y);
    });

    let (mx, my) = (lab.compare.scatter_x, lab.compare.scatter_y);
    let interactive = lab.interactive_charts;
    let mut sets: Vec<(String, usize, Vec<[f64; 2]>)> = Vec::new();
    for (index, s) in series.iter().enumerate() {
        let points: Vec<[f64; 2]> = s
            .runs
            .iter()
            .filter_map(|r| Some([mx.value(r)?, my.value(r)?]))
            .collect();
        if !points.is_empty() {
            sets.push((s.name.clone(), index, points));
        }
    }

    if sets.is_empty() {
        ui.label(
            RichText::new(format!(
                "No run records both {} and {}.",
                mx.label(),
                my.label()
            ))
            .weak(),
        );
        return;
    }

    let (x_label, y_label) = (
        axis_label(lab, mx.label(), mx.is_throughput()),
        axis_label(lab, my.label(), my.is_throughput()),
    );
    plot_tools::maybe_pinned("stress_lab_compare_scatter", interactive)
        .legend(Legend::default())
        .height(260.0)
        .x_axis_label(x_label)
        .y_axis_label(y_label)
        .show(ui, |plot| {
            if !interactive {
                plot.set_auto_bounds(true);
            }
            for (name, index, points) in &sets {
                plot.points(
                    Points::new(name.clone(), PlotPoints::new(points.clone()))
                        .color(series_color(*index))
                        .shape(MarkerShape::Circle)
                        .radius(4.0),
                );
            }
        });
}

fn bar_chart(lab: &mut StressLab, ui: &mut Ui, series: &[Series]) {
    ui.horizontal_wrapped(|ui| {
        ui.heading(format!("{} By part", icons::CHART));
        metric_combo(ui, "stress_lab_bar_metric", &mut lab.compare.bar_metric);
        ComboBox::from_id_salt("stress_lab_bar_agg")
            .width(100.0)
            .selected_text(lab.compare.bar_agg.label())
            .show_ui(ui, |ui| {
                for agg in [Agg::Max, Agg::Mean] {
                    ui.selectable_value(&mut lab.compare.bar_agg, agg, agg.label());
                }
            });
    });

    let metric = lab.compare.bar_metric;
    let agg = lab.compare.bar_agg;
    let interactive = lab.interactive_charts;
    let mut bars: Vec<Bar> = Vec::new();
    let mut labels: Vec<String> = Vec::new();
    for (index, s) in series.iter().enumerate() {
        let values: Vec<f64> = s.runs.iter().filter_map(|r| metric.value(r)).collect();
        let Some(value) = agg.apply(&values) else {
            continue;
        };
        bars.push(
            Bar::new(labels.len() as f64, value)
                .name(format!("{} — {}", s.name, metric.label()))
                .fill(series_color(index)),
        );
        labels.push(short_name(&s.name));
    }

    if bars.is_empty() {
        ui.label(RichText::new(format!("No part records {}.", metric.label())).weak());
        return;
    }

    let axis_labels = labels.clone();
    let y_label = axis_label(lab, metric.label(), metric.is_throughput());
    plot_tools::maybe_pinned("stress_lab_compare_bars", interactive)
        .legend(Legend::default())
        .height(240.0)
        .y_axis_label(y_label)
        .x_axis_formatter(move |mark, _| {
            let index = mark.value.round();
            if (mark.value - index).abs() > 0.01 || index < 0.0 {
                return String::new();
            }
            axis_labels.get(index as usize).cloned().unwrap_or_default()
        })
        .show(ui, move |plot| {
            if !interactive {
                plot.set_auto_bounds(true);
            }
            plot.bar_chart(BarChart::new(metric.label(), bars).width(0.6));
        });
}

fn result_mix_chart(ui: &mut Ui, series: &[Series], interactive: bool) {
    ui.heading(format!("{} Result mix", icons::p::CHART_BAR));

    let buckets: [(&str, RunResult); 4] = [
        ("pass", RunResult::Pass),
        ("fail", RunResult::Fail),
        ("aborted", RunResult::Aborted),
        ("inconclusive", RunResult::Inconclusive),
    ];
    let offsets = [-0.3_f64, -0.1, 0.1, 0.3];
    let colors = [
        theme::result_pass(ui),
        theme::result_fail(ui),
        theme::result_aborted(ui),
        theme::result_inconclusive(ui),
    ];

    let labels: Vec<String> = series.iter().map(|s| short_name(&s.name)).collect();
    let mut charts: Vec<BarChart> = Vec::new();
    for (slot, (name, result)) in buckets.iter().enumerate() {
        let bars: Vec<Bar> = series
            .iter()
            .enumerate()
            .filter_map(|(index, s)| {
                let count = s.runs.iter().filter(|r| r.result == *result).count();
                (count > 0).then(|| {
                    Bar::new(index as f64 + offsets[slot], count as f64)
                        .name(format!("{} {name}", s.name))
                })
            })
            .collect();
        if !bars.is_empty() {
            charts.push(BarChart::new(*name, bars).width(0.18).color(colors[slot]));
        }
    }

    if charts.is_empty() {
        ui.label(RichText::new("No completed runs to summarise.").weak());
        return;
    }

    plot_tools::maybe_pinned("stress_lab_compare_results", interactive)
        .legend(Legend::default())
        .height(200.0)
        .y_axis_label("runs")
        .x_axis_formatter(move |mark, _| {
            let index = mark.value.round();
            if (mark.value - index).abs() > 0.01 || index < 0.0 {
                return String::new();
            }
            labels.get(index as usize).cloned().unwrap_or_default()
        })
        .show(ui, move |plot| {
            if !interactive {
                plot.set_auto_bounds(true);
            }
            for chart in charts {
                plot.bar_chart(chart);
            }
        });
}

fn metric_combo(ui: &mut Ui, id: &str, current: &mut RunMetric) {
    ComboBox::from_id_salt(id)
        .width(180.0)
        .selected_text(current.label())
        .show_ui(ui, |ui| {
            for metric in RunMetric::VALUES {
                ui.selectable_value(current, metric, metric.label());
            }
        });
}

/// Axis label for `metric_label`, naming the throughput unit when the metric is
/// one whose numbers only mean anything within a single unit.
fn axis_label(lab: &StressLab, metric_label: &str, is_throughput: bool) -> String {
    if !is_throughput {
        return metric_label.to_string();
    }
    match (&lab.compare.unit, throughput_units(lab).as_slice()) {
        (Some(unit), _) => format!("{metric_label} ({unit})"),
        (None, [only]) => format!("{metric_label} ({only})"),
        (None, []) => metric_label.to_string(),
        (None, _) => format!("{metric_label} (mixed units)"),
    }
}

/// Axis ticks have no room for a full part name.
fn short_name(name: &str) -> String {
    if name.chars().count() <= 18 {
        return name.to_string();
    }
    name.chars().take(17).collect::<String>() + "…"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mean_and_max_ignore_an_empty_set() {
        assert_eq!(Agg::Max.apply(&[]), None);
        assert_eq!(Agg::Mean.apply(&[]), None);
        assert_eq!(Agg::Max.apply(&[1.0, 9.0, 4.0]), Some(9.0));
        assert_eq!(Agg::Mean.apply(&[1.0, 2.0, 3.0]), Some(2.0));
    }

    #[test]
    fn the_bucket_keeps_the_shortest_run_chartable() {
        // Volume sizing already leaves the 60s run 10 points, so it stands.
        assert_eq!(compare_bucket(&[60.0, 30_000.0]), 6);
        // A uniform set is sized by volume alone.
        assert_eq!(compare_bucket(&[1800.0, 1800.0]), 1);
        // An 8s run beside a 60000s one cannot drag the fetch past the cap.
        assert_eq!(compare_bucket(&[8.0, 60_000.0]), 4);
        assert_eq!(compare_bucket(&[]), 30);
        assert_eq!(compare_bucket(&[0.0]), 30);
    }

    #[test]
    fn axis_names_are_trimmed_not_dropped() {
        assert_eq!(short_name("AMD Ryzen 5 5600"), "AMD Ryzen 5 5600");
        assert_eq!(
            short_name("AMD Ryzen 9 7950X3D 16-Core Processor"),
            "AMD Ryzen 9 7950X…"
        );
    }
}
