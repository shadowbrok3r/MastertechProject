//! Soft glass theme family: low-chroma glass over a near-black base, with outlined controls.
//!
//! One [`SoftPalette`] per theme drives everything — [`soft_style`] builds the `Style` and
//! [`soft_glass_params`] the backdrop-blur material — the same shape as the neon glass family.
//! What differs is where the color goes.
//!
//! Neon glass carries a saturated accent in every widget fill. Here the fills stay nearly
//! colorless and the *outline* does the work: [`SoftPalette::frame`] is a cool near-neutral that
//! draws a visible hairline around every control at rest, and only hover and press swap it for the
//! accent. That keeps the accent for state changes instead of spending it on chrome, so the whole
//! surface reads quieter while the buttons read as buttons.
//!
//! Base panels are a near-black cast toward each palette's hue rather than true black, and the
//! geometry is the tightest of the glass families: small radii, short buttons, narrow margins.

use eframe::egui::style::{
    HandleShape, ImeComposition, Interaction, Selection, Spacing, TextCursorStyle, WidgetVisuals,
    Widgets,
};
use eframe::egui::{Color32, CornerRadius, Margin, Shadow, Stroke, Style, Vec2, Visuals};

use super::glass_backdrop::GlassParams;
use super::mtech_glass::{edge, glass_interaction, glass_spacing, glass_text_styles, lift, pane, pane_over};

/// The colors one soft glass theme is generated from.
#[derive(Clone, Copy, Debug)]
pub struct SoftPalette {
    pub name: &'static str,
    /// Panel background: near-black, cast toward the palette's hue.
    pub base: Color32,
    /// Hue of every floating surface and sunken well.
    pub surface: Color32,
    /// How opaque floating surfaces are; the nested-popup readability dial.
    pub surface_alpha: f32,
    /// Resting outline of every control. Near-neutral, so a frame is visible without being loud.
    pub frame: Color32,
    /// Hover accent, links, selection.
    pub primary: Color32,
    /// Press accent.
    pub secondary: Color32,
    /// Warnings and highlights.
    pub tertiary: Color32,
    pub success: Color32,
    pub error: Color32,
    pub text: Color32,
    /// Shadow color under floating surfaces.
    pub shade: Color32,
}

const WIDGET_RADIUS: u8 = 3;
const WINDOW_RADIUS: u8 = 5;
const MENU_RADIUS: u8 = 4;

/// Opacity of the resting outline. The dial the family exists for: below ~0.5 controls stop
/// reading as controls, because the fills are too faint to bound them on their own.
const FRAME_OPACITY: f32 = 0.72;

/// Graphite neutral under muted violet — the quietest of the family.
pub const OBSIDIAN: SoftPalette = SoftPalette {
    name: "Obsidian Glass",
    base: Color32::from_rgb(8, 8, 10),
    surface: Color32::from_rgb(17, 17, 21),
    surface_alpha: 0.82,
    frame: Color32::from_rgb(108, 112, 138),
    primary: Color32::from_rgb(150, 134, 224),
    secondary: Color32::from_rgb(188, 124, 220),
    tertiary: Color32::from_rgb(126, 200, 230),
    success: Color32::from_rgb(128, 206, 168),
    error: Color32::from_rgb(236, 98, 132),
    text: Color32::from_rgb(226, 226, 234),
    shade: Color32::from_rgb(2, 2, 4),
};

/// Warm plum base under mauve, pressing to rose.
pub const VELVET: SoftPalette = SoftPalette {
    name: "Velvet Glass",
    base: Color32::from_rgb(12, 9, 12),
    surface: Color32::from_rgb(25, 18, 26),
    surface_alpha: 0.82,
    frame: Color32::from_rgb(124, 108, 126),
    primary: Color32::from_rgb(182, 142, 206),
    secondary: Color32::from_rgb(226, 132, 158),
    // Scarlet rather than rose: it has to alarm apart from the rose press accent.
    tertiary: Color32::from_rgb(150, 196, 220),
    success: Color32::from_rgb(142, 206, 164),
    error: Color32::from_rgb(238, 88, 96),
    text: Color32::from_rgb(234, 228, 236),
    shade: Color32::from_rgb(5, 2, 5),
};

/// Deep indigo under periwinkle, pressing to soft magenta.
pub const TWILIGHT: SoftPalette = SoftPalette {
    name: "Twilight Glass",
    base: Color32::from_rgb(8, 9, 14),
    surface: Color32::from_rgb(17, 19, 31),
    surface_alpha: 0.82,
    frame: Color32::from_rgb(100, 108, 142),
    primary: Color32::from_rgb(138, 152, 232),
    secondary: Color32::from_rgb(196, 130, 224),
    tertiary: Color32::from_rgb(122, 198, 222),
    success: Color32::from_rgb(124, 208, 176),
    error: Color32::from_rgb(240, 110, 138),
    text: Color32::from_rgb(222, 226, 240),
    shade: Color32::from_rgb(2, 3, 7),
};

