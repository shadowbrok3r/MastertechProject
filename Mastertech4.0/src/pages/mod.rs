use eframe::egui::Ui;
use egui_dock::DockArea;

use crate::app_state::{MastertechContext, MasterTechApp};
use displays::tabs::{TabContext, TabId, WorkMode};
use displays::ui_tools::toasts::{Toast, ToastKind, ToastOptions};

pub mod menu_bar;
pub mod work_mode;

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

        let mode = self.context.work_mode.active.unwrap_or_default();

        if !self.context.pending_tab_removes.is_empty() || !self.context.pending_tab_adds.is_empty()
        {
            for tab in self.context.pending_tab_removes.drain(..) {
                if let Some(index) = tree.find_tab(&tab) {
                    tree.remove_tab(index);
                }
            }
            for (path, tab) in std::mem::take(&mut self.context.pending_tab_adds) {
                if !mode.allows(tab) {
                    refuse(&mut self.context, mode, tab);
                    continue;
                }
                tree.set_focused_node_and_surface(path);
                tree.push_to_focused_leaf(tab);
            }
        }

        if let Some(request) = displays::ui_data::agent_session_notify::take_open_request() {
            self.context.shared_ctx.agent_sessions.open(request.thread, request.is_open);
            self.context.pending_tab_opens.push(TabId::AgentSessions);
            self.context.pending_activate_tab = Some(TabId::AgentSessions);
        }

        for tab in std::mem::take(&mut self.context.pending_tab_opens) {
            if !mode.allows(tab) {
                refuse(&mut self.context, mode, tab);
                continue;
            }
            if tree.find_tab(&tab).is_none() {
                tree.push_to_focused_leaf(tab);
            }
        }

        if let Some(tab) = self.context.pending_activate_tab.take() {
            if let Some(path) = tree.find_tab(&tab) {
                let _ = tree.set_active_tab(path);
            }
        }

        self.dock.tree = tree;
    }
}

/// Tells the operator a tab was refused rather than dropping the request silently.
fn refuse(context: &mut MastertechContext, mode: WorkMode, tab: TabId) {
    let title = tab.title(TabContext::MastertechNative);
    log::debug!("{} refused opening {title}", mode.title());
    context.shared_ctx.toasts.add(Toast {
        kind: ToastKind::Info,
        text: format!("{title} is not part of {}. Switch modes to open it.", mode.title()).into(),
        options: ToastOptions::default()
            .show_progress(true)
            .duration_in_seconds(6.0),
        ..Default::default()
    });
}
