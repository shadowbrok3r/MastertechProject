//! Historical metric charts for a selected stress_test_run.

use database::schema::StressTestMetric;
use egui_plot::{Legend, Line, Plot, PlotPoints};
use eframe::egui::{ScrollArea, Ui};

use crate::ui_tools::theme;

pub fn render_metric_plots(ui: &mut Ui, metrics: &[StressTestMetric]) {
    if metrics.is_empty() {
        ui.label("No metric samples for this run.");
        return;
    }

    let t0 = chrono::DateTime::<chrono::Utc>::from(metrics[0].captured_at);
    let mut throughput: Vec<[f64; 2]> = Vec::new();
    let mut max_temp: Vec<[f64; 2]> = Vec::new();
    let mut memory_pct: Vec<[f64; 2]> = Vec::new();
    let mut whea: Vec<[f64; 2]> = Vec::new();

    for m in metrics {
        let t = chrono::DateTime::<chrono::Utc>::from(m.captured_at);
        let x = (t - t0).num_seconds() as f64;
        if let Some(tp) = m.throughput {
            throughput.push([x, tp]);
        }
        if let Some(pct) = m.memory_used_pct {
            memory_pct.push([x, pct as f64]);
        }
        if let Some(w) = m.whea_delta_count {
            whea.push([x, w as f64]);
        }
        let peak_core_temp = m
            .cores
            .iter()
            .filter_map(|c| c.temp_c)
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        if let Some(temp) = peak_core_temp {
            max_temp.push([x, temp as f64]);
        }
    }

    let palette = theme::chart_palette(ui);
    ui.label("Telemetry (elapsed seconds from run start)");
    plot_series(ui, "stress_throughput", "Throughput", &throughput, palette.throughput);
    plot_series(ui, "stress_max_temp", "Max core °C", &max_temp, palette.temperature);
    plot_series(ui, "stress_memory", "Memory %", &memory_pct, palette.memory);
    plot_series(ui, "stress_whea", "WHEA delta", &whea, palette.whea);
}

fn plot_series(ui: &mut Ui, id: &str, label: &str, points: &[[f64; 2]], color: eframe::egui::Color32) {
    if points.len() < 2 {
        return;
    }
    ui.label(label);
    let height = 140.0;
    Plot::new(id)
        .legend(Legend::default())
        .height(height)
        .width(ui.available_width() - 8.0)
        .allow_drag(true)
        .allow_zoom(true)
        .show(ui, |plot_ui| {
            plot_ui.line(
                Line::new(label, PlotPoints::new(points.to_vec())).color(color),
            );
        });
}

pub fn render_events(ui: &mut Ui, events: &[database::schema::StressTestEvent]) {
    ui.heading("Events");
    ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
        if events.is_empty() {
            ui.label("No stress_test_event rows.");
            return;
        }
        for e in events {
            let at = chrono::DateTime::<chrono::Utc>::from(e.at)
                .format("%H:%M:%S")
                .to_string();
            ui.label(format!(
                "{at}  {}  [{}] {}",
                e.kind.as_str(),
                e.source,
                e.detail
            ));
        }
    });
}
