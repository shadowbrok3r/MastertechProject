
use database::{schema::{utilities::{get_notifications, get_store_users, get_tasks_for_store, NotificationMod}, Notification, Store}, DATABASE};
use eframe::egui::{Button, Color32, ComboBox, Context, FontId, Frame, Layout, Margin, ProgressBar, RichText, ScrollArea, Separator, Stroke, TopBottomPanel, Vec2, Widget};
use crate::{app_state::{default_tree}, tabs::github::{get_github_releases, self_updater::run}};
use crate::app_state::MasterTechApp;
use displays::{app_state::{AppState, MainPages}, ui_tools::show_notification};
use std::collections::BTreeSet;
use log::{error, info};
use tokio::spawn;

impl MasterTechApp {
    pub fn menu_bar(&mut self, ctx: &Context) {
        let inputs = BTreeSet::new();
        TopBottomPanel::top("egui_dock::MenuBar").show(ctx, |ui| {
            eframe::egui::menu::bar(ui, |ui| {
                if let Some(usr) = self.context.shared_ctx.current_user.as_mut() {
                    ui.menu_button(RichText::new("View").color(ui.style().visuals.error_fg_color).heading().underline(), |ui| {
                        // allow certain tabs to be toggled
                        for tab in &[
                            &"TUR Sheet".to_string(),
                            &"Console".to_string(),
                            // &"Part Order".to_string(),
                            &"Scene Editor".to_string(),
                            &"Scripts".to_string(),
                            &"File Browser 📂".to_string(),
                            &"SysInfo".to_string(),
                            &"Minidump Analysis".to_string(),
                            &"Ai".to_string(),
                            &"Resource Monitor".to_string(),
                            // &"QC ☑️".to_string(),
                            &"My Tasks".to_string(),
                            &"Store Tasks".to_string(),
                            &"Completed Tasks".to_string(),
                            &"Bug Tracker".to_string(),
                            &"Websockets".to_string(),
                            &"Admin Console".to_string(),
                            &"My Tools".to_string(),
                            &"Store Stock".to_string(),
                            &"Task Audit".to_string(),
                            &"Company Stock".to_string(),
                            &"Downloads".to_string(),
                            &"Threads".to_string(),
                            &"Logs".to_string(),
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
                    
                    ui.with_layout(Layout::right_to_left(eframe::egui::Align::Center), |ui| {
                        ui.add_space(8.0);
                        let txt = RichText::new(usr.get_username()).color(Color32::from_rgb(191, 33, 101));

                        ui.menu_button(txt, |ui| {
                            ui.set_width(300.0);
                            ui.set_height(600.0);
                            ui.vertical_centered_justified(|ui| {
                                if Button::new(
                                    RichText::new("Update")
                                        .monospace()
                                        .font(FontId::proportional(14.0)),
                                )
                                .stroke(Stroke::new(0.5, Color32::LIGHT_RED))
                                .min_size(Vec2::new(36.0, 20.0))
                                .ui(ui)
                                .clicked()
                                {
                                    let client = self.context.client.clone();
                                    let tx = self.context.bytes_tx.clone();
        
                                    spawn(async move {
                                        let _ = run(client, tx.clone()).await;
                                    });
                                }

                                if Button::new(
                                    RichText::new("Terminal Mode")
                                        .monospace()
                                        .font(FontId::proportional(14.0)),
                                )
                                .stroke(Stroke::new(0.5, Color32::MAGENTA))
                                .min_size(Vec2::new(36.0, 20.0))
                                .ui(ui)
                                .clicked()
                                {
                                    let restart_in_terminal_mode = restart_in_terminal_mode();
                                    log::info!("restart_in_terminal_mode: {restart_in_terminal_mode:?}");
                                    ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Close);
                                }
                    
                                ui.add_space(10.0);
                                Separator::default().shrink(20.0).ui(ui);
                                ui.add_space(10.0);
                    
                                let selected = &mut self.context.shared_ctx.store_selection;
                                let current = selected.clone();
                        
                                let selected_text = match selected {
                                    76 => Store::RIV.as_str(),
                                    73 => Store::LTN.as_str(),
                                    74 => Store::MUR.as_str(),
                                    78 => Store::WJ.as_str(),
                                    75 => Store::ORE.as_str(),
                                    72 => Store::AF.as_str(),
                                    77 => Store::SAN.as_str(),
                                    _ => Store::RIV.as_str(),
                                };
                        
                                ui.horizontal(|ui| {
                                    ui.add_space(ui.available_width()/2.5);
                                    Frame::default().stroke(ui.style().visuals.window_stroke).corner_radius(eframe::egui::CornerRadius::same(5)).show(ui, |ui| {
                                        ComboBox::new("Store_Selection", "")                    
                                        .width(60.)
                                        .selected_text(selected_text)
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(selected, 76, "RIV");
                                            ui.selectable_value(selected, 73, "LTN");
                                            ui.selectable_value(selected, 74, "MUR");
                                            ui.selectable_value(selected, 78, "WJ");
                                            ui.selectable_value(selected, 75, "ORE");
                                            ui.selectable_value(selected, 72, "AF");
                                            ui.selectable_value(selected, 77, "SAN");
                                        });
                            
                                        if *selected != current {
                                            self.context.shared_ctx.store_users.clear();
                                            self.context.shared_ctx.tasks.clear();
                                            self.context.shared_ctx.task_layouts.clear();
                                            let tasks_tx = self.context.shared_ctx.initial_tasks_tx.clone();
                                            let store_users_tx = self.context.shared_ctx.store_users_tx.clone();
                                            let store_selection = std::convert::Into::<Store>::into(*selected);
                                            
                                            info!("Store: {store_selection:?}//{:?}", store_selection.clone().as_str().to_string());
                                            spawn(async move {
                                                let store_tasks = get_tasks_for_store(tasks_tx.clone(), store_selection.clone().as_str().to_string()).await;
                                                let get_store_users = get_store_users(store_users_tx, store_selection).await;
                                
                                                info!("get_tasks_for_store: {store_tasks:?}");
                                                info!("get_store_users: {get_store_users:?}");
                                            });
                                        }
                                    });
                                });

                                if ui.add(Button::new("Preferences")).clicked() {
                                    self.context.shared_ctx.state =
                                        AppState::Authenticated(MainPages::UserPreferences);
                                    match self.context.shared_ctx.app_state_tx.try_send(
                                        AppState::Authenticated(MainPages::UserPreferences),
                                    ) {
                                        Ok(_) => info!("Switching to UserPreferences Page"),
                                        Err(e) => error!("Error: {e:?}"),
                                    }
                                }
                                
                                if ui.add(Button::new("Downloads")).clicked() {
                                    self.context.shared_ctx.state = AppState::Authenticated(MainPages::Downloads);
                                    let github_releases_tx = self.context.github_releases_channel.0.clone();
                                    let client = self.context.client.clone();
                                    spawn(async move {
                                        let get_releases = get_github_releases(github_releases_tx, client).await;
                                        info!("get_releases: {get_releases:?}");
                                    });
                    
                                    match self
                                        .context.shared_ctx
                                        .app_state_tx
                                        .try_send(AppState::Authenticated(MainPages::Downloads))
                                    {
                                        Ok(_) => info!("Switching to Downloads Page"),
                                        Err(e) => error!("Error: {e:?}"),
                                    }
                                }
                    
                                if ui.add(Button::new("Modify Theme")).clicked() {
                                    self.context.shared_ctx.modify_theme = true;
                                    ui.close_menu();
                                }
                    
                                if ui.add(Button::new("Refresh Data")).clicked() {
                                    self.context.shared_ctx.first_run = true;
                                }
                    
                                if ui.add(Button::new("Logout")).clicked() {
                                    let file_path = "data.enc";
                                    match std::fs::exists(file_path){
                                        Ok(exists) => {
                                            if exists {
                                                match std::fs::remove_file(file_path){
                                                    Ok(_) => info!("Removed data file, logged out."),
                                                    Err(e) => log::error!("Error removing data file: {e:?}"),
                                                }
                                            } else {
                                                info!("Data file does not exist");
                                            }
                                        },
                                        Err(e) => log::error!("*.enc File not found {e:?}"),
                                    };

                                    let _ = self.context.shared_ctx.app_state_tx.try_send(AppState::NoAuth("Login".to_string()));
                                    spawn(async move {
                                        let invalidation = DATABASE.invalidate().await;
                                        info!("invalidated connection: {:?}", invalidation);
                                    });

                                    let logout_msg = "Logged out".to_string();
                                    self.context.shared_ctx.state = AppState::NoAuth(logout_msg.clone());
                                    match self
                                        .context.shared_ctx
                                        .app_state_tx
                                        .try_send(AppState::NoAuth(logout_msg))
                                    {
                                        Ok(_) => info!("Logged out"),
                                        Err(e) => error!("Error: {e:?}"),
                                    }
                                }
                            });
                    
                            Separator::default().shrink(20.0).ui(ui);
                            ui.add_space(10.0);
                            ui.vertical_centered(|ui| {
                                if ui.button(RichText::new("Show Notifications").heading()).clicked() {
                                    let notif_tx = self.context.shared_ctx.notification_tx.clone();
                                    spawn(async move {
                                        let notifications = get_notifications(notif_tx.clone()).await;
                                        info!("Get Notifications: {notifications:?}");
                                    });
                                }
                            });
                    
                            ui.horizontal_top(|ui| {
                                let read_button = ui.button(
                                    RichText::new("Read")
                                        .color(Color32::from_rgba_premultiplied(42, 222, 192, 60)),
                                );
                                ui.add_space(ui.available_width() - 50.0);
                                let unread_button = ui.button(
                                    RichText::new("Unread").color(Color32::from_rgb(191, 33, 101)),
                                );
                                if read_button.clicked() {
                                    self.context.read_notifications = true;
                                }
                                if unread_button.clicked() {
                                    self.context.read_notifications = false;
                                }
                            });

                            let row_height = 100.;
                            let total_rows = self.context.shared_ctx.notifications.len();
                            let scroll_area = ScrollArea::vertical().auto_shrink(false);
                            ui.ctx().options_mut(|o| o.line_scroll_speed = 80.0);

                            scroll_area.show_rows(ui, row_height, total_rows, |ui, row_range| {
                                for row in row_range {
                                    let mut notifications: Vec<Notification> =
                                        if self.context.shared_ctx.read_notifications {
                                            self.context
                                                .shared_ctx
                                                .notifications
                                                .iter()
                                                .filter(|n| n.status == "Read")
                                                .cloned()
                                                .collect()
                                        } else {
                                            self.context
                                                .shared_ctx
                                                .notifications
                                                .iter()
                                                .filter(|n| n.status == "Unread")
                                                .cloned()
                                                .collect()
                                        };

                                    if let Some(notification) = notifications.get_mut(row) {
                                        eframe::egui::Frame::new()
                                            .fill(ui.style().visuals.extreme_bg_color)
                                            .corner_radius(eframe::egui::CornerRadius::same(12))
                                            .inner_margin(Margin::same(10))
                                            .outer_margin(Margin::same(5))
                                            .stroke(Stroke::new(
                                                0.5,
                                                if notification.status == "Read" {
                                                    Color32::from_rgba_premultiplied(
                                                        42, 222, 192, 60,
                                                    )
                                                } else {
                                                    Color32::from_rgb(191, 33, 101)
                                                },
                                            ))
                                            .show(ui, |ui| {
                                                ui.horizontal_top(|ui| {
                                                    let w = 250.0;
                                                    ui.set_width(w);
                                                    ui.add_space(w / 3.0);
                                                    ui.colored_label(
                                                        Color32::from_rgba_premultiplied(
                                                            42, 222, 192, 60,
                                                        ),
                                                        RichText::new(
                                                            notification.notification_type.clone(),
                                                        )
                                                        .font(FontId::proportional(12.0)),
                                                    );
                                                    ui.add_space(80.0);
                                                    let button = Button::new(
                                                        RichText::new("X")
                                                            .color(Color32::from_rgb(191, 33, 101)),
                                                    )
                                                    .ui(ui);
                                                    if button.clicked() {
                                                        let mut notif = notification.clone();
                                                        if notification.status == "Read" {
                                                            spawn(async move {
                                                                notif
                                                                    .delete_notification()
                                                                    .await
                                                                    .unwrap();
                                                            });
                                                        } else {
                                                            notification.status =
                                                                "Read".to_string();
                                                            spawn(async move {
                                                                notif
                                                                    .mark_notification()
                                                                    .await
                                                                    .unwrap();
                                                            });
                                                        }
                                                    }
                                                });
                                                show_notification(
                                                    ui,
                                                    &notification.notification_description,
                                                    &inputs,
                                                    self.context.shared_ctx.ui_actions_tx.clone(),
                                                    &self.context.shared_ctx.tasks,
                                                );
                                            })
                                            .inner;
                                    }
                                }
                            });
                        });
                        ui.add_space(1.0);
                        ui.label("Welcome, ");
                    
                        ui.add_space(20.);

                        ui.menu_button(RichText::new("Ui Layout").color(ui.style().visuals.error_fg_color).strong().underline(), |ui| {
                            ui.vertical_centered_justified(|ui| {
                                ui.set_width(200.0);    
                                // ui.set_height(60.0);
                                ui.style_mut().visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_additive_luminance(60));
                                ui.visuals_mut().widgets.inactive.bg_fill = Color32::from_additive_luminance(120);
                                let submit = Button::new(RichText::new(" Save Ui Layout ").monospace()).ui(ui);
                                ui.add_space(5.0);
                                let organize = Button::new(RichText::new(" Organize Windows ").monospace()).ui(ui);
                                ui.add_space(10.0);
                                ui.separator();
                                ui.add_space(10.0);
                                let reset_ui = Button::new(RichText::new(" Reset Ui Layout ").color(Color32::LIGHT_RED).monospace()).ui(ui);
                                ui.add_space(5.0);
                                let reset_mem = Button::new(RichText::new(" Reset Memory ").monospace()).ui(ui);
                                let tree = default_tree();
                                if reset_ui.clicked() {
                                    let default_layout = serde_json::to_value(&tree).unwrap();
                                    usr.set_ui_layout_mastertech(default_layout.clone());
                                    
                                    self.tree = tree.0;
                                    self.context.open_tabs = tree.1;
                                    let mut user = usr.clone();
                                    spawn(async move {
                                        match user.save_mtechserver_ui_layout(default_layout.clone()).await {
                                            Ok(_) => info!("Updated User Settings"),
                                            Err(e) => log::error!("Error updating User Settings: {e:?}"),
                                        }
                                    });
                                }
                                if submit.clicked() {
                                    let val = serde_json::to_value(self.tree.clone()).unwrap_or_default();
                                    usr.set_ui_layout_mastertech(val.clone());

                                    let mut user = usr.clone();
                                    spawn(async move {
                                        match user.save_mastertech_ui_layout(val.clone()).await {
                                            Ok(_) => info!("Updated User Settings"),
                                            Err(e) => log::error!("Error updating User Settings: {e:?}"),
                                        }
                                    });
                                }
                                if organize.clicked() {
                                    ctx.memory_mut(|mem| mem.reset_areas());
                                    ctx.memory_mut(|mem| {
                                        for layer in mem.areas_mut().visible_layer_ids().iter() {
                                            info!("Visible layers: {layer:?}");
                                        }
                                    })
                                }
                                if reset_mem.clicked() {
                                    ctx.memory_mut(|mem| *mem = Default::default());
                                }
                            });
                        });

                        ui.add_space(20.0);
                        ui.colored_label(Color32::LIGHT_RED, RichText::new(self.context.client_title.clone()).monospace());
                        ui.colored_label(Color32::WHITE, RichText::new("Client ID: ").monospace());

                        let progress = self.context.progress;

                        if progress.0 > 0.0 {
                            let _ = ProgressBar::new(progress.0 / progress.1)
                                .fill(Color32::from_rgba_premultiplied(255, 77, 210, 20))
                                .desired_width(ui.available_width() / 4.0)
                                .show_percentage()
                                .animate(true)
                                .ui(ui);
                            ui.add_space(50.);
                        }

                        ui.add_space(ui.available_width() / 5.);
                        let txt = RichText::new(format!(
                            "Mastertech Server {}",
                            env!("CARGO_PKG_VERSION")
                        )).heading().color(Color32::WHITE);

                        if ui
                            .add(Button::new(txt))
                            .clicked()
                        {
                            self.context.shared_ctx.state = AppState::Authenticated(MainPages::Tasks);
                            match self
                                .context
                                .shared_ctx
                                .app_state_tx
                                .try_send(AppState::Authenticated(MainPages::Tasks))
                            {
                                Ok(_) => info!("AppState::Authenticated(MainPages::Tasks)"),
                                Err(e) => error!("Error: {e:?}"),
                            }
                        }
                    });
                } else {
                    if Button::new("Login").ui(ui).clicked() {
                        let _ = self
                            .context.shared_ctx
                            .app_state_tx
                            .send(AppState::NoAuth("Login".to_string()));
                    }
                }
            })
        });
    }
}


pub fn restart_in_terminal_mode() -> std::io::Result<()> {
    let current_exe = std::env::current_exe()?;
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        std::process::Command::new("cmd")
            .arg("/C")
            .arg(&current_exe)
            .arg("-t")
            .creation_flags(0x00000010) // CREATE_NEW_CONSOLE flag
            .creation_flags(0x00000008) // DETACHED_PROCESS flag
            .spawn()?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::process::Command::new(&current_exe)
            .arg("-t")
            .spawn()?;
        // On Unix-like systems, the process is detached by default when the parent exits
    }
    Ok(())
}