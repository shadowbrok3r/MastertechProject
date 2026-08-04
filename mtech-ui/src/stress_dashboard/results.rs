//! Results column: verdict banner, per-stage rule violations, recent runs.

use eframe::egui::{self, RichText, Ui};

use super::{configure::fmt_dur, StressLive};
use crate::{icons, theme};

pub(super) fn show(ui: &mut Ui, live: &StressLive) {
    match &live.verdict {
        Some(_) => verdict_card(ui, live),
        None => {
            ui.label(RichText::new("Results").strong());
            ui.label(RichText::new("No run recorded in this session yet.").weak());
        }
    }

    if !live.stage_verdicts.is_empty() {
        ui.add_space(8.0);
        stage_verdicts(ui, live);
    }

    if !live.recent_runs.is_empty() {
        ui.add_space(8.0);
        recent(ui, live);
    }
}

fn verdict_card(ui: &mut Ui, live: &StressLive) {
    let Some(v) = &live.verdict else { return };
    let col = super::result_color(ui, v.result);

    egui::Frame::group(ui.style())
        .fill(col.linear_multiply(0.12))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(super::result_label(v.result, v.failure_kind.as_deref()))
                        .color(col)
                        .heading()
                        .strong(),
                );
            });

            ui.add_space(4.0);
            egui::Grid::new("stress_verdict_grid")
                .num_columns(2)
                .spacing([10.0, 3.0])
                .show(ui, |ui| {
                    ui.label(RichText::new("Ran for").weak());
                    ui.label(fmt_dur(v.duration_secs as u64));
                    ui.end_row();

                    if let Some(t) = v.max_temp_c {
                        ui.label(RichText::new("Max temp").weak());
                        let tcol = theme::temp_level(ui, t);
                        ui.colored_label(tcol, format!("{t:.0} °C"));
                        ui.end_row();
                    }

                    ui.label(RichText::new("WHEA").weak());
                    if v.whea_delta > 0 {
                        let c = theme::error(ui);
                        ui.colored_label(c, format!("+{}", v.whea_delta))
                            .on_hover_text("New machine-check errors during the run — treat as hardware");
                    } else {
                        ui.label("none");
                    }
                    ui.end_row();

                    ui.label(RichText::new("TDR").weak());
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
                ui.add_space(4.0);
                let wcol = theme::warn(ui);
                ui.label(
                    RichText::new(format!("{}  {short}", icons::STATUS_WARN))
                        .color(wcol)
                        .small(),
                );
            }

            if let Some(id) = &v.run_id {
                ui.add_space(2.0);
                ui.label(RichText::new(id).weak().small());
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
    let failed = live.stage_verdicts.iter().filter(|s| !s.pass).count();
    ui.horizontal(|ui| {
        ui.label(RichText::new("Stages").strong());
        if failed > 0 {
            let c = theme::error(ui);
            ui.colored_label(c, format!("{failed} failed"));
        }
    });

    for row in &live.stage_verdicts {
        let (glyph, col) = if row.pass {
            (icons::STATUS_ON, theme::result_pass(ui))
        } else {
            (icons::STATUS_ERR, theme::result_fail(ui))
        };
        ui.horizontal(|ui| {
            ui.colored_label(col, glyph);
            ui.label(&row.label);
            if let Some(pt) = row.peak_throughput {
                ui.label(RichText::new(format!("peak {pt:.1}")).weak().small());
            }
        });
        for v in &row.violations {
            ui.indent(&row.label, |ui| {
                let c = theme::warn(ui);
                ui.label(RichText::new(format!("- {v}")).color(c).small());
            });
        }
    }
}

fn recent(ui: &mut Ui, live: &StressLive) {
    ui.label(RichText::new("Recent runs").strong());
    egui::Grid::new("stress_recent_runs")
        .num_columns(3)
        .striped(true)
        .spacing([8.0, 3.0])
        .show(ui, |ui| {
            for r in &live.recent_runs {
                let col = super::result_color(ui, r.result);
                ui.colored_label(col, icons::STATUS_DOT);
                ui.label(RichText::new(&r.label).small())
                    .on_hover_text(format!("{} · {}", r.when, fmt_dur(r.duration_secs as u64)));
                ui.label(RichText::new(&r.when).weak().small());
                ui.end_row();
            }
        });
}
