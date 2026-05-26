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
        let mut style = egui_dock::Style::from_egui(&ctx.global_style());
        style.overlay.selection_color = Color32::from_additive_luminance(255);
        style.separator.color_hovered = Color32::from_rgba_premultiplied(50, 93, 80, 77);
        style.separator.color_dragged = Color32::from_rgba_premultiplied(189, 189, 189, 130);
        style.buttons.add_tab_align = egui_dock::TabAddAlign::Left;
        style.main_surface_border_rounding.nw = 10;
        style.main_surface_border_rounding.ne = 10;
        style.buttons.close_tab_color = crate::ui_tools::theme::accent_secondary_ctx(ctx);
        style.tab_bar.hline_color = Color32::DARK_GRAY;
        style.separator.color_idle = Color32::DARK_GRAY;
        style.main_surface_border_stroke = Stroke::new(0.25_f32, Color32::TRANSPARENT);
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
