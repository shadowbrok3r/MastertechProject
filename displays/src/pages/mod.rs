use eframe::egui::*;
pub mod account_settings;
pub mod signup_page;
pub mod login_page;
pub mod menu_bar;

impl crate::app_state::SharedContext {
    pub fn main_page(&mut self, ctx: &Context){ // , tab_viewer: &mut impl egui_dock::TabViewer<Tab = String>
       let mut style = egui_dock::Style::from_egui(&ctx.style());
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
        let mut tree = std::mem::replace(
            &mut self.tree,
            egui_dock::DockState::new(Vec::<String>::new()),
        );

        egui_dock::DockArea::new(&mut tree)
            .style(style)
            .show_close_buttons(true)
            .show_add_buttons(true)
            .show_add_popup(true)
            .draggable_tabs(true)
            .show(ctx, self);

        // Apply any pending add/remove requests queued from TabViewer to avoid double-borrowing
        if !self.pending_tab_removes.is_empty() || !self.pending_tab_adds.is_empty() {
            // First remove tabs
            for name in self.pending_tab_removes.drain(..) {
                if let Some(index) = tree.find_tab(&name) {
                    tree.remove_tab(index);
                }
                self.open_tabs.remove(&name);
            }
            // Then add tabs to focused leaf
            for (surface, node, name) in self.pending_tab_adds.drain(..) {
                // Focus the target location before pushing
                tree.set_focused_node_and_surface((surface, node));
                tree.push_to_focused_leaf(name.clone());
                self.open_tabs.insert(name);
            }
        }

        self.tree = tree;

    }
}