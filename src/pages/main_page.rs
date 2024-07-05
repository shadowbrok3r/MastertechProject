use eframe::egui::{Button, Context, Layout, ProgressBar, RichText, Stroke, Widget};
use eframe::egui::{CentralPanel, Color32, Frame, TopBottomPanel};
use egui_dock::{DockArea, Style as DockStyle};
use tokio::spawn;
use crate::app_state::MasterTechApp;
use crate::tabs::github::self_updater::run;


impl MasterTechApp {
    pub fn main_page(&mut self, ctx: &Context) {
        TopBottomPanel::top("egui_dock::MenuBar").show(ctx, |ui| 
        {
            eframe::egui::menu::bar(ui, |ui| {
                ui.menu_button("View", |ui| {
                    // allow certain tabs to be toggled
                    for tab in &[
                        &"TUR Sheet".to_string(),
                        &"Scripts".to_string(),
                        &"Console".to_string(),
                        &"System Information".to_string(),
                        &"File Browser 📂".to_string(),
                        &"Minidump Analysis".to_string(),
                        &"Profiler".to_string(),
                        &"QC".to_string(),
                        &"Tasks".to_string(),
                        &"Websockets".to_string()
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
                ui.with_layout(Layout::right_to_left(eframe::egui::Align::Max), |ui| {
                    if Button::new("Update Mastertech")
                        .stroke(
                            Stroke::new(0.5, Color32::LIGHT_RED)
                        )
                        .ui(ui)
                        .clicked()
                    {
                        let client = self.context.client.clone();
                        let tx = self.context.bytes_tx.clone();

                        spawn(async move {
                            let _ = run(client, tx.clone()).await;
                        });
                        
                        while let Ok(res) = self.context.bytes_rx.try_recv(){
                            self.context.output_text = format!("Downloaded Bytes: {}/{}", &res.0, &res.1);
                            
                            if res.0 == res.1{
                                self.context.output_text += "\nFinished";
                            }

                            let _ = ProgressBar::new(res.0 as f32 / res.1 as f32)
                                .show_percentage()
                                .fill(Color32::from_rgb(255, 77, 210))
                                .animate(true).ui(ui);
                        }
                    }
                    ui.add_space(20.0);
                    if let Some(usr) = self.context.current_user.as_ref(){
                        let welcome_msg = RichText::new(format!("Welcome, {}", usr.name));
                        ui.colored_label(Color32::from_rgb(100,50,100), welcome_msg);
                    }
                    if self.context.current_user.is_none(){
                        if Button::new("Login").ui(ui).clicked(){
                            let _ = self.context.app_state_tx.send(crate::app_state::AppState::NoAuth("Needs Login".to_string()));
                        }
                    }
                    ui.add_space(20.0);

                    if let Some(sys_id) = &self.context.computer_data.id{
                        ui.colored_label(Color32::LIGHT_RED, sys_id.0.id.to_raw());
                        ui.colored_label(Color32::WHITE, "Client ID: ");
                    }
                });
            })
        });
    
        CentralPanel::default() // When displaying a DockArea in another UI, it looks better
            .frame(Frame::central_panel(&ctx.style()).inner_margin(4.)) // to set inner margins to 0.
            .show(ctx, |ui| 
        {
            let mut style = self.context.style.get_or_insert(DockStyle::from_egui(ui.style())).clone();
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
                .show_tab_name_on_hover(false)
                .show_inside(ui, &mut self.context);
        });
    }
}