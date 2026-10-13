//! Help page: what every mode and test does, and when to reach for it.
//!
//! Entirely generated from `stress_runner::stressor_info`, so it cannot drift
//! from the hover hints or the Scripts catalog.

use eframe::egui::{self, Align, Layout, RichText, Ui, vec2};
use stress_runner::{
    CERT_PRESET_NAMES, PanelMode, Subsystem, cert_preset_info, info_for, mode_info,
};

use super::{mode_icon, subsystem_icon};
use crate::controls::{self, BarKind, Icon, Metrics, Tokens};
use crate::icons::svg;
use crate::theme;

/// Width of the term column for modes and tiers.
const TERM_W: f32 = 130.0;
/// Width of the term column for tests and result terms.
const TEST_TERM_W: f32 = 120.0;

pub(super) fn show(ui: &mut Ui, open: &mut bool) {
    let m = Metrics::get(ui.ctx());
    controls::bar(ui, BarKind::Header, |ui| {
        controls::heading(ui, svg::BOOK_OPEN, "Choosing a stress test");
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if controls::icon_text_button(ui, svg::X, "Close").clicked() {
                *open = false;
            }
        });
    });
    ui.add_space(m.gap.y);

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            modes(ui);
            ui.add_space(m.gap.y * 2.0);
            tiers(ui);
            ui.add_space(m.gap.y * 2.0);
            tests(ui);
            ui.add_space(m.gap.y * 2.0);
            reading_results(ui);
        });
}

/// A fixed-width term cell: optional icon and strong text.
fn term(ui: &mut Ui, width: f32, icon: Option<Icon>, text: &str) {
    let (t, m) = (Tokens::get(ui.ctx()), Metrics::get(ui.ctx()));
    let height = ui.spacing().interact_size.y;
    ui.allocate_ui_with_layout(
        vec2(width, height),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.set_min_width(width);
            ui.spacing_mut().item_spacing.x = m.pad.y;
            if let Some(icon) = icon {
                icon.show_tinted(ui, m.icon_sm, t.icon);
            }
            ui.label(RichText::new(text).strong());
        },
    );
}

fn modes(ui: &mut Ui) {
    let m = Metrics::get(ui.ctx());
    controls::heading(ui, svg::SLIDERS_HORIZONTAL, "Run modes");
    ui.add_space(m.pad.y);
    for mode in [
        PanelMode::Single,
        PanelMode::Scenario,
        PanelMode::QcBenchmark,
        PanelMode::Certification,
        PanelMode::Concurrent,
    ] {
        let icon = mode_icon(&mode);
        let i = mode_info(mode);
        ui.horizontal_top(|ui| {
            term(ui, TERM_W, Some(icon), i.label);
            ui.vertical(|ui| {
                ui.label(i.what);
                let col = theme::primary(ui);
                controls::IconLabel::new(svg::LIGHTBULB, &format!("Use when: {}", i.when))
                    .tone(col)
                    .small(true)
                    .selectable(true)
                    .show(ui);
            });
        });
        ui.add_space(m.pad.y);
    }
}

fn tiers(ui: &mut Ui) {
    let m = Metrics::get(ui.ctx());
    controls::heading(ui, svg::MEDAL, "Certification tiers");
    ui.add_space(m.pad.y);
    for name in CERT_PRESET_NAMES {
        let Some(i) = cert_preset_info(name) else {
            continue;
        };
        ui.horizontal_top(|ui| {
            term(ui, TERM_W, Some(svg::CERTIFICATE), i.label);
            ui.vertical(|ui| {
                ui.label(i.what);
                let col = theme::primary(ui);
                controls::IconLabel::new(svg::LIGHTBULB, i.when)
                    .tone(col)
                    .small(true)
                    .selectable(true)
                    .show(ui);
            });
        });
        ui.add_space(m.pad.y);
    }
    let col = theme::warn(ui);
    controls::IconLabel::new(svg::WARNING, "A tier only certifies a machine if it runs to completion. A shortened or interrupted run is a smoke test.").tone(col).small(true).selectable(true).show(ui);
}

fn tests(ui: &mut Ui) {
    let m = Metrics::get(ui.ctx());
    controls::heading(ui, svg::FLASK, "Tests by subsystem");
    ui.add_space(m.pad.y);

    for group in Subsystem::ALL {
        let default_open = matches!(group, Subsystem::Cpu | Subsystem::Gpu);
        controls::Section::new(group.label(), group.label())
            .icon(subsystem_icon(group))
            .default_open(default_open)
            .show(ui, |ui| {
                ui.label(RichText::new(group.blurb()).weak().small());
                ui.add_space(m.pad.y);
                for choice in group.stressors() {
                    let i = info_for(*choice);
                    ui.horizontal_top(|ui| {
                        term(ui, TEST_TERM_W, None, choice.label());
                        ui.vertical(|ui| {
                            ui.label(i.what);
                            let acc = theme::primary(ui);
                            controls::IconLabel::new(
                                svg::LIGHTBULB,
                                &format!("Use when: {}", i.when),
                            )
                            .tone(acc)
                            .small(true)
                            .selectable(true)
                            .show(ui);
                            if let Some(pass) = i.pass {
                                let col = theme::weak_text(ui);
                                controls::IconLabel::new(
                                    svg::SEAL_CHECK,
                                    &format!("Passes if: {pass}"),
                                )
                                .tone(col)
                                .small(true)
                                .selectable(true)
                                .show(ui);
                            }
                            if let Some(caveat) = i.caveat {
                                let w = theme::warn(ui);
                                controls::IconLabel::new(svg::WARNING, caveat)
                                    .tone(w)
                                    .small(true)
                                    .selectable(true)
                                    .show(ui);
                            }
                        });
                    });
                    ui.add_space(m.gap.y);
                }
            });
    }
}

fn reading_results(ui: &mut Ui) {
    let m = Metrics::get(ui.ctx());
    controls::heading(ui, svg::CLIPBOARD_TEXT, "Reading a result");
    ui.add_space(m.pad.y);
    for (icon, term_text, meaning) in [
        (
            svg::WARNING_OCTAGON,
            "WHEA",
            "Machine-check errors reported by the CPU, memory controller, or PCIe. Any new one during a run points at hardware, not software.",
        ),
        (
            svg::GRAPHICS_CARD,
            "TDR",
            "The GPU stopped responding and the driver reset it. Repeated TDRs mean the card or its driver, not the test.",
        ),
        (
            svg::BINARY,
            "Data error",
            "A verifying test read back something it did not write. This is a hardware fault, not a slow score.",
        ),
        (
            svg::QUESTION,
            "Inconclusive",
            "The load never actually ran — a missing GPU, an aborted stage, or a run that stopped early. It proves nothing either way.",
        ),
        (
            svg::TREND_DOWN,
            "Below floor",
            "The test ran but was slower than the rule allows. On old or low-end parts read the throughput number before calling it a fault.",
        ),
    ] {
        ui.horizontal_top(|ui| {
            term(ui, TEST_TERM_W, Some(icon), term_text);
            ui.label(meaning);
        });
        ui.add_space(m.pad.y);
    }
}
