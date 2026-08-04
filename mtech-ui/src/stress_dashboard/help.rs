//! Help page: what every mode and test does, and when to reach for it.
//!
//! Entirely generated from `stress_runner::stressor_info`, so it cannot drift
//! from the hover hints or the Scripts catalog.

use eframe::egui::{self, RichText, Ui};
use stress_runner::{
    cert_preset_info, info_for, mode_info, PanelMode, Subsystem, CERT_PRESET_NAMES,
};

use crate::{icons, theme};

pub(super) fn show(ui: &mut Ui, open: &mut bool) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Choosing a stress test").heading());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(format!("{}  Close", icons::CLOSE)).clicked() {
                *open = false;
            }
        });
    });
    ui.add_space(6.0);

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            modes(ui);
            ui.add_space(10.0);
            tiers(ui);
            ui.add_space(10.0);
            tests(ui);
            ui.add_space(10.0);
            reading_results(ui);
        });
}

fn modes(ui: &mut Ui) {
    ui.label(RichText::new("Run modes").heading());
    ui.add_space(2.0);
    for mode in [
        PanelMode::Single,
        PanelMode::Scenario,
        PanelMode::QcBenchmark,
        PanelMode::Certification,
        PanelMode::Concurrent,
    ] {
        let i = mode_info(mode);
        ui.horizontal_top(|ui| {
            ui.add_sized(
                [130.0, ui.spacing().interact_size.y],
                egui::Label::new(RichText::new(i.label).strong()),
            );
            ui.vertical(|ui| {
                ui.label(i.what);
                let col = theme::accent(ui);
                ui.label(RichText::new(format!("Use when: {}", i.when)).color(col).small());
            });
        });
        ui.add_space(4.0);
    }
}

fn tiers(ui: &mut Ui) {
    ui.label(RichText::new("Certification tiers").heading());
    ui.add_space(2.0);
    for name in CERT_PRESET_NAMES {
        let Some(i) = cert_preset_info(name) else { continue };
        ui.horizontal_top(|ui| {
            ui.add_sized(
                [130.0, ui.spacing().interact_size.y],
                egui::Label::new(RichText::new(i.label).strong()),
            );
            ui.vertical(|ui| {
                ui.label(i.what);
                let col = theme::accent(ui);
                ui.label(RichText::new(i.when).color(col).small());
            });
        });
        ui.add_space(4.0);
    }
    let col = theme::warn(ui);
    ui.label(
        RichText::new(format!(
            "{}  A tier only certifies a machine if it runs to completion. A shortened or interrupted run is a smoke test.",
            icons::STATUS_WARN
        ))
        .color(col)
        .small(),
    );
}

fn tests(ui: &mut Ui) {
    ui.label(RichText::new("Tests by subsystem").heading());
    ui.add_space(2.0);

    for group in Subsystem::ALL {
        egui::CollapsingHeader::new(RichText::new(group.label()).strong())
            .id_salt(group.label())
            .default_open(matches!(group, Subsystem::Cpu | Subsystem::Gpu))
            .show(ui, |ui| {
                ui.label(RichText::new(group.blurb()).weak().small());
                ui.add_space(4.0);
                for choice in group.stressors() {
                    let i = info_for(*choice);
                    ui.horizontal_top(|ui| {
                        ui.add_sized(
                            [120.0, ui.spacing().interact_size.y],
                            egui::Label::new(RichText::new(choice.label()).strong()),
                        );
                        ui.vertical(|ui| {
                            ui.label(i.what);
                            let acc = theme::accent(ui);
                            ui.label(
                                RichText::new(format!("Use when: {}", i.when)).color(acc).small(),
                            );
                            if let Some(pass) = i.pass {
                                ui.label(RichText::new(format!("Passes if: {pass}")).weak().small());
                            }
                            if let Some(caveat) = i.caveat {
                                let w = theme::warn(ui);
                                ui.label(
                                    RichText::new(format!("{}  {caveat}", icons::STATUS_WARN))
                                        .color(w)
                                        .small(),
                                );
                            }
                        });
                    });
                    ui.add_space(6.0);
                }
            });
    }
}

fn reading_results(ui: &mut Ui) {
    ui.label(RichText::new("Reading a result").heading());
    ui.add_space(2.0);
    for (term, meaning) in [
        (
            "WHEA",
            "Machine-check errors reported by the CPU, memory controller, or PCIe. Any new one during a run points at hardware, not software.",
        ),
        (
            "TDR",
            "The GPU stopped responding and the driver reset it. Repeated TDRs mean the card or its driver, not the test.",
        ),
        (
            "Data error",
            "A verifying test read back something it did not write. This is a hardware fault, not a slow score.",
        ),
        (
            "Inconclusive",
            "The load never actually ran — a missing GPU, an aborted stage, or a run that stopped early. It proves nothing either way.",
        ),
        (
            "Below floor",
            "The test ran but was slower than the rule allows. On old or low-end parts read the throughput number before calling it a fault.",
        ),
    ] {
        ui.horizontal_top(|ui| {
            ui.add_sized(
                [120.0, ui.spacing().interact_size.y],
                egui::Label::new(RichText::new(term).strong()),
            );
            ui.label(meaning);
        });
        ui.add_space(4.0);
    }
}
