//! Neon glass theme family: AMOLED-black base, translucent accent panes, glowing edges.
//!
//! One [`NeonPalette`] per theme drives everything — [`neon_style`] builds the `Style` and
//! [`neon_glass_params`] the backdrop-blur material, so a new theme is a palette and nothing else.
//!
//! Panels are true black; every floating surface is a tinted pane of `surface` at
//! [`NeonPalette::surface_alpha`]. That alpha is the readability dial: egui draws windows *and*
//! popups from `Visuals::window_fill`, and a frost cannot blur a sibling surface that paints after
//! it, so a combo dropped inside a window is occluded only by its own fill. Values near 0.8 keep
//! such a dropdown legible while still letting the blur read through at roughly a fifth of the pane.

use eframe::egui::style::{
    HandleShape, ImeComposition, Selection, TextCursorStyle, WidgetVisuals, Widgets,
};
use eframe::egui::{Color32, CornerRadius, Shadow, Stroke, Style, Visuals};

use super::glass_backdrop::GlassParams;
use super::mtech_glass::{edge, glass_interaction, glass_spacing, glass_text_styles, lift, pane, pane_over};

/// The colors one neon glass theme is generated from.
#[derive(Clone, Copy, Debug)]
pub struct NeonPalette {
    pub name: &'static str,
    /// Panel background. True black for AMOLED.
    pub base: Color32,
    /// Hue of every floating surface and sunken well.
    pub surface: Color32,
    /// How opaque floating surfaces are; the nested-popup readability dial.
    pub surface_alpha: f32,
    /// Idle and hover accent.
    pub primary: Color32,
    /// Press accent.
    pub secondary: Color32,
    /// Warnings and highlights.
    pub tertiary: Color32,
    pub success: Color32,
    pub error: Color32,
    pub text: Color32,
    /// Shadow color behind floating surfaces.
    pub glow: Color32,
}

const WIDGET_RADIUS: u8 = 7;
const WINDOW_RADIUS: u8 = 10;
const MENU_RADIUS: u8 = 8;

/// Violet and magenta over deep space — the closest of the family to MTech Noir.
pub const NEBULA: NeonPalette = NeonPalette {
    name: "Nebula Glass",
    base: Color32::BLACK,
    surface: Color32::from_rgb(11, 8, 20),
    surface_alpha: 0.82,
    primary: Color32::from_rgb(139, 110, 255),
    // Orchid rather than magenta: it has to press apart from the rose error.
    secondary: Color32::from_rgb(216, 102, 255),
    tertiary: Color32::from_rgb(94, 226, 255),
    success: Color32::from_rgb(94, 232, 178),
    error: Color32::from_rgb(255, 73, 137),
    text: Color32::from_rgb(233, 230, 245),
    glow: Color32::from_rgb(108, 68, 255),
};

/// Aqua and mint with violet edges.
pub const AURORA: NeonPalette = NeonPalette {
    name: "Aurora Glass",
    base: Color32::BLACK,
    surface: Color32::from_rgb(5, 15, 16),
    surface_alpha: 0.82,
    primary: Color32::from_rgb(72, 232, 196),
    secondary: Color32::from_rgb(94, 226, 255),
    tertiary: Color32::from_rgb(167, 139, 255),
    success: Color32::from_rgb(110, 240, 170),
    error: Color32::from_rgb(255, 92, 140),
    text: Color32::from_rgb(226, 241, 238),
    glow: Color32::from_rgb(28, 190, 172),
};

/// Hot pink over a magenta-black base.
pub const SUPERNOVA: NeonPalette = NeonPalette {
    name: "Supernova Glass",
    base: Color32::BLACK,
    surface: Color32::from_rgb(18, 6, 15),
    surface_alpha: 0.82,
    primary: Color32::from_rgb(255, 92, 170),
    secondary: Color32::from_rgb(177, 108, 255),
    tertiary: Color32::from_rgb(94, 226, 255),
    success: Color32::from_rgb(94, 232, 178),
    // Warm enough to read as an alarm next to the pink primary.
    error: Color32::from_rgb(255, 104, 72),
    text: Color32::from_rgb(247, 230, 241),
    glow: Color32::from_rgb(255, 58, 150),
};

