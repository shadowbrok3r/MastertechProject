//! Telemetry charts and the event log for one selected run.

use database::schema::StressTestEvent;
use eframe::egui::{Color32, RichText, Ui};
use egui_plot::{Legend, Line, PlotPoints};

use crate::ui_tools::{plots as plot_tools, theme};

use super::data::SeriesBucket;
use super::metrics::SeriesMetric;

/// Every series the run actually sampled, one chart each, on a shared elapsed-
/// time axis. Buckets are absolute so the first one anchors elapsed zero.
pub fn run_detail(ui: &mut Ui, bucket_secs: u32, buckets: &[SeriesBucket], interactive: bool) {
    if buckets.is_empty() {
        ui.label(RichText::new("No telemetry samples for this run.").weak());
        return;
    }

    let origin = buckets.iter().map(|b| b.bucket).min().unwrap_or(0);
    let palette = theme::chart_palette(ui);
    ui.label(
        RichText::new(format!("Elapsed seconds, {bucket_secs}s buckets"))
            .weak()
            .small(),
    );

    let mut charted = 0usize;
    for metric in SeriesMetric::VALUES {
        if !metric.present_in(buckets) {
            continue;
        }
        let points: Vec<[f64; 2]> = buckets
            .iter()
            .filter_map(|b| {
                metric
                    .value(b)
                    .map(|v| [((b.bucket - origin) * bucket_secs as i64) as f64, v])
            })
            .collect();
        if points.len() < 2 {
            continue;
        }
        let color = match metric {
            SeriesMetric::Throughput => palette.throughput,
            SeriesMetric::CpuTempC | SeriesMetric::GpuTempC => palette.temperature,
            SeriesMetric::CpuClockMhz | SeriesMetric::GpuClockMhz => palette.clock,
            SeriesMetric::PowerW | SeriesMetric::GpuPowerW => palette.disk,
            SeriesMetric::CpuUsagePct | SeriesMetric::GpuUsagePct => palette.avg_cpu,
            SeriesMetric::MemoryUsedPct => palette.memory,
            SeriesMetric::WheaDelta => palette.whea,
        };
        line_chart(ui, metric.label(), &points, color, interactive);
        charted += 1;
    }

    if charted == 0 {
        ui.label(
            RichText::new(format!(
                "Only {} telemetry bucket(s) at {bucket_secs}s — too short to chart.",
                buckets.len()
            ))
            .weak(),
        );
    }
}

fn line_chart(ui: &mut Ui, label: &str, points: &[[f64; 2]], color: Color32, interactive: bool) {
    ui.label(RichText::new(label).small());
    plot_tools::maybe_pinned(("stress_lab_detail", label), interactive)
        .legend(Legend::default())
        .height(130.0)
        .show(ui, |plot| {
            if !interactive {
                plot.set_auto_bounds(true);
            }
            plot.line(Line::new(label.to_owned(), PlotPoints::new(points.to_vec())).color(color));
        });
}

pub fn events(ui: &mut Ui, events: &[StressTestEvent]) {
    ui.heading("Events");
    if events.is_empty() {
        ui.label(RichText::new("No stress_test_event rows.").weak());
        return;
    }
    for e in events {
        let at = chrono::DateTime::<chrono::Utc>::from(e.at)
            .format("%H:%M:%S")
            .to_string();
        let color = match e.kind.as_str() {
            "bsod" | "whea_hit" | "disk_io_error" | "memory_error" | "tdr"
            | "unexpected_shutdown" => theme::result_fail(ui),
            "thermal_throttle" | "vrm_throttle" => theme::result_aborted(ui),
            _ => theme::weak_text(ui),
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(at).weak().small());
            ui.label(RichText::new(e.kind.as_str()).color(color).small());
            ui.label(RichText::new(format!("[{}] {}", e.source, e.detail)).small());
        });
    }
}
