//! Graph styling, matching the comfyui-android graphview identity.

use eframe::egui::Color32;
use egui_snarl::ui::{BackgroundPattern, NodeLayout, PinPlacement, SnarlStyle, WireStyle};

pub const MIN_SCALE: f32 = 0.05;
pub const MAX_SCALE: f32 = 2.5;

/// Primary signal: anything selected or wanting attention.
pub const PINK: Color32 = Color32::from_rgb(255, 61, 139);
pub const PINK_BRIGHT: Color32 = Color32::from_rgb(255, 110, 168);
/// Secondary signal: settled, confirmed, done.
pub const AQUA: Color32 = Color32::from_rgb(43, 226, 214);
/// Third colour, not an interaction signal.
pub const VIOLET: Color32 = Color32::from_rgb(163, 140, 255);

/// Dim white hairline every surface carries.
pub const RIM_BRIGHT: Color32 = Color32::from_rgba_premultiplied(72, 72, 80, 72);
/// Node body: dark glass a step above the black canvas.
pub const NODE_FILL: Color32 = Color32::from_rgba_premultiplied(16, 16, 25, 190);
pub const NODE_CORNER: f32 = 8.0;

/// Canvas dot-grid spacing in graph units, so it scales with the nodes.
pub const DOT_SPACING: f32 = 28.0;
pub const DOT_RADIUS: f32 = 1.7;
/// Dim teal ink; reads as a faint field because the dots are small.
pub const DOT_COLOR: Color32 = Color32::from_rgb(30, 70, 74);

pub const CANVAS: Color32 = Color32::from_rgb(3, 3, 5);

pub fn style() -> SnarlStyle {
    let mut s = SnarlStyle::new();
    s.bg_frame = Some(eframe::egui::Frame::new().fill(CANVAS));
    s.bg_pattern = Some(BackgroundPattern::NoPattern);
    s.min_scale = Some(MIN_SCALE);
    s.max_scale = Some(MAX_SCALE);
    s.centering = Some(true);
    // Orthogonal wires with rounded corners read as a network diagram, not droopy beziers.
    s.wire_style = Some(WireStyle::AxisAligned { corner_radius: 8.0 });
    s.wire_width = Some(2.6);
    s.pin_placement = Some(PinPlacement::Outside { margin: 3.0 });
    s.pin_size = Some(15.0);
    // Stacks inputs above outputs so output labels stop adding dead horizontal weight.
    s.node_layout = Some(NodeLayout::sandwich());
    s
}
