//! Preview: frosted glass in a real `eframe`-on-glow app, via `backdrop-blur-egui`'s
//! [`GrabPassRenderer`]. This is the grab-pass **integration proof** — it exercises the whole path
//! the headless tests cannot assemble at once: egui_glow invokes the paint callback with the live
//! `glow::Context`, the adapter grabs the backdrop behind the panel, blurs it, and composites the
//! frosted surface back. Run it to *see* the glass; it needs a display.
//!
//! Layout per frame: (1) paint a vivid, sharp-edged backdrop (so the blur is obvious), (2) frost a
//! centered panel (the blur source is the backdrop directly behind it), (3) paint the panel's
//! foreground text over the frosted background. The backdrop animates (drifting circles), so the
//! panel demonstrates [`RepaintPolicy::Live`]; a slider drives the blur radius.

use backdrop_blur_egui::{
    BlurRadius, CornerRadius, GrabPassRenderer, LinearRgba, Presence, RepaintPolicy, Surface, Tint,
    glow,
};
use egui::{Align2, Color32, FontId, Pos2, Rect, Vec2, pos2, vec2};

struct FrostApp {
    /// `None` if the backend could not be built (e.g. a context too old) — then the panel falls
    /// back to a plain translucent fill so the app still runs, and the cause is printed to stderr.
    renderer: Option<GrabPassRenderer>,
    blur_radius: f32,
    panel: Rect,
}

impl FrostApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let renderer = cc
            .gl
            .as_ref()
            .and_then(|gl| match GrabPassRenderer::new(gl) {
                Ok(renderer) => Some(renderer),
                Err(err) => {
                    eprintln!(
                        "backdrop-blur grab-pass unavailable, using the plain-fill fallback: {err}"
                    );
                    None
                }
            });
        Self {
            renderer,
            blur_radius: 24.0,
            panel: Rect::from_min_size(pos2(140.0, 110.0), vec2(300.0, 200.0)),
        }
    }

    /// A vivid, sharp-edged backdrop: vertical color bands plus two hard-contrast circles drifting
    /// on slow sine paths (`t` is seconds), so the blur visibly softens edges AND visibly tracks
    /// moving content — this animation is what makes [`RepaintPolicy::Live`] the right policy.
    fn paint_backdrop(painter: &egui::Painter, rect: Rect, t: f32) {
        const BANDS: [Color32; 4] = [
            Color32::from_rgb(231, 76, 60),
            Color32::from_rgb(46, 204, 113),
            Color32::from_rgb(52, 152, 219),
            Color32::from_rgb(241, 196, 15),
        ];
        let n = 10;
        let bw = rect.width() / n as f32;
        for i in 0..n {
            let band = Rect::from_min_size(
                pos2(rect.left() + i as f32 * bw, rect.top()),
                vec2(bw, rect.height()),
            );
            painter.rect_filled(band, 0.0, BANDS[i as usize % BANDS.len()]);
        }
        let white_drift = vec2((t * 0.5).sin() * 60.0, (t * 0.7).cos() * 40.0);
        let black_drift = vec2((t * 0.9 + 1.3).cos() * 50.0, (t * 0.4 + 2.1).sin() * 45.0);
        painter.circle_filled(rect.center() + white_drift, 64.0, Color32::WHITE);
        painter.circle_filled(
            rect.left_top() + Vec2::splat(90.0) + black_drift,
            44.0,
            Color32::BLACK,
        );
    }
}

impl eframe::App for FrostApp {
    // eframe hands the central-panel `Ui` directly (it wraps the `CentralPanel`).
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let full = ui.max_rect();

        // 1) The colorful backdrop FIRST (lower z = the grabbed blur source).
        Self::paint_backdrop(ui.painter(), full, ui.input(|i| i.time) as f32);

        // A live blur-radius slider (drawn over the backdrop, top-left, clear of the panel).
        ui.add(egui::Slider::new(&mut self.blur_radius, 0.0..=64.0).text("blur radius"));

        // 2) Frost the panel: grab → blur → composite, in the paint callback.
        if let Some(renderer) = &self.renderer {
            renderer.frost(
                ui,
                Surface {
                    rect: self.panel,
                    blur_radius: BlurRadius::new(self.blur_radius),
                    // A faint white film over the blur (linear, 10% opacity).
                    tint: Tint::new(LinearRgba::new(1.0, 1.0, 1.0, 0.10)),
                    corner_radius: CornerRadius::new(18.0),
                    presence: Presence::default(),
                    // Live because the backdrop BEHIND the glass animates — the grab must re-run
                    // every frame. With a static backdrop this would be RepaintPolicy::Static:
                    // interaction repaints (the slider) keep the UI responsive on their own.
                    repaint: RepaintPolicy::Live,
                },
            );
        } else {
            ui.painter()
                .rect_filled(self.panel, 18.0, Color32::from_black_alpha(160));
        }

        // 3) The panel's FOREGROUND over the frosted background.
        painter_text(ui.painter(), self.panel.center());
    }

    fn on_exit(&mut self, gl: Option<&glow::Context>) {
        // Free the backend's GL objects while the context is still current (DESIGN §11).
        if let (Some(renderer), Some(gl)) = (self.renderer.as_ref(), gl) {
            renderer.destroy(gl);
        }
    }
}

fn painter_text(painter: &egui::Painter, center: Pos2) {
    painter.text(
        center,
        Align2::CENTER_CENTER,
        "Frosted glass",
        FontId::proportional(26.0),
        Color32::WHITE,
    );
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "backdrop-blur — frosted glass (eframe / glow)",
        options,
        Box::new(|cc| Ok(Box::new(FrostApp::new(cc)))),
    )
}