/// Cool smoke with the family's lightest surfaces, under lilac.
pub const QUARTZ: SoftPalette = SoftPalette {
    name: "Quartz Glass",
    base: Color32::from_rgb(14, 14, 17),
    surface: Color32::from_rgb(31, 31, 37),
    surface_alpha: 0.80,
    frame: Color32::from_rgb(128, 130, 152),
    primary: Color32::from_rgb(170, 158, 226),
    secondary: Color32::from_rgb(216, 140, 180),
    tertiary: Color32::from_rgb(140, 196, 214),
    success: Color32::from_rgb(140, 204, 172),
    error: Color32::from_rgb(238, 100, 120),
    text: Color32::from_rgb(232, 232, 238),
    shade: Color32::from_rgb(3, 3, 5),
};

/// Every palette in the family, in picker order.
pub const SOFT_THEMES: [SoftPalette; 4] = [OBSIDIAN, VELVET, TWILIGHT, QUARTZ];

/// One widget state: a faint pane of `fill_tint` over the base, bounded by an outline of
/// `edge_tint` — the two are separate so a resting control can be neutral-framed and color-free.
#[allow(clippy::too_many_arguments)]
fn soft_widget(
    p: &SoftPalette,
    fill_tint: Color32,
    edge_tint: Color32,
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
        bg_fill: pane_over(fill_tint, fill, p.base),
        weak_bg_fill: pane(fill_tint, weak_fill),
        bg_stroke: Stroke::new(edge_width, edge(edge_tint, edge_opacity)),
        corner_radius: CornerRadius::same(WIDGET_RADIUS),
        fg_stroke: Stroke::new(fg_width, fg),
        expansion,
    }
}

fn soft_widgets(p: &SoftPalette) -> Widgets {
    Widgets {
        // Group and separator chrome: framed, but well under a control's resting outline.
        noninteractive: soft_widget(p, p.primary, p.frame, 0.035, 0.025, 0.30, 1.0, p.text, 1.0, 0.0),
        inactive: soft_widget(p, p.primary, p.frame, 0.085, 0.055, FRAME_OPACITY, 1.0, p.text, 1.0, 0.0),
        hovered: soft_widget(
            p,
            p.primary,
            p.primary,
            0.16,
            0.12,
            0.95,
            1.4,
            lift(p.text, 0.4),
            1.4,
            1.0,
        ),
        active: soft_widget(
            p,
            p.secondary,
            p.secondary,
            0.24,
            0.18,
            1.0,
            1.6,
            Color32::WHITE,
            1.8,
            1.0,
        ),
        open: soft_widget(p, p.primary, p.primary, 0.13, 0.10, 0.70, 1.2, p.text, 1.0, 0.0),
    }
}

fn soft_selection(p: &SoftPalette) -> Selection {
    Selection {
        bg_fill: pane(p.primary, 0.30),
        stroke: Stroke::new(1.0, edge(p.primary, 0.85)),
    }
}

fn soft_visuals(p: &SoftPalette) -> Visuals {
    Visuals {
        dark_mode: true,
        // None so text follows each state's fg_stroke and brightens on hover and press.
        override_text_color: None,
        widgets: soft_widgets(p),
        selection: soft_selection(p),
        hyperlink_color: lift(p.primary, 0.15),
        faint_bg_color: pane(p.primary, 0.04),
        // Text-edit interiors and troughs sit a full step above the card tone, not a shade off the
        // base — a well that only just clears black reads as a hole, not as an input.
        extreme_bg_color: pane_over(lift(p.surface, 0.06), 0.95, p.base),
        code_bg_color: pane_over(p.surface, 0.90, p.base),
        warn_fg_color: p.tertiary,
        error_fg_color: p.error,
        window_corner_radius: CornerRadius::same(WINDOW_RADIUS),
        // Offset downward rather than glowing outward: an elevated card, not a neon sign.
        window_shadow: Shadow {
            offset: [0, 8],
            blur: 28,
            spread: 0,
            color: pane(p.shade, 0.60),
        },
        window_fill: pane(p.surface, p.surface_alpha),
        window_stroke: Stroke::new(1.0, edge(p.frame, 0.80)),
        menu_corner_radius: CornerRadius::same(MENU_RADIUS),
        panel_fill: p.base,
        popup_shadow: Shadow {
            offset: [0, 4],
            blur: 18,
            spread: 0,
            color: pane(p.shade, 0.50),
        },
        resize_corner_size: 12.0,
        text_cursor: TextCursorStyle {
            stroke: Stroke::new(2.0, lift(p.primary, 0.45)),
            ..Default::default()
        },
        ime_composition: ImeComposition {
            active_underline_stroke: Stroke::new(2.0, lift(p.primary, 0.45)),
            inactive_underline_stroke: Stroke::new(2.0, pane(lift(p.primary, 0.45), 0.5)),
            ..Default::default()
        },
        button_frame: true,
        collapsing_header_frame: true,
        indent_has_left_vline: true,
        striped: true,
        slider_trailing_fill: true,
        handle_shape: HandleShape::Rect { aspect_ratio: 0.6 },
        image_loading_spinners: true,
        ..Default::default()
    }
}

