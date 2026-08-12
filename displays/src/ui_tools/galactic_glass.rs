//! Galactic glass theme: AMOLED-black page, violet glass surfaces, two neon accents.
//!
//! Ported from the ComfyUI Android AMOLED galactic theme. Hot pink is the primary accent
//! (selection, press/active, errors) and aqua the secondary (hover, links, warnings). Violet is
//! not an interaction signal: it is the cast on every surface, which is why it can be everywhere
//! without competing with the two accents. Resting edges are dim white hairlines, not colored —
//! a hued outline is what stops a surface reading as glass.

use eframe::egui::style::{
    HandleShape, ImeComposition, Selection, TextCursorStyle, WidgetVisuals, Widgets,
};
use eframe::egui::{Color32, CornerRadius, Shadow, Stroke, Style, Visuals};

use super::glass_backdrop::GlassParams;
use super::mtech_glass::{glass_interaction, glass_spacing, glass_text_styles, pane, pane_over};

pub const NAME: &str = "Galactic Glass";

/// Primary accent: selection, press/active, error ink.
const PINK: Color32 = Color32::from_rgb(255, 61, 139);
/// Lifted pink for the selection ring and text cursor.
const PINK_BRIGHT: Color32 = Color32::from_rgb(255, 110, 168);
/// Secondary accent: hover, links, open widgets.
const AQUA: Color32 = Color32::from_rgb(43, 226, 214);
/// Lifted aqua for warning text.
const AQUA_BRIGHT: Color32 = Color32::from_rgb(120, 240, 232);
/// Ambient third color: IME underlines and the semantic secondary accent's sibling.
const VIOLET: Color32 = Color32::from_rgb(163, 140, 255);
/// Body ink.
const INK: Color32 = Color32::from_rgb(233, 233, 239);
/// Hover ink.
const INK_BRIGHT: Color32 = Color32::from_rgb(248, 250, 252);
/// Hue of floating surfaces: violet-cast near-black.
const SURFACE: Color32 = Color32::from_rgb(19, 17, 30);
/// Violet-cast pane hues for widget rest states.
const PANE_REST: Color32 = Color32::from_rgb(31, 28, 47);
const PANE_REST_WEAK: Color32 = Color32::from_rgb(25, 23, 38);
const PANE_QUIET: Color32 = Color32::from_rgb(18, 16, 28);
const PANE_QUIET_WEAK: Color32 = Color32::from_rgb(14, 12, 22);
/// Dim white hairline on resting panes.
const RIM: Color32 = Color32::from_rgba_premultiplied(46, 46, 52, 46);
/// Brighter hairline for interactive rest surfaces.
const RIM_BRIGHT: Color32 = Color32::from_rgba_premultiplied(72, 72, 80, 72);

const WIDGET_RADIUS: u8 = 5;
const WINDOW_RADIUS: u8 = 8;

/// Interaction grammar: rest = violet glass under a white rim, hover = aqua fill and rim,
/// press/active = pink fill and rim, open = rest fill under an aqua rim.
fn galactic_widgets() -> Widgets {
    let radius = CornerRadius::same(WIDGET_RADIUS);
    Widgets {
        noninteractive: WidgetVisuals {
            bg_fill: pane_over(PANE_QUIET, 0.52, Color32::BLACK),
            weak_bg_fill: pane(PANE_QUIET_WEAK, 0.47),
            bg_stroke: Stroke::new(1.0, RIM),
            corner_radius: radius,
            fg_stroke: Stroke::new(1.0, INK),
            expansion: 0.0,
        },
        inactive: WidgetVisuals {
            // Opaque composite: slider handles and checkbox interiors must occlude the rail.
            bg_fill: pane_over(PANE_REST, 0.65, Color32::BLACK),
            weak_bg_fill: pane(PANE_REST_WEAK, 0.59),
            bg_stroke: Stroke::new(1.0, RIM_BRIGHT),
            corner_radius: radius,
            fg_stroke: Stroke::new(1.0, INK),
            expansion: 0.0,
        },
        hovered: WidgetVisuals {
            bg_fill: pane_over(AQUA, 0.165, Color32::BLACK),
            weak_bg_fill: pane(AQUA, 0.165),
            bg_stroke: Stroke::new(1.5, pane(AQUA, 0.94)),
            corner_radius: radius,
            fg_stroke: Stroke::new(1.5, INK_BRIGHT),
            expansion: 1.0,
        },
        active: WidgetVisuals {
            bg_fill: pane_over(PINK, 0.21, Color32::BLACK),
            weak_bg_fill: pane(PINK, 0.21),
            bg_stroke: Stroke::new(1.7, pane(PINK, 0.96)),
            corner_radius: radius,
            fg_stroke: Stroke::new(2.0, Color32::WHITE),
            expansion: 1.0,
        },
        open: WidgetVisuals {
            bg_fill: pane_over(PANE_REST, 0.65, Color32::BLACK),
            weak_bg_fill: pane(PANE_REST_WEAK, 0.59),
            bg_stroke: Stroke::new(1.3, pane(AQUA, 0.80)),
            corner_radius: radius,
            fg_stroke: Stroke::new(1.0, INK),
            expansion: 0.0,
        },
    }
}

