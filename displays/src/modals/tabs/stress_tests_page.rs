//! Stress Tests tab — lists every `StressTestRun` recorded against the
//! computer this task references, with per-run summary stats so a tech can
//! find a stress test and its results without leaving the task.

use chrono::{DateTime, Utc};
use database::schema::{Datetime, RecordId, RecordIdExt, StressTestRun};
use eframe::egui::{Grid, RichText, ScrollArea, Ui, Vec2};

use super::diagnostics_page::status_section;
use crate::ui_tools::{
    controls::{self, BarKind, Icon, IconLabel, Metrics, SpinnerSize},
    icons::svg,
    theme,
};

/// Render the stress-tests page: a scrollable list of runs for the task's
/// computer, each expandable into verdict, timing, and rolled-up telemetry.
pub fn display_stress_tests_page(
    ui: &mut Ui,
    _avail_size: Vec2,
    runs: &[StressTestRun],
    loading: bool,
    error: Option<&str>,
    computer_linked: bool,
    selected: &mut Option<RecordId>,
) {
    ui.vertical(|ui| {
        let m = Metrics::get(ui.ctx());
        controls::bar(ui, BarKind::Header, |ui| {
            controls::icon_label(ui, svg::MT_STRESS, &format!("Stress Tests ({})", runs.len()));
            if loading {
                controls::spinner(ui, SpinnerSize::Sm);
            }
        });

        if let Some(err) = error {
            controls::status_label(ui, svg::WARNING_CIRCLE, err, theme::error(ui));
        }

        if runs.is_empty() && !loading {
            if computer_linked {
                controls::empty_state(
                    ui,
                    svg::MT_STRESS,
                    "No stress tests yet",
                    "No stress tests recorded for this computer yet.",
                    &[],
                );
            } else {
                // Computer still resolving from its async fetch; avoid a
                // false "none recorded" verdict before the query can run.
                controls::loading_state(ui, "Loading…");
            }
            return;
        }

        ui.add_space(m.pad.y);
        // Fill nearly all remaining vertical space with the run list.
        let list_h = ui.available_height() * 0.99;
        ScrollArea::vertical()
            .id_salt("stress_runs_scroll")
            .auto_shrink([false; 2])
            .max_height(list_h)
            .show(ui, |ui| {
                for (idx, run) in runs.iter().enumerate() {
                    let is_selected = selected
                        .as_ref()
                        .is_some_and(|s| s == &run.id);
                    render_run(ui, run, idx, is_selected, |id| {
                        *selected = Some(id);
                    });
                    ui.add_space(m.gap.y);
                }
            });
    });
}

/// Icon for a run or stage result.
fn result_icon(result: &str) -> Icon {
    match result {
        "pass" => svg::CHECK_CIRCLE,
        "fail" => svg::X_CIRCLE,
        "aborted" => svg::MINUS_CIRCLE,
        "inconclusive" => svg::QUESTION,
        _ => svg::CIRCLE_DASHED,
    }
}

fn render_run(
    ui: &mut Ui,
    run: &StressTestRun,
    idx: usize,
    selected: bool,
    mut on_select: impl FnMut(RecordId),
) {
    let m = Metrics::get(ui.ctx());
    let result_color = match run.result.as_str() {
        "pass" => theme::result_pass(ui),
        "fail" => theme::result_fail(ui),
        "aborted" => theme::result_aborted(ui),
        "inconclusive" => theme::result_inconclusive(ui),
        _ => theme::result_unknown(ui),
    };

    let header = format!(
        "{} • {} • {}",
        format_datetime(&run.started_at),
        run.tool_label,
        run.result.as_str(),
    );

    let salt = format!("stress_run_{idx}_{}", run.id.key_string());
    let (clicked, _) = status_section(
        ui,
        &salt,
        result_icon(run.result.as_str()),
        result_color,
        &header,
        idx == 0 || selected,
        |ui| {
            Grid::new(format!("stress_meta_grid_{idx}"))
                .num_columns(2)
                .striped(false)
                .show(ui, |ui| {
                    ui.label(RichText::new("Result").weak());
                    ui.colored_label(result_color, run.result.as_str());
                    ui.end_row();

                    ui.label(RichText::new("Target").weak());
                    let target = match run.target_component.as_ref() {
                        Some(c) => format!("{} — {}", run.target_kind.as_str(), c.key_string()),
                        None => run.target_kind.as_str().to_string(),
                    };
                    ui.label(target);
                    ui.end_row();

                    if let Some(preset) = run.preset_label.as_deref() {
                        if !preset.trim().is_empty() {
                            ui.label(RichText::new("Preset").weak());
                            ui.label(preset);
                            ui.end_row();
                        }
                    }

                    if run.failure_kind != "none" && !run.failure_kind.is_empty() {
                        ui.label(RichText::new("Failure").weak());
                        ui.colored_label(theme::result_fail(ui), &run.failure_kind);
                        ui.end_row();
                    }

                    if let Some(d) = run.duration_actual_secs {
                        ui.label(RichText::new("Duration").weak());
                        ui.label(format!("{d:.1}s"));
                        ui.end_row();
                    }

                    if let Some(tech) = run.tech.as_deref() {
                        if !tech.trim().is_empty() {
                            ui.label(RichText::new("Tech").weak());
                            ui.label(tech);
                            ui.end_row();
                        }
                    }

                    if let Some(host) = run.hostname.as_deref() {
                        if !host.trim().is_empty() {
                            ui.label(RichText::new("Host").weak());
                            ui.label(host);
                            ui.end_row();
                        }
                    }
                });

            render_summary(ui, run, idx);

            if !run.scenario_stages.is_empty() {
                ui.add_space(m.gap.y);
                controls::icon_label(
                    ui,
                    svg::STACK,
                    &format!("Stages ({})", run.scenario_stages.len()),
                );
                for stage in run.scenario_stages.iter() {
                    let stage_color = match stage.result.as_deref() {
                        Some("fail") => theme::result_fail(ui),
                        Some("pass") => theme::result_pass(ui),
                        Some("inconclusive") => theme::result_inconclusive(ui),
                        _ => theme::result_unknown(ui),
                    };
                    ui.horizontal_wrapped(|ui| {
                        let stage_text = format!("{}. {}", stage.index, stage.label);
                        IconLabel::new(result_icon(stage.result.as_deref().unwrap_or("")), &stage_text)
                            .tone(stage_color)
                            .show(ui);
                        if stage.had_error {
                            if let Some(err) = stage.last_error.as_deref() {
                                ui.colored_label(theme::result_fail(ui), err);
                            }
                        }
                        for gap in stage.unevaluated.iter() {
                            ui.colored_label(theme::result_inconclusive(ui), gap);
                        }
                    });
                }
            }

            if let Some(notes) = run.notes.as_deref() {
                if !notes.trim().is_empty() {
                    ui.add_space(m.pad.y);
                    controls::icon_label(ui, svg::NOTE, "Notes");
                    ui.label(notes);
                }
            }
        },
    );

    if clicked {
        on_select(run.id.clone());
    }
}