/// Tighter than the shared glass baseline everywhere it differs from it. `x` padding holds at the
/// baseline: this family outlines its controls, and a frame closer than that to the label reads as
/// a box drawn around the text rather than a button.
fn soft_spacing() -> Spacing {
    Spacing {
        item_spacing: Vec2 { x: 3.0, y: 2.0 },
        window_margin: Margin::same(4),
        button_padding: Vec2 { x: 5.0, y: 1.0 },
        menu_margin: Margin::same(5),
        indent: 14.0,
        ..glass_spacing()
    }
}

fn soft_interaction() -> Interaction {
    Interaction {
        tooltip_delay: 0.35,
        ..glass_interaction()
    }
}

/// The full `Style` for a palette.
pub fn soft_style(p: &SoftPalette) -> Style {
    Style {
        text_styles: glass_text_styles(),
        spacing: soft_spacing(),
        interaction: soft_interaction(),
        visuals: soft_visuals(p),
        animation_time: 0.12,
        explanation_tooltips: false,
        url_in_tooltip: true,
        compact_menu_style: true,
        ..Default::default()
    }
}

/// The backdrop-blur material for a palette: a wider smear than the neon family under a slightly
/// heavier film, which is what reads as frosted rather than merely translucent.
pub fn soft_glass_params(p: &SoftPalette) -> GlassParams {
    let [r, g, b, _] = p.surface.to_srgba_unmultiplied();
    GlassParams {
        enabled: true,
        blur_radius: 32.0,
        tint: Color32::from_rgba_unmultiplied(r, g, b, 34),
        corner_radius: MENU_RADIUS as f32,
        presence: 1.0,
    }
}

