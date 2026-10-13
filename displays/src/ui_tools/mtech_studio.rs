//! MTech Studio: opaque near-black surfaces, one fill per widget state, and three accent roles.

use eframe::egui::style::{HandleShape, Selection, TextCursorStyle, WidgetVisuals, Widgets};
use eframe::egui::{Color32, CornerRadius, Shadow, Stroke, Style, Visuals};

use super::glass_backdrop::GlassParams;
use super::tokens::Tokens;

pub const NAME: &str = "MTech Studio";

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// Window and menu corner radius; the frost pane shares it.
const WINDOW_RADIUS: u8 = 10;
/// Opacity of floating windows over their frost.
const WINDOW_OPACITY: f32 = 0.9;
/// Fill of an open combo or menu button.
const OPEN: Color32 = rgb(27, 24, 38);
const CODE: Color32 = rgb(16, 16, 20);
/// Text of selected labels and rows.
const SELECTED_TEXT: Color32 = rgb(255, 170, 196);
/// Text of pressed controls.
const STRONG_TEXT: Color32 = rgb(246, 246, 250);
const SHADE: Color32 = Color32::from_black_alpha(140);
/// Film over the frost blur.
const GLASS_TINT: Color32 = Color32::from_rgba_premultiplied(5, 5, 7, 81);
const GLASS_BLUR: f32 = 32.0;

/// The preset's exact tokens; its style is built from them.
pub const TOKENS: Tokens = Tokens {
    base: rgb(8, 8, 10),
    sunken: rgb(5, 5, 7),
    raised: rgb(13, 13, 18),
    window: rgb(17, 17, 21),
    field: rgb(20, 18, 28),
    field_border: rgb(48, 46, 60),
    hover: rgb(31, 28, 44),
    pressed: rgb(53, 21, 39),
    separator: rgb(30, 30, 36),
    card_border: rgb(36, 36, 44),
    text: rgb(226, 226, 234),
    text_dim: rgb(150, 150, 160),
    text_faint: rgb(104, 104, 116),
    icon: rgb(192, 192, 202),
    primary: rgb(255, 42, 109),
    primary_soft: rgb(52, 14, 28),
    primary_border: rgb(173, 34, 81),
    on_primary: rgb(12, 12, 14),
    secondary: rgb(139, 124, 246),
    secondary_soft: rgb(32, 29, 52),
    tertiary: rgb(34, 211, 238),
    tertiary_soft: rgb(13, 45, 51),
    success: rgb(52, 211, 153),
    warn: rgb(251, 191, 36),
    error: rgb(255, 99, 71),
    info: rgb(34, 211, 238),
    radius_sm: 6,
    radius_md: WINDOW_RADIUS,
    radius_lg: WINDOW_RADIUS,
};

/// One widget state: a single opaque fill inside a 1 px outline.
fn state(fill: Color32, outline: Color32, fg: Color32) -> WidgetVisuals {
    WidgetVisuals {
        bg_fill: fill,
        weak_bg_fill: fill,
        bg_stroke: Stroke::new(1.0, outline),
        corner_radius: CornerRadius::same(TOKENS.radius_sm),
        fg_stroke: Stroke::new(1.0, fg),
        expansion: 0.0,
    }
}

fn studio_visuals() -> Visuals {
    let t = &TOKENS;
    let window_radius = CornerRadius::same(WINDOW_RADIUS);
    Visuals {
        dark_mode: true,
        override_text_color: None,
        weak_text_color: Some(t.text_dim),
        widgets: Widgets {
            noninteractive: state(t.raised, t.separator, t.text),
            inactive: state(t.field, t.field_border, t.text),
            hovered: state(t.hover, t.field_border, t.text),
            active: state(t.pressed, t.primary_border, STRONG_TEXT),
            open: state(OPEN, t.field_border, t.text),
        },
        selection: Selection {
            bg_fill: t.primary_soft,
            stroke: Stroke::new(1.0, SELECTED_TEXT),
        },
        hyperlink_color: t.info,
        faint_bg_color: t.raised,
        extreme_bg_color: t.sunken,
        text_edit_bg_color: Some(t.field),
        code_bg_color: CODE,
        warn_fg_color: t.warn,
        error_fg_color: t.error,
        window_corner_radius: window_radius,
        window_shadow: Shadow {
            offset: [0, 10],
            blur: 32,
            spread: 0,
            color: SHADE,
        },
        window_fill: t.window.gamma_multiply(WINDOW_OPACITY),
        window_stroke: Stroke::new(1.0, t.card_border),
        menu_corner_radius: window_radius,
        panel_fill: t.base,
        popup_shadow: Shadow {
            offset: [0, 6],
            blur: 20,
            spread: 0,
            color: SHADE,
        },
        text_cursor: TextCursorStyle {
            stroke: Stroke::new(2.0, t.primary),
            ..Default::default()
        },
        collapsing_header_frame: false,
        indent_has_left_vline: false,
        striped: false,
        slider_trailing_fill: true,
        handle_shape: HandleShape::Circle,
        ..Visuals::dark()
    }
}

/// The preset's `Style`, before the metrics overlay.
pub fn studio_style() -> Style {
    Style {
        visuals: studio_visuals(),
        animation_time: 0.12,
        explanation_tooltips: false,
        url_in_tooltip: true,
        compact_menu_style: true,
        ..Default::default()
    }
}

