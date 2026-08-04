//! Live column: run status, stage progress, per-lane meters, host telemetry.

use eframe::egui::{self, RichText, Ui};
use stress_runner::{info_for, mode_info, StressPanelConfig};

use super::{configure::fmt_dur, StressLive};
use crate::{icons, theme};

pub(super) fn show(
    ui: &mut Ui,
    cfg: &StressPanelConfig,
    live: &StressLive,
    running: bool,
    chart: impl FnOnce(&mut Ui),
) {
    status_header(ui, cfg, live, running);
    ui.add_space(6.0);

    if let Some(stage) = &live.stage {
        if stage.count > 0 {
            let frac = (stage.index as f32 + 1.0) / stage.count as f32;
            ui.add(
                egui::ProgressBar::new(frac)
                    .text(format!(
                        "stage {}/{}  ·  {}",
                        stage.index + 1,
                        stage.count,
                        stage.label
                    ))
                    .desired_height(18.0),
            );
            ui.add_space(6.0);
        }
    }

    if !live.lanes.is_empty() {
        lane_meters(ui, live);
        ui.add_space(6.0);
    }

    if live.history.len() > 1 {
        sparkline(ui, live);
        ui.add_space(6.0);
    }

    if let Some(err) = &live.last_error {
        let col = theme::warn(ui);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.label(RichText::new(format!("{}  {err}", icons::STATUS_WARN)).color(col));
        });
        ui.add_space(6.0);
    }

    ui.label(RichText::new(format!("{}  Live telemetry", icons::CHART)).strong());
    chart(ui);
}

fn status_header(ui: &mut Ui, cfg: &StressPanelConfig, live: &StressLive, running: bool) {
    let info = mode_info(cfg.mode.clone());
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.horizontal(|ui| {
            if running {
                let col = theme::accent(ui);
                ui.colored_label(col, RichText::new(icons::STATUS_WAIT).size(18.0));
                ui.vertical(|ui| {
                    ui.label(RichText::new(format!("Running · {}", info.label)).strong());
                    ui.label(
                        RichText::new(format!("elapsed {}", fmt_dur(live.elapsed_secs as u64)))
                            .weak(),
                    );
                });
            } else if let Some(v) = &live.verdict {
                let col = super::result_color(ui, v.result);
                ui.colored_label(col, RichText::new(icons::STATUS_DOT).size(18.0));
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
                ui.colored_label(col, RichText::new(icons::STATUS_IDLE).size(18.0));
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
    ui.label(RichText::new("Lanes").strong());
    let peak = live
        .lanes
        .iter()
        .map(|l| l.throughput)
        .fold(0.0_f64, f64::max)
        .max(f64::MIN_POSITIVE);

    egui::Grid::new("stress_lane_meters")
        .num_columns(3)
        .striped(true)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            for lane in &live.lanes {
                let resp = ui.label(&lane.label);
                if let Some(choice) = lane.stressor {
                    let i = info_for(choice);
                    resp.on_hover_ui(|ui| {
                        ui.set_max_width(320.0);
                        ui.label(RichText::new(choice.label()).strong());
                        ui.label(i.what);
                    });
                }

                let frac = (lane.throughput / peak).clamp(0.0, 1.0) as f32;
                ui.add(
                    egui::ProgressBar::new(frac)
                        .text(format!("{:.1} {}", lane.throughput, lane.unit))
                        .desired_height(14.0),
                );

                if lane.errors > 0 {
                    let col = theme::error(ui);
                    ui.colored_label(col, format!("{} {}", icons::STATUS_ERR, lane.errors))
                        .on_hover_text(
                            lane.last_error
                                .clone()
                                .unwrap_or_else(|| "data errors reported by this lane".into()),
                        );
                } else {
                    let col = theme::result_pass(ui);
                    ui.colored_label(col, icons::STATUS_READY);
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
    let height = 44.0;
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let stroke_col = theme::accent(ui);

    let n = live.history.len();
    let dx = if n > 1 { rect.width() / (n - 1) as f32 } else { 0.0 };
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

    painter.rect_filled(rect, 2.0, theme::bg_faint(ui));
    if points.len() > 1 {
        painter.add(egui::Shape::line(
            points,
            egui::Stroke::new(1.5, stroke_col),
        ));
    }
    ui.label(
        RichText::new(format!("peak {:.1} {}", peak, live.throughput_unit))
            .weak()
            .small(),
    );
}