/// Electric blue and cyan over navy-black.
pub const EVENT_HORIZON: NeonPalette = NeonPalette {
    name: "Event Horizon Glass",
    base: Color32::BLACK,
    surface: Color32::from_rgb(5, 10, 22),
    surface_alpha: 0.82,
    primary: Color32::from_rgb(92, 142, 255),
    secondary: Color32::from_rgb(94, 226, 255),
    tertiary: Color32::from_rgb(167, 139, 255),
    success: Color32::from_rgb(94, 232, 178),
    error: Color32::from_rgb(255, 86, 132),
    text: Color32::from_rgb(223, 232, 250),
    glow: Color32::from_rgb(48, 96, 255),
};

/// Every palette in the family, in picker order.
pub const NEON_THEMES: [NeonPalette; 4] = [NEBULA, AURORA, SUPERNOVA, EVENT_HORIZON];

/// One widget state: a translucent pane of `tint` over the base, wrapped in a brighter cut of it.
fn neon_widget(
    p: &NeonPalette,
    tint: Color32,
    fill: f32,
    weak_fill: f32,
    edge_opacity: f32,
    edge_width: f32,
    fg: Color32,
    fg_width: f32,
    expansion: f32,
) -> WidgetVisuals {
    WidgetVisuals {
        // Opaque composite: slider handles and checkbox interiors must occlude the rail behind them.
        bg_fill: pane_over(tint, fill, p.base),
        weak_bg_fill: pane(tint, weak_fill),
        bg_stroke: Stroke::new(edge_width, edge(tint, edge_opacity)),
        corner_radius: CornerRadius::same(WIDGET_RADIUS),
        fg_stroke: Stroke::new(fg_width, fg),
        expansion,
    }
}

fn neon_widgets(p: &NeonPalette) -> Widgets {
    Widgets {
        noninteractive: neon_widget(p, p.primary, 0.05, 0.03, 0.22, 1.0, p.text, 1.0, 0.0),
        inactive: neon_widget(p, p.primary, 0.10, 0.065, 0.40, 1.0, p.text, 1.0, 0.0),
        hovered: neon_widget(
            p,
            p.primary,
            0.19,
            0.14,
            0.85,
            1.4,
            lift(p.text, 0.45),
            1.4,
            1.0,
        ),
        active: neon_widget(
            p,
            p.secondary,
            0.27,
            0.19,
            1.0,
            1.6,
            Color32::WHITE,
            1.8,
            1.0,
        ),
        open: neon_widget(p, p.primary, 0.15, 0.11, 0.56, 1.0, p.text, 1.0, 0.0),
    }
}

fn neon_selection(p: &NeonPalette) -> Selection {
    Selection {
        bg_fill: pane(p.primary, 0.34),
        stroke: Stroke::new(1.0, edge(p.primary, 0.9)),
    }
}

