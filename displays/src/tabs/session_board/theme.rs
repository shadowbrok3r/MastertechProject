//! Graph styling, ported from the comfyui-android graphview/theme identity.
//!
//! Interaction grammar: rest = restrained dark glass, hover = aqua edge,
//! press/active/selected = pink. Edges are white, never coloured — a hued outline
//! is what stops a surface reading as glass.
//!
//! Applied to the board's `Ui` rather than the context, so the rest of the app
//! keeps its own theme.

use eframe::egui::{Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Ui};
use egui_snarl::ui::{BackgroundPattern, NodeLayout, PinPlacement, SnarlStyle, WireStyle};

pub const MIN_SCALE: f32 = 0.08;
pub const MAX_SCALE: f32 = 2.0;

/// Primary accent — anything active, open, or chosen.
pub const PINK: Color32 = Color32::from_rgb(255, 61, 139);
/// Lifted pink for ink and rings, where base pink reads dim on black.
pub const PINK_BRIGHT: Color32 = Color32::from_rgb(255, 110, 168);
/// Secondary accent — hover feedback and live markers.
pub const AQUA: Color32 = Color32::from_rgb(43, 226, 214);
pub const AQUA_BRIGHT: Color32 = Color32::from_rgb(120, 240, 232);
/// Third colour: ambient light, not an interaction signal.
pub const VIOLET: Color32 = Color32::from_rgb(163, 140, 255);

/// Pane edge — a dim white hairline, not a coloured one.
pub const RIM: Color32 = Color32::from_rgba_premultiplied(46, 46, 52, 46);
pub const RIM_BRIGHT: Color32 = Color32::from_rgba_premultiplied(72, 72, 80, 72);
/// Body ink — cool near-white on the black page.
pub const INK: Color32 = Color32::from_rgb(233, 233, 239);
const INK_BRIGHT: Color32 = Color32::from_rgb(248, 250, 252);

/// Node body: dark glass a step above the black canvas.
// Premultiplied because only that constructor is const; this is (20, 19, 32) at alpha 150.
pub const NODE_FILL: Color32 = Color32::from_rgba_premultiplied(12, 11, 19, 150);
pub const NODE_CORNER: f32 = 8.0;

/// Canvas dot-grid spacing in graph units, so it scales with the nodes.
pub const DOT_SPACING: f32 = 28.0;
pub const DOT_RADIUS: f32 = 1.7;
/// Dim teal ink; reads as a faint field because the dots are small.
pub const DOT_COLOR: Color32 = Color32::from_rgb(30, 70, 74);

pub const CANVAS: Color32 = Color32::from_rgb(3, 3, 5);

/// Node title size — large enough to stay legible a couple of zoom steps out.
pub const TITLE_SIZE: f32 = 16.0;
pub const SUBJECT_SIZE: f32 = 14.5;
pub const META_SIZE: f32 = 11.5;

fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

pub fn style() -> SnarlStyle {
    let mut s = SnarlStyle::new();
    s.bg_frame = Some(eframe::egui::Frame::new().fill(CANVAS));
    s.bg_pattern = Some(BackgroundPattern::NoPattern);
    s.min_scale = Some(MIN_SCALE);
    s.max_scale = Some(MAX_SCALE);
    // Fitting 49 sessions into the viewport zooms past legibility; open at 1:1 on the
    // most-outstanding band instead and let wheel-zoom do the rest.
    s.centering = Some(false);
    // Orthogonal wires with rounded corners read as a network diagram, not droopy beziers.
    s.wire_style = Some(WireStyle::AxisAligned { corner_radius: 8.0 });
    s.wire_width = Some(2.6);
    s.pin_placement = Some(PinPlacement::Outside { margin: 3.0 });
    s.pin_size = Some(15.0);
    // Coil keeps the body one full-width column; sandwich collapsed wrapped labels
    // into single-character strips here.
    s.node_layout = Some(NodeLayout::coil());
    s
}

