
use eframe::egui::{Color32, Context, Margin, Stroke};
use egui_dock::{DockArea, Style as DockStyle};
use crate::MtechServer;

impl MtechServer{
    pub fn main_page(&mut self, ctx: &Context){
        let mut style = DockStyle::from_egui(&ctx.style());
        style.overlay.selection_color = Color32::from_additive_luminance(255);
        style.separator.color_hovered = Color32::from_rgba_premultiplied(50,93,80,77);
        style.separator.color_dragged = Color32::from_rgba_premultiplied(189,189,189,130);
        style.buttons.add_tab_align = egui_dock::TabAddAlign::Left;
        style.main_surface_border_rounding.nw = 10;
        style.main_surface_border_rounding.ne = 10;
        style.buttons.close_tab_color = Color32::from_rgb(191, 33, 101);
        style.tab_bar.hline_color = Color32::DARK_GRAY;
        style.separator.color_idle = Color32::DARK_GRAY;
        style.main_surface_border_stroke = Stroke::new(0.25, Color32::TRANSPARENT);
        style.tab_bar.height = 20.0;
        style.tab.tab_body.inner_margin = Margin::same(1);
        style.tab.hline_below_active_tab_name = true;

        style.tab.focused = egui_dock::TabInteractionStyle {
            outline_color: Color32::from_additive_luminance(std::u8::MAX),
            text_color: Color32::from_additive_luminance(std::u8::MAX),
            bg_fill: Color32::BLACK,
            ..Default::default()
        };

        DockArea::new(&mut self.shared_ctx.tree)
            .style(style)
            .show_close_buttons(true)
            .show_add_buttons(true)
            .show_add_popup(true)
            .draggable_tabs(true)
            .show(ctx, &mut self);

        // Apply any pending add/remove requests queued from TabViewer/logic
        if !self.shared_ctx.pending_tab_removes.is_empty() || !self.shared_ctx.pending_tab_adds.is_empty() {
            // First remove tabs
            for name in self.shared_ctx.pending_tab_removes.drain(..) {
                if let Some(index) = self.shared_ctx.tree.find_tab(&name) {
                    self.shared_ctx.tree.remove_tab(index);
                }
                self.shared_ctx.open_tabs.remove(&name);
            }
            // Then add tabs to requested surface/node if provided, otherwise to focused leaf
            for (surface, node, name) in self.shared_ctx.pending_tab_adds.drain(..) {
                // Focus the target location before pushing
                self.shared_ctx.tree.set_focused_node_and_surface((surface, node));
                self.shared_ctx.tree.push_to_focused_leaf(name.clone());
                self.shared_ctx.open_tabs.insert(name);
            }
        }

        // Apply any pending tab activation now
        if let Some(name) = self.shared_ctx.pending_activate_tab.take() {
            if let Some((surface_index, node_index, tab_index)) = self.shared_ctx.tree.find_tab(&name) {
                self.shared_ctx.tree.set_active_tab((surface_index, node_index, tab_index));
            } else {
                log::warn!("(wasm) Pending activate tab '{}' not found after render", name);
            }
        }
    }
}