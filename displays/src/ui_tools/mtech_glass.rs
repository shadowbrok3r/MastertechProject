//! Tinted-glass theme: translucent accent panes over a near-black base.
//! Fills are low-opacity tints; each outline is the same tint lifted toward
//! white at higher opacity. Accent roles: violet primary, magenta secondary,
//! cyan tertiary.

use eframe::egui;
use eframe::egui::style::{
    HandleShape, ImeComposition, Interaction, ScrollStyle, Selection, Spacing, TextCursorStyle,
    WidgetVisuals, Widgets,
};
use eframe::egui::{Color32, CornerRadius, Margin, Shadow, Stroke, Vec2, Visuals};

use super::carl_dark::Aesthetix;

pub struct MtechGlass;

const TEXT: Color32 = Color32::from_rgb(232, 234, 240);
const BG_PANEL: Color32 = Color32::from_rgb(6, 6, 10);
const BG_FAINT: Color32 = Color32::from_rgb(13, 13, 20);
const BG_SUNKEN: Color32 = Color32::from_rgb(10, 10, 16);
const BG_CODE: Color32 = Color32::from_rgb(15, 14, 24);
const BG_WINDOW: Color32 = Color32::from_rgb(13, 12, 22);
const BORDER_BASE: Color32 = Color32::from_rgb(58, 56, 92);
const VIOLET: Color32 = Color32::from_rgb(126, 108, 224);
const VIOLET_BRIGHT: Color32 = Color32::from_rgb(158, 140, 255);
const MAGENTA: Color32 = Color32::from_rgb(232, 62, 128);
const CYAN: Color32 = Color32::from_rgb(80, 220, 255);
const EMERALD: Color32 = Color32::from_rgb(90, 220, 160);
const ERROR_PINK: Color32 = Color32::from_rgb(255, 84, 146);
const GLOW: Color32 = Color32::from_rgb(70, 50, 160);

const EDGE_LIFT: f32 = 0.32;
const WIDGET_RADIUS: u8 = 6;
const WINDOW_RADIUS: u8 = 8;

/// Lifts a color toward white in gamma space.
fn lift(color: Color32, t: f32) -> Color32 {
    let ch = |v: u8| (v as f32 + (255.0 - v as f32) * t).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(ch(color.r()), ch(color.g()), ch(color.b()))
}

/// Translucent pane of the given tint.
fn pane(tint: Color32, opacity: f32) -> Color32 {
    tint.gamma_multiply(opacity)
}

/// Pane pre-composited opaque over the given base so it occludes what is under it.
fn pane_over(tint: Color32, opacity: f32, base: Color32) -> Color32 {
    let p = pane(tint, opacity);
    let inv = 1.0 - p.a() as f32 / 255.0;
    Color32::from_rgb(
        (p.r() as f32 + base.r() as f32 * inv).round() as u8,
        (p.g() as f32 + base.g() as f32 * inv).round() as u8,
        (p.b() as f32 + base.b() as f32 * inv).round() as u8,
    )
}

/// Opaque hue of a possibly-premultiplied color.
fn tint_of(c: Color32) -> Color32 {
    let [r, g, b, _] = c.to_srgba_unmultiplied();
    Color32::from_rgb(r, g, b)
}

/// Max minus min channel of the unmultiplied color.
fn chroma(c: Color32) -> u8 {
    let [r, g, b, _] = c.to_srgba_unmultiplied();
    r.max(g).max(b) - r.min(g).min(b)
}

const TINT_CHROMA_MIN: u8 = 12;

/// Most chromatic of the state's own colors, else the style-wide accent.
fn state_tint(wv: &WidgetVisuals, fallback: Color32) -> Color32 {
    [wv.bg_stroke.color, wv.weak_bg_fill, wv.bg_fill]
        .into_iter()
        .map(tint_of)
        .max_by_key(|c| chroma(*c))
        .filter(|c| chroma(*c) >= TINT_CHROMA_MIN)
        .unwrap_or(fallback)
}