fn galactic_visuals() -> Visuals {
    Visuals {
        dark_mode: true,
        // None so text follows each state's fg_stroke and brightens on hover and press.
        override_text_color: None,
        widgets: galactic_widgets(),
        // Pink selection; egui also uses this fill for progress bars and slider trails.
        selection: Selection {
            bg_fill: pane(PINK, 0.55),
            stroke: Stroke::new(1.4, PINK_BRIGHT),
        },
        hyperlink_color: AQUA,
        faint_bg_color: Color32::from_rgb(11, 10, 16),
        extreme_bg_color: Color32::from_rgb(8, 7, 13),
        code_bg_color: Color32::from_rgb(6, 5, 10),
        warn_fg_color: AQUA_BRIGHT,
        error_fg_color: PINK,
        window_corner_radius: CornerRadius::same(WINDOW_RADIUS),
        window_shadow: Shadow {
            offset: [0, 2],
            blur: 12,
            spread: 2,
            color: Color32::from_black_alpha(200),
        },
        // Sheerer than opaque, but dense enough that a popup dropped inside a window stays legible.
        window_fill: pane(SURFACE, 0.82),
        window_stroke: Stroke::new(1.2, RIM),
        menu_corner_radius: CornerRadius::same(WINDOW_RADIUS),
        // True black; the source's 232-alpha page fed a mobile ambience layer that has no desktop
        // counterpart.
        panel_fill: Color32::BLACK,
        popup_shadow: Shadow {
            offset: [0, 2],
            blur: 10,
            spread: 1,
            color: Color32::from_black_alpha(170),
        },
        resize_corner_size: 12.0,
        text_cursor: TextCursorStyle {
            stroke: Stroke::new(2.0, PINK_BRIGHT),
            ..Default::default()
        },
        ime_composition: ImeComposition {
            active_underline_stroke: Stroke::new(2.0, VIOLET),
            inactive_underline_stroke: Stroke::new(2.0, pane(VIOLET, 0.5)),
            ..Default::default()
        },
        button_frame: true,
        collapsing_header_frame: true,
        indent_has_left_vline: true,
        striped: true,
        slider_trailing_fill: true,
        handle_shape: HandleShape::Circle,
        image_loading_spinners: true,
        ..Default::default()
    }
}

/// The full `Style`. Geometry, spacing and text styles are shared with the glass families so the
/// preset feels like part of one system.
pub fn galactic_style() -> Style {
    Style {
        text_styles: glass_text_styles(),
        spacing: glass_spacing(),
        interaction: glass_interaction(),
        visuals: galactic_visuals(),
        animation_time: 0.1,
        explanation_tooltips: false,
        url_in_tooltip: true,
        compact_menu_style: true,
        ..Default::default()
    }
}

/// The backdrop-blur material: the desktop counterpart of the source theme's frost.
pub fn galactic_glass_params() -> GlassParams {
    GlassParams {
        enabled: true,
        blur_radius: 26.0,
        tint: Color32::from_rgba_unmultiplied(19, 17, 30, 26),
        corner_radius: WINDOW_RADIUS as f32,
        presence: 1.0,
    }
}

/// Success and secondary-accent colors, which have no `egui::Visuals` slot.
pub fn galactic_semantic_colors() -> (Color32, Color32) {
    (Color32::from_rgb(94, 232, 178), AQUA)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The theme must be AMOLED, translucent, and glassy — with windows dense enough that a
    // popup dropped inside one stays legible over its own fill.
    #[test]
    fn amoled_translucent_and_readable() {
        let style = galactic_style();
        let v = &style.visuals;

        assert_eq!(v.panel_fill, Color32::BLACK, "panels must be true black");
        assert!(v.window_fill.a() < 255, "floating surfaces must be translucent");
        assert!(v.window_fill.a() >= 178, "window fill too sheer for nested popups");
        assert!(
            v.widgets.inactive.weak_bg_fill.a() < 255,
            "widget panes must be translucent"
        );
        assert!(galactic_glass_params().is_visible(), "glass material must draw");
    }

    // Violet is the glass: surfaces carry its cast (blue leading red leading green) at every
    // alpha, while the two interaction accents keep their own hues.
    #[test]
    fn surfaces_carry_the_violet_cast() {
        let style = galactic_style();
        let v = &style.visuals;
        for (what, c) in [
            ("window_fill", v.window_fill),
            ("widget rest (weak)", v.widgets.inactive.weak_bg_fill),
            ("noninteractive frame (weak)", v.widgets.noninteractive.weak_bg_fill),
            ("text well", v.extreme_bg_color),
            ("striped row", v.faint_bg_color),
        ] {
            let [r, g, b, _] = c.to_srgba_unmultiplied();
            assert!(b > r && r >= g, "{what} ({c:?}) is not violet-cast");
        }
        assert_eq!(v.error_fg_color, PINK);
        assert_eq!(v.hyperlink_color, AQUA);
    }

    // Hover is aqua, press is pink: the two accents must not have swapped or muddied.
    #[test]
    fn hover_is_aqua_and_press_is_pink() {
        let w = galactic_widgets();
        let [hr, hg, hb, _] = w.hovered.bg_stroke.color.to_srgba_unmultiplied();
        assert!(hg > hr && hb > hr, "hover rim {:?} is not aqua", w.hovered.bg_stroke.color);
        let [ar, ag, ab, _] = w.active.bg_stroke.color.to_srgba_unmultiplied();
        assert!(ar > ag && ar > ab, "press rim {:?} is not pink", w.active.bg_stroke.color);
    }

    // Sunken wells must not vanish into the black base.
    #[test]
    fn sunken_wells_stay_visible_over_true_black() {
        let extreme = galactic_style().visuals.extreme_bg_color;
        let [r, g, b, a] = extreme.to_srgba_unmultiplied();
        assert_eq!(a, 255, "extreme_bg must be opaque");
        assert!(
            r as u32 + g as u32 + b as u32 > 12,
            "extreme_bg {extreme:?} is indistinguishable from black"
        );
    }
}
