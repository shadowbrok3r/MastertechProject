#![allow(deprecated)]
use crate::{app_state::{default_tree, default_tree_wasm, AppState, MainPages, SharedContext}, pages::view_menu, tabs::{github::get_github_releases, TabContext}, ui_tools::theme, PlatformSpawner, Spawner, TaskUiActions};
use database::{schema::{utilities::{get_completed_tasks_for_store, get_store_users, get_tasks_for_store}, FilterLiveTasks, LiveTaskPayload, Notification, Store}, DATABASE};
use eframe::egui::{containers::menu::MenuConfig, *};

impl SharedContext {
    pub fn menu_bar(&mut self, ctx: &Context) {
        eframe::egui::Panel::top("egui_dock::MenuBar").show(ctx, |ui| {
            MenuBar::new()
            .config(
                MenuConfig::default().close_behavior(PopupCloseBehavior::CloseOnClickOutside),
            )
            .ui(ui, |ui| {
                if let Some(user) = self.current_user.as_mut() {
                    let tab_ctx = TabContext::for_user(user.is_warehouse());
                    let mut inputs = std::collections::BTreeSet::new();
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        ui.add_space(1.0);
                        ui.menu_button(RichText::new("View").color(ui.global_style().visuals.error_fg_color).heading().underline(), |ui| {
                            view_menu(ui, &mut self.dock, tab_ctx, None);
                        });

                        ui.add_space(30.0);

                        // Populate inputs with task names and service numbers
                        for task in self.task_index.values() {
                            inputs.insert(task.task_name.clone());
                            inputs.insert(format!("{}", task.service_number.clone().unwrap_or_default()));
                        }

                        ui.style_mut().visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, Color32::from_additive_luminance(60));
                        ui.visuals_mut().widgets.inactive.bg_fill = Color32::from_additive_luminance(120);

                        let result = TextEdit::singleline(&mut self.search_input).desired_width(165.0).hint_text(" Search Tasks").ui(ui);
                        ui.add_space(5.);
                        if ui.button("Clear").clicked() {
                            self.search_results = None;
                            self.search_input.clear();
                        }
                        let accepted_by_keyboard = ui.input_mut(|input| input.key_pressed(Key::Enter));

                        if self.search_input.is_empty() && result.has_focus() {
                            self.search_results = None;
                        }

                        if !self.search_input.is_empty() {
                            // info!("Global search: {}", self.context.search_input);
                            let search = self.search_input.clone();
                            // Perform fuzzy search using FilterTasks
                            let filtered_tasks = self
                                .task_index
                                .values()
                                .cloned()
                                .collect::<Vec<LiveTaskPayload>>()
                                .filter_by_task_name(inputs.clone(), search.clone());
                            
                            self.search_results = Some(filtered_tasks);
                        } else if accepted_by_keyboard && self.search_input.is_empty() {
                            // Clear search results on Enter with empty input
                            self.search_results = None;
                            self.search_input.clear();
                            // info!("Cleared global search");
                        } else if ( result.secondary_clicked() || accepted_by_keyboard )&& !self.search_input.is_empty() {
                            self.search_results = None;
                            let search = self.search_input.clone();
                            self.search_input.clear();
                            if let Some(input) = inputs.get(&search) {
                                let task = self.tasks.iter().find(|&x| {
                                    x.task_name == *input
                                        || format!("{}", x.service_number.clone().unwrap_or_default())
                                            == format!("{}", *input)
                                });

                                if let Some(task) = task {
                                    let _ = self.ui_actions_tx.try_send(TaskUiActions::OpenTaskModal(task.clone()));
                                }
                            }
                        }

                        if result.lost_focus() && self.search_input.is_empty() {
                            self.search_results = None;
                        }
                    });

                    ui.add_space(ui.available_width() / 3.);
                    let txt = RichText::new(format!(
                        "Mastertech Server {}",
                        database::version_with_build!()
                    )).heading().color(Color32::WHITE);

                    if ui.add(Button::new(txt)).clicked()
                    {
                        self.state = AppState::Authenticated(MainPages::Tasks);
                        match self.app_state_tx.try_send(AppState::Authenticated(MainPages::Tasks))
                        {
                            Ok(_) => log::info!("AppState::Authenticated(MainPages::Tasks)"),
                            Err(e) => log::error!("Error: {e:?}"),
                        }
                    }

                    while let Ok(res) = self.bytes_channel.1.try_recv() {
                        self.total_download_size = res.1 as f32;
                        for y in res.0 {
                            self.download_progress += y as f32;
                        }
                    }

                    if self.download_progress == self.total_download_size {
                        self.download_progress = 0.0;
                        self.total_download_size = 0.0;
                    }
                    
                    if self.download_progress.ne(&0.0) {
                        ui.add_space(30.);
                        ProgressBar::new(
                            self.download_progress / self.total_download_size,
                        )
                        .fill(Color32::from_rgba_premultiplied(50, 10, 50, 65))
                        .show_percentage()
                        .desired_width(150.0)
                        .ui(ui);
                    }

                    ui.add_space(ui.available_width()/7.0);

                    let selected = &mut self.store_selection;
                    let current = selected.clone();
                    let _user_store = user.get_store();
                    let selected_text = Store::from_presta_store_id(&selected.to_string());

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_space(8.0);
                        
                        // Notification count badges
                        let unread_count = self.notification_center.unread_count();
                        let alert_count = self.notification_center.alert_count();
                        
                        // Alert badge (red, shown first/rightmost)
                        if alert_count > 0 {
                            let alert_badge = RichText::new(format!("⚠ {}", alert_count))
                                .color(Color32::WHITE)
                                .small()
                                .strong();
                            ui.add(
                                Button::new(alert_badge)
                                    .fill(theme::error(ui))
                                    .corner_radius(10.0)
                                    .min_size(vec2(28.0, 18.0))
                            ).on_hover_text(format!("{} unread alerts", alert_count));
                            ui.add_space(4.0);
                        }
                        
                        // General unread badge (teal)
                        if unread_count > 0 {
                            let badge_text = if unread_count > 99 {
                                "99+".to_string()
                            } else {
                                unread_count.to_string()
                            };
                            let unread_badge = RichText::new(format!("🔔 {}", badge_text))
                                .color(Color32::WHITE)
                                .small()
                                .strong();
                            ui.add(
                                Button::new(unread_badge)
                                    .fill(theme::success(ui))
                                    .corner_radius(10.0)
                                    .min_size(vec2(28.0, 18.0))
                            ).on_hover_text(format!("{} unread notifications", unread_count));
                            ui.add_space(4.0);
                        }
                        
                        let txt = RichText::new(format!(" {} ", user.get_name())).color(ui.global_style().visuals.error_fg_color).strong().underline();
                        ui.menu_button(txt, |ui| {
                            ui.set_width(300.0);
                            ui.set_height(800.);

                            ui.vertical_centered_justified(|ui| {
                                
                                if ui.add(Button::new("Preferences")).clicked() {
                                    self.state = AppState::Authenticated(MainPages::UserPreferences);
                                    match self.app_state_tx.try_send(
                                        AppState::Authenticated(MainPages::UserPreferences),
                                    ) {
                                        Ok(_) => self.account_mod.set_user(user.clone()),
                                        Err(e) => log::error!("Error: {e:?}"),
                                    }
                                }
                                ui.separator();

                                if ui.add(Button::new("Downloads")).clicked() {
                                    self.state = AppState::Authenticated(MainPages::Downloads);
                                    
                                    let github_releases_tx = self.github_releases_channel.0.clone();
                                    PlatformSpawner::spawn(async move {
                                        let get_releases = get_github_releases(github_releases_tx).await;
                                        log::info!("get_releases: {get_releases:?}");
                                    });

                                    match self.app_state_tx.try_send(AppState::Authenticated(MainPages::Downloads))
                                    {
                                        Ok(_) => log::info!("Switching to Downloads Page"),
                                        Err(e) => log::error!("Error: {e:?}"),
                                    }
                                }
                                
                                ui.separator();

                                if ui.add(Button::new("Modify Theme")).clicked() {
                                    self.modify_theme = true;
                                    ui.close_kind(UiKind::Menu)
                                }
                                
                                ui.separator();

                                if ui.add(Button::new("Refresh Data")).clicked() {
                                    self.first_run = true;
                                }

                                ui.separator();

                                if user.is_admin() {
                                    ui.add_space(10.);
                                    ui.horizontal(|ui| {
                                        ui.add_space(40.);
                                        TextEdit::singleline(&mut self.admin_notification_text)
                                            .background_color(ui.global_style().visuals.code_bg_color)
                                            .vertical_align(Align::Center)
                                            .hint_text("Admin Notification")
                                            .desired_rows(2)
                                            .desired_width(200.)
                                            .ui(ui);

                                        ui.add_space(10.);
                                        if Button::new("⬈").min_size(vec2(30., 30.)).stroke(Stroke::new(0.5_f32, ui.global_style().visuals.error_fg_color)).ui(ui).clicked() {
                                            let txt = self.admin_notification_text.clone();
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

                                if ui.add(Button::new(RichText::new("Logout").color(ui.global_style().visuals.error_fg_color))).clicked() {
                                    #[cfg(target_arch = "wasm32")]
                                    {
                                        wasm_cookies::delete("user");
                                        wasm_cookies::delete("jwt");
                                    }
                                    PlatformSpawner::spawn(async move {
                                        let invalidation = DATABASE.invalidate().await;
                                        log::info!("invalidated connection: {:?}", invalidation);
                                    });

                                    #[cfg(target_arch = "wasm32")]
                                    if let Some(window) = web_sys::window() {
                                        // Clear localStorage and sessionStorage immediately
                                        if let Ok(storage) = window.local_storage() {
                                            if let Some(storage) = storage {
                                                let clear = storage.clear();
                                                log::info!("Clearing localStorage: {clear:?}");
                                            }
                                        }
                                        if let Ok(storage) = window.session_storage() {
                                            if let Some(storage) = storage {
                                                let clear = storage.clear();
                                                log::info!("Clearing sessionStorage: {clear:?}");
                                            }
                                        }

                                        // Perform async cleanup for CacheStorage and Service Workers, then reload
                                        
                                        PlatformSpawner::spawn(async move {
                                            use wasm_bindgen_futures::JsFuture;
                                            use js_sys::wasm_bindgen::JsCast;

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
                                            log::info!("Reloading window: {reload:?}");
                                        });
                                    } else {
                                        log::info!("No window");
                                    }
                                    
                                    let logout_msg = "Logged out".to_string();
                                    self.state = AppState::NoAuth(logout_msg.clone());
                                    match self.app_state_tx.try_send(AppState::NoAuth(logout_msg))
                                    {
                                        Ok(_) => log::info!("Logged out"),
                                        Err(e) => log::error!("Error: {e:?}"),
                                    }
                                }
                            });

                            Separator::default().shrink(20.0).ui(ui);

                            self.notification_center.ui(ui, &inputs, self.ui_actions_tx.clone(), &self.tasks);
                        });

                        ui.add_space(2.0);
                        ui.label("Welcome, ");
                        ui.add_space(20.);

                        ui.menu_button(RichText::new("Ui Layout").color(ui.global_style().visuals.error_fg_color).strong().underline(), |ui| {
                            ui.vertical_centered_justified(|ui| {
                                ui.set_width(200.0);    
                                ui.style_mut().visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, Color32::from_additive_luminance(60));
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
                                let new_tree = if cfg!(target_arch="wasm32") {
                                    default_tree_wasm()
                                } else {
                                    default_tree()
                                };

                                if reset_ui.clicked() {
                                    let default_layout = serde_json::to_value(&new_tree.tree).unwrap();
                                    self.user_settings.set_ui_layout_mtechserver(default_layout.clone());
                                    user.set_ui_layout_mtechserver(default_layout.clone());
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
                
                                        let user_string = serde_json::to_string(&user.clone()).unwrap();
                                        let compressed: Vec<u8> = compress_string(&user_string);
                                        let encoded: String = general_purpose::STANDARD.encode(&compressed);
                                        log::info!("Compressed data: {}\nEncoded: {}\nOriginal: {}", compressed.len(), encoded.len(), user_string.len());
                                        wasm_cookies::delete("user");
                                        let duration = web_time::Duration::from_secs(172800);
                                        let cookie_opts = wasm_cookies::CookieOptions::default()
                                            .with_same_site(wasm_cookies::SameSite::Strict)
                                            .secure()
                                            .expires_after(duration);
                                        wasm_cookies::set("user", &encoded, &cookie_opts);
                                    }
                                    
                                    self.dock = new_tree;
                                    let mut user = user.clone();
                                    PlatformSpawner::spawn(async move {
                                        match user.save_mtechserver_ui_layout(default_layout.clone()).await {
                                            Ok(_) => log::info!("Updated User Settings"),
                                            Err(e) => log::error!("Error updating User Settings: {e:?}"),
                                        }
                                    });
                                    self.update_settings = true;
                                }
                                
                                if submit.clicked() {
                                    let val = serde_json::to_value(self.dock.tree.clone()).unwrap_or_default();
                                    self.user_settings.set_ui_layout_mtechserver(val.clone());
                                    user.set_ui_layout_mtechserver(val.clone());
                                    log::debug!("user_settings: {:#?}", user.get_user_settings());
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
                                        let user_string = serde_json::to_string(&user.clone()).unwrap();
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
                                    let mut user = user.clone();
                                    PlatformSpawner::spawn(async move {
                                        match user.save_mtechserver_ui_layout(val.clone()).await {
                                            Ok(_) => log::info!("Updated User Settings"),
                                            Err(e) => log::error!("Error updating User Settings: {e:?}"),
                                        }
                                    });
                                    self.update_settings = true;
                                }
                                if organize.clicked() {
                                    ctx.memory_mut(|mem| mem.reset_areas());
                                    ctx.memory_mut(|mem| {
                                        for layer in mem.areas_mut().visible_layer_ids().iter() {
                                            log::info!("Visible layers: {layer:?}");
                                        }
                                    })
                                }
                                if reset_mem.clicked() {
                                    ctx.memory_mut(|mem| *mem = Default::default());
                                }
                            });
                        });

                        ui.add_space(20.);

                        Frame::default().stroke(ui.global_style().visuals.window_stroke).corner_radius(eframe::egui::CornerRadius::same(5)).show(ui, |ui| {
                            ComboBox::new("Store_Selection", "")                    
                            .width(60.)
                            .selected_text(selected_text.as_str())
                            .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(selected, Store::RIV.into_store_id() as u64, Store::RIV.as_str());
                                ui.selectable_value(selected, Store::LTN.into_store_id() as u64, Store::LTN.as_str());
                                ui.selectable_value(selected, Store::MUR.into_store_id() as u64, Store::MUR.as_str());
                                ui.selectable_value(selected, Store::ORE.into_store_id() as u64, Store::ORE.as_str());
                                ui.selectable_value(selected, Store::SAN.into_store_id() as u64, Store::SAN.as_str());
                            });
                        });

                        if *selected != current {
                            let tasks_tx = self.initial_tasks_tx.clone();
                            let store_users_tx = self.store_users_tx.clone();
                            let store_selection = Store::from_presta_store_id(&selected.to_string());
                            self.store_users.clear();
                            self.tasks.clear();
                            self.layout_configs = None; // Force reinitialization
                            log::info!("Switching to store: {:?}", store_selection.as_str());
                            log::info!("Store: {store_selection:?}//{:?}", store_selection.clone().as_str().to_string());
                            PlatformSpawner::spawn(async move {
                                let store_tasks = get_tasks_for_store(tasks_tx.clone(), store_selection.clone().as_str().to_string()).await;
                                let tasks = get_completed_tasks_for_store(tasks_tx.clone(), store_selection.clone().as_str().to_string()).await;
                                let get_store_users = get_store_users(store_users_tx, store_selection).await;
                                log::info!("get_completed_tasks_for_store: {tasks:?}");
                                log::info!("get_tasks_for_store: {store_tasks:?}");
                                log::info!("get_store_users: {get_store_users:?}");
                            });
                        }
                                    
                    });
                } else {
                    ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                        if Button::new("Login").ui(ui).clicked() {
                            self.state = AppState::NoAuth("Needs Login".to_string());
                            match self.app_state_tx.try_send(AppState::NoAuth("clicked login button".to_string()))
                            {
                                Ok(_) => log::info!("Switching to Login Page"),
                                Err(e) => log::error!("Error: {e:?}"),
                            }
                        }
                        if ui.add(Button::new("Downloads")).clicked() {
                            self.state = AppState::Authenticated(MainPages::Downloads);
                            match self.app_state_tx.try_send(AppState::Authenticated(MainPages::Downloads))
                            {
                                Ok(_) => log::info!("Switching to Downloads Page"),
                                Err(e) => log::error!("Error: {e:?}"),
                            }
                        }
                    });
                }
            });
        });
    }
}