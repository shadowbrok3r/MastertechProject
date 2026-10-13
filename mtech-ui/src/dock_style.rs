//! egui_dock chrome: rounded cards on a gutter, with pill tabs, built from the published tokens.

use eframe::egui::{Color32, Context, CornerRadius, Margin, Stroke};
use egui_dock::{Style, TabAddAlign, TabInteractionStyle};

use crate::{metrics::Metrics, tokens::Tokens};

/// Gutter around and between dock cards.
const GUTTER: i8 = 8;
/// Width of the transparent drag strip between cards.
const SEPARATOR_W: f32 = 6.0;
/// Padding between the tab bar's edge and its pills.
const TAB_BAR_PAD_X: i8 = 6;
const TAB_BAR_PAD_Y: i8 = 4;
/// Gap between adjacent pills.
const TAB_GAP: f32 = 2.0;
const TAB_MIN_W: f32 = 44.0;
/// Inner margin of a tab body.
const BODY_MARGIN: Margin = Margin {
    left: 8,
    right: 8,
    top: 6,
    bottom: 8,
};
/// Opacity of the drop-target wash while a tab is dragged.
const DROP_WASH: f32 = 0.22;
/// Opacity of the hovered-leaf highlight.
const LEAF_WASH: f32 = 0.08;

/// Builds dock chrome from the published tokens and metrics.
pub fn style(ctx: &Context) -> Style {
    let t = Tokens::get(ctx);
    let m = Metrics::get(ctx);
    let mut dock = Style::from_egui(&ctx.global_style());
    let card = t.radius_md;
    let top = CornerRadius {
        nw: card,
        ne: card,
        ..CornerRadius::ZERO
    };
    let bottom = CornerRadius {
        sw: card,
        se: card,
        ..CornerRadius::ZERO
    };

    dock.dock_area_padding = Some(Margin::same(GUTTER));
    dock.main_surface_border_stroke = Stroke::NONE;
    dock.main_surface_border_rounding = CornerRadius::same(card);

    dock.separator.width = SEPARATOR_W;
    dock.separator.extra_interact_width = 2.0;
    dock.separator.color_idle = Color32::TRANSPARENT;
    dock.separator.color_hovered = t.primary_border;
    dock.separator.color_dragged = t.primary;

    dock.tab_bar.height = m.control_h + 2.0 * TAB_BAR_PAD_Y as f32;
    dock.tab_bar.inner_margin = Margin::symmetric(TAB_BAR_PAD_X, TAB_BAR_PAD_Y);
    dock.tab_bar.bg_fill = t.raised;
    dock.tab_bar.corner_radius = top;
    dock.tab_bar.hline_color = Color32::TRANSPARENT;
    dock.tab_bar.fill_tab_bar = false;

    dock.tab.minimum_width = Some(TAB_MIN_W);
    dock.tab.spacing = TAB_GAP;
    dock.tab.hline_below_active_tab_name = false;
    dock.tab.tab_body.inner_margin = BODY_MARGIN;
    dock.tab.tab_body.stroke = Stroke::NONE;
    dock.tab.tab_body.bg_fill = t.raised;
    dock.tab.tab_body.corner_radius = bottom;

    let pill = |bg_fill, text_color| TabInteractionStyle {
        outline_color: Color32::TRANSPARENT,
        bg_fill,
        text_color,
        corner_radius: CornerRadius::same(t.radius_sm),
    };
    let ring = |style: TabInteractionStyle| TabInteractionStyle {
        outline_color: t.primary_border,
        ..style
    };
    dock.tab.inactive = pill(Color32::TRANSPARENT, t.text_dim);
    dock.tab.hovered = pill(t.hover, t.text);
    dock.tab.active = TabInteractionStyle {
        outline_color: t.field_border,
        ..pill(t.field, t.text)
    };
    dock.tab.focused = pill(t.primary_soft, t.primary);
    dock.tab.inactive_with_kb_focus = ring(dock.tab.inactive.clone());
    dock.tab.active_with_kb_focus = ring(dock.tab.active.clone());
    dock.tab.focused_with_kb_focus = ring(dock.tab.focused.clone());

    let b = &mut dock.buttons;
    b.add_tab_align = TabAddAlign::Left;
    b.close_tab_color = t.text_faint;
    b.close_tab_active_color = t.text;
    b.close_tab_bg_fill = t.hover;
    b.add_tab_color = t.text_dim;
    b.add_tab_active_color = t.text;
    b.add_tab_bg_fill = t.hover;
    b.add_tab_border_color = Color32::TRANSPARENT;
    b.close_all_tabs_color = t.text_dim;
    b.close_all_tabs_active_color = t.text;
    b.close_all_tabs_bg_fill = t.hover;
    b.close_all_tabs_border_color = Color32::TRANSPARENT;
    b.collapse_tabs_color = t.text_dim;
    b.collapse_tabs_active_color = t.text;
    b.collapse_tabs_bg_fill = t.hover;
    b.collapse_tabs_border_color = Color32::TRANSPARENT;

    dock.overlay.selection_color = t.primary.gamma_multiply(DROP_WASH);
    dock.overlay.button_color = t.primary;
    dock.overlay.button_border_stroke = Stroke::new(1.0, t.primary_border);
    dock.overlay.hovered_leaf_highlight.color = t.primary.gamma_multiply(LEAF_WASH);
    dock.overlay.hovered_leaf_highlight.stroke = Stroke::new(1.0, t.primary_border);
    dock.overlay.hovered_leaf_highlight.corner_radius = CornerRadius::same(card);
    dock.overlay.hovered_leaf_highlight.expansion = 0.0;

    dock
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::{self, Density};
    use eframe::egui::Style as EguiStyle;

    fn context(density: Density) -> Context {
        let ctx = Context::default();
        metrics::set_density(&ctx, density);
        metrics::install_overlaid_style(&ctx, EguiStyle::default());
        ctx
    }

    #[test]
    fn pills_fill_the_tab_bar_less_its_padding() {
        for density in Density::ALL {
            let ctx = context(density);
            let dock = style(&ctx);
            let m = Metrics::for_density(density);
            let margin = dock.tab_bar.inner_margin;
            let pill_h = dock.tab_bar.height - (margin.top + margin.bottom) as f32;
            assert_eq!(pill_h, m.control_h, "{density:?}");
        }
    }

    #[test]
    fn cards_sit_on_a_transparent_gutter() {
        let ctx = context(Density::Regular);
        let dock = style(&ctx);
        let t = Tokens::get(&ctx);
        assert_eq!(dock.dock_area_padding, Some(Margin::same(GUTTER)));
        assert_eq!(dock.separator.color_idle, Color32::TRANSPARENT);
        assert_eq!(dock.tab_bar.bg_fill, dock.tab.tab_body.bg_fill);
        assert_eq!(dock.tab.tab_body.bg_fill, t.raised);
        let (bar, body) = (dock.tab_bar.corner_radius, dock.tab.tab_body.corner_radius);
        assert_eq!(
            (bar.nw, bar.ne, bar.sw, bar.se),
            (t.radius_md, t.radius_md, 0, 0)
        );
        assert_eq!(
            (body.nw, body.ne, body.sw, body.se),
            (0, 0, t.radius_md, t.radius_md)
        );
    }

    #[test]
    fn only_the_focused_tab_spends_the_primary_role() {
        let ctx = context(Density::Regular);
        let dock = style(&ctx);
        let t = Tokens::get(&ctx);
        let tabs = [&dock.tab.inactive, &dock.tab.hovered, &dock.tab.active];
        for tab in tabs {
            assert_ne!(tab.text_color, t.primary);
        }
        for tab in [&dock.tab.inactive, &dock.tab.hovered] {
            assert_eq!(tab.outline_color, Color32::TRANSPARENT);
        }
        assert_eq!(dock.tab.active.outline_color, t.field_border);
        assert_eq!(dock.tab.focused.text_color, t.primary);
        assert_eq!(dock.tab.inactive.bg_fill, Color32::TRANSPARENT);
    }
}