/// Success and secondary-accent colors, which have no `egui::Visuals` slot.
pub fn soft_semantic_colors(p: &SoftPalette) -> (Color32, Color32) {
    (p.success, p.secondary)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Max minus min channel: how much color a swatch actually carries.
    fn chroma(c: Color32) -> i32 {
        let [r, g, b, _] = c.to_srgba_unmultiplied();
        (r.max(g).max(b) as i32) - (r.min(g).min(b) as i32)
    }

    fn distance(a: Color32, b: Color32) -> i32 {
        let (x, y) = (a.to_srgba_unmultiplied(), b.to_srgba_unmultiplied());
        (0..3).map(|i| (x[i] as i32 - y[i] as i32).abs()).sum()
    }

    // The reason the family exists: a control must be bounded by a visible outline before it is
    // hovered, and the resting outline must not be an accent.
    #[test]
    fn a_resting_control_is_framed_and_uncolored() {
        for p in SOFT_THEMES {
            let w = soft_style(&p).visuals.widgets.inactive;

            assert!(w.bg_stroke.width >= 1.0, "{}: resting frame is sub-pixel", p.name);
            assert!(
                w.bg_stroke.color.a() >= 150,
                "{}: resting frame alpha {} is too sheer to read",
                p.name,
                w.bg_stroke.color.a(),
            );
            // The frame outreads the fill it wraps, which is what makes the control legible.
            assert!(
                w.bg_stroke.color.a() > w.weak_bg_fill.a(),
                "{}: fill is louder than the frame around it",
                p.name,
            );
            assert!(
                chroma(w.bg_stroke.color) < chroma(p.primary),
                "{}: resting frame is as chromatic as the accent",
                p.name,
            );
        }
    }

    // Hover and press are the only states that spend accent color, and each must be a visible
    // step up from the state before it.
    #[test]
    fn hover_and_press_escalate_the_outline() {
        for p in SOFT_THEMES {
            let w = &soft_style(&p).visuals.widgets;
            assert!(
                w.hovered.bg_stroke.width > w.inactive.bg_stroke.width,
                "{}: hover does not thicken the frame",
                p.name,
            );
            assert!(
                w.active.bg_stroke.width > w.hovered.bg_stroke.width,
                "{}: press does not thicken the frame",
                p.name,
            );
            assert!(
                chroma(w.hovered.bg_stroke.color) > chroma(w.inactive.bg_stroke.color),
                "{}: hover does not bring in the accent",
                p.name,
            );
        }
    }

    // Low chroma is the family's whole premise; a palette that drifts loud belongs in neon glass.
    #[test]
    fn accents_stay_below_the_neon_families_saturation() {
        for p in SOFT_THEMES {
            for (role, c) in [
                ("primary", p.primary),
                ("secondary", p.secondary),
                ("tertiary", p.tertiary),
                ("success", p.success),
            ] {
                assert!(
                    chroma(c) <= 120,
                    "{}: {role} chroma {} is neon, not soft",
                    p.name,
                    chroma(c),
                );
            }
        }
    }

    // A dropdown inside a window is occluded only by its own fill, so the surface alpha is the
    // readability floor for the whole family.
    #[test]
    fn surface_alpha_keeps_nested_popups_readable() {
        for p in SOFT_THEMES {
            assert!(
                (0.70..=0.92).contains(&p.surface_alpha),
                "{}: surface_alpha {} is outside the readable band",
                p.name,
                p.surface_alpha,
            );
            let fill = soft_style(&p).visuals.window_fill;
            assert!(fill.a() >= 178, "{}: window fill alpha {} too sheer", p.name, fill.a());
        }
    }

    // Sunken wells (text-edit interiors, progress troughs) must not vanish into the base.
    #[test]
    fn sunken_wells_stay_visible_over_the_base() {
        for p in SOFT_THEMES {
            let v = soft_style(&p).visuals;
            let [r, g, b, a] = v.extreme_bg_color.to_srgba_unmultiplied();
            assert_eq!(a, 255, "{}: extreme_bg must be opaque", p.name);
            let [br, bg, bb, _] = p.base.to_srgba_unmultiplied();
            let lift: i32 = (r as i32 - br as i32) + (g as i32 - bg as i32) + (b as i32 - bb as i32);
            assert!(lift > 40, "{}: extreme_bg sits flush with the base", p.name);
            // Above the card tone too, or an input inside a card disappears into it.
            assert!(
                v.extreme_bg_color.to_srgba_unmultiplied()[2] > p.surface.to_srgba_unmultiplied()[2],
                "{}: extreme_bg does not clear the surface tone",
                p.name,
            );
        }
    }

    // Each theme's alarm color has to stand apart from its own accents, or errors read as chrome.
    #[test]
    fn error_is_distinguishable_from_the_accents() {
        for p in SOFT_THEMES {
            for (label, accent) in [("primary", p.primary), ("secondary", p.secondary)] {
                assert!(
                    distance(p.error, accent) > 90,
                    "{}: error is too close to {label} ({})",
                    p.name,
                    distance(p.error, accent),
                );
            }
        }
    }

    // Every palette must produce a theme that is actually translucent and actually glassy.
    #[test]
    fn every_palette_is_translucent_and_glassy() {
        for p in SOFT_THEMES {
            let v = soft_style(&p).visuals;
            assert!(v.window_fill.a() < 255, "{}: surfaces must be translucent", p.name);
            assert!(
                v.widgets.inactive.weak_bg_fill.a() < 255,
                "{}: widget panes must be translucent",
                p.name,
            );
            assert!(soft_glass_params(&p).is_visible(), "{}: glass must draw", p.name);
        }
    }

    // This is the compact family. Every knob has to sit at or under the shared glass baseline, or
    // the next tuning pass quietly turns "soft" back into "roomy".
    #[test]
    fn geometry_stays_at_or_under_the_shared_glass_baseline() {
        let base = glass_spacing();
        let s = soft_spacing();

        assert!(s.item_spacing.y <= base.item_spacing.y, "row spacing grew");
        assert!(s.item_spacing.x <= base.item_spacing.x, "column spacing grew");
        assert!(s.button_padding.x <= base.button_padding.x, "button width grew");
        assert!(s.button_padding.y <= base.button_padding.y, "button height grew");
        assert!(s.window_margin.left <= base.window_margin.left, "window margin grew");
        assert!(s.menu_margin.left <= base.menu_margin.left, "menu margin grew");
        assert!(s.indent <= base.indent, "indent grew");
        assert!(s.interact_size.y <= base.interact_size.y, "controls got taller");

        assert!(WIDGET_RADIUS <= 4, "widget rounding {WIDGET_RADIUS} is no longer square");
        assert!(WINDOW_RADIUS <= 6, "window rounding {WINDOW_RADIUS} is no longer square");
        // The frosted rect is cut to the menu radius, so the two must not drift apart.
        for p in SOFT_THEMES {
            assert_eq!(soft_glass_params(&p).corner_radius, MENU_RADIUS as f32);
        }
    }

    #[test]
    fn palette_names_are_unique() {
        let mut names: Vec<&str> = SOFT_THEMES.iter().map(|p| p.name).collect();
        names.sort_unstable();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count, "palette names must be unique");
    }
}
