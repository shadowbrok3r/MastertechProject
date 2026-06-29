use eframe::egui::Ui;
use egui_dock::DockArea;

use crate::app_state::MasterTechApp;
use displays::tabs::TabId;

pub mod menu_bar;

impl MasterTechApp {
    pub fn main_page(&mut self, ui: &mut Ui) {
        let style = displays::ui_tools::dock_style::style(ui.ctx());

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
            .show_inside(ui, &mut self.context);

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
