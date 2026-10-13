//! Configure column: mode selector plus the active mode's knobs.
//!
//! The stressor picker is grouped by subsystem and shared between Single
//! (one choice) and Concurrent (many lanes), replacing the flat 30-button row.

use eframe::egui::{self, RichText, Ui};
use stress_runner::{
    CERT_PRESET_NAMES, PanelMode, ScenarioStageConfig, StressPanelConfig, StressorChoice,
    Subsystem, cert_preset_info, info_for, mode_info,
};

use super::{DashboardAction, StressDashboard, mode_icon, subsystem_icon};
use crate::controls::{self, IconBox, IconButton, MenuButton, Metrics, TextField};
use crate::icons::svg;
use crate::theme;

/// Width of a duration or size field.
const VALUE_W: f32 = 96.0;
/// Width of a thread-count field.
const THREADS_W: f32 = 64.0;
/// Width of a stage label field.
const STAGE_LABEL_W: f32 = 96.0;
/// Widest a hover card runs.
const HINT_W: f32 = 340.0;

/// Compact segmented mode control with a hover hint per mode.
pub(super) fn mode_selector(ui: &mut Ui, cfg: &mut StressPanelConfig, running: bool) {
    ui.add_enabled_ui(!running, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = Metrics::get(ui.ctx()).pad.y;
            for mode in [
                PanelMode::Single,
                PanelMode::Scenario,
                PanelMode::QcBenchmark,
                PanelMode::Certification,
                PanelMode::Concurrent,
            ] {
                let info = mode_info(mode.clone());
                let selected = cfg.mode == mode;
                if controls::pill(ui, mode_icon(&mode), info.label, selected)
                    .on_hover_ui(|ui| hint(ui, info.label, info.what, Some(info.when), None, None))
                    .clicked()
                {
                    cfg.mode = mode;
                }
            }
        });
    });
}

pub(super) fn show(
    dash: &mut StressDashboard,
    ui: &mut Ui,
    cfg: &mut StressPanelConfig,
    running: bool,
    _action: &mut DashboardAction,
) {
    let m = Metrics::get(ui.ctx());
    let info = mode_info(cfg.mode.clone());
    controls::subheading(ui, mode_icon(&cfg.mode), info.label);
    ui.label(RichText::new(info.what).weak());
    ui.add_space(m.gap.y);

    ui.add_enabled_ui(!running, |ui| match cfg.mode {
        PanelMode::Single => single(dash, ui, cfg),
        PanelMode::Scenario => scenario(ui, cfg),
        PanelMode::QcBenchmark => qc_benchmark(ui, cfg),
        PanelMode::Certification => certification(ui, cfg),
        PanelMode::Concurrent => concurrent(dash, ui, cfg),
    });
}

// ---------------------------------------------------------------------------
// Shared stressor picker
// ---------------------------------------------------------------------------

