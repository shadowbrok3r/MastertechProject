use eframe::egui::menu;
use egui::{Button, CentralPanel, Color32, Frame, Layout, RichText, TopBottomPanel};
use egui_dock::{DockArea, Style as DockStyle};
use crate::app_state::{AppState, MainPages, MtechServer};

impl MtechServer{
    pub fn main_page(&mut self, ctx: &egui::Context){
        
        TopBottomPanel::top("top_panel")
            .show(ctx, |ui| 
        {
            menu::bar(ui, |ui| {
                if let Some(usr) = &self.context.current_user{
                    if ui.add(Button::new("MasterTech Server")).clicked(){

                    }
                    ui.add_space(50.0);
                    let welcome_msg = RichText::new(format!("Welcome, {}", usr.name));
                    ui.colored_label(Color32::from_additive_luminance(255), welcome_msg);
                    ui.with_layout(Layout::right_to_left(egui::Align::Max), |ui| {
                        if ui.add(Button::new("Logout")).clicked(){
                            self.state = AppState::NoAuth;
                        }
                        if ui.add(Button::new("Web Console")).clicked(){
                            self.state = AppState::Authenticated(MainPages::WebConsole);
                        }
                        if ui.add(Button::new("Downloads")).clicked(){
                            self.state = AppState::Authenticated(MainPages::Downloads);
                        }
                        if ui.add(Button::new("ChatGPT")).clicked(){
                            self.state = AppState::Authenticated(MainPages::ChatGpt);
                        }
                    });
                }else{
                    ui.add(Button::new("Login"));
                }

            });
        });

        TopBottomPanel::top("egui_dock::MenuBar")
            .show(ctx, |ui| 
        {
            menu::bar(ui, |ui| {
                ui.menu_button("View", |ui| {
                    // allow certain tabs to be toggled
                    for tab in &[
                        &"Store Tasks".to_string(),
                        &"My Tasks".to_string(),
                        &"Terminal".to_string(),
                        &"Web Console".to_string(),
                        &"Completed Tasks".to_string()
                    ] {
                        if ui
                            .selectable_label(self.context.open_tabs.contains(*tab), *tab)
                            .clicked()
                        {
                            if let Some(index) = self.tree.find_tab(&tab.to_string()) {
                                self.tree.remove_tab(index);
                                self.context.open_tabs.remove(*tab);
                            } else {
                                self.tree.push_to_focused_leaf(tab.to_string());
                            }
                            ui.close_menu();
                        }
                    }
                });
            })
        });
        
        CentralPanel::default()
            .frame(Frame::central_panel(&ctx.style()).inner_margin(1.))
            .show(ctx, |ui| 
        {
                let dock_style = DockStyle::from_egui(ui.style());
                let mut style = self.context.style.get_or_insert(dock_style).clone();
                style.overlay.selection_color = Color32::from_rgb(92,0,87);
                style.separator.color_hovered = Color32::from_rgba_premultiplied(50,93,80,77);
                style.separator.color_idle = Color32::from_rgba_premultiplied(17,17,33,5);
                style.separator.color_dragged = Color32::from_rgba_premultiplied(189,189,189,130);
                style.buttons.add_tab_align = egui_dock::TabAddAlign::Left;
                style.main_surface_border_rounding.nw = 15.0;
                style.main_surface_border_rounding.ne = 15.0;
                style.buttons.close_tab_color = Color32::from_rgba_premultiplied(118, 0, 129, 58);
                
                DockArea::new(&mut self.tree)
                    .style(style)
                    .show_close_buttons(true)
                    .show_add_buttons(true)
                    .show_add_popup(true)
                    .draggable_tabs(true)
                    .show_inside(ui, &mut self.context);
        });
    }
}