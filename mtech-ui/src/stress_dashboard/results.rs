//! Results column: verdict banner, per-stage rule violations, recent runs.

use eframe::egui::{self, RichText, Ui};

use super::{StressLive, configure::fmt_dur, result_icon};
use crate::controls::{self, Icon, Metrics, Tokens};
use crate::icons::svg;
use crate::theme;

/// Share of the result colour in the verdict card fill.
const VERDICT_WASH: f32 = 0.12;

pub(super) fn show(ui: &mut Ui, live: &StressLive) {
    let m = Metrics::get(ui.ctx());
    match &live.verdict {
        Some(_) => verdict_card(ui, live),
        None => {
            controls::subheading(ui, svg::CLIPBOARD_TEXT, "Results");
            controls::empty_state(
                ui,
                svg::MT_STRESS,
                "No run recorded yet",
                "Verdicts from this session's runs show up here.",
                &[],
            );
        }
    }

    if !live.stage_verdicts.is_empty() {
        ui.add_space(m.gap.x);
        stage_verdicts(ui, live);
    }

    if !live.recent_runs.is_empty() {
        ui.add_space(m.gap.x);
        recent(ui, live);
    }
}

/// A weak grid key led by an icon.
fn key(ui: &mut Ui, icon: Icon, text: &str) {
    let (t, m) = (Tokens::get(ui.ctx()), Metrics::get(ui.ctx()));
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = m.pad.y;
        icon.show_tinted(ui, m.icon_sm, t.icon);
        ui.label(RichText::new(text).weak());
    });
}

fn verdict_card(ui: &mut Ui, live: &StressLive) {
    let Some(v) = &live.verdict else { return };
    let m = Metrics::get(ui.ctx());
    let col = super::result_color(ui, v.result);

    egui::Frame::group(ui.style())
        .fill(col.linear_multiply(VERDICT_WASH))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                result_icon(v.result).show_tinted(ui, m.icon_lg, col);
                ui.label(
                    RichText::new(super::result_label(v.result, v.failure_kind.as_deref()))
                        .color(col)
                        .heading()
                        .strong(),
                );
            });

            ui.add_space(m.pad.y);
            egui::Grid::new("stress_verdict_grid")
                .num_columns(2)
                .spacing([m.gap.x, m.pad.y])
                .show(ui, |ui| {
                    key(ui, svg::TIMER, "Ran for");
                    ui.label(fmt_dur(v.duration_secs as u64));
                    ui.end_row();

                    if let Some(t) = v.max_temp_c {
                        key(ui, svg::THERMOMETER, "Max temp");
                        let tcol = theme::temp_level(ui, t);
                        ui.colored_label(tcol, format!("{t:.0} °C"));
                        ui.end_row();
                    }

                    key(ui, svg::WARNING_OCTAGON, "WHEA");
                    if v.whea_delta > 0 {
                        let c = theme::error(ui);
                        ui.colored_label(c, format!("+{}", v.whea_delta))
                            .on_hover_text(
                                "New machine-check errors during the run — treat as hardware",
                            );
                    } else {
                        ui.label("none");
                    }
                    ui.end_row();

                    key(ui, svg::GRAPHICS_CARD, "TDR");
                    if v.tdr_count > 0 {
                        let c = theme::error(ui);
                        ui.colored_label(c, v.tdr_count.to_string())
                            .on_hover_text("GPU reset during the run");
                    } else {
                        ui.label("none");
                    }
                    ui.end_row();
                });

            if let Some(short) = short_run_warning(v) {
                ui.add_space(m.pad.y);
                let wcol = theme::warn(ui);
                controls::IconLabel::new(svg::WARNING, &short)
                    .tone(wcol)
                    .small(true)
                    .selectable(true)
                    .show(ui);
            }

            if let Some(id) = &v.run_id {
                ui.add_space(m.pad.y / 2.0);
                let col = theme::weak_text(ui);
                controls::IconLabel::new(svg::HASH, id)
                    .tone(col)
                    .small(true)
                    .selectable(true)
                    .show(ui);
            }
        });
}

/// Flags a run that stopped well short of its plan, so a partial run is never
/// read as a clean sign-off.
fn short_run_warning(v: &super::VerdictView) -> Option<String> {
    let planned = v.planned_secs?;
    if planned == 0 {
        return None;
    }
    let pct = (v.duration_secs / planned as f64) * 100.0;
    if pct >= 95.0 {
        return None;
    }
    Some(format!(
        "Stopped at {pct:.0}% of the planned {} — not a complete run.",
        fmt_dur(planned)
    ))
}

fn stage_verdicts(ui: &mut Ui, live: &StressLive) {
    let m = Metrics::get(ui.ctx());
    let failed = live.stage_verdicts.iter().filter(|s| !s.pass).count();
    ui.horizontal(|ui| {
        controls::subheading(ui, svg::LIST_CHECKS, "Stages");
        if failed > 0 {
            let c = theme::error(ui);
            controls::badge(ui, &format!("{failed} failed"), c);
        }
    });

    for row in &live.stage_verdicts {
        let (icon, col) = if row.pass {
            (svg::CHECK_CIRCLE, theme::result_pass(ui))
        } else {
            (svg::X_CIRCLE, theme::result_fail(ui))
        };
        ui.horizontal(|ui| {
            icon.show_tinted(ui, m.icon_sm, col);
            ui.label(&row.label);
            if let Some(pt) = row.peak_throughput {
                ui.label(RichText::new(format!("peak {pt:.1}")).weak().small());
            }
        });
        for v in &row.violations {
            ui.indent(&row.label, |ui| {
                let c = theme::warn(ui);
                controls::IconLabel::new(svg::WARNING, v)
                    .tone(c)
                    .small(true)
                    .selectable(true)
                    .show(ui);
            });
        }
    }
}

fn recent(ui: &mut Ui, live: &StressLive) {
    let m = Metrics::get(ui.ctx());
    controls::subheading(ui, svg::CLOCK_COUNTER_CLOCKWISE, "Recent runs");
    egui::Grid::new("stress_recent_runs")
        .num_columns(3)
        .striped(true)
        .spacing([m.gap.x, m.pad.y])
        .show(ui, |ui| {
            for r in &live.recent_runs {
                let col = super::result_color(ui, r.result);
                result_icon(r.result).show_tinted(ui, m.icon_sm, col);
                ui.label(RichText::new(&r.label).small())
                    .on_hover_text(format!("{} · {}", r.when, fmt_dur(r.duration_secs as u64)));
                ui.label(RichText::new(&r.when).weak().small());
                ui.end_row();
            }
        });
}