/// Backdrop-blur material behind floating windows.
pub fn studio_glass_params() -> GlassParams {
    GlassParams {
        enabled: true,
        blur_radius: GLASS_BLUR,
        tint: GLASS_TINT,
        corner_radius: WINDOW_RADIUS as f32,
        presence: 1.0,
    }
}

/// Success and secondary-accent colours, which have no `egui::Visuals` slot.
pub fn studio_semantic_colors() -> (Color32, Color32) {
    (TOKENS.success, TOKENS.secondary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_tools::tokens::contrast;

    fn surfaces() -> [(&'static str, Color32); 4] {
        let t = &TOKENS;
        [
            ("base", t.base),
            ("raised", t.raised),
            ("field", t.field),
            ("window", t.window),
        ]
    }

    fn assert_floor(name: &str, fg: Color32, floor: f32) {
        for (surface, bg) in surfaces() {
            let ratio = contrast(fg, bg);
            assert!(ratio >= floor, "{name} on {surface}: {ratio:.2} < {floor}");
        }
    }

    #[test]
    fn text_meets_its_contrast_floors() {
        assert_floor("text", TOKENS.text, 7.0);
        assert_floor("text_dim", TOKENS.text_dim, 4.5);
        assert_floor("text_faint", TOKENS.text_faint, 3.0);
        let on_primary = contrast(TOKENS.on_primary, TOKENS.primary);
        assert!(on_primary >= 4.5, "on_primary: {on_primary:.2}");
    }

    #[test]
    fn roles_and_status_read_as_text() {
        let t = &TOKENS;
        for (name, c) in [
            ("primary", t.primary),
            ("secondary", t.secondary),
            ("tertiary", t.tertiary),
            ("success", t.success),
            ("warn", t.warn),
            ("error", t.error),
        ] {
            assert_floor(name, c, 4.5);
        }
        assert!(contrast(SELECTED_TEXT, t.primary_soft) >= 7.0);
        assert!(contrast(STRONG_TEXT, t.pressed) >= 7.0);
    }

    #[test]
    fn error_is_not_the_primary() {
        let (p, e) = (TOKENS.primary, TOKENS.error);
        let distance: i32 = [(p.r(), e.r()), (p.g(), e.g()), (p.b(), e.b())]
            .iter()
            .map(|(a, b)| (*a as i32 - *b as i32).abs())
            .sum();
        assert!(distance >= 60, "error sits {distance} from primary");
        assert_eq!(TOKENS.info, TOKENS.tertiary);
    }

    #[test]
    fn every_token_is_opaque() {
        for (name, c) in TOKENS.colors() {
            assert_eq!(c.a(), 255, "{name} is translucent");
        }
    }

    #[test]
    fn states_share_one_fill_and_hover_changes_only_the_fill() {
        let v = studio_style().visuals;
        let w = &v.widgets;
        for state in [
            &w.noninteractive,
            &w.inactive,
            &w.hovered,
            &w.active,
            &w.open,
        ] {
            assert_eq!(state.bg_fill, state.weak_bg_fill);
            assert_eq!(state.bg_fill.a(), 255);
            assert_eq!(state.bg_stroke.width, 1.0);
            assert_eq!(state.fg_stroke.width, 1.0);
        }
        assert_eq!(w.inactive.bg_stroke.color, TOKENS.field_border);
        assert_eq!(w.hovered.bg_stroke, w.inactive.bg_stroke);
        assert_ne!(w.hovered.bg_fill, w.inactive.bg_fill);
        assert_eq!(w.active.bg_stroke.color, TOKENS.primary_border);
        assert_eq!(v.selection.bg_fill, TOKENS.primary_soft);
        assert_eq!(v.text_edit_bg_color, Some(TOKENS.field));
        assert_ne!(v.extreme_bg_color, TOKENS.field);
    }

    #[test]
    fn windows_menus_and_frost_share_one_radius() {
        let v = studio_style().visuals;
        assert_eq!(v.window_corner_radius, v.menu_corner_radius);
        assert_eq!(
            studio_glass_params().corner_radius,
            v.window_corner_radius.nw as f32
        );
        assert!(studio_glass_params().is_visible());
        assert!((220..255).contains(&v.window_fill.a()));
    }

    #[test]
    fn the_style_carries_the_tokens_it_was_built_from() {
        let (success, secondary) = studio_semantic_colors();
        let derived = Tokens::derive(&studio_style(), success, secondary);
        let t = &TOKENS;
        for (name, exact, got) in [
            ("base", t.base, derived.base),
            ("sunken", t.sunken, derived.sunken),
            ("raised", t.raised, derived.raised),
            ("field", t.field, derived.field),
            ("field_border", t.field_border, derived.field_border),
            ("hover", t.hover, derived.hover),
            ("pressed", t.pressed, derived.pressed),
            ("separator", t.separator, derived.separator),
            ("text", t.text, derived.text),
            ("text_dim", t.text_dim, derived.text_dim),
            ("secondary", t.secondary, derived.secondary),
            ("success", t.success, derived.success),
            ("warn", t.warn, derived.warn),
            ("error", t.error, derived.error),
            ("info", t.info, derived.info),
        ] {
            assert_eq!(exact, got, "{name}");
        }
        assert_eq!(derived.radius_sm, t.radius_sm);
        assert_eq!(derived.radius_md, t.radius_md);
        assert_eq!(derived.radius_lg, t.radius_lg);
    }
}
