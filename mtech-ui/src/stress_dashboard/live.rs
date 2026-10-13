//! Live column: run status, stage progress, per-lane meters, host telemetry.

use eframe::egui::{self, RichText, Ui};
use stress_runner::{StressPanelConfig, info_for, mode_info};

use super::{StressLive, configure::fmt_dur, result_icon};
use crate::controls::{self, Metrics, SpinnerSize, Tokens};
use crate::icons::svg;
use crate::theme;

/// Height of the throughput sparkline.
const SPARK_H: f32 = 44.0;
/// Widest a lane hover card runs.
const LANE_HINT_W: f32 = 320.0;

pub(super) fn show(
    ui: &mut Ui,
    cfg: &StressPanelConfig,
    live: &StressLive,
    running: bool,
    chart: impl FnOnce(&mut Ui),
) {
    let m = Metrics::get(ui.ctx());
    status_header(ui, cfg, live, running);
    ui.add_space(m.gap.y);

    if let Some(stage) = &live.stage
        && stage.count > 0
    {
        let frac = (stage.index as f32 + 1.0) / stage.count as f32;
        controls::ProgressBar::new(frac)
            .text(&format!(
                "stage {}/{}  ·  {}",
                stage.index + 1,
                stage.count,
                stage.label
            ))
            .show(ui);
        ui.add_space(m.gap.y);
    }

    if !live.lanes.is_empty() {
        lane_meters(ui, live);
        ui.add_space(m.gap.y);
    }

    if live.history.len() > 1 {
        sparkline(ui, live);
        ui.add_space(m.gap.y);
    }

    if let Some(err) = &live.last_error {
        let col = theme::warn(ui);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            controls::status_label(ui, svg::WARNING, err, col);
        });
        ui.add_space(m.gap.y);
    }

    controls::subheading(ui, svg::CHART_LINE, "Live telemetry");
    chart(ui);
}

fn status_header(ui: &mut Ui, cfg: &StressPanelConfig, live: &StressLive, running: bool) {
    let info = mode_info(cfg.mode.clone());
    let m = Metrics::get(ui.ctx());
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.horizontal(|ui| {
            if running {
                controls::spinner(ui, SpinnerSize::Md);
                ui.vertical(|ui| {
                    ui.label(RichText::new(format!("Running · {}", info.label)).strong());
                    ui.label(
                        RichText::new(format!("elapsed {}", fmt_dur(live.elapsed_secs as u64)))
                            .weak(),
                    );
                });
            } else if let Some(v) = &live.verdict {
                let col = super::result_color(ui, v.result);
                result_icon(v.result).show_tinted(ui, m.icon_lg, col);
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(super::result_label(v.result, v.failure_kind.as_deref()))
                            .color(col)
                            .strong(),
                    );
                    ui.label(
                        RichText::new(format!("last run · {}", fmt_dur(v.duration_secs as u64)))
                            .weak(),
                    );
                });
            } else {
                let col = theme::weak_text(ui);
                svg::PAUSE_CIRCLE.show_tinted(ui, m.icon_lg, col);
                ui.vertical(|ui| {
                    ui.label(RichText::new("Idle").strong());
                    ui.label(RichText::new(info.when).weak());
                });
            }

            if running || live.throughput > 0.0 {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(format!("{:.1}", live.throughput))
                                .heading()
                                .strong(),
                        );
                        ui.label(RichText::new(live.throughput_unit).weak().small());
                    });
                });
            }
        });
    });
}

/// One row per lane: name, live throughput bar, error count.
fn lane_meters(ui: &mut Ui, live: &StressLive) {
    let m = Metrics::get(ui.ctx());
    controls::subheading(ui, svg::ROWS, "Lanes");
    let peak = live
        .lanes
        .iter()
        .map(|l| l.throughput)
        .fold(0.0_f64, f64::max)
        .max(f64::MIN_POSITIVE);

    egui::Grid::new("stress_lane_meters")
        .num_columns(3)
        .striped(true)
        .spacing([m.gap.x, m.pad.y])
        .show(ui, |ui| {
            for lane in &live.lanes {
                let resp = ui.label(&lane.label);
                if let Some(choice) = lane.stressor {
                    let i = info_for(choice);
                    resp.on_hover_ui(|ui| {
                        ui.set_max_width(LANE_HINT_W);
                        ui.label(RichText::new(choice.label()).strong());
                        ui.label(i.what);
                    });
                }

                let frac = (lane.throughput / peak).clamp(0.0, 1.0) as f32;
                controls::ProgressBar::new(frac)
                    .text(&format!("{:.1} {}", lane.throughput, lane.unit))
                    .show(ui);

                if lane.errors > 0 {
                    let col = theme::error(ui);
                    controls::status_label(ui, svg::X_CIRCLE, &lane.errors.to_string(), col)
                        .on_hover_text(
                            lane.last_error
                                .clone()
                                .unwrap_or_else(|| "data errors reported by this lane".into()),
                        );
                } else {
                    let col = theme::result_pass(ui);
                    svg::CHECK.show_tinted(ui, m.icon_sm, col);
                }
                ui.end_row();
            }
        });
}

/// Throughput over the recent window, normalised to its own peak.
fn sparkline(ui: &mut Ui, live: &StressLive) {
    let peak = live.history.iter().copied().fold(0.0_f32, f32::max);
    if peak <= 0.0 {
        return;
    }
    let t = Tokens::get(ui.ctx());
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, SPARK_H), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let stroke_col = theme::primary(ui);

    let n = live.history.len();
    let dx = if n > 1 {
        rect.width() / (n - 1) as f32
    } else {
        0.0
    };
    let points: Vec<egui::Pos2> = live
        .history
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let x = rect.left() + i as f32 * dx;
            let y = rect.bottom() - (v / peak).clamp(0.0, 1.0) * rect.height();
            egui::pos2(x, y)
        })
        .collect();

    painter.rect_filled(rect, t.radius_sm, theme::bg_faint(ui));
    if points.len() > 1 {
        painter.add(egui::Shape::line(
            points,
            egui::Stroke::new(1.5, stroke_col),
        ));
    }
    let col = theme::weak_text(ui);
    controls::IconLabel::new(
        svg::TREND_UP,
        &format!("peak {:.1} {}", peak, live.throughput_unit),
    )
    .tone(col)
    .small(true)
    .selectable(true)
    .show(ui);
}