fn glassify_state(
    wv: &mut WidgetVisuals,
    base: Color32,
    fallback: Color32,
    fill: f32,
    weak_fill: f32,
    edge_opacity: f32,
) {
    let tint = state_tint(wv, fallback);
    wv.bg_fill = pane_over(tint, fill, base);
    wv.weak_bg_fill = pane(tint, weak_fill);
    wv.bg_stroke.color = edge(tint, edge_opacity);
    // Lift sub-pixel widths to a full pixel; epaint fades thin strokes toward transparent.
    if wv.bg_stroke.width < 1.0 {
        wv.bg_stroke.width = 1.0;
    }
}

/// Applies the tinted-glass treatment to an existing style without changing its palette:
/// widget fills become translucent panes of each state's own most chromatic color,
/// outlines become lifted, more opaque cuts of the same tint. Backgrounds, rounding,
/// spacing, fonts, and text colors are left untouched.
pub fn glassify(style: &eframe::egui::Style) -> eframe::egui::Style {
    let mut out = style.clone();
    let v = &mut out.visuals;
    let base = v.panel_fill;

    let fallback = [v.selection.bg_fill, v.hyperlink_color, v.selection.stroke.color]
        .into_iter()
        .map(tint_of)
        .max_by_key(|c| chroma(*c))
        .unwrap_or(VIOLET);

    glassify_state(&mut v.widgets.noninteractive, base, fallback, 0.055, 0.035, 0.26);
    glassify_state(&mut v.widgets.inactive, base, fallback, 0.11, 0.07, 0.42);
    glassify_state(&mut v.widgets.hovered, base, fallback, 0.19, 0.14, 0.72);
    glassify_state(&mut v.widgets.active, base, fallback, 0.24, 0.17, 0.90);
    glassify_state(&mut v.widgets.open, base, fallback, 0.15, 0.11, 0.58);

    let sel_tint = [v.selection.bg_fill, v.selection.stroke.color]
        .into_iter()
        .map(tint_of)
        .max_by_key(|c| chroma(*c))
        .filter(|c| chroma(*c) >= TINT_CHROMA_MIN)
        .unwrap_or(fallback);
    v.selection.bg_fill = pane(sel_tint, 0.32);
    v.selection.stroke.color = edge(sel_tint, 0.85);
    if v.selection.stroke.width < 1.0 {
        v.selection.stroke.width = 1.0;
    }

    // Floating windows become smoked glass; their outline is a lifted cut of its own tint.
    v.window_fill = pane(tint_of(v.window_fill), 0.93);
    let window_tint = Some(tint_of(v.window_stroke.color))
        .filter(|c| chroma(*c) >= TINT_CHROMA_MIN)
        .unwrap_or(fallback);
    v.window_stroke.color = edge(window_tint, 0.50);
    if v.window_stroke.width < 1.0 {
        v.window_stroke.width = 1.0;
    }

    out
}

/// Outline brighter and more opaque than the pane it wraps.
fn edge(tint: Color32, opacity: f32) -> Color32 {
    lift(tint, EDGE_LIFT).gamma_multiply(opacity)
}

fn glass_widget(
    tint: Color32,
    fill: f32,
    weak_fill: f32,
    edge_opacity: f32,
    edge_width: f32,
    fg: Color32,
    fg_width: f32,
) -> WidgetVisuals {
    WidgetVisuals {
        // Opaque composite: slider handles and checkbox interiors must occlude the rail/stripes.
        bg_fill: pane_over(tint, fill, BG_PANEL),
        weak_bg_fill: pane(tint, weak_fill),
        bg_stroke: Stroke::new(edge_width, edge(tint, edge_opacity)),
        corner_radius: CornerRadius::same(WIDGET_RADIUS),
        fg_stroke: Stroke::new(fg_width, fg),
        expansion: 0.0,
    }
}

pub fn glass_style() -> egui::Style {
    egui::Style {
        text_styles: glass_text_styles(),
        spacing: glass_spacing(),
        interaction: glass_interaction(),
        visuals: glass_visuals(),
        animation_time: 0.1,
        explanation_tooltips: false,
        url_in_tooltip: true,
        compact_menu_style: true,
        ..Default::default()
    }
}

fn glass_selection() -> Selection {
    Selection {
        bg_fill: pane(VIOLET, 0.32),
        stroke: Stroke::new(1.0, edge(VIOLET, 0.85)),
    }
}

