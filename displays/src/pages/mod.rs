use eframe::egui::*;
pub mod account_settings;
pub mod signup_page;
pub mod login_page;
pub mod menu_bar;

use crate::tabs::{TabContext, TabId};

pub fn view_menu(
    ui: &mut Ui,
    session: &mut crate::tabs::DockSession,
    tab_ctx: TabContext,
    mut anchor: Option<&mut dyn FnMut(TabId, Rect)>,
) {
    for &tab in TabId::visible_for(tab_ctx) {
        let label = tab.title(tab_ctx);
        let item = ui.selectable_label(session.is_open(tab), label);
        if let Some(push) = anchor.as_deref_mut() {
            push(tab, item.rect);
        }
        if item.clicked() {
            session.toggle(tab);
            ui.close_kind(UiKind::Menu);
        }
    }
}

impl crate::app_state::SharedContext {
    pub fn main_page(&mut self, ctx: &Context) {
        // Admin sessions must stay reachable for MCP/remote commands even
        // when the Admin Console tab is hidden or another client is focused.
        self.web_console_layout.pump_sessions(ctx);

        let style = crate::ui_tools::dock_style::style(ctx);
        let mut tree = std::mem::replace(
            &mut self.dock.tree,
            egui_dock::DockState::new(Vec::<TabId>::new()),
        );

        egui_dock::DockArea::new(&mut tree)
            .style(style)
            .show_close_buttons(true)
            .show_add_buttons(true)
            .show_add_popup(true)
            .draggable_tabs(true)
            .show(ctx, self);

        if !self.pending_tab_removes.is_empty() || !self.pending_tab_adds.is_empty() {
            for tab in self.pending_tab_removes.drain(..) {
                if let Some(index) = tree.find_tab(&tab) {
                    tree.remove_tab(index);
                }
            }
            for (surface, node, tab) in self.pending_tab_adds.drain(..) {
                tree.set_focused_node_and_surface((surface, node));
                tree.push_to_focused_leaf(tab);
            }
        }

        for tab in self.pending_tab_opens.drain(..) {
            if tree.find_tab(&tab).is_none() {
                tree.push_to_focused_leaf(tab);
            }
        }

        if let Some(tab) = self.pending_activate_tab.take() {
            if let Some((surface_index, node_index, tab_index)) = tree.find_tab(&tab) {
                tree.set_active_tab((surface_index, node_index, tab_index));
            }
        }

        self.dock.tree = tree;
    }
}
