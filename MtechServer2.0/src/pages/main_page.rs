
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

        // self.added_nodes.drain(..).for_each(|(surface_idx, node_idx)| {
        //     self.tree.
        // });
    }
}