/// Rolled-up telemetry from `RunSummary`, rendered as a compact grid of only
/// the fields that are populated.
fn render_summary(ui: &mut Ui, run: &StressTestRun, idx: usize) {
    let s = &run.summary;
    ui.add_space(Metrics::get(ui.ctx()).pad.y);
    controls::icon_label(ui, svg::CHART_BAR, "Summary");
    Grid::new(format!("stress_summary_grid_{idx}"))
        .num_columns(2)
        .striped(false)
        .show(ui, |ui| {
            summary_temp(ui, "Max temp", s.max_temp_c);
            summary_temp(ui, "Avg temp", s.avg_temp_c);
            summary_temp(ui, "Max CPU temp", s.max_cpu_temp_c);
            summary_temp(ui, "Max GPU temp", s.max_gpu_temp_c);

            if let Some(v) = s.min_v12_v {
                ui.label(RichText::new("12V min").weak());
                ui.label(format!("{v:.2} V (uncalibrated)"));
                ui.end_row();
            }

            if let Some(clk) = s.max_clock_mhz {
                ui.label(RichText::new("Max clock").weak());
                ui.label(format!("{clk} MHz"));
                ui.end_row();
            }
            if let (Some(tp), Some(unit)) = (s.peak_throughput, s.throughput_unit.as_deref()) {
                ui.label(RichText::new("Peak throughput").weak());
                ui.label(format!("{tp:.1} {unit}"));
                ui.end_row();
            }
            if let Some(pw) = s.max_power_w {
                ui.label(RichText::new("Max power").weak());
                ui.label(format!("{pw} W"));
                ui.end_row();
            }
            if s.whea_delta_count > 0 {
                ui.label(RichText::new("WHEA errors").weak());
                ui.colored_label(theme::result_fail(ui), s.whea_delta_count.to_string());
                ui.end_row();
            }
            if s.tdr_count > 0 {
                ui.label(RichText::new("TDR events").weak());
                ui.colored_label(theme::result_fail(ui), s.tdr_count.to_string());
                ui.end_row();
            }
            if s.memory_errors > 0 {
                ui.label(RichText::new("Memory errors").weak());
                ui.colored_label(theme::result_fail(ui), s.memory_errors.to_string());
                ui.end_row();
            }
            if s.disk_io_errors > 0 {
                ui.label(RichText::new("Disk I/O errors").weak());
                ui.colored_label(theme::result_fail(ui), s.disk_io_errors.to_string());
                ui.end_row();
            }
            if s.test_errors > 0 {
                ui.label(RichText::new("Test errors").weak());
                ui.colored_label(theme::result_fail(ui), s.test_errors.to_string());
                ui.end_row();
            }
            if s.bsod_detected {
                ui.label(RichText::new("BSOD").weak());
                ui.colored_label(
                    theme::result_fail(ui),
                    s.bsod_code.as_deref().unwrap_or("detected"),
                );
                ui.end_row();
            }
            if s.thermal_throttle_detected {
                ui.label(RichText::new("Thermal throttle").weak());
                controls::status_label(ui, svg::THERMOMETER_HOT, "yes", theme::warn(ui));
                ui.end_row();
            }
        });
}

fn summary_temp(ui: &mut Ui, label: &str, value: Option<f32>) {
    if let Some(v) = value {
        ui.label(RichText::new(label).weak());
        ui.label(format!("{v:.0}°C"));
        ui.end_row();
    }
}

fn format_datetime(dt: &Datetime) -> String {
    DateTime::<Utc>::from(dt.clone())
        .format("%Y-%m-%d %H:%M UTC")
        .to_string()
}