/// How the grouped picker behaves.
enum PickMode<'a> {
    /// Exactly one stressor selected.
    One(&'a mut StressorChoice),
    /// Any number of lanes selected.
    Many(&'a mut Vec<StressorChoice>),
}

/// Subsystem-grouped, collapsible stressor picker with per-test hover hints.
fn stressor_picker(dash: &mut StressDashboard, ui: &mut Ui, mut pick: PickMode<'_>) {
    let m = Metrics::get(ui.ctx());
    for group in Subsystem::ALL {
        let label = group.label();
        let selected_in_group = group
            .stressors()
            .iter()
            .filter(|c| match &pick {
                PickMode::One(cur) => **cur == **c,
                PickMode::Many(lanes) => lanes.contains(c),
            })
            .count();
        let open = dash.group_open(label, selected_in_group > 0);

        let header = controls::fold_header(ui, open, subsystem_icon(group), label)
            .on_hover_ui(|ui| hint(ui, label, group.blurb(), None, None, None));
        if header.clicked() {
            dash.toggle_group(label, open);
        }
        let row = header.rect.shrink2(egui::vec2(m.pad.y, 0.0));
        let mut trailing = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(row)
                .layout(egui::Layout::right_to_left(egui::Align::Center)),
        );
        if let PickMode::Many(lanes) = &mut pick {
            let all_on = group.stressors().iter().all(|c| lanes.contains(c));
            let (icon, verb) = if all_on {
                (svg::SQUARE, "none")
            } else {
                (svg::CHECK_SQUARE, "all")
            };
            let tip = format!("Select {verb} in {label}");
            if IconButton::new(icon, IconBox::Sm)
                .tip(&tip)
                .show(&mut trailing)
                .clicked()
            {
                for c in group.stressors() {
                    lanes.retain(|l| l != c);
                    if !all_on {
                        lanes.push(*c);
                    }
                }
            }
        }
        if selected_in_group > 0 {
            let tone = theme::primary(&trailing);
            controls::badge(&mut trailing, &selected_in_group.to_string(), tone);
        }

        if !open {
            continue;
        }

        ui.indent(label, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for choice in group.stressors() {
                let info = info_for(*choice);
                let on = match &pick {
                    PickMode::One(cur) => **cur == *choice,
                    PickMode::Many(lanes) => lanes.contains(choice),
                };
                let icon = info.caveat.map(|_| svg::INFO);
                let resp =
                    controls::menu_check_item(ui, icon, choice.label(), on).on_hover_ui(|ui| {
                        hint(
                            ui,
                            choice.label(),
                            info.what,
                            Some(info.when),
                            info.pass,
                            info.caveat,
                        )
                    });
                if resp.clicked() {
                    match &mut pick {
                        PickMode::One(cur) => **cur = *choice,
                        PickMode::Many(lanes) => {
                            if on {
                                lanes.retain(|l| l != choice);
                            } else {
                                lanes.push(*choice);
                            }
                        }
                    }
                }
            }
        });
        ui.add_space(m.pad.y);
    }
}

/// Rich hover card: what it does, when to reach for it, pass rule, caveat.
fn hint(
    ui: &mut Ui,
    title: &str,
    what: &str,
    when: Option<&str>,
    pass: Option<&str>,
    caveat: Option<&str>,
) {
    let m = Metrics::get(ui.ctx());
    ui.set_max_width(HINT_W);
    ui.label(RichText::new(title).strong());
    ui.label(what);
    if let Some(when) = when {
        ui.add_space(m.pad.y);
        let col = theme::primary(ui);
        controls::status_label(ui, svg::LIGHTBULB, &format!("Use when: {when}"), col);
    }
    if let Some(pass) = pass {
        ui.add_space(m.pad.y);
        let col = theme::weak_text(ui);
        controls::status_label(ui, svg::SEAL_CHECK, &format!("Passes if: {pass}"), col);
    }
    if let Some(caveat) = caveat {
        ui.add_space(m.pad.y);
        let col = theme::warn(ui);
        controls::status_label(ui, svg::WARNING, caveat, col);
    }
}

// ---------------------------------------------------------------------------
// Per-mode config
// ---------------------------------------------------------------------------

/// "Stop after" row: an on/off switch and the seconds it stops at.
fn stop_after(ui: &mut Ui, enabled: &mut bool, secs: &mut u64) {
    ui.horizontal(|ui| {
        controls::toggle(ui, enabled, "");
        ui.add_enabled_ui(*enabled, |ui| {
            controls::value_field(ui, secs, 1..=86_400, "s", VALUE_W);
        });
    });
}

