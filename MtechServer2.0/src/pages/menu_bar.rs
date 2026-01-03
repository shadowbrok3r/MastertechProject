use database::{schema::{utilities::{get_completed_tasks_for_store, get_store_users, get_tasks_for_store}, FilterLiveTasks, LiveTaskPayload, Notification, Store}, DATABASE};
use eframe::egui::{containers::menu::MenuConfig, vec2, Align, Button, Color32, ComboBox, Context, Frame, Key, Layout, MenuBar, PopupCloseBehavior, ProgressBar, RichText, Separator, Stroke, TextEdit, TopBottomPanel, UiKind, Widget};
use displays::{app_state::{default_tree, AppState, MainPages}, tabs::{github::get_github_releases, TABS}, PlatformSpawner, Spawner}; // ui_tools::autocomplete::AutoCompleteTextEdit, 
use crate::app_state::MtechServer;
use wasm_bindgen_futures::spawn_local;
use std::collections::BTreeSet;
use displays::TaskUiActions;
use log::{error, info};

impl MtechServer {
    pub fn menu_bar(&mut self, ctx: &Context) {
        let mut inputs = BTreeSet::new();
        TopBottomPanel::top("egui_dock::MenuBar").show(ctx, |ui| {
            MenuBar::new()
            .config(
                MenuConfig::default().close_behavior(PopupCloseBehavior::CloseOnClickOutside),
            )
            .ui(ui, |ui| {
                if let Some(usr) = self.shared_ctx.current_user.as_mut() {
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        ui.add_space(1.0);
                        ui.menu_button(RichText::new("View").color(ui.style().visuals.error_fg_color).heading().underline(), |ui| {
                            // allow certain tabs to be toggled
                            for tab in TABS {
                                if ui
                                    .selectable_label(self.open_tabs.contains(tab), tab)
                                    .clicked()
                                {
                                    if let Some(index) = self.shared_ctx.tree.find_tab(&tab.to_string()) {
                                        self.shared_ctx.tree.remove_tab(index);
                                        self.open_tabs.remove(tab);
                                    } else {
                                        self.open_tabs.insert(tab.to_string());
                                        self.shared_ctx.tree.push_to_focused_leaf(tab.to_string());
                                    }
                                    ui.close_kind(UiKind::Menu);
                                }
                            }
                        });

                        ui.add_space(30.0);

                        // Populate inputs with task names and service numbers
                        for task in self.shared_ctx.task_index.values() {
                            inputs.insert(task.task_name.clone());
                            inputs.insert(format!("{}", task.service_number.clone().unwrap_or_default()));
                        }

                        ui.style_mut().visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_additive_luminance(60));
                        ui.visuals_mut().widgets.inactive.bg_fill = Color32::from_additive_luminance(120);

                        let result = TextEdit::singleline(&mut self.shared_ctx.search_input).desired_width(165.0).hint_text(" Search Tasks").ui(ui);
                        ui.add_space(5.);
                        if ui.button("Clear").clicked() {
                            self.shared_ctx.search_results = None;
                            self.shared_ctx.search_input.clear();
                        }
                        let accepted_by_keyboard = ui.input_mut(|input| input.key_pressed(Key::Enter));

                        if self.shared_ctx.search_input.is_empty() && result.has_focus() {
                            self.shared_ctx.search_results = None;
                        }

                        if !self.shared_ctx.search_input.is_empty() {
                            // info!("Global search: {}", self.search_input);
                            let search = self.shared_ctx.search_input.clone();
                            // Perform fuzzy search using FilterTasks
                            let filtered_tasks = self.shared_ctx
                                .task_index
                                .values()
                                .cloned()
                                .collect::<Vec<LiveTaskPayload>>()
                                .filter_by_task_name(inputs.clone(), search.clone());
                            
                            self.shared_ctx.search_results = Some(filtered_tasks);
                        } else if accepted_by_keyboard && self.shared_ctx.search_input.is_empty() {
                            // Clear search results on Enter with empty input
                            self.shared_ctx.search_results = None;
                            self.shared_ctx.search_input.clear();
                            // info!("Cleared global search");
                        } else if ( result.secondary_clicked() || accepted_by_keyboard )&& !self.shared_ctx.search_input.is_empty() {
                            self.shared_ctx.search_results = None;
                            let search = self.shared_ctx.search_input.clone();
                            self.shared_ctx.search_input.clear();
                            if let Some(input) = inputs.get(&search) {
                                let task = self.shared_ctx.tasks.iter().find(|&x| {
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

                        if result.lost_focus() && self.shared_ctx.search_input.is_empty() {
                            self.shared_ctx.search_results = None;
                        }
                    });

                    ui.add_space(ui.available_width() / 3.);
                    let txt = RichText::new(format!(
                        "Mastertech Server {}",
                        env!("CARGO_PKG_VERSION")
                    )).heading().color(Color32::WHITE);

                    if ui
                        .add(Button::new(txt))
                        .clicked()
                    {
                        self.shared_ctx.state = AppState::Authenticated(MainPages::Tasks);
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

                    while let Ok(res) = self.bytes_channel.1.try_recv() {
                        self.shared_ctx.total_download_size = res.1 as f32;
                        for y in res.0 {
                            self.shared_ctx.download_progress += y as f32;
                        }
                    }

                    if self.shared_ctx.download_progress == self.shared_ctx.total_download_size {
                        self.shared_ctx.download_progress = 0.0;
                        self.shared_ctx.total_download_size = 0.0;
                    }
                    
                    if self.shared_ctx.download_progress.ne(&0.0) {
                        ui.add_space(30.);
                        ProgressBar::new(
                            self.shared_ctx.download_progress / self.shared_ctx.total_download_size,
                        )
                        .fill(Color32::from_rgba_premultiplied(50, 10, 50, 65))
                        .show_percentage()
                        .desired_width(150.0)
                        .ui(ui);
                    }

                    ui.add_space(ui.available_width()/7.0);

                    let selected = &mut self.shared_ctx.store_selection;
                    let current = selected.clone();
                    let usr_store = usr.get_store();
                    let selected_text = match selected {
                        76 => Store::RIV.as_str(),
                        73 => Store::LTN.as_str(),
                        74 => Store::MUR.as_str(),
                        78 => Store::WJ.as_str(),
                        75 => Store::ORE.as_str(),
                        77 => Store::SAN.as_str(),
                        _ => usr_store.as_str(),
                    };

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_space(8.0);
                        let txt = RichText::new(format!(" {} ", usr.get_name())).color(ui.style().visuals.error_fg_color).strong().underline();
                        ui.menu_button(txt, |ui| {
                            ui.set_width(300.0);
                            ui.set_height(800.);

                            ui.vertical_centered_justified(|ui| {
                                
                                if ui.add(Button::new("Preferences")).clicked() {
                                    self.shared_ctx.state = AppState::Authenticated(MainPages::UserPreferences);
                                    match self.shared_ctx.app_state_tx.try_send(
                                        AppState::Authenticated(MainPages::UserPreferences),
                                    ) {
                                        Ok(_) => self.shared_ctx.account_mod.set_user(usr.clone()),
                                        Err(e) => error!("Error: {e:?}"),
                                    }
                                }
                                ui.separator();

                                if ui.add(Button::new("Downloads")).clicked() {
                                    self.shared_ctx.state = AppState::Authenticated(MainPages::Downloads);
                                    
                                    let github_releases_tx = self.shared_ctx.github_releases_channel.0.clone();
                                    spawn_local(async move {
                                        let get_releases = get_github_releases(github_releases_tx).await;
                                        info!("get_releases: {get_releases:?}");
                                    });

                                    match self
                                        .context
                                        .shared_ctx
                                        .app_state_tx
                                        .try_send(AppState::Authenticated(MainPages::Downloads))
                                    {
                                        Ok(_) => info!("Switching to Downloads Page"),
                                        Err(e) => error!("Error: {e:?}"),
                                    }
                                }
                                
                                ui.separator();

                                if ui.add(Button::new("Modify Theme")).clicked() {
                                    self.shared_ctx.modify_theme = true;
                                    ui.close_kind(UiKind::Menu)
                                }
                                
                                ui.separator();

                                if ui.add(Button::new("Refresh Data")).clicked() {
                                    self.shared_ctx.first_run = true;
                                }

                                ui.separator();

                                if usr.is_admin() {
                                    ui.add_space(10.);
                                    ui.horizontal(|ui| {
                                        ui.add_space(40.);
                                        TextEdit::singleline(&mut self.shared_ctx.admin_notification_text)
                                            .background_color(ui.style().visuals.code_bg_color)
                                            .vertical_align(Align::Center)
                                            .hint_text("Admin Notification")
                                            .desired_rows(2)
                                            .desired_width(200.)
                                            .ui(ui);

                                        ui.add_space(10.);
                                        if Button::new("⬈").min_size(vec2(30., 30.)).stroke(Stroke::new(0.5, ui.style().visuals.error_fg_color)).ui(ui).clicked() {
                                            let txt = self.shared_ctx.admin_notification_text.clone();
                                            PlatformSpawner::spawn(async move {
                                                let res = Notification::default()
                                                    .set_description(txt)
                                                    .set_type("Admin")
                                                    .create()
                                                    .await;
                                                log::info!("Notification Response: {:?}", res);
                                            });
                                        }
                                    });
                                }

                                ui.add_space(10.0);
                                Separator::default().shrink(20.0).ui(ui);

                                if ui.add(Button::new(RichText::new("Logout").color(ui.style().visuals.error_fg_color))).clicked() {
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
                                        // Clear localStorage and sessionStorage immediately
                                        if let Ok(storage) = window.local_storage() {
                                            if let Some(storage) = storage {
                                                let clear = storage.clear();
                                                info!("Clearing localStorage: {clear:?}");
                                            }
                                        }
                                        if let Ok(storage) = window.session_storage() {
                                            if let Some(storage) = storage {
                                                let clear = storage.clear();
                                                info!("Clearing sessionStorage: {clear:?}");
                                            }
                                        }

                                        // Perform async cleanup for CacheStorage and Service Workers, then reload
                                        // #[cfg(target_arch = "wasm32")]
                                        wasm_bindgen_futures::spawn_local(async move {
                                            use wasm_bindgen::JsCast;
                                            use wasm_bindgen_futures::JsFuture;

                                            let win = web_sys::window().expect("window");

                                            // Clear CacheStorage
                                            if let Ok(caches) = win.caches() {
                                                if let Ok(keys_js) = JsFuture::from(caches.keys()).await {
                                                let keys = js_sys::Array::from(&keys_js);
                                                for key in keys.iter() {
                                                    if let Some(k) = key.as_string() {
                                                        let _ = JsFuture::from(caches.delete(&k)).await;
                                                    }
                                                }
                                                }
                                            }

                                            // Unregister all Service Workers
                                            let swc = win.navigator().service_worker();
                                            if let Ok(regs_js) = JsFuture::from(swc.get_registrations()).await {
                                                let regs = js_sys::Array::from(&regs_js);
                                                for reg_val in regs.iter() {
                                                    if let Ok(reg) = reg_val.dyn_into::<web_sys::ServiceWorkerRegistration>() {
                                                        if let Ok(promise) = reg.unregister() {
                                                            let _ = JsFuture::from(promise).await;
                                                        }
                                                    }
                                                }
                                            }
                                            

                                            // Finally, force reload to ensure fresh assets
                                            let reload = win.location().reload();
                                            info!("Reloading window: {reload:?}");
                                        });
                                    } else {
                                        info!("No window");
                                    }
                                    
                                    let logout_msg = "Logged out".to_string();
                                    self.shared_ctx.state = AppState::NoAuth(logout_msg.clone());
                                    match self
                                        .context
                                        .shared_ctx
                                        .app_state_tx
                                        .try_send(AppState::NoAuth(logout_msg))
                                    {
                                        Ok(_) => info!("Logged out"),
                                        Err(e) => error!("Error: {e:?}"),
                                    }
                                }
                            });

                            Separator::default().shrink(20.0).ui(ui);
                            self.shared_ctx.notification_center.ui(
                                ui, 
                                &inputs, 
                                self.shared_ctx.ui_actions_tx.clone(), 
                                &self.shared_ctx.tasks
                            );
                        });

                        ui.add_space(2.0);
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
                                    self.shared_ctx.user_settings.set_ui_layout_mtechserver(default_layout.clone());
                                    usr.set_ui_layout_mtechserver(default_layout.clone());
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
                                    
                                    self.shared_ctx.tree = tree.0;
                                    self.open_tabs = tree.1;
                                    let mut user = usr.clone();
                                    spawn_local(async move {
                                        match user.save_mtechserver_ui_layout(default_layout.clone()).await {
                                            Ok(_) => info!("Updated User Settings"),
                                            Err(e) => log::error!("Error updating User Settings: {e:?}"),
                                        }
                                    });
                                    self.shared_ctx.update_settings = true;
                                }
                                if submit.clicked() {
                                    let val = serde_json::to_value(self.shared_ctx.tree.clone()).unwrap_or_default();
                                    self.shared_ctx.user_settings.set_ui_layout_mtechserver(val.clone());
                                    usr.set_ui_layout_mtechserver(val.clone());
                                    log::debug!("user_settings: {:#?}", usr.get_user_settings());
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
                                            Err(e) => log::error!("Error updating User Settings: {e:?}"),
                                        }
                                    });
                                    self.shared_ctx.update_settings = true;
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

                        ui.add_space(20.);

                        Frame::default().stroke(ui.style().visuals.window_stroke).corner_radius(eframe::egui::CornerRadius::same(5)).show(ui, |ui| {
                            ComboBox::new("Store_Selection", "")                    
                            .width(60.)
                            .selected_text(selected_text)
                            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(selected, 76, "RIV");
                                ui.selectable_value(selected, 73, "LTN");
                                ui.selectable_value(selected, 74, "MUR");
                                ui.selectable_value(selected, 78, "WJ");
                                ui.selectable_value(selected, 75, "ORE");
                                ui.selectable_value(selected, 77, "SAN");
                            });
                        });

                        if *selected != current {
                            let tasks_tx = self.shared_ctx.initial_tasks_tx.clone();
                            let store_users_tx = self.shared_ctx.store_users_tx.clone();
                            let store_selection = std::convert::Into::<Store>::into(*selected);
                            self.shared_ctx.store_users.clear();
                            self.shared_ctx.tasks.clear();
                            self.shared_ctx.layout_configs = None; // Force reinitialization
                            info!("Switching to store: {:?}", store_selection.as_str());
                            info!("Store: {store_selection:?}//{:?}", store_selection.clone().as_str().to_string());
                            spawn_local(async move {
                                let store_tasks = get_tasks_for_store(tasks_tx.clone(), store_selection.clone().as_str().to_string()).await;
                                let tasks = get_completed_tasks_for_store(tasks_tx.clone(), store_selection.clone().as_str().to_string()).await;
                                let get_store_users = get_store_users(store_users_tx, store_selection).await;
                                info!("get_completed_tasks_for_store: {tasks:?}");
                                info!("get_tasks_for_store: {store_tasks:?}");
                                info!("get_store_users: {get_store_users:?}");
                            });
                        }
                                    
                    });
                } else {
                    ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                        if Button::new("Login").ui(ui).clicked() {
                            self.shared_ctx.state = AppState::NoAuth("Needs Login".to_string());
                            match self
                                .context
                                .shared_ctx
                                .app_state_tx
                                .try_send(AppState::NoAuth("clicked login button".to_string()))
                            {
                                Ok(_) => info!("Switching to Login Page"),
                                Err(e) => error!("Error: {e:?}"),
                            }
                        }
                        if ui.add(Button::new("Downloads")).clicked() {
                            self.shared_ctx.state = AppState::Authenticated(MainPages::Downloads);
                            match self
                                .context
                                .shared_ctx
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
            eframe::egui::Frame::new()
                .fill(ui.style().visuals.window_fill)
                .corner_radius(eframe::egui::CornerRadius::same(12.0))
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