fn glass_visuals() -> Visuals {
    Visuals {
        dark_mode: true,
        // None so text follows per-state fg_stroke (brightens on hover/press).
        override_text_color: None,
        widgets: Widgets {
            noninteractive: glass_widget(VIOLET, 0.055, 0.035, 0.26, 1.0, TEXT, 1.0),
            inactive: glass_widget(VIOLET, 0.11, 0.07, 0.42, 1.0, TEXT, 1.0),
            hovered: glass_widget(VIOLET, 0.19, 0.14, 0.72, 1.2, lift(TEXT, 0.5), 1.5),
            active: glass_widget(MAGENTA, 0.24, 0.17, 0.90, 1.2, Color32::WHITE, 2.0),
            open: glass_widget(VIOLET, 0.15, 0.11, 0.58, 1.0, TEXT, 1.0),
        },
        selection: glass_selection(),
        hyperlink_color: VIOLET_BRIGHT,
        faint_bg_color: pane(VIOLET, 0.05),
        extreme_bg_color: pane(VIOLET, 0.10),
        code_bg_color: pane(BG_CODE, 0.90),
        warn_fg_color: CYAN,
        error_fg_color: ERROR_PINK,
        window_corner_radius: CornerRadius::same(WINDOW_RADIUS),
        window_shadow: Shadow {
            offset: [0, 0],
            blur: 18,
            spread: 2,
            color: pane(GLOW, 0.35),
        },
        window_fill: pane(BG_WINDOW, 0.93),
        window_stroke: Stroke::new(1.0, edge(VIOLET, 0.50)),
        menu_corner_radius: CornerRadius::same(WINDOW_RADIUS),
        panel_fill: BG_PANEL,
        popup_shadow: Shadow {
            offset: [0, 0],
            blur: 12,
            spread: 1,
            color: pane(GLOW, 0.28),
        },
        resize_corner_size: 12.0,
        text_cursor: TextCursorStyle {
            stroke: Stroke::new(2.0, lift(VIOLET, 0.55)),
            ..Default::default()
        },
        ime_composition: ImeComposition {
            active_underline_stroke: Stroke::new(2.0, lift(VIOLET, 0.55)),
            inactive_underline_stroke: Stroke::new(2.0, lift(VIOLET, 0.55).gamma_multiply(0.5)),
            ..Default::default()
        },
        clip_rect_margin: 3.0,
        button_frame: true,
        collapsing_header_frame: true,
        indent_has_left_vline: true,
        striped: true,
        slider_trailing_fill: true,
        handle_shape: HandleShape::Rect { aspect_ratio: 0.75 },
        image_loading_spinners: true,
        ..Default::default()
    }
}

fn glass_text_styles() -> std::collections::BTreeMap<egui::TextStyle, egui::FontId> {
    use egui::FontFamily::{Monospace, Proportional};
    [
        (egui::TextStyle::Small, egui::FontId::new(9.0, Proportional)),
        (egui::TextStyle::Body, egui::FontId::new(13.0, Proportional)),
        (egui::TextStyle::Monospace, egui::FontId::new(13.0, Monospace)),
        (egui::TextStyle::Button, egui::FontId::new(13.0, Proportional)),
        (egui::TextStyle::Heading, egui::FontId::new(18.0, Proportional)),
    ]
    .into()
}

fn glass_spacing() -> Spacing {
    Spacing {
        item_spacing: Vec2::splat(3.0),
        window_margin: Margin::same(6),
        button_padding: Vec2 { x: 5.0, y: 2.0 },
        menu_margin: Margin::same(6),
        indent: 18.0,
        interact_size: Vec2 { x: 40.0, y: 18.0 },
        slider_width: 100.0,
        slider_rail_height: 8.0,
        combo_width: 100.0,
        text_edit_width: 280.0,
        icon_width: 14.0,
        icon_width_inner: 8.0,
        icon_spacing: 4.0,
        tooltip_width: 500.0,
        menu_width: 400.0,
        menu_spacing: 2.0,
        indent_ends_with_horizontal_line: false,
        combo_height: 200.0,
        scroll: glass_scroll(),
        ..Default::default()
    }
}

