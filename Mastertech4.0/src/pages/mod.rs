use eframe::egui::{Color32, Context, Margin, Stroke};
use egui_dock::{DockArea, Style};

use crate::app_state::MasterTechApp;
use displays::tabs::TabId;

pub mod menu_bar;

impl MasterTechApp {
    pub fn main_page(&mut self, ctx: &Context) {
        let mut style = Style::from_egui(&ctx.style());
        style.overlay.selection_color = Color32::from_additive_luminance(255);
        style.separator.color_hovered = Color32::from_rgba_premultiplied(50, 93, 80, 77);
        style.separator.color_dragged = Color32::from_rgba_premultiplied(189, 189, 189, 130);
        style.buttons.add_tab_align = egui_dock::TabAddAlign::Left;
        style.main_surface_border_rounding.nw = 15;
        style.main_surface_border_rounding.ne = 15;
        style.buttons.close_tab_color = displays::ui_tools::theme::accent_secondary_ctx(ctx);
        style.tab_bar.hline_color = Color32::DARK_GRAY;
        style.separator.color_idle = Color32::DARK_GRAY;
        style.separator.extra_interact_width = 7.;
        style.separator.width = 1.3;
        style.main_surface_border_stroke = Stroke::new(0.25, Color32::TRANSPARENT);
        style.tab_bar.height = 20.0;
        style.tab.tab_body.inner_margin = Margin::same(1);
        style.tab.focused = egui_dock::TabInteractionStyle {
            outline_color: Color32::from_additive_luminance(std::u8::MAX),
            text_color: Color32::from_additive_luminance(std::u8::MAX),
            bg_fill: Color32::BLACK,
            ..Default::default()
        };

        let mut tree = std::mem::replace(
            &mut self.dock.tree,
            egui_dock::DockState::new(Vec::<TabId>::new()),
        );

        DockArea::new(&mut tree)
            .style(style)
            .show_close_buttons(true)
            .show_add_buttons(true)
            .show_add_popup(true)
            .draggable_tabs(true)
            .show(ctx, &mut self.context);

        if !self.context.pending_tab_removes.is_empty() || !self.context.pending_tab_adds.is_empty()
        {
            for tab in self.context.pending_tab_removes.drain(..) {
                if let Some(index) = tree.find_tab(&tab) {
                    tree.remove_tab(index);
                }
            }
            for (surface, node, tab) in self.context.pending_tab_adds.drain(..) {
                tree.set_focused_node_and_surface((surface, node));
                tree.push_to_focused_leaf(tab);
            }
        }

        for tab in self.context.pending_tab_opens.drain(..) {
            if tree.find_tab(&tab).is_none() {
                tree.push_to_focused_leaf(tab);
            }
        }

        if let Some(tab) = self.context.pending_activate_tab.take() {
            if let Some((surface_index, node_index, tab_index)) = tree.find_tab(&tab) {
                tree.set_active_tab((surface_index, node_index, tab_index));
            }
        }

        self.dock.tree = tree;
    }
}
