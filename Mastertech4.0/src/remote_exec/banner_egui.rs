//! Consent banner for the egui client.
//!
//! Painting this banner is what admits RemoteExec jobs: [`gate::stamp_banner`]
//! is the heartbeat the gate checks, so a minimised window, a wedged UI or a
//! build without this call all refuse work rather than running unattended.

use displays::ui_tools::icons;
use eframe::egui::{self, Color32, RichText};

use super::{gate, registry};

/// Paint the banner and stamp the gate. No-op when nothing is armed.
///
/// Call once per frame from the root UI, before any tab content, so it cannot
/// be scrolled out of view.
pub fn show(ui: &mut egui::Ui) {
    let Some(info) = gate::banner_info() else {
        return;
    };

    gate::stamp_banner();
    // The gate goes stale after 2s, so the banner must keep painting on an
    // idle desktop.
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));

    let running = registry::running_count();
    let accent = Color32::from_rgb(255, 145, 0);

    egui::Frame::new()
        .fill(Color32::from_rgb(48, 26, 0))
        .stroke(egui::Stroke::new(1.0, accent))
        .inner_margin(egui::Margin::symmetric(10, 6))
        .corner_radius(6.0)
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(icons::icon_sized(icons::EYE, 18.0).color(accent));
                ui.label(
                    RichText::new("Remote control active")
                        .color(accent)
                        .strong(),
                );
                ui.separator();
                ui.label(
                    RichText::new(format!("{} is connected to this computer", info.tech))
                        .color(Color32::from_gray(230)),
                );

                if running > 0 {
                    ui.separator();
                    ui.label(
                        RichText::new(format!(
                            "{running} command{} running",
                            if running == 1 { "" } else { "s" }
                        ))
                        .color(accent),
                    );
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let end = ui
                        .button(
                            RichText::new(format!("{} End remote session", icons::STOP))
                                .color(Color32::WHITE),
                        )
                        .on_hover_text(
                            "Revokes access immediately and stops anything currently running.",
                        );
                    if end.clicked() {
                        end_session();
                    }
                    ui.label(
                        RichText::new(format!("{} left", fmt_remaining(info.expires_in_secs)))
                            .color(Color32::from_gray(170))
                            .small(),
                    );
                });
            });

            ui.label(
                RichText::new(format!("Reason: {}", info.reason))
                    .color(Color32::from_gray(200))
                    .small(),
            );
        });
    ui.add_space(4.0);
}

/// Revoke the lease and terminate anything running under it.
fn end_session() {
    let killed = registry::cancel_all();
    gate::disarm();
    log::warn!("[remote_exec] session ended from the client UI; {killed} job(s) terminated");
}

fn fmt_remaining(secs: u64) -> String {
    match secs {
        s if s >= 3600 => format!("{}h {}m", s / 3600, (s % 3600) / 60),
        s if s >= 60 => format!("{}m", s / 60),
        s => format!("{s}s"),
    }
}
