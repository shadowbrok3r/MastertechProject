use database::schema::Notification;
use eframe::egui::{menu, Align, Context, Margin, Rounding, ScrollArea, Separator, TextEdit};
use eframe::egui::{Button, Color32, FontId, Layout, RichText, Stroke, TopBottomPanel, Widget};
use crate::utilities::ui_tools::autocomplete::AutoCompleteTextEdit;
use std::collections::BTreeSet;
use log::info;
use crate::{app_state::{AppState, MainPages, MtechServer}, utilities::TaskUiActions};

impl MtechServer{
    pub fn menu_bar(&mut self, ctx: &Context) {
        TopBottomPanel::top("egui_dock::MenuBar")
        .show(ctx, |ui| 
    {
        menu::bar(ui, |ui| {
            ui.with_layout(Layout::left_to_right(Align::Min), |ui|{
                ui.add_space(10.0);
                ui.menu_button("View", |ui| {
                    // allow certain tabs to be toggled
                    for tab in &[
                        &"Store Tasks".to_string(),
                        &"My Tasks".to_string(),
                        &"Terminal".to_string(),
                        &"Web Console".to_string(),
                        &"Completed Tasks".to_string(),
                        &"Bug Report".to_string(),
                        &"Ai Playground".to_string(),
                        // &"Bug Report".to_string()
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
                

                    for task in self.context.tasks.iter(){
                        inputs.insert(task.task_name.clone());
                        inputs.insert(format!("{}",task.service_number.clone().unwrap_or_default()));
                    }
                    ui.style_mut().visuals.widgets.inactive.bg_stroke = Stroke::new(2.0, Color32::from_rgb(50, 2, 43));
                    ui.visuals_mut().extreme_bg_color = Color32::from_rgb(12,12,14);
                    ui.visuals_mut().widgets.inactive.bg_fill = Color32::from_additive_luminance(100);
                    let result = AutoCompleteTextEdit::new(&mut self.context.search_input, inputs.clone())
                        .highlight_matches(true)
                        .max_suggestions(10)
                        .set_text_edit_properties(|text_edit: TextEdit<'_>| 
                    {
                        text_edit
                            .hint_text("Search for task")
                            .desired_width(150.0)
                            .font(FontId::proportional(12.0))
                            .frame(true)
                    }).ui(ui);

                    if result.clicked(){
                        info!("selected? {}", self.context.search_input.clone());
                        if let Some(input) = inputs.get(&self.context.search_input){
                            let task = self.context.tasks.iter().find(|&x| 
                                x.task_name == *input || format!("{}",x.service_number.clone().unwrap_or_default()) == format!("{}",*input)
                            );

                            if let Some(task) = task{
                                let _ = self.context.ui_actions_tx.try_send(TaskUiActions::OpenTaskModal(task.clone()));
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

                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.add_space(20.0);
                    let txt = RichText::new(usr.name.clone()).color(Color32::from_rgb(100,50,100));
                    ui.menu_button(txt, |ui| {
                        ui.set_width(300.0);
                        ui.set_height(600.0);
                        ui.vertical_centered_justified( |ui| {

                            if ui.add(Button::new("Account Settings")).clicked(){
                                
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
                        Separator::default().shrink(20.0).ui(ui);
                        ui.add_space(10.0);
                        ui.vertical_centered(|ui| ui.label(RichText::new("Notifications").heading()));

                        ui.horizontal_top(|ui| {
                            let read_button = ui.button(RichText::new("Read").color(Color32::LIGHT_GREEN));
                            ui.add_space(ui.available_width() - 50.0);
                            let unread_button = ui.button(RichText::new("Unread").color(Color32::LIGHT_RED));
                            if read_button.clicked() { self.context.read_notifications = true; }
                            if unread_button.clicked() { self.context.read_notifications = false; }
                        });

                        ScrollArea::vertical()
                            .auto_shrink(false)
                            .show(ui, |ui| 
                        {
                            

                            let mut notifications: Vec<Notification> = if self.context.read_notifications { 
                                self.context.notifications.iter().filter(|n| n.status == "Read" ).cloned().collect() 
                            } else { 
                                self.context.notifications.iter().filter(|n| n.status == "Unread" ).cloned().collect() 
                            };

                            for notification in notifications.iter_mut(){
                                eframe::egui::Frame::none().fill(ui.style().visuals.extreme_bg_color)
                                    .rounding(Rounding::same(12.0))
                                    .inner_margin(Margin::same(10.0))
                                    .outer_margin(Margin::same(5.0))
                                    .stroke(Stroke::new(0.5, if notification.status == "Read" { Color32::LIGHT_GREEN } else { Color32::LIGHT_RED }))
                                    .show(ui, |ui| 
                                {
                                    ui.horizontal_top( |ui| {
                                        let w = 250.0;
                                        ui.set_width(w);
                                        ui.add_space(w / 3.0);
                                        ui.colored_label(Color32::LIGHT_RED, RichText::new(notification.notification_type.clone()).font(FontId::proportional(12.0)));
                                        ui.add_space(80.0);
                                        let button = Button::new(RichText::new("X").color(Color32::LIGHT_RED)).ui(ui);
                                        if button.clicked(){ 
                                            notification.status = "Read".to_string(); 
                                            // let id = notification
                                            // spawn_local(async move {
                                            //     let _x: Option<Record> = DATABASE.query("UPDATE notification SET status = 'Read' WHERE id == $id")
                                            //         // .bind(("id", id.clone()))
                                            //         .await.unwrap().take(0).unwrap();
                                            // });
                                        }
                                    });

                                    eframe::egui::Frame::none().fill(ui.style().visuals.window_fill)
                                        .rounding(Rounding::same(12.0))
                                        .inner_margin(Margin::same(15.0))
                                        .outer_margin(Margin::same(5.0))
                                        .show(ui, |ui| 
                                    {
                                        ui.label(notification.notification_description.clone());
                                    }).inner;

                                }).inner;
                            }
                        });
                    });
                    ui.add_space(5.0);
                    ui.label("Welcome, ");
                });
            }else{
                ui.add(Button::new("Login"));
            }
        })
    });
    }
}