fn glass_scroll() -> ScrollStyle {
    ScrollStyle {
        floating: true,
        bar_width: 10.0,
        handle_min_length: 12.0,
        bar_inner_margin: 4.0,
        bar_outer_margin: 0.0,
        floating_width: 2.0,
        floating_allocated_width: 0.0,
        foreground_color: true,
        dormant_background_opacity: 0.0,
        active_background_opacity: 0.4,
        interact_background_opacity: 0.7,
        dormant_handle_opacity: 0.0,
        active_handle_opacity: 0.6,
        interact_handle_opacity: 1.0,
        ..Default::default()
    }
}

fn glass_interaction() -> Interaction {
    Interaction {
        interact_radius: 5.0,
        resize_grab_radius_side: 3.0,
        resize_grab_radius_corner: 10.0,
        show_tooltips_only_when_still: true,
        tooltip_delay: 0.5,
        tooltip_grace_time: 0.2,
        selectable_labels: true,
        multi_widget_text_select: true,
        ..Default::default()
    }
}

impl Aesthetix for MtechGlass {
    fn name(&self) -> &'static str {
        "MTech Glass"
    }

    fn primary_accent_color_visuals(&self) -> Color32 {
        VIOLET
    }

    fn secondary_accent_color_visuals(&self) -> Color32 {
        MAGENTA
    }

    fn bg_primary_color_visuals(&self) -> Color32 {
        BG_PANEL
    }

    fn bg_secondary_color_visuals(&self) -> Color32 {
        BG_FAINT
    }

    fn bg_triage_color_visuals(&self) -> Color32 {
        BG_SUNKEN
    }

    fn bg_auxiliary_color_visuals(&self) -> Color32 {
        BG_CODE
    }

    fn bg_contrast_color_visuals(&self) -> Color32 {
        BORDER_BASE
    }

    fn fg_primary_text_color_visuals(&self) -> Option<Color32> {
        Some(TEXT)
    }

    fn fg_success_text_color_visuals(&self) -> Color32 {
        EMERALD
    }

    fn fg_warn_text_color_visuals(&self) -> Color32 {
        CYAN
    }

    fn fg_error_text_color_visuals(&self) -> Color32 {
        ERROR_PINK
    }

    fn fg_info_color_visuals(&self) -> Color32 {
        VIOLET_BRIGHT
    }

    fn dark_mode_visuals(&self) -> bool {
        true
    }

    fn margin_style(&self) -> i8 {
        6
    }

    fn button_padding(&self) -> Vec2 {
        Vec2 { x: 5.0, y: 2.0 }
    }

    fn item_spacing_style(&self) -> f32 {
        3.0
    }

    fn scroll_bar_width_style(&self) -> f32 {
        10.0
    }

    fn rounding_visuals(&self) -> u8 {
        WIDGET_RADIUS
    }

    // Keeps the Colors-only variant's selection/focus ring on the glass palette.
    fn custom_selection_visual(&self) -> Selection {
        glass_selection()
    }

    fn custom_style(&self) -> egui::Style {
        glass_style()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glassify_keeps_base_geometry_and_derives_glass_states() {
        let mut style = egui::Style::default();
        style.visuals = egui::Visuals::dark();
        style.visuals.panel_fill = Color32::BLACK;
        style.visuals.widgets.hovered.bg_stroke.color = Color32::from_rgb(116, 109, 187);
        style.visuals.widgets.hovered.corner_radius = CornerRadius::same(2);

        let glass = glassify(&style);

        assert_eq!(glass.visuals.panel_fill, Color32::BLACK);
        assert_eq!(glass.text_styles, style.text_styles);
        assert_eq!(glass.spacing, style.spacing);
        assert_eq!(
            glass.visuals.widgets.hovered.corner_radius,
            CornerRadius::same(2),
        );
        assert_eq!(
            glass.visuals.override_text_color,
            style.visuals.override_text_color,
        );

        // Weak fills are translucent panes; outlines are brighter cuts of the same tint.
        let hovered = &glass.visuals.widgets.hovered;
        assert!(hovered.weak_bg_fill.a() < 255);
        assert!(hovered.bg_stroke.width > 0.0);
        let [fr, fg_, fb, _] = hovered.weak_bg_fill.to_srgba_unmultiplied();
        let [sr, sg, sb, _] = hovered.bg_stroke.color.to_srgba_unmultiplied();
        assert!(sr >= fr && sg >= fg_ && sb >= fb);
    }
}
