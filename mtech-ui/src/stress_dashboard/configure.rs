//! Configure column: mode selector plus the active mode's knobs.
//!
//! The stressor picker is grouped by subsystem and shared between Single
//! (one choice) and Concurrent (many lanes), replacing the flat 30-button row.

use eframe::egui::{self, RichText, Ui};
use stress_runner::{
    info_for, mode_info, cert_preset_info, PanelMode, ScenarioStageConfig, StressPanelConfig,
    StressorChoice, Subsystem, CERT_PRESET_NAMES,
};

use super::{DashboardAction, StressDashboard};
use crate::{icons, theme};

/// Compact segmented mode control with a hover hint per mode.
pub(super) fn mode_selector(ui: &mut Ui, cfg: &mut StressPanelConfig, running: bool) {
    ui.add_enabled_ui(!running, |ui| {
        ui.horizontal(|ui| {
            for mode in [
                PanelMode::Single,
                PanelMode::Scenario,
                PanelMode::QcBenchmark,
                PanelMode::Certification,
                PanelMode::Concurrent,
            ] {
                let info = mode_info(mode.clone());
                let selected = cfg.mode == mode;
                if ui
                    .selectable_label(selected, info.label)
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
    let info = mode_info(cfg.mode.clone());
    ui.label(RichText::new(info.label).strong());
    ui.label(RichText::new(info.what).weak());
    ui.add_space(6.0);

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

        ui.horizontal(|ui| {
            let chev = if open {
                icons::CHEV_OPEN
            } else {
                icons::CHEV_CLOSED
            };
            if ui
                .selectable_label(false, format!("{chev}  {label}"))
                .on_hover_ui(|ui| hint(ui, label, group.blurb(), None, None, None))
                .clicked()
            {
                dash.toggle_group(label, open);
            }
            if selected_in_group > 0 {
                let col = theme::accent(ui);
                ui.colored_label(col, format!("{selected_in_group}"));
            }
            if let PickMode::Many(lanes) = &mut pick {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let all_on = group.stressors().iter().all(|c| lanes.contains(c));
                    let verb = if all_on { "none" } else { "all" };
                    if ui
                        .small_button(verb)
                        .on_hover_text(format!("Select {verb} in {label}"))
                        .clicked()
                    {
                        for c in group.stressors() {
                            lanes.retain(|l| l != c);
                            if !all_on {
                                lanes.push(*c);
                            }
                        }
                    }
                });
            }
        });

        if !open {
            continue;
        }

        ui.indent(label, |ui| {
            for choice in group.stressors() {
                let info = info_for(*choice);
                let on = match &pick {
                    PickMode::One(cur) => **cur == *choice,
                    PickMode::Many(lanes) => lanes.contains(choice),
                };
                let text = match info.caveat {
                    Some(_) => format!("{}  {}", choice.label(), icons::INFO),
                    None => choice.label().to_string(),
                };
                let resp = ui.selectable_label(on, text).on_hover_ui(|ui| {
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
        ui.add_space(2.0);
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
    ui.set_max_width(340.0);
    ui.label(RichText::new(title).strong());
    ui.label(what);
    if let Some(when) = when {
        ui.add_space(3.0);
        let col = theme::accent(ui);
        ui.label(RichText::new(format!("Use when: {when}")).color(col));
    }
    if let Some(pass) = pass {
        ui.add_space(3.0);
        ui.label(RichText::new(format!("Passes if: {pass}")).weak());
    }
    if let Some(caveat) = caveat {
        ui.add_space(3.0);
        let col = theme::warn(ui);
        ui.label(RichText::new(format!("{}  {caveat}", icons::STATUS_WARN)).color(col));
    }
}

// ---------------------------------------------------------------------------
// Per-mode config
// ---------------------------------------------------------------------------

fn single(dash: &mut StressDashboard, ui: &mut Ui, cfg: &mut StressPanelConfig) {
    let chosen = cfg.single.stressor;
    let info = info_for(chosen);

    ui.label(RichText::new(chosen.label()).strong());
    ui.label(RichText::new(info.what).weak().small());
    if let Some(caveat) = info.caveat {
        let col = theme::warn(ui);
        ui.label(RichText::new(format!("{}  {caveat}", icons::STATUS_WARN)).color(col).small());
    }
    ui.add_space(6.0);

    egui::Grid::new("stress_single_grid")
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            let c = &mut cfg.single;

            ui.label("Threads").on_hover_text("0 uses every logical core");
            ui.add(
                egui::DragValue::new(&mut c.threads)
                    .range(0..=1024)
                    .custom_formatter(|n, _| {
                        if n <= 0.0 {
                            "auto".to_owned()
                        } else {
                            format!("{n:.0}")
                        }
                    }),
            );
            ui.end_row();

            ui.label("Stop after")
                .on_hover_text("Leave off to run until stopped by hand");
            ui.horizontal(|ui| {
                ui.checkbox(&mut c.use_timeout, "");
                ui.add_enabled(
                    c.use_timeout,
                    egui::DragValue::new(&mut c.timeout_secs)
                        .suffix(" s")
                        .range(1..=86_400),
                );
            });
            ui.end_row();

            if uses_memory_cap(chosen) {
                ui.label("Memory cap")
                    .on_hover_text("Per-worker heap ceiling");
                ui.add(
                    egui::DragValue::new(&mut c.memory_cap_mb)
                        .suffix(" MB")
                        .range(16..=1_048_576),
                );
                ui.end_row();
            }

            if chosen == StressorChoice::Disk {
                ui.label("Disk file").on_hover_text("Temp file size written per worker");
                ui.add(
                    egui::DragValue::new(&mut c.disk_file_mb)
                        .suffix(" MB")
                        .range(1..=1_048_576),
                );
                ui.end_row();
            }
        });

    ui.add_space(8.0);
    ui.label(RichText::new("Test").strong());
    stressor_picker(dash, ui, PickMode::One(&mut cfg.single.stressor));
}

fn concurrent(dash: &mut StressDashboard, ui: &mut Ui, cfg: &mut StressPanelConfig) {
    ui.label(
        RichText::new("Every selected test runs at the same time, in its own lane.")
            .weak()
            .small(),
    );
    ui.add_space(4.0);

    let lanes = cfg.concurrent.lanes.clone();
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(format!("{} lane(s):", lanes.len())).strong());
        if lanes.is_empty() {
            let col = theme::warn(ui);
            ui.colored_label(col, "pick at least one test");
        } else {
            for c in &lanes {
                ui.label(RichText::new(c.label()).small());
            }
        }
    });

    if lanes.len() > 1 && !lanes.iter().any(|c| c.is_gpu()) {
        ui.add_space(2.0);
        ui.label(
            RichText::new("No GPU lane selected — this only loads CPU, RAM, and disk.")
                .weak()
                .small(),
        );
    }

    ui.add_space(6.0);
    egui::Grid::new("stress_concurrent_grid")
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            let c = &mut cfg.concurrent;
            ui.label("Stop after")
                .on_hover_text("Shared by every lane — per-lane durations are ignored");
            ui.horizontal(|ui| {
                ui.checkbox(&mut c.use_timeout, "");
                ui.add_enabled(
                    c.use_timeout,
                    egui::DragValue::new(&mut c.duration_secs)
                        .suffix(" s")
                        .range(1..=86_400),
                );
            });
            ui.end_row();

            ui.label("Memory cap / lane");
            ui.add(
                egui::DragValue::new(&mut c.memory_cap_mb)
                    .suffix(" MB")
                    .range(16..=1_048_576),
            );
            ui.end_row();

            ui.label("Disk file / lane");
            ui.add(
                egui::DragValue::new(&mut c.disk_file_mb)
                    .suffix(" MB")
                    .range(1..=1_048_576),
            );
            ui.end_row();
        });

    ui.add_space(8.0);
    ui.label(RichText::new("Lanes").strong());
    stressor_picker(dash, ui, PickMode::Many(&mut cfg.concurrent.lanes));
}

fn scenario(ui: &mut Ui, cfg: &mut StressPanelConfig) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Stages run in order").weak());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .button(format!("{}  Add", icons::PLUS))
                .on_hover_text("Append a stage")
                .clicked()
            {
                cfg.scenario.stages.push(ScenarioStageConfig::default_cpu());
            }
        });
    });
    ui.add_space(4.0);

    let mut remove = None;
    let mut move_up = None;
    let count = cfg.scenario.stages.len();
    for i in 0..count {
        ui.push_id(i, |ui| {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                let s = &mut cfg.scenario.stages[i];
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{}.", i + 1)).strong());
                    ui.add(
                        egui::TextEdit::singleline(&mut s.label)
                            .desired_width(96.0)
                            .hint_text("label"),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if count > 1
                            && ui
                                .small_button(icons::TRASH)
                                .on_hover_text("Remove stage")
                                .clicked()
                        {
                            remove = Some(i);
                        }
                        if i > 0
                            && ui
                                .small_button(icons::UP)
                                .on_hover_text("Move earlier")
                                .clicked()
                        {
                            move_up = Some(i);
                        }
                    });
                });

                let info = info_for(s.stressor);
                egui::ComboBox::from_id_salt("stage_stressor")
                    .selected_text(s.stressor.label())
                    .show_ui(ui, |ui| {
                        for group in Subsystem::ALL {
                            ui.label(RichText::new(group.label()).weak().small());
                            for choice in group.stressors() {
                                let i = info_for(*choice);
                                ui.selectable_value(&mut s.stressor, *choice, choice.label())
                                    .on_hover_ui(|ui| {
                                        hint(ui, choice.label(), i.what, Some(i.when), i.pass, i.caveat)
                                    });
                            }
                        }
                    });
                ui.label(RichText::new(info.what).weak().small());

                ui.horizontal(|ui| {
                    ui.label("for");
                    ui.add(
                        egui::DragValue::new(&mut s.duration_secs)
                            .suffix(" s")
                            .range(1..=86_400),
                    );
                    ui.label("threads");
                    ui.add(egui::DragValue::new(&mut s.threads).range(0..=1024));
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

    ui.add_space(6.0);
    let sc = &mut cfg.scenario;
    ui.checkbox(&mut sc.use_total, "Total wall-clock cap")
        .on_hover_text("Stop the whole scenario at this point regardless of stage progress");
    ui.add_enabled_ui(sc.use_total, |ui| {
        ui.horizontal(|ui| {
            ui.add(
                egui::DragValue::new(&mut sc.total_wall_secs)
                    .suffix(" s")
                    .range(1..=604_800),
            );
            ui.checkbox(&mut sc.repeat_until_total, "Repeat")
                .on_hover_text("Loop the stage list until the cap is reached");
        });
    });

    let planned: u64 = sc.stages.iter().map(|s| s.duration_secs).sum();
    ui.add_space(4.0);
    ui.label(RichText::new(format!("{} stages, {}", sc.stages.len(), fmt_dur(planned))).weak());
}

fn qc_benchmark(ui: &mut Ui, cfg: &mut StressPanelConfig) {
    let mult = &mut cfg.qc_benchmark.duration_multiplier;
    ui.horizontal(|ui| {
        ui.label("Duration");
        ui.add(egui::Slider::new(mult, 0.1..=10.0).suffix("x"));
    });
    let per_stage = (20.0 * *mult).round() as u64;
    ui.label(
        RichText::new(format!(
            "{}s per stage, {} total",
            per_stage,
            fmt_dur(per_stage * 8)
        ))
        .weak(),
    );
    ui.add_space(6.0);
    ui.label(RichText::new("Stages").strong());
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
    ui.label(RichText::new("Tier").strong());
    for name in CERT_PRESET_NAMES {
        let selected = cfg.certification.preset_name == *name;
        let info = cert_preset_info(name);
        let label = info.map(|i| i.label).unwrap_or(name);
        let resp = ui.selectable_label(selected, label).on_hover_ui(|ui| {
            match info {
                Some(i) => hint(ui, i.label, i.what, Some(i.when), None, None),
                None => {
                    ui.label(*name);
                }
            }
        });
        if resp.clicked() {
            cfg.certification.preset_name = (*name).to_string();
        }
        if let Some(i) = info {
            if selected {
                ui.indent(name, |ui| {
                    ui.label(RichText::new(i.what).weak().small());
                });
            }
        }
    }

    ui.add_space(8.0);
    let m = &mut cfg.certification.duration_multiplier;
    ui.horizontal(|ui| {
        ui.label("Duration");
        ui.add(egui::Slider::new(m, 0.001..=1.0).suffix("x"));
    });
    if *m < 1.0 {
        let col = theme::warn(ui);
        ui.label(
            RichText::new(format!(
                "{}  Shortened to {:.1}% — a smoke test, not a valid certification.",
                icons::STATUS_WARN,
                *m * 100.0
            ))
            .color(col)
            .small(),
        );
    }
    ui.add_space(4.0);
    ui.label(
        RichText::new("Each stage carries verdict rules for temperature, WHEA, TDR, and throughput stability.")
            .weak()
            .small(),
    );
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