fn single(dash: &mut StressDashboard, ui: &mut Ui, cfg: &mut StressPanelConfig) {
    let m = Metrics::get(ui.ctx());
    let chosen = cfg.single.stressor;
    let info = info_for(chosen);

    ui.label(RichText::new(chosen.label()).strong());
    ui.label(RichText::new(info.what).weak().small());
    if let Some(caveat) = info.caveat {
        let col = theme::warn(ui);
        controls::IconLabel::new(svg::WARNING, caveat)
            .tone(col)
            .small(true)
            .selectable(true)
            .show(ui);
    }
    ui.add_space(m.gap.y);

    egui::Grid::new("stress_single_grid")
        .num_columns(2)
        .spacing([m.gap.x, m.gap.y])
        .show(ui, |ui| {
            let c = &mut cfg.single;

            ui.label("Threads")
                .on_hover_text("0 uses every logical core");
            controls::ValueField::new(&mut c.threads, 0..=1024)
                .formatter(|n| {
                    if n <= 0.0 {
                        "auto".to_owned()
                    } else {
                        format!("{n:.0}")
                    }
                })
                .parser(|text| {
                    let text = text.trim();
                    if text.eq_ignore_ascii_case("auto") {
                        Some(0.0)
                    } else {
                        text.parse().ok()
                    }
                })
                .show(ui);
            ui.end_row();

            ui.label("Stop after")
                .on_hover_text("Leave off to run until stopped by hand");
            stop_after(ui, &mut c.use_timeout, &mut c.timeout_secs);
            ui.end_row();

            if uses_memory_cap(chosen) {
                ui.label("Memory cap")
                    .on_hover_text("Per-worker heap ceiling");
                controls::value_field(ui, &mut c.memory_cap_mb, 16..=1_048_576, "MB", VALUE_W);
                ui.end_row();
            }

            if chosen == StressorChoice::Disk {
                ui.label("Disk file")
                    .on_hover_text("Temp file size written per worker");
                controls::value_field(ui, &mut c.disk_file_mb, 1..=1_048_576, "MB", VALUE_W);
                ui.end_row();
            }
        });

    ui.add_space(m.gap.x);
    controls::subheading(ui, svg::FLASK, "Test");
    stressor_picker(dash, ui, PickMode::One(&mut cfg.single.stressor));
}

fn concurrent(dash: &mut StressDashboard, ui: &mut Ui, cfg: &mut StressPanelConfig) {
    let m = Metrics::get(ui.ctx());
    ui.label(
        RichText::new("Every selected test runs at the same time, in its own lane.")
            .weak()
            .small(),
    );
    ui.add_space(m.pad.y);

    let lanes = cfg.concurrent.lanes.clone();
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(format!("{} lane(s):", lanes.len())).strong());
        if lanes.is_empty() {
            let col = theme::warn(ui);
            controls::status_label(ui, svg::WARNING, "pick at least one test", col);
        } else {
            for c in &lanes {
                ui.label(RichText::new(c.label()).small());
            }
        }
    });

    if lanes.len() > 1 && !lanes.iter().any(|c| c.is_gpu()) {
        ui.add_space(m.pad.y / 2.0);
        let col = theme::weak_text(ui);
        controls::IconLabel::new(
            svg::INFO,
            "No GPU lane selected — this only loads CPU, RAM, and disk.",
        )
        .tone(col)
        .small(true)
        .selectable(true)
        .show(ui);
    }

    ui.add_space(m.gap.y);
    egui::Grid::new("stress_concurrent_grid")
        .num_columns(2)
        .spacing([m.gap.x, m.gap.y])
        .show(ui, |ui| {
            let c = &mut cfg.concurrent;
            ui.label("Stop after")
                .on_hover_text("Shared by every lane — per-lane durations are ignored");
            stop_after(ui, &mut c.use_timeout, &mut c.duration_secs);
            ui.end_row();

            ui.label("Memory cap / lane");
            controls::value_field(ui, &mut c.memory_cap_mb, 16..=1_048_576, "MB", VALUE_W);
            ui.end_row();

            ui.label("Disk file / lane");
            controls::value_field(ui, &mut c.disk_file_mb, 1..=1_048_576, "MB", VALUE_W);
            ui.end_row();
        });

    ui.add_space(m.gap.x);
    controls::subheading(ui, svg::ROWS, "Lanes");
    stressor_picker(dash, ui, PickMode::Many(&mut cfg.concurrent.lanes));
}

