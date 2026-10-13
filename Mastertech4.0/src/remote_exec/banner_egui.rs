//! Consent banner for the egui client.
//!
//! Painting this banner is what admits RemoteExec jobs: [`gate::stamp_banner`]
//! is the heartbeat the gate checks, so a minimised window, a wedged UI or a
//! build without this call all refuse work rather than running unattended.

use displays::ui_tools::{
    controls::{self, ButtonKind, Metrics, TextButton, Tokens},
    icons::svg,
    tokens,
};
use eframe::egui::{self, RichText};

use super::{gate, registry};

/// Opacity of the warn hue in the banner fill.
const WASH: f32 = 0.14;

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
    let t = Tokens::get(ui.ctx());
    let m = Metrics::get(ui.ctx());
    let accent = t.warn;

    egui::Frame::new()
        .fill(tokens::composite(accent.gamma_multiply(WASH), t.base))
        .stroke(egui::Stroke::new(1.0, accent))
        .inner_margin(egui::Margin::symmetric(m.pad.x as i8, m.gap.y as i8))
        .corner_radius(t.radius_sm)
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.add(svg::EYE.image(m.icon_lg).tint(accent));
                ui.label(
                    RichText::new("Remote control active")
                        .color(accent)
                        .strong(),
                );
                controls::vline(ui, m.icon_lg);
                ui.label(
                    RichText::new(format!("{} is connected to this computer", info.tech))
                        .color(t.text),
                );

                if running > 0 {
                    controls::vline(ui, m.icon_lg);
                    ui.add(svg::TERMINAL.image(m.icon_sm).tint(accent));
                    ui.label(
                        RichText::new(format!(
                            "{running} command{} running",
                            if running == 1 { "" } else { "s" }
                        ))
                        .color(accent),
                    );
                }
                if super::screen_is_live() {
                    controls::vline(ui, m.icon_lg);
                    ui.add(svg::MONITOR_PLAY.image(m.icon_sm).tint(t.error));
                    ui.label(
                        RichText::new("VIEWING YOUR SCREEN")
                            .color(t.error)
                            .strong(),
                    );
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let end = TextButton::new("End remote session")
                        .icon(svg::STOP)
                        .kind(ButtonKind::Danger)
                        .show(ui)
                        .on_hover_text(
                            "Revokes access immediately and stops anything currently running.",
                        );
                    if end.clicked() {
                        end_session();
                    }
                    ui.label(
                        RichText::new(format!("{} left", fmt_remaining(info.expires_in_secs)))
                            .color(t.text_dim)
                            .small(),
                    );
                    ui.add(svg::TIMER.image(m.icon_sm).tint(t.text_dim));
                });
            });

            ui.horizontal(|ui| {
                ui.add(svg::NOTE.image(m.icon_sm).tint(t.text_dim));
                ui.add(
                    egui::Label::new(
                        RichText::new(format!("Reason: {}", info.reason))
                            .color(t.text_dim)
                            .small(),
                    )
                    .wrap(),
                );
            });
        });
    ui.add_space(m.gap.y);
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
