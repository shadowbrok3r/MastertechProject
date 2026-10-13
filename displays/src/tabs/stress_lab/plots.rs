//! Telemetry charts and the event log for one selected run.

use database::schema::StressTestEvent;
use eframe::egui::{Color32, RichText, Ui};
use egui_plot::{Legend, Line, PlotPoints};

use crate::ui_tools::controls::{self, IconLabel, Metrics};
use crate::ui_tools::icons::svg::{self, Icon};
use crate::ui_tools::{plots as plot_tools, theme};

use super::data::SeriesBucket;
use super::metrics::SeriesMetric;

/// Every series the run actually sampled, one chart each, on a shared elapsed-
/// time axis. Buckets are absolute so the first one anchors elapsed zero.
pub fn run_detail(ui: &mut Ui, bucket_secs: u32, buckets: &[SeriesBucket], interactive: bool) {
    if buckets.is_empty() {
        controls::empty_state(ui, svg::CHART_LINE, "No telemetry samples for this run", "", &[]);
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
        let (color, icon) = match metric {
            SeriesMetric::Throughput => (palette.throughput, svg::GAUGE),
            SeriesMetric::CpuTempC | SeriesMetric::GpuTempC => (palette.temperature, svg::THERMOMETER),
            SeriesMetric::CpuClockMhz | SeriesMetric::GpuClockMhz => (palette.clock, svg::SPEEDOMETER),
            SeriesMetric::PowerW | SeriesMetric::GpuPowerW => (palette.disk, svg::LIGHTNING),
            SeriesMetric::CpuUsagePct => (palette.avg_cpu, svg::CPU),
            SeriesMetric::GpuUsagePct => (palette.avg_cpu, svg::GRAPHICS_CARD),
            SeriesMetric::MemoryUsedPct => (palette.memory, svg::MEMORY),
            SeriesMetric::WheaDelta => (palette.whea, svg::SHIELD_WARNING),
        };
        line_chart(ui, icon, metric.label(), &points, color, interactive);
        charted += 1;
    }

    if charted == 0 {
        note(
            ui,
            svg::HOURGLASS,
            &format!(
                "Only {} telemetry bucket(s) at {bucket_secs}s — too short to chart.",
                buckets.len()
            ),
        );
    }
}

fn line_chart(ui: &mut Ui, icon: Icon, label: &str, points: &[[f64; 2]], color: Color32, interactive: bool) {
    ui.horizontal(|ui| {
        let size = Metrics::get(ui.ctx()).icon_sm;
        ui.add(icon.image(size).tint(color));
        ui.label(RichText::new(label).small());
    });
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
    let m = Metrics::get(ui.ctx());
    controls::panel_header(ui, svg::LIST_BULLETS, "Events", |_| {});
    if events.is_empty() {
        note(ui, svg::CHECK_CIRCLE, "No stress_test_event rows.");
        return;
    }
    for e in events {
        let at = chrono::DateTime::<chrono::Utc>::from(e.at)
            .format("%H:%M:%S")
            .to_string();
        let (color, icon) = match e.kind.as_str() {
            "bsod" | "whea_hit" | "disk_io_error" | "memory_error" | "tdr"
            | "unexpected_shutdown" => (theme::result_fail(ui), svg::WARNING_OCTAGON),
            "thermal_throttle" | "vrm_throttle" => (theme::result_aborted(ui), svg::THERMOMETER_HOT),
            _ => (theme::weak_text(ui), svg::DOT),
        };
        ui.horizontal_wrapped(|ui| {
            ui.add(icon.image(m.icon_sm).tint(color));
            ui.label(RichText::new(at).weak().small());
            ui.label(RichText::new(e.kind.as_str()).color(color).small());
            ui.label(RichText::new(format!("[{}] {}", e.source, e.detail)).small());
        });
    }
}

/// Faint icon and wrapping weak text.
fn note(ui: &mut Ui, icon: Icon, text: &str) {
    IconLabel::new(icon, text)
        .tone(theme::weak_text(ui))
        .icon_tone(theme::faint_text(ui))
        .wrap(true)
        .show(ui);
}