fn scenario(ui: &mut Ui, cfg: &mut StressPanelConfig) {
    let m = Metrics::get(ui.ctx());
    ui.horizontal(|ui| {
        ui.label(RichText::new("Stages run in order").weak());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if controls::icon_text_button(ui, svg::PLUS, "Add")
                .on_hover_text("Append a stage")
                .clicked()
            {
                cfg.scenario.stages.push(ScenarioStageConfig::default_cpu());
            }
        });
    });
    ui.add_space(m.pad.y);

    let mut remove = None;
    let mut move_up = None;
    let count = cfg.scenario.stages.len();
    for i in 0..count {
        ui.push_id(i, |ui| {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                let s = &mut cfg.scenario.stages[i];
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{}.", i + 1)).strong());
                    TextField::new(&mut s.label)
                        .hint("label")
                        .width(STAGE_LABEL_W)
                        .show(ui);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if count > 1
                            && controls::icon_button(
                                ui,
                                svg::TRASH,
                                IconBox::Sm,
                                false,
                                "Remove stage",
                            )
                            .clicked()
                        {
                            remove = Some(i);
                        }
                        if i > 0
                            && controls::icon_button(
                                ui,
                                svg::ARROW_UP,
                                IconBox::Sm,
                                false,
                                "Move earlier",
                            )
                            .clicked()
                        {
                            move_up = Some(i);
                        }
                    });
                });

                let info = info_for(s.stressor);
                let current = Subsystem::ALL
                    .into_iter()
                    .find(|g| g.stressors().contains(&s.stressor))
                    .map_or(svg::FLASK, subsystem_icon);
                MenuButton::new(s.stressor.label())
                    .icon(current)
                    .tip("Stressor for this stage")
                    .show(ui, |ui| {
                        for group in Subsystem::ALL {
                            controls::section_label(ui, group.label());
                            for choice in group.stressors() {
                                let i = info_for(*choice);
                                let picked = s.stressor == *choice;
                                if controls::menu_check_item(
                                    ui,
                                    subsystem_icon(group),
                                    choice.label(),
                                    picked,
                                )
                                .on_hover_ui(|ui| {
                                    hint(ui, choice.label(), i.what, Some(i.when), i.pass, i.caveat)
                                })
                                .clicked()
                                {
                                    s.stressor = *choice;
                                }
                            }
                        }
                    });
                ui.label(RichText::new(info.what).weak().small());

                ui.horizontal(|ui| {
                    ui.label("for");
                    controls::value_field(ui, &mut s.duration_secs, 1..=86_400, "s", VALUE_W);
                    ui.label("threads");
                    controls::value_field(ui, &mut s.threads, 0..=1024, "", THREADS_W);
                });
            });
        });
    }
    if let Some(i) = remove {
        cfg.scenario.stages.remove(i);
    }
    if let Some(i) = move_up {
        cfg.scenario.stages.swap(i - 1, i);
    }

    ui.add_space(m.gap.y);
    let sc = &mut cfg.scenario;
    controls::toggle(ui, &mut sc.use_total, "Total wall-clock cap")
        .on_hover_text("Stop the whole scenario at this point regardless of stage progress");
    ui.add_enabled_ui(sc.use_total, |ui| {
        ui.horizontal(|ui| {
            controls::value_field(ui, &mut sc.total_wall_secs, 1..=604_800, "s", VALUE_W);
            controls::toggle(ui, &mut sc.repeat_until_total, "Repeat")
                .on_hover_text("Loop the stage list until the cap is reached");
        });
    });

    let planned: u64 = sc.stages.iter().map(|s| s.duration_secs).sum();
    ui.add_space(m.pad.y);
    let col = theme::weak_text(ui);
    controls::status_label(
        ui,
        svg::TIMER,
        &format!("{} stages, {}", sc.stages.len(), fmt_dur(planned)),
        col,
    );
}

