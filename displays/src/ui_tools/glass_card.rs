//! Themed card surfaces for content *inside* a panel.
//!
//! Floating surfaces get their pane from [`glass_backdrop::frost_open_windows`]. A card in a panel
//! body has no such sweep, so every call site used to invent its own fill — which is why the admin
//! console had a dozen hand-picked greys that ignored the active theme.
//!
//! [`card`] and [`titled_card`] take their fill, outline and rounding from the theme, and frost
//! their own backdrop through [`glass_backdrop::glass_frame`], so a card is a translucent pane on a
//! glass theme and a raised opaque surface on a flat one, without the call site choosing.
//!
//! # What the frost reaches
//!
//! A frost grabs the framebuffer where the card sits, so it blurs whatever was painted before it:
//! the panel fill, and any content already laid out above or behind the card. It cannot blur a
//! sibling that paints later. On a flat panel there is nothing to smear and the card reads as a
//! plain tinted pane — which is what [`glass_backdrop::GlassParams::tint`] is for.

use eframe::egui::{
    Align, AsIdSalt, Color32, CornerRadius, Frame, InnerResponse, Layout, Margin, RichText, Stroke,
    Ui, Vec2,
};

use super::glass_backdrop;
use super::{icons, theme};

/// Band the theme's window margin is clamped into for card padding: below the floor content
/// touches the outline, above the ceiling a card of key/value rows turns mostly into margin.
const CARD_PAD_MIN: i8 = 6;
const CARD_PAD_MAX: i8 = 10;
/// Vertical gap between stacked cards.
const CARD_GAP: i8 = 3;
/// Corner radius when the theme asks for square windows; a card is never a hard rectangle.
const MIN_RADIUS: u8 = 3;

/// Card padding, tracking the theme's own window margin so a compact theme gets compact cards.
fn card_padding(ui: &Ui) -> i8 {
    ui.spacing()
        .window_margin
        .left
        .clamp(CARD_PAD_MIN, CARD_PAD_MAX)
}

/// Composites `over` onto `base` when it is translucent.
fn composite(over: Color32, base: Color32) -> Color32 {
    let inv = 1.0 - over.a() as f32 / 255.0;
    Color32::from_rgb(
        (over.r() as f32 + base.r() as f32 * inv).round() as u8,
        (over.g() as f32 + base.g() as f32 * inv).round() as u8,
        (over.b() as f32 + base.b() as f32 * inv).round() as u8,
    )
}

/// The card surface tone for the active theme.
///
/// A glass theme's `window_fill` is translucent, and that translucency is the point — it is the
/// film the frost reads through, and it is the same tone every window and popup already paints,
/// so cards match them. A flat theme's is opaque and often equal to `panel_fill`, which would make
/// a card invisible; those get the faint tint composited over the panel instead.
pub fn card_fill(ui: &Ui) -> Color32 {
    let v = ui.visuals();
    if v.window_fill.a() < 255 {
        return v.window_fill;
    }
    let lifted = composite(v.faint_bg_color, v.panel_fill);
    if lifted == v.panel_fill {
        composite(v.widgets.noninteractive.weak_bg_fill, v.panel_fill)
    } else {
        lifted
    }
}

/// Fill for a panel that holds cards — a sidebar or a rail — pushed away from `panel_fill` so the
/// cards drawn on it with [`card_fill`] still read as raised.
pub fn recessed_fill(ui: &Ui) -> Color32 {
    let v = ui.visuals();
    let [r, g, b, a] = v.panel_fill.to_srgba_unmultiplied();
    // Toward black on a dark theme, toward white on a light one: either way, away from the cards.
    let shift = |c: u8| {
        if v.dark_mode {
            (c as f32 * 0.72).round() as u8
        } else {
            (c as f32 + (255.0 - c as f32) * 0.28).round() as u8
        }
    };
    Color32::from_rgba_unmultiplied(shift(r), shift(g), shift(b), a)
}

/// The card outline for the active theme, falling back to the noninteractive widget edge when the
/// theme leaves `window_stroke` invisible.
pub fn card_stroke(ui: &Ui) -> Stroke {
    let v = ui.visuals();
    if v.window_stroke.width > 0.0 && v.window_stroke.color.a() > 0 {
        v.window_stroke
    } else {
        v.widgets.noninteractive.bg_stroke
    }
}

/// A [`Frame`] shaped like a card in the active theme. Use [`card`] instead unless you need to
/// adjust the frame before showing it — a bare frame does not frost.
pub fn card_frame(ui: &Ui) -> Frame {
    let radius = ui.visuals().window_corner_radius.nw.max(MIN_RADIUS);
    Frame::new()
        .fill(card_fill(ui))
        .stroke(card_stroke(ui))
        .corner_radius(CornerRadius::same(radius))
        .inner_margin(Margin::same(card_padding(ui)))
        .outer_margin(Margin::symmetric(0, CARD_GAP))
}

/// A themed card that frosts its own backdrop.
///
/// `id_salt` must be unique among the cards of one `Ui`; it keys the last-frame rect the frost is
/// enqueued against, so two cards sharing a salt would frost each other's position.
///
/// Each call costs one grab-pass, so this is for the handful of standing surfaces on a page. Use
/// [`group`] for anything that repeats per row.
pub fn card<R>(
    ui: &mut Ui,
    id_salt: impl AsIdSalt,
    contents: impl FnOnce(&mut Ui) -> R,
) -> InnerResponse<R> {
    let frame = card_frame(ui);
    glass_backdrop::glass_frame(ui, id_salt, frame, contents)
}