/// Applies the palette, spacing, and text sizes to the board's `Ui` only.
pub fn apply(ui: &mut Ui) {
    let v = ui.visuals_mut();
    v.override_text_color = Some(INK);
    v.panel_fill = CANVAS;
    v.window_fill = rgba(19, 17, 30, 200);
    v.window_stroke = Stroke::new(1.2, RIM);
    v.faint_bg_color = Color32::from_rgb(11, 10, 16);
    v.extreme_bg_color = Color32::from_rgb(8, 7, 13);
    v.code_bg_color = Color32::from_rgb(6, 5, 10);
    v.hyperlink_color = AQUA;
    v.warn_fg_color = AQUA_BRIGHT;
    v.error_fg_color = PINK;
    v.selection.bg_fill = rgba(255, 61, 139, 140);
    v.selection.stroke = Stroke::new(1.4, PINK_BRIGHT);
    v.window_corner_radius = CornerRadius::same(8);
    v.menu_corner_radius = CornerRadius::same(8);
    // Text is copied with the per-item button, so drag-select never fights node dragging.
    v.window_shadow = eframe::egui::epaint::Shadow {
        offset: [0, 2],
        blur: 12,
        spread: 2,
        color: rgba(0, 0, 0, 200),
    };
    v.popup_shadow = eframe::egui::epaint::Shadow {
        offset: [0, 2],
        blur: 10,
        spread: 1,
        color: rgba(0, 0, 0, 170),
    };
    widget_palette(&mut ui.visuals_mut().widgets);

    let style = ui.style_mut();
    style.interaction.selectable_labels = false;
    style.spacing.item_spacing = eframe::egui::vec2(6.0, 5.0);
    style.spacing.button_padding = eframe::egui::vec2(7.0, 4.0);
    style.text_styles = [
        (TextStyle::Heading, FontId::new(18.0, FontFamily::Proportional)),
        (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(13.5, FontFamily::Proportional)),
        (TextStyle::Small, FontId::new(META_SIZE, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(13.0, FontFamily::Monospace)),
    ]
    .into();
}

/// Rest = restrained dark glass, hover = aqua edge, press/active = pink.
fn widget_palette(w: &mut eframe::egui::style::Widgets) {
    let radius = CornerRadius::same(5);

    w.noninteractive.bg_fill = rgba(18, 16, 28, 132);
    w.noninteractive.weak_bg_fill = rgba(14, 12, 22, 120);
    w.noninteractive.bg_stroke = Stroke::new(1.0, RIM);
    w.noninteractive.fg_stroke = Stroke::new(1.0, INK);
    w.noninteractive.corner_radius = radius;

    w.inactive.bg_fill = rgba(31, 28, 47, 165);
    w.inactive.weak_bg_fill = rgba(25, 23, 38, 150);
    w.inactive.bg_stroke = Stroke::new(1.0, RIM_BRIGHT);
    w.inactive.fg_stroke = Stroke::new(1.0, INK);
    w.inactive.corner_radius = radius;

    w.hovered.bg_fill = rgba(43, 226, 214, 42);
    w.hovered.weak_bg_fill = rgba(43, 226, 214, 42);
    w.hovered.bg_stroke = Stroke::new(1.5, rgba(43, 226, 214, 240));
    w.hovered.fg_stroke = Stroke::new(1.5, INK_BRIGHT);
    w.hovered.corner_radius = radius;

    w.active.bg_fill = rgba(255, 61, 139, 54);
    w.active.weak_bg_fill = rgba(255, 61, 139, 54);
    w.active.bg_stroke = Stroke::new(1.7, rgba(255, 61, 139, 245));
    w.active.fg_stroke = Stroke::new(2.0, Color32::WHITE);
    w.active.corner_radius = radius;

    w.open.bg_fill = rgba(31, 28, 47, 165);
    w.open.weak_bg_fill = rgba(25, 23, 38, 150);
    w.open.bg_stroke = Stroke::new(1.3, rgba(43, 226, 214, 205));
    w.open.fg_stroke = Stroke::new(1.0, INK);
    w.open.corner_radius = radius;
}

/// Glass material for node bodies: a dark smoked film that leaves most of the blur
/// visible. A light tint would wash the blur out and drop label contrast.
pub fn node_glass() -> crate::ui_tools::glass_backdrop::GlassParams {
    crate::ui_tools::glass_backdrop::GlassParams {
        enabled: true,
        blur_radius: 24.0,
        tint: Color32::from_rgba_unmultiplied(11, 9, 19, 92),
        corner_radius: NODE_CORNER,
        presence: 1.0,
    }
}

/// Below this zoom a node is too small for the blur to read, and there are too many.
pub const MIN_FROST_SCALE: f32 = 0.5;
/// Cap on frosted panes per frame; each one is a grab-pass.
pub const MAX_FROST_PANES: usize = 64;

/// Three pools of coloured light, so the canvas reads as lit rather than flat black.
/// A blur can only reveal what is behind it, and this page is black.
pub fn ambience(painter: &eframe::egui::Painter, rect: eframe::egui::Rect, ring_alpha: u8) {
    let d = rect.width().min(rect.height()).max(1.0);
    for (fx, fy, fr, color) in [
        (0.12, 0.14, 0.46, VIOLET),
        (0.94, 0.38, 0.38, AQUA),
        (0.46, 0.97, 0.42, PINK),
    ] {
        light_pool(
            painter,
            rect.lerp_inside(eframe::egui::vec2(fx, fy)),
            d * fr,
            color,
            ring_alpha,
        );
    }
}

/// One pool: nested discs of a constant low alpha, largest first.
fn light_pool(
    painter: &eframe::egui::Painter,
    center: eframe::egui::Pos2,
    radius: f32,
    color: Color32,
    ring_alpha: u8,
) {
    const RINGS: usize = 16;
    let fill = rgba(color.r(), color.g(), color.b(), ring_alpha);
    for i in 0..RINGS {
        let t = 1.0 - i as f32 / RINGS as f32;
        painter.circle_filled(center, radius * t, fill);
    }
}