fn qc_benchmark(ui: &mut Ui, cfg: &mut StressPanelConfig) {
    let m = Metrics::get(ui.ctx());
    let mult = &mut cfg.qc_benchmark.duration_multiplier;
    ui.horizontal(|ui| {
        ui.label("Duration");
        controls::Slider::new(mult, 0.1..=10.0).value("x").show(ui);
    });
    let per_stage = (20.0 * *mult).round() as u64;
    let col = theme::weak_text(ui);
    controls::status_label(
        ui,
        svg::TIMER,
        &format!("{}s per stage, {} total", per_stage, fmt_dur(per_stage * 8)),
        col,
    );
    ui.add_space(m.gap.y);
    controls::subheading(ui, svg::LIST_NUMBERS, "Stages");
    for (n, s) in [
        "cpu", "matrix", "fp", "stream", "cache", "branch", "memory", "vm",
    ]
    .iter()
    .enumerate()
    {
        ui.label(RichText::new(format!("{}. {s}", n + 1)).small().weak());
    }
}

fn certification(ui: &mut Ui, cfg: &mut StressPanelConfig) {
    let m = Metrics::get(ui.ctx());
    controls::subheading(ui, svg::MEDAL, "Tier");
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        for name in CERT_PRESET_NAMES {
            let selected = cfg.certification.preset_name == *name;
            let info = cert_preset_info(name);
            let label = info.map(|i| i.label).unwrap_or(name);
            let resp = controls::menu_check_item(ui, svg::CERTIFICATE, label, selected)
                .on_hover_ui(|ui| match info {
                    Some(i) => hint(ui, i.label, i.what, Some(i.when), None, None),
                    None => {
                        ui.label(*name);
                    }
                });
            if resp.clicked() {
                cfg.certification.preset_name = (*name).to_string();
            }
            if let Some(i) = info
                && selected
            {
                ui.indent(name, |ui| {
                    ui.label(RichText::new(i.what).weak().small());
                });
            }
        }
    });

    ui.add_space(m.gap.x);
    let mult = &mut cfg.certification.duration_multiplier;
    ui.horizontal(|ui| {
        ui.label("Duration");
        controls::Slider::new(mult, 0.001..=1.0).value("x").show(ui);
    });
    if *mult < 1.0 {
        let col = theme::warn(ui);
        controls::IconLabel::new(
            svg::WARNING,
            &format!(
                "Shortened to {:.1}% — a smoke test, not a valid certification.",
                *mult * 100.0
            ),
        )
        .tone(col)
        .small(true)
        .selectable(true)
        .show(ui);
    }
    ui.add_space(m.pad.y);
    let col = theme::weak_text(ui);
    controls::IconLabel::new(
        svg::SHIELD_CHECK,
        "Each stage carries verdict rules for temperature, WHEA, TDR, and throughput stability.",
    )
    .tone(col)
    .small(true)
    .selectable(true)
    .show(ui);
}

/// Stressors whose plan actually reads `memory_cap_mb`.
fn uses_memory_cap(choice: StressorChoice) -> bool {
    use StressorChoice as S;
    matches!(
        choice,
        S::Memory
            | S::Vm
            | S::MemTest
            | S::Linpack
            | S::CpuMem
            | S::Combined
            | S::GpuVram
            | S::GpuPcie
    )
}

/// `90s` / `12m` / `3h 30m`.
pub(super) fn fmt_dur(secs: u64) -> String {
    if secs < 120 {
        return format!("{secs}s");
    }
    let mins = secs / 60;
    if mins < 60 {
        return format!("{mins}m");
    }
    let (h, m) = (mins / 60, mins % 60);
    if m == 0 {
        format!("{h}h")
    } else {
        format!("{h}h {m}m")
    }
}
