use std::collections::BTreeSet;

use eframe::egui::menu;
use egui::{Button, CentralPanel, Color32, FontId, Frame, Layout, RichText, Stroke, TopBottomPanel, Widget};
use egui_autocomplete::AutoCompleteTextEdit;
use egui_dock::{DockArea, Style as DockStyle};
use log::info;
use crate::{app_state::{AppState, MainPages, MtechServer}, utilities::TaskUiActions};

impl MtechServer{
    pub fn main_page(&mut self, ctx: &egui::Context){
        
        TopBottomPanel::top("top_panel")
            .show(ctx, |ui| 
        {
            menu::bar(ui, |ui| {
                if let Some(usr) = &self.context.current_user{
                    if ui.add(Button::new("MasterTech Server")).clicked(){

                    }
                    ui.add_space(20.0);
                    let welcome_msg = RichText::new(format!("Welcome, {}", usr.name));
                    ui.colored_label(Color32::from_rgb(100,50,100), welcome_msg);
                    ui.with_layout(Layout::right_to_left(egui::Align::Max), |ui| {
                        if ui.add(Button::new("Logout")).clicked(){
                            wasm_cookies::delete("user");
                            wasm_cookies::delete("jwt");
                            let logout_msg = "Logged out".to_string();
                            self.state = AppState::NoAuth(logout_msg.clone());
                            match self.context.app_state_tx.try_send(AppState::NoAuth(logout_msg)){
                                Ok(_) => info!("Logged out"),
                                Err(e) => info!("Error: {e:?}"),
                            }
                        }
                        // if ui.add(Button::new("Web Console")).clicked(){
                        //     self.state = AppState::Authenticated(MainPages::WebConsole);
                        // }
                        if ui.add(Button::new("Downloads")).clicked(){
                            self.state = AppState::Authenticated(MainPages::Downloads);
                            match self.context.app_state_tx.try_send(AppState::Authenticated(MainPages::Downloads)){
                                Ok(_) => info!("Logged out"),
                                Err(e) => info!("Error: {e:?}"),
                            }
                        }
                        if ui.add(Button::new("ChatGPT")).clicked(){
                            self.state = AppState::Authenticated(MainPages::ChatGpt);
                            match self.context.app_state_tx.try_send(AppState::Authenticated(MainPages::ChatGpt)){
                                Ok(_) => info!("Logged out"),
                                Err(e) => info!("Error: {e:?}"),
                            }
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
                ui.with_layout(Layout::top_down(egui::Align::Center), |ui|{

                
                    let mut inputs = BTreeSet::new();
                    
                    if let Some(tasks) = &self.context.tasks{
                        for task in tasks.iter(){
                            inputs.insert(task.task_name.clone());
                            inputs.insert(format!("{}",task.service_number.unwrap_or(0)));
                        }
                        ui.style_mut().visuals.widgets.inactive.bg_stroke = Stroke::new(2.0, Color32::from_rgb(50, 2, 43));
                        ui.visuals_mut().extreme_bg_color = Color32::from_rgb(12,12,14);
                        ui.visuals_mut().widgets.inactive.bg_fill = Color32::from_additive_luminance(100);
                        let result = AutoCompleteTextEdit::new(&mut self.context.search_input, inputs.clone())
                            .highlight_matches(true)
                            .max_suggestions(10)
                            .set_text_edit_properties(|text_edit: egui::TextEdit<'_>| 
                        {
                            
                            text_edit
                                .hint_text("Search for task")
                                .desired_width(150.0)
                                .font(FontId::proportional(12.0))
                                .frame(true)
                                // .horizontal_align(egui::Align::Center)
                        })
                        .ui(ui);
                    
                        // result.
                        if result.clicked(){
                            info!("selected? {}", self.context.search_input.clone());
                            if let Some(input) = inputs.get(&self.context.search_input){
                                let task = tasks.iter().find(|&x| 
                                    x.task_name == *input || format!("{}",x.service_number.unwrap_or(0)) == format!("{}",*input)
                                );

                                if let Some(task) = task{
                                    let _ = self.context.ui_actions_tx.try_send(TaskUiActions::OpenTaskModal(task.clone()));
                                }
                            }
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
                
                // egui_dock
                DockArea::new(&mut self.tree)
                    .style(style)
                    // .
                    .show_close_buttons(true)
                    .show_add_buttons(true)
                    .show_add_popup(true)
                    .draggable_tabs(true)
                    .show_inside(ui, &mut self.context);
        });
    }
}