/// Drop-in for [`Ui::group`] that draws the theme's card instead of egui's default group frame.
///
/// No frost: this is what a per-row card uses, and a grab-pass per row would cost far more than
/// blurring a flat panel is worth. The translucent fill still lets the panel read through.
///
/// [`Ui::group`]: eframe::egui::Ui::group
pub fn group<R>(ui: &mut Ui, contents: impl FnOnce(&mut Ui) -> R) -> InnerResponse<R> {
    card_frame(ui).show(ui, contents)
}

/// A themed card with an accent glyph, a title, and an optional right-aligned subtitle.
///
/// Salts itself from the `Ui`'s auto-id sequence rather than from `title`, so a loop that emits
/// the same title twice still gives each card its own stored rect.
pub fn titled_card<R>(
    ui: &mut Ui,
    glyph: &str,
    title: &str,
    subtitle: Option<&str>,
    contents: impl FnOnce(&mut Ui) -> R,
) -> R {
    let salt = ui.next_auto_id();
    ui.skip_ahead_auto_ids(1);
    card(ui, salt, |ui| {
        ui.set_min_width(ui.available_width());
        card_header(ui, glyph, title, subtitle);
        contents(ui)
    })
    .inner
}

/// The glyph + title row a [`titled_card`] opens with, for call sites that build their own frame.
pub fn card_header(ui: &mut Ui, glyph: &str, title: &str, subtitle: Option<&str>) {
    let accent = theme::accent_secondary(ui);
    ui.horizontal(|ui| {
        if !glyph.is_empty() {
            ui.label(icons::icon_colored(glyph, accent).small());
        }
        ui.label(
            RichText::new(title)
                .strong()
                .small()
                .color(theme::strong_text(ui)),
        );
        if let Some(subtitle) = subtitle {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(subtitle).small().color(theme::weak_text(ui)));
            });
        }
    });
    hairline(ui);
}

/// A one-pixel rule in the card outline color, dimmer than `ui.separator()`.
pub fn hairline(ui: &mut Ui) {
    let stroke = card_stroke(ui);
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 5.0), eframe::egui::Sense::hover());
    let y = rect.center().y;
    ui.painter().hline(
        rect.left()..=rect.right(),
        y,
        Stroke::new(1.0, stroke.color.gamma_multiply(0.7)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_tools::theme_config::{style_for_preset, PresetStyles};
    use eframe::egui::Context;

    /// Runs `f` against a `Ui` carrying `preset`'s style.
    fn with_preset<R>(preset: PresetStyles, f: impl FnOnce(&mut Ui) -> R) -> R {
        let ctx = Context::default();
        ctx.set_global_style(std::sync::Arc::new(style_for_preset(preset)));
        // `run_ui` wants `FnMut`, so the one-shot closure is handed over through a slot.
        let mut once = Some(f);
        let mut out = None;
        let mut full = ctx.run_ui(Default::default(), |ui| {
            if let Some(f) = once.take() {
                out = Some(f(ui));
            }
        });
        // epaint 0.36 panics on a dropped `TexturesDelta` no renderer consumed.
        full.textures_delta.clear();
        out.expect("the ui closure ran")
    }

    // The bug the helper exists to prevent: a card whose fill equals the panel it sits on is
    // invisible, and every flat theme in the picker has at least one slot that collapses that way.
    #[test]
    fn a_card_never_disappears_into_its_panel() {
        for preset in [
            PresetStyles::ShippedClassic,
            PresetStyles::LegacyClassic,
            PresetStyles::DefaultEgui,
            PresetStyles::MtechNoir,
            PresetStyles::MtechNoirGlass,
            PresetStyles::NebulaGlass,
            PresetStyles::ObsidianGlass,
            PresetStyles::VelvetGlass,
            PresetStyles::TwilightGlass,
            PresetStyles::QuartzGlass,
            PresetStyles::MtechGlassFull,
            PresetStyles::CarlDarkFull,
            PresetStyles::TokyoNightFull,
            PresetStyles::RerunMtechOledFull,
        ] {
            let (fill, panel, stroke) = with_preset(preset, |ui| {
                (card_fill(ui), ui.visuals().panel_fill, card_stroke(ui))
            });
            let separated = fill != panel
                || (stroke.width > 0.0 && stroke.color.a() > 0 && stroke.color != panel);
            assert!(
                separated,
                "{}: a card is indistinguishable from its panel (fill {fill:?}, panel {panel:?})",
                preset.as_str(),
            );
        }
    }

    // A glass theme's card has to stay translucent, or the frost behind it is wasted work.
    #[test]
    fn a_glass_theme_keeps_its_cards_translucent() {
        for preset in [
            PresetStyles::ObsidianGlass,
            PresetStyles::VelvetGlass,
            PresetStyles::TwilightGlass,
            PresetStyles::QuartzGlass,
            PresetStyles::NebulaGlass,
        ] {
            let fill = with_preset(preset, |ui| card_fill(ui));
            assert!(fill.a() < 255, "{}: card fill is opaque", preset.as_str());
        }
    }
}
