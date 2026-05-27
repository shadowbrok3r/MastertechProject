//! egui_dock chrome derived from the active Mastertech theme.

use eframe::egui::{Color32, Context, CornerRadius, Margin, Stroke};
use egui_dock::{Style, TabAddAlign, TabInteractionStyle};

/// Builds dock chrome from the active egui theme and semantic accent colors.
pub fn style(ctx: &Context) -> Style {
    let egui_style = ctx.style();
    let visuals = &egui_style.visuals;
    let mut dock = Style::from_egui(&egui_style);

    let accent = visuals.selection.bg_fill;
    let accent2 = super::theme::accent_secondary_ctx(ctx);
    let success = super::theme::success_ctx(ctx);
    let border = visuals.window_stroke.color;
    let faint = visuals.faint_bg_color;
    let extreme = visuals.extreme_bg_color;
    let window = visuals.window_fill();
    let strong = visuals.strong_text_color();
    let weak = visuals.weak_text_color();
    let rounding = visuals.window_corner_radius;

    dock.main_surface_border_stroke = Stroke::NONE;
    dock.main_surface_border_rounding = CornerRadius {
        nw: rounding.nw,
        ne: rounding.ne,
        ..CornerRadius::ZERO
    };

    dock.separator.width = 1.0;
    dock.separator.extra_interact_width = 5.0;
    dock.separator.color_idle = border.gamma_multiply(0.35);
    dock.separator.color_hovered = accent2.gamma_multiply(0.75);
    dock.separator.color_dragged = accent;

    dock.tab_bar.height = 26.0;
    dock.tab_bar.bg_fill = extreme;
    dock.tab_bar.hline_color = border.gamma_multiply(0.45);
    dock.tab_bar.fill_tab_bar = false;
    dock.tab_bar.inner_margin = Margin {
        left: 6,
        right: 4,
        top: 0,
        bottom: 0,
    };

    dock.tab.hline_below_active_tab_name = true;
    dock.tab.spacing = 1.0;
    dock.tab.tab_body.inner_margin = Margin::same(2);
    dock.tab.tab_body.stroke = Stroke::NONE;
    dock.tab.tab_body.bg_fill = window;

    dock.tab.active.outline_color = Color32::TRANSPARENT;
    dock.tab.active.bg_fill = window;
    dock.tab.active.text_color = strong;

    dock.tab.inactive.outline_color = Color32::TRANSPARENT;
    dock.tab.inactive.text_color = weak;

    dock.tab.hovered.outline_color = accent2.gamma_multiply(0.35);
    dock.tab.hovered.text_color = strong;
    dock.tab.hovered.bg_fill = faint;

    let focused = TabInteractionStyle {
        outline_color: accent2,
        text_color: success,
        bg_fill: faint,
        corner_radius: dock.tab.active.corner_radius,
    };
    dock.tab.focused = focused.clone();
    dock.tab.focused_with_kb_focus = focused;

    dock.buttons.add_tab_align = TabAddAlign::Left;
    dock.buttons.close_tab_color = weak;
    dock.buttons.close_tab_active_color = accent2;
    dock.buttons.close_tab_bg_fill = Color32::TRANSPARENT;
    dock.buttons.add_tab_color = weak;
    dock.buttons.add_tab_active_color = accent;
    dock.buttons.add_tab_bg_fill = Color32::TRANSPARENT;

    dock.overlay.selection_color = accent.linear_multiply(0.4);
    dock.overlay.button_color = accent2;
    dock.overlay.hovered_leaf_highlight.color = accent.linear_multiply(0.12);
    dock.overlay.hovered_leaf_highlight.stroke = Stroke::new(1.0, accent2.gamma_multiply(0.55));
    dock.overlay.hovered_leaf_highlight.corner_radius = rounding;
    dock.overlay.hovered_leaf_highlight.expansion = 1.0;

    dock
}
