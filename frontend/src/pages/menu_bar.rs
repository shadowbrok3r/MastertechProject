use eframe::egui::menu;
use eframe::egui::{Button, Color32, FontId, Layout, RichText, Stroke, TopBottomPanel, Widget};
use crate::utilities::ui_tools::autocomplete::AutoCompleteTextEdit;
use std::collections::BTreeSet;
use log::info;
use crate::{app_state::{AppState, MainPages, MtechServer}, utilities::TaskUiActions};

impl MtechServer{
    pub fn menu_bar(&mut self, ctx: &egui::Context) {
        TopBottomPanel::top("egui_dock::MenuBar")
        .show(ctx, |ui| 
    {
        menu::bar(ui, |ui| {
            ui.with_layout(Layout::left_to_right(egui::Align::Min), |ui|{
                ui.add_space(10.0);
                ui.menu_button("View", |ui| {
                    // allow certain tabs to be toggled
                    for tab in &[
                        &"Store Tasks".to_string(),
                        &"My Tasks".to_string(),
                        &"Terminal".to_string(),
                        &"Web Console".to_string(),
                        &"Completed Tasks".to_string(),
                        &"Bug Report".to_string()
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

                ui.add_space(30.0);
                let mut inputs = BTreeSet::new();
                
                if let Some(tasks) = &self.context.tasks{
                    for task in tasks.iter(){
                        inputs.insert(task.task_name.clone());
                        inputs.insert(format!("{}",task.service_number.clone().unwrap_or_default()));
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
                                x.task_name == *input || format!("{}",x.service_number.clone().unwrap_or_default()) == format!("{}",*input)
                            );

                            if let Some(task) = task{
                                let _ = self.context.ui_actions_tx.try_send(TaskUiActions::OpenTaskModal(task.clone()));
                            }
                        }
                    }
                }
            });

            if let Some(usr) = &self.context.current_user{
                ui.vertical_centered(|ui| {
                    if ui.add(Button::new(format!("Mastertech Server {}", env!("CARGO_PKG_VERSION")))).clicked(){
                        self.state = AppState::Authenticated(MainPages::Tasks);
                        match self.context.app_state_tx.try_send(AppState::Authenticated(MainPages::Tasks)){
                            Ok(_) => info!("AppState::Authenticated(MainPages::Tasks)"),
                            Err(e) => info!("Error: {e:?}"),
                        }
                    }
                });

                ui.with_layout(Layout::right_to_left(egui::Align::Max), |ui| {
                    ui.add_space(10.0);
                    let txt = RichText::new(format!("Welcome, {}", usr.name)).color(Color32::from_rgb(100,50,100));
                    ui.menu_button(txt, |ui| {
                        if ui.add(Button::new("Account Settings")).clicked(){
                        
                        }
                        if ui.add(Button::new("ChatGPT")).clicked(){
                            // self.state = AppState::Authenticated(MainPages::ChatGpt);
                            // match self.context.app_state_tx.try_send(AppState::Authenticated(MainPages::ChatGpt)){
                            //     Ok(_) => info!("Logged out"),
                            //     Err(e) => info!("Error: {e:?}"),
                            // }
                        }
                        if ui.add(Button::new("Downloads")).clicked(){
                            self.state = AppState::Authenticated(MainPages::Downloads);
                            match self.context.app_state_tx.try_send(AppState::Authenticated(MainPages::Downloads)){
                                Ok(_) => info!("Switching to Downloads Page"),
                                Err(e) => info!("Error: {e:?}"),
                            }
                        }
                        
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
                    });
                    
                    // if ui.add(Button::new("Web Console")).clicked(){
                    //     self.state = AppState::Authenticated(MainPages::WebConsole);
                    // }
                    

                });
            }else{
                ui.add(Button::new("Login"));
            }
        })
    });
    }
}