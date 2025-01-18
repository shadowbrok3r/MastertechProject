use database::{live_data::listen_data, schema::{helper_traits::UserHelper, utilities::{get_connected_clients, get_notifications, get_store_users, get_tasks_for_store, NotificationMod}, Notification, Store, CONNECTED_CLIENT_TABLE}, DATABASE};
use eframe::egui::{menu, Align, ComboBox, Context, Frame, Key, Margin, ProgressBar, Rounding, ScrollArea, Separator, TextEdit, Button, Color32, FontId, Layout, RichText, Stroke, TopBottomPanel, Widget};
use crate::app_state::{default_tree, AppState, MainPages, MtechServer};
use displays::ui_tools::autocomplete::AutoCompleteTextEdit;
use crate::pages::downloads_page::get_github_releases;
use displays::ui_tools::show_notification;
use wasm_bindgen_futures::spawn_local;
use std::collections::BTreeSet;
use displays::TaskUiActions;
use log::{error, info};

impl MtechServer {
    pub fn menu_bar(&mut self, ctx: &Context) {
        let mut inputs = BTreeSet::new();
        TopBottomPanel::top("egui_dock::MenuBar").show(ctx, |ui| {
            menu::bar(ui, |ui| {
                if let Some(usr) = self.context.shared_ctx.current_user.as_mut() {
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        ui.add_space(1.0);
                        ui.menu_button(RichText::new("View"), |ui| {
                            // allow certain tabs to be toggled
                            for tab in &[
                                &"Store Tasks".to_string(),
                                &"My Tasks".to_string(),
                                &"Terminal".to_string(),
                                &"Admin Console".to_string(),
                                &"Completed Tasks".to_string(),
                                &"Bug Report".to_string(),
                                &"Ai".to_string(),
                                &"Json Viewer".to_string(),
                                &"Query Builder".to_string(),
                                &"Task Audit".to_string(),
                                &"Store Stock".to_string(),
                                &"Company Stock".to_string(),
                                &"My Tools".to_string(),
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

                        ui.add_space(30.0);

                        for task in self.context.shared_ctx.tasks.iter() {
                            inputs.insert(task.task_name.clone());
                        }
                        ui.style_mut().visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_additive_luminance(60));
                        ui.visuals_mut().widgets.inactive.bg_fill = Color32::from_additive_luminance(120);
                        
                        let result =
                            AutoCompleteTextEdit::new(&mut self.context.search_input, inputs.clone())
                                .highlight_matches(true)
                                .max_suggestions(10)
                                .set_text_edit_properties(|text_edit: TextEdit<'_>| {
                                    text_edit
                                        .hint_text("  Search for task")
                                        .desired_width(150.0)
                                        .font(FontId::proportional(12.0))
                                        .frame(true)
                                })
                                .ui(ui);

                        let accepted_by_keyboard = ui.input_mut(|input| input.key_pressed(Key::Enter));

                        if result.secondary_clicked() || accepted_by_keyboard && !self.context.search_input.is_empty() {
                            info!("selected? {}", self.context.search_input.clone());
                            let search = self.context.search_input.clone();
                            self.context.search_input.clear();
                            if let Some(input) = inputs.get(&search) {
                                let task = self.context.shared_ctx.tasks.iter().find(|&x| {
                                    x.task_name == *input
                                        || format!("{}", x.service_number.clone().unwrap_or_default())
                                            == format!("{}", *input)
                                });

                                if let Some(task) = task {
                                    let _ = self
                                        .context
                                        .shared_ctx
                                        .ui_actions_tx
                                        .try_send(TaskUiActions::OpenTaskModal(task.clone()));
                                }
                            }
                        }
                    
                        ui.add_space(20.);
                        if Button::new(RichText::new(" Organize Windows ").monospace()).ui(ui).clicked() {
                            // ctx.send_viewport_cmd(command);

                            //let organize_shortcut = KeyboardShortcut::new(Modifiers::CTRL | Modifiers::SHIFT, Key::O);
                            // if ctx.input_mut(|i| i.consume_shortcut(&organize_shortcut)) {}
                            ctx.memory_mut(|mem| mem.reset_areas());

                            ctx.memory_mut(|mem| {
                                
                                for layer in mem.areas_mut().visible_layer_ids().iter() {
                                    info!("Visible layers: {layer:?}");
                                }
                            })
                        }
                        ui.add_space(20.);
                        if Button::new(RichText::new(" Reset Memory ").monospace()).ui(ui).clicked() {
                            ctx.memory_mut(|mem| *mem = Default::default());
                        }
                    });

                    let notif_tx = self.context.shared_ctx.notification_tx.clone();
                    ui.add_space(ui.available_width() / 7.);
                    let txt = RichText::new(format!(
                        "Mastertech Server {}",
                        env!("CARGO_PKG_VERSION")
                    )).heading().color(Color32::WHITE);

                    if ui
                        .add(Button::new(txt))
                        .clicked()
                    {
                        self.state = AppState::Authenticated(MainPages::Tasks);
                        match self
                            .context
                            .app_state_tx
                            .try_send(AppState::Authenticated(MainPages::Tasks))
                        {
                            Ok(_) => info!("AppState::Authenticated(MainPages::Tasks)"),
                            Err(e) => error!("Error: {e:?}"),
                        }
                    }

                    while let Ok(res) = self.context.bytes_channel.1.try_recv() {
                        self.context.total_download_size = res.1 as f32;
                        for y in res.0 {
                            self.context.download_progress += y as f32;
                        }
                    }

                    if self.context.download_progress == self.context.total_download_size {
                        self.context.download_progress = 0.0;
                        self.context.total_download_size = 0.0;
                    }
                    

                    if self.context.download_progress.ne(&0.0) {
                        ui.add_space(30.);
                        ProgressBar::new(
                            self.context.download_progress / self.context.total_download_size,
                        )
                        .fill(Color32::from_rgba_premultiplied(50, 10, 50, 65))
                        .show_percentage()
                        .desired_width(150.0)
                        .ui(ui);
                    }

                    ui.add_space(ui.available_width()/7.0);

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
                        _ => usr.store.as_str(),
                    };
            
                    ui.label("Show tasks in: ");
                    ui.add_space(5.);
                    Frame::default().stroke(ui.style().visuals.window_stroke).rounding(Rounding::same(5.0)).show(ui, |ui| {
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
                            spawn_local(async move {
                                let store_tasks = get_tasks_for_store(tasks_tx.clone(), store_selection.clone().as_str().to_string()).await;
                                let get_store_users = get_store_users(store_users_tx, store_selection).await;
                
                                info!("get_tasks_for_store: {store_tasks:?}");
                                info!("get_store_users: {get_store_users:?}");
                            });
                        }

                    });

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_space(8.0);
                        let txt =
                            RichText::new(format!(" {} ", usr.name.clone())).color(Color32::from_rgb(191, 33, 101));
                        ui.menu_button(txt, |ui| {
                            ui.set_width(300.0);
                            ui.set_height(600.0);
                            ui.vertical_centered_justified(|ui| {
                                if ui.add(Button::new("Admin Console")).clicked() {
                                    self.state = AppState::Authenticated(MainPages::WebConsole);
                                    let live_clients_tx = self.context.shared_ctx.live_clients_tx.clone();
                                    let tx = self.context.shared_ctx.connected_clients_tx.clone();
                                    spawn_local(async move {
                                        let get_connected_clients = get_connected_clients(tx).await;
                                        info!("get_connected_clients: {get_connected_clients:?}");
                                    });
                                    spawn_local(async move {
                                        let listen_data = listen_data(live_clients_tx, CONNECTED_CLIENT_TABLE).await;
                                        info!("listen_clients: {listen_data:?}");
                                    });

                                    match self
                                        .context
                                        .app_state_tx
                                        .try_send(AppState::Authenticated(MainPages::WebConsole))
                                    {
                                        Ok(_) => info!("Logged out"),
                                        Err(e) => error!("Error: {e:?}"),
                                    }
                                }

                                if ui.add(Button::new("Downloads")).clicked() {
                                    self.state = AppState::Authenticated(MainPages::Downloads);
                                    
                                    let github_releases_tx = self.context.github_releases_channel.0.clone();
                                    spawn_local(async move {
                                        let get_releases = get_github_releases(github_releases_tx).await;
                                        info!("get_releases: {get_releases:?}");
                                    });

                                    match self
                                        .context
                                        .app_state_tx
                                        .try_send(AppState::Authenticated(MainPages::Downloads))
                                    {
                                        Ok(_) => info!("Switching to Downloads Page"),
                                        Err(e) => error!("Error: {e:?}"),
                                    }
                                }

                                if ui.add(Button::new("Account Settings")).clicked() {
                                    self.state =
                                        AppState::Authenticated(MainPages::AccountSettings);
                                    match self.context.app_state_tx.try_send(
                                        AppState::Authenticated(MainPages::AccountSettings),
                                    ) {
                                        Ok(_) => info!("Switching to AccountSettings Page"),
                                        Err(e) => error!("Error: {e:?}"),
                                    }
                                }

                                if ui.add(Button::new("Modify Theme")).clicked() {
                                    self.context.shared_ctx.modify_theme = true;
                                    ui.close_menu();
                                }

                                if ui.add(Button::new("Refresh Data")).clicked() {
                                    self.context.first_run = true;
                                }

                                if ui.add(Button::new("Logout")).clicked() {
                                    #[cfg(target_arch = "wasm32")]
                                    {
                                        wasm_cookies::delete("user");
                                        wasm_cookies::delete("jwt");
                                    }
                                    spawn_local(async move {
                                        let invalidation = DATABASE.invalidate().await;
                                        info!("invalidated connection: {:?}", invalidation);
                                    });

                                    if let Some(window) = web_sys::window() {
                                        let reload = window.location().reload();
                                        info!("Reloading winodw: {reload:?}");
                                        if let Ok(storage) = window.local_storage() {
                                            if let Some(storage) = storage {
                                                let clear = storage.clear();
                                                info!("Clearing storage: {clear:?}");
                                            }
                                        }
                                    } else {
                                        info!("No window");
                                    }
                                    let logout_msg = "Logged out".to_string();
                                    self.state = AppState::NoAuth(logout_msg.clone());
                                    match self
                                        .context
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
                                if self.context.shared_ctx.notifications.is_empty() {
                                    if ui.button(RichText::new("Show Notifications").heading()).clicked() {
                                        spawn_local(async move {
                                            let notifications = get_notifications(notif_tx.clone()).await;
                                            info!("Get Notifications: {notifications:?}");
                                        });
                                    }
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
                                    self.context.shared_ctx.read_notifications = true;
                                }
                                if unread_button.clicked() {
                                    self.context.shared_ctx.read_notifications = false;
                                }
                            });
                            let row_height = 100.;
                            let total_rows = self.context.shared_ctx.notifications.len();
                            let scroll_area = ScrollArea::vertical().auto_shrink(false);
                            ui.ctx().options_mut(|o| o.line_scroll_speed = 15.0);

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
                                        eframe::egui::Frame::none()
                                            .fill(ui.style().visuals.extreme_bg_color)
                                            .rounding(Rounding::same(12.0))
                                            .inner_margin(Margin::same(10.0))
                                            .outer_margin(Margin::same(5.0))
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
                                                            spawn_local(async move {
                                                                notif
                                                                    .delete_notification()
                                                                    .await
                                                                    .unwrap();
                                                            });
                                                        } else {
                                                            notification.status =
                                                                "Read".to_string();
                                                            spawn_local(async move {
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
                        ui.style_mut().visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_additive_luminance(60));
                        ui.visuals_mut().widgets.inactive.bg_fill = Color32::from_additive_luminance(120);
                        let reset_ui = Button::new(RichText::new(" Reset Ui Layout ").color(Color32::LIGHT_RED).monospace()).ui(ui);
                        let tree = default_tree();
                        if reset_ui.clicked() {
                            let default_layout = serde_json::to_value(&tree).unwrap();
                            self.context.user_settings.ui_layout.mtechserver = default_layout.clone();
                            usr.user_settings.ui_layout.mtechserver = default_layout.clone();
                            #[cfg(target_arch = "wasm32")]
                            {
                                use brotli::CompressorReader;
                                use base64::{engine::general_purpose, Engine as _};
        
                                fn compress_string(input: &str) -> Vec<u8> {
                                    let mut compressed = Vec::new();
                                    {
                                        let mut compressor = CompressorReader::new(input.as_bytes(), 4096, 11, 22);
                                        std::io::copy(&mut compressor, &mut compressed).unwrap();
                                    }
                                    compressed
                                }
        
                                let user_string = serde_json::to_string(&usr.clone()).unwrap();
                                let compressed: Vec<u8> = compress_string(&user_string);
                                let encoded: String = general_purpose::STANDARD.encode(&compressed);
                                info!("Compressed data: {}\nEncoded: {}\nOriginal: {}", compressed.len(), encoded.len(), user_string.len());
                                wasm_cookies::delete("user");
                                let duration = web_time::Duration::from_secs(172800);
                                let cookie_opts = wasm_cookies::CookieOptions::default()
                                    .with_same_site(wasm_cookies::SameSite::Strict)
                                    .secure()
                                    .expires_after(duration);
                                wasm_cookies::set("user", &encoded, &cookie_opts);
                            }
                            
                            self.tree = tree.0;
                            self.context.open_tabs = tree.1;
                            let mut user = usr.clone();
                            spawn_local(async move {
                                match user.save_mtechserver_ui_layout(default_layout.clone()).await {
                                    Ok(_) => info!("Updated User Settings"),
                                    Err(e) => info!("Error updating User Settings: {e:?}"),
                                }
                            });
                            self.context.update_settings = true;
                        }
                        ui.add_space(5.);
                        let submit = Button::new(RichText::new(" Save Ui Layout ").monospace()).ui(ui);
                        
                        if submit.clicked() {
                            let val = serde_json::to_value(self.tree.clone()).unwrap_or_default();
                            self.context.user_settings.ui_layout.mtechserver = val.clone();
                            usr.user_settings.ui_layout.mtechserver = val.clone();
                            info!("user_settings: {:#?}", usr.user_settings.ui_layout);
                            #[cfg(target_arch = "wasm32")]
                            {
                                use brotli::CompressorReader;
                                use base64::{engine::general_purpose, Engine as _};
        
                                fn compress_string(input: &str) -> Vec<u8> {
                                    let mut compressed = Vec::new();
                                    {
                                        let mut compressor = CompressorReader::new(input.as_bytes(), 4096, 11, 22);
                                        std::io::copy(&mut compressor, &mut compressed).unwrap();
                                    }
                                    compressed
                                }
                                let user_string = serde_json::to_string(&usr.clone()).unwrap();
                                let compressed: Vec<u8> = compress_string(&user_string);
                                let encoded: String = general_purpose::STANDARD.encode(&compressed);

                                wasm_cookies::delete("user");
                                let duration = web_time::Duration::from_secs(172800);
                                let cookie_opts = wasm_cookies::CookieOptions::default()
                                    .with_same_site(wasm_cookies::SameSite::Strict)
                                    .secure()
                                    .expires_after(duration);
                                wasm_cookies::set("user", &encoded, &cookie_opts);
                            }
                            let mut user = usr.clone();
                            spawn_local(async move {
                                match user.save_mtechserver_ui_layout(val.clone()).await {
                                    Ok(_) => info!("Updated User Settings"),
                                    Err(e) => info!("Error updating User Settings: {e:?}"),
                                }
                            });
                            self.context.update_settings = true;
                        }
                    });
                } else {
                    ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                        if Button::new("Login").ui(ui).clicked() {
                            self.state = AppState::Authenticated(MainPages::Downloads);
                            match self
                                .context
                                .app_state_tx
                                .try_send(AppState::NoAuth("clicked login button".to_string()))
                            {
                                Ok(_) => info!("Switching to Login Page"),
                                Err(e) => error!("Error: {e:?}"),
                            }
                        }
                        if ui.add(Button::new("Downloads")).clicked() {
                            self.state = AppState::Authenticated(MainPages::Downloads);
                            match self
                                .context
                                .app_state_tx
                                .try_send(AppState::Authenticated(MainPages::Downloads))
                            {
                                Ok(_) => info!("Switching to Downloads Page"),
                                Err(e) => error!("Error: {e:?}"),
                            }
                        }
                    });
                }
            })
        });
    }
}