fn neon_visuals(p: &NeonPalette) -> Visuals {
    Visuals {
        dark_mode: true,
        // None so text follows each state's fg_stroke and brightens on hover and press.
        override_text_color: None,
        widgets: neon_widgets(p),
        selection: neon_selection(p),
        hyperlink_color: lift(p.tertiary, 0.1),
        faint_bg_color: pane(p.primary, 0.045),
        // Sunken wells stay lifted off true black so text-edit interiors and troughs read.
        extreme_bg_color: pane_over(p.primary, 0.09, p.base),
        code_bg_color: pane_over(p.surface, 0.9, p.base),
        warn_fg_color: p.tertiary,
        error_fg_color: p.error,
        window_corner_radius: CornerRadius::same(WINDOW_RADIUS),
        window_shadow: Shadow {
            offset: [0, 0],
            blur: 26,
            spread: 3,
            color: pane(p.glow, 0.42),
        },
        window_fill: pane(p.surface, p.surface_alpha),
        window_stroke: Stroke::new(1.0, edge(p.primary, 0.55)),
        menu_corner_radius: CornerRadius::same(MENU_RADIUS),
        panel_fill: p.base,
        popup_shadow: Shadow {
            offset: [0, 0],
            blur: 18,
            spread: 2,
            color: pane(p.glow, 0.34),
        },
        resize_corner_size: 12.0,
        text_cursor: TextCursorStyle {
            stroke: Stroke::new(2.0, lift(p.tertiary, 0.4)),
            ..Default::default()
        },
        ime_composition: ImeComposition {
            active_underline_stroke: Stroke::new(2.0, lift(p.primary, 0.5)),
            inactive_underline_stroke: Stroke::new(2.0, pane(lift(p.primary, 0.5), 0.5)),
            ..Default::default()
        },
        clip_rect_margin: 3.0,
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

/// The full `Style` for a palette. Geometry, spacing and text styles are shared with the MTech
/// Glass family so the whole set feels like one system.
pub fn neon_style(p: &NeonPalette) -> Style {
    Style {
        text_styles: glass_text_styles(),
        spacing: glass_spacing(),
        interaction: glass_interaction(),
        visuals: neon_visuals(p),
        animation_time: 0.1,
        explanation_tooltips: false,
        url_in_tooltip: true,
        compact_menu_style: true,
        ..Default::default()
    }
}

/// The backdrop-blur material for a palette.
///
/// The tint is deliberately a whisper: the surface fill above it already carries the color and the
/// occlusion, so a heavier film here would only subtract from the blur it sits on.
pub fn neon_glass_params(p: &NeonPalette) -> GlassParams {
    let [r, g, b, _] = p.surface.to_srgba_unmultiplied();
    GlassParams {
        enabled: true,
        blur_radius: 26.0,
        tint: Color32::from_rgba_unmultiplied(r, g, b, 26),
        corner_radius: MENU_RADIUS as f32,
        presence: 1.0,
    }
}

/// Success and secondary-accent colors, which have no `egui::Visuals` slot.
pub fn neon_semantic_colors(p: &NeonPalette) -> (Color32, Color32) {
    (p.success, p.secondary)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Every palette must produce a theme that is actually AMOLED, actually translucent, and
    // actually glassy — the three things the family is for.
    #[test]
    fn every_palette_is_amoled_translucent_and_glassy() {
        for p in NEON_THEMES {
            let style = neon_style(&p);
            let v = &style.visuals;

            assert_eq!(v.panel_fill, Color32::BLACK, "{}: panels must be true black", p.name);
            assert!(
                v.window_fill.a() < 255,
                "{}: floating surfaces must be translucent",
                p.name
            );
            assert!(
                v.widgets.inactive.weak_bg_fill.a() < 255,
                "{}: widget panes must be translucent",
                p.name
            );
            assert!(
                neon_glass_params(&p).is_visible(),
                "{}: glass material must draw",
                p.name
            );
        }
    }

    // A dropdown inside a window is occluded only by its own fill, so the surface alpha is the
    // readability floor for the whole family. Below ~0.7 nested popups stop being legible.
    #[test]
    fn surface_alpha_keeps_nested_popups_readable() {
        for p in NEON_THEMES {
            assert!(
                (0.70..=0.92).contains(&p.surface_alpha),
                "{}: surface_alpha {} is outside the readable band",
                p.name,
                p.surface_alpha
            );
            let fill = neon_style(&p).visuals.window_fill;
            assert!(fill.a() >= 178, "{}: window fill alpha {} too sheer", p.name, fill.a());
        }
    }

    // Sunken wells (text-edit interiors, progress troughs) must not vanish into the black base.
    #[test]
    fn sunken_wells_stay_visible_over_true_black() {
        for p in NEON_THEMES {
            let extreme = neon_style(&p).visuals.extreme_bg_color;
            let [r, g, b, a] = extreme.to_srgba_unmultiplied();
            assert_eq!(a, 255, "{}: extreme_bg must be opaque", p.name);
            assert!(
                r as u32 + g as u32 + b as u32 > 12,
                "{}: extreme_bg {extreme:?} is indistinguishable from black",
                p.name
            );
        }
    }

    // Each theme's alarm color has to stand apart from its own accents, or errors read as chrome.
    #[test]
    fn error_is_distinguishable_from_the_accents() {
        fn distance(a: Color32, b: Color32) -> i32 {
            let (x, y) = (a.to_srgba_unmultiplied(), b.to_srgba_unmultiplied());
            (0..3)
                .map(|i| (x[i] as i32 - y[i] as i32).abs())
                .sum()
        }
        for p in NEON_THEMES {
            for (label, accent) in [("primary", p.primary), ("secondary", p.secondary)] {
                assert!(
                    distance(p.error, accent) > 90,
                    "{}: error is too close to {label}",
                    p.name
                );
            }
        }
    }

    #[test]
    fn palette_names_are_unique() {
        let mut names: Vec<&str> = NEON_THEMES.iter().map(|p| p.name).collect();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count, "palette names must be unique");
    }
}