/*
pub fn find_task_in_description(
    notification_description: &str,
    task_names: &BTreeSet<String>, // BTreeSet of task names
) -> Vec<String> {
    // Collect matches where the task name is found in the notification description
    let matches: Vec<String> = task_names
        .iter()
        .filter_map(|task_name| {
            if notification_description.contains(task_name) {
                Some(task_name.clone()) // Add the matching task name
            } else {
                None
            }
        })
        .collect();

    matches
}


fn show_notification(
    ui: &mut Ui,
    notification_description: &str,
    task_names: &BTreeSet<String>,
    ui_actions_tx: crossbeam::channel::Sender<TaskUiActions>,
    tasks: &Vec<TaskPayload>,
) {
    // Find task names in the notification description
    let matches = find_task_in_description(notification_description, task_names);
    // We assume only one match for simplicity; handle multiple matches if necessary
    if let Some(task_name) = matches.get(0) {
        // Find where the task name is in the notification description
        if let Some(pos) = notification_description.find(task_name) {
            // Split the text into before, task name, and after
            let before = &notification_description[..pos];
            let after = &notification_description[pos + task_name.len()..];

            // Display the text parts with different formatting
            eframe::egui::Frame::none()
                .fill(ui.style().visuals.window_fill)
                .rounding(Rounding::same(12.0))
                .inner_margin(Margin::same(15.0))
                .outer_margin(Margin::same(5.0))
                .show(ui, |ui| {
                    info!("{pos:?}, {before:?}, {task_name:?}, {after:?}");
                    ui.horizontal_wrapped(|ui| {
                        // Show the text before the task name
                        ui.label(RichText::new(before));

                        // Show the task name in a different color (e.g., blue)
                        if Button::new(
                            RichText::new(task_name)
                                .color(Color32::from_rgba_premultiplied(42, 222, 192, 60)),
                        )
                        .ui(ui)
                        .clicked
                        {
                            let task = tasks.iter().find(|&x| {
                                x.task_name == *task_name
                                    || format!("{}", x.service_number.clone().unwrap_or_default())
                                        == format!("{}", *task_name)
                            });

                            if let Some(task) = task {
                                let _ = ui_actions_tx
                                    .try_send(TaskUiActions::OpenTaskModal(task.clone()));
                            }
                        }

                        // Show the text after the task name
                        ui.label(after);
                    });
                });
        } else {
            // info!("No task found");
            // If no task name is found, display the whole description normally
            ui.label(notification_description);
        }
    } else {
        // info!("No Task Name is matched");
        // If no task name is matched, just show the description
        ui.label(notification_description);
    }
}

 */
