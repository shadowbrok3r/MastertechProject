use crate::app_state::{AppState, MainPages, MasterTechApp};
use crate::tabs::github::self_updater::run;
use database::schema::utilities::get_notifications;
use database::DATABASE;
use eframe::egui::{Button, Context, FontId, Layout, ProgressBar, RichText, Separator, Stroke, Vec2, Widget};
use eframe::egui::{CentralPanel, Color32, Frame, TopBottomPanel};
use egui_dock::{DockArea, Style as DockStyle};
use log::{error, info};
use tokio::spawn;

impl MasterTechApp {
    pub fn main_page(&mut self, ctx: &Context) {
        TopBottomPanel::top("egui_dock::MenuBar").show(ctx, |ui| {
            eframe::egui::menu::bar(ui, |ui| {
                ui.menu_button("View", |ui| {
                    // allow certain tabs to be toggled
                    for tab in &[
                        &"TUR Sheet".to_string(),
                        &"Console".to_string(),
                        // &"Part Order".to_string(),
                        &"Scripts".to_string(),
                        &"File Browser 📂".to_string(),
                        &"SysInfo".to_string(),
                        &"Minidump Analysis".to_string(),
                        // &"QC ☑️".to_string(),
                        &"Tasks".to_string(),
                        &"Bug Tracker".to_string(),
                        &"Websockets".to_string(),
                        &"ToolBox".to_string(),
                        &"Downloads".to_string(),
                        &"Logs".to_string(),
                        &"Stock".to_string(),
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
                ui.with_layout(Layout::right_to_left(eframe::egui::Align::Min), |ui| {
                    if let Some(usr) = &self.context.shared_ctx.current_user {

                    
                        ui.add_space(8.0);
                        let txt =
                            RichText::new(usr.name.clone()).color(Color32::from_rgb(100, 50, 100));
                        ui.menu_button(txt, |ui| {
                            ui.set_width(300.0);
                            ui.set_height(600.0);
                            ui.vertical_centered_justified(|ui| {
                                if ui.add(Button::new("Web Console")).clicked() {
                                    self.state = AppState::Authenticated(MainPages::WebConsole);
                                    let live_clients_tx = self.context.shared_ctx.live_clients_tx.clone();
                                    let tx = self.context.shared_ctx.connected_clients_tx.clone();
                                    let user = usr.clone();
                                    spawn(async move {
                                        let get_connected_clients = get_connected_clients(tx, user.clone()).await;
                                        info!("get_connected_clients: {get_connected_clients:?}");
                                    });
                                    spawn(async move {
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
                                    spawn(async move {
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
                                    spawn(async move {
                                        let invalidation = DATABASE.invalidate().await;
                                        info!("invalidated connection: {:?}", invalidation);
                                    });

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
                                if ui.button(RichText::new("Show Notifications").heading()).clicked() {
                                    let user_id = usr.clone().id;
                                    spawn(async move {
                                        let notifications = get_notifications(notif_tx.clone(), user_id).await;
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
                            let total_rows = self.context.notifications.len();
                            let scroll_area = ScrollArea::vertical().auto_shrink(false);
                            ui.ctx().options_mut(|o| o.line_scroll_speed = 15.0);
                    
                            scroll_area.show_rows(ui, row_height, total_rows, |ui, row_range| {
                                for row in row_range {
                                    let mut notifications: Vec<Notification> =
                                        if self.context.read_notifications {
                                            self.context
                                                .notifications
                                                .iter()
                                                .filter(|n| n.status == "Read")
                                                .cloned()
                                                .collect()
                                        } else {
                                            self.context
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
                        ui.style_mut().visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_additive_luminance(60));
                        ui.visuals_mut().widgets.inactive.bg_fill = Color32::from_additive_luminance(120);
                        let reset_ui = Button::new(RichText::new("Reset Ui Layout").color(Color32::LIGHT_RED).monospace()).ui(ui);
                    
                        if reset_ui.clicked() {
                            // #[cfg(target_arch = "wasm32")]
                            // {
                            //     let mut user = usr.clone();
                            //     self.context.user_settings.ui_layout = None;
                            //     user.user_settings = Some(self.context.user_settings.clone());
                            //     wasm_cookies::delete("user");
                            //     let usr = serde_json::to_string(&user.clone()).unwrap();
                            //     let duration = web_time::Duration::from_secs(172800);
                            //     let cookie_opts = wasm_cookies::CookieOptions::default()
                            //         .with_same_site(wasm_cookies::SameSite::Strict)
                            //         .secure()
                            //         .expires_after(duration);
                            
                            //     use brotli::CompressorReader;
                            //     use base64::{engine::general_purpose, Engine as _};
                    
                            //     fn compress_string(input: &str) -> Vec<u8> {
                            //         let mut compressed = Vec::new();
                            //         {
                            //             let mut compressor = CompressorReader::new(input.as_bytes(), 4096, 11, 22);
                            //             std::io::copy(&mut compressor, &mut compressed).unwrap();
                            //         }
                            //         compressed
                            //     }
                    
                            //     let compressed: Vec<u8> = compress_string(&usr);
                            //     let encoded: String = general_purpose::STANDARD.encode(&compressed);
                            //     info!("Compressed data: {}\nEncoded: {}\nOriginal: {}", compressed.len(), encoded.len(), usr.len());
                            //     wasm_cookies::set("user", &encoded, &cookie_opts);
                            // }
                            let tree = default_tree();
                            self.tree = tree.0;
                            self.context.open_tabs = tree.1;
                        }
                        ui.add_space(5.);
                        let submit = Button::new(RichText::new("Save Ui Layout").monospace()).ui(ui);
                        if submit.clicked() {
                            // self.context.user_settings.ui_layout = Some(serde_json::to_value(self.tree.clone()).unwrap());
                            // usr.user_settings.as_mut().unwrap_or(&mut self.context.user_settings.clone()).ui_layout = self.context.user_settings.ui_layout.clone();
                            let user_settings = usr.user_settings.as_mut().unwrap();
                            user_settings.ui_layout = Some(serde_json::to_value(self.tree.clone()).unwrap());
                            info!("self.context.user_settings: {:?}\nusr.user_settings: {:?}", self.context.user_settings, usr.user_settings);
                    
                            // #[cfg(target_arch = "wasm32")]
                            // {
                            //     wasm_cookies::delete("user");
                            //     let usr = serde_json::to_string(&usr.clone()).unwrap();
                            //     let duration = web_time::Duration::from_secs(172800);
                            //     let cookie_opts = wasm_cookies::CookieOptions::default()
                            //         .with_same_site(wasm_cookies::SameSite::Strict)
                            //         .secure()
                            //         .expires_after(duration);
                            
                            //     use brotli::CompressorReader;
                            //     use base64::{engine::general_purpose, Engine as _};
                    
                            //     fn compress_string(input: &str) -> Vec<u8> {
                            //         let mut compressed = Vec::new();
                            //         {
                            //             let mut compressor = CompressorReader::new(input.as_bytes(), 4096, 11, 22);
                            //             std::io::copy(&mut compressor, &mut compressed).unwrap();
                            //         }
                            //         compressed
                            //     }
                    
                            //     let compressed: Vec<u8> = compress_string(&usr);
                            //     let encoded: String = general_purpose::STANDARD.encode(&compressed);
                            //     info!("Compressed data: {}\nEncoded: {}\nOriginal: {}", compressed.len(), encoded.len(), usr.len());
                            //     wasm_cookies::set("user", &encoded, &cookie_opts);
                            // }
                            let mut user = usr.clone();
                            spawn(async move {
                                match user.save_user_ui_layout().await {
                                    Ok(_) => info!("Updated User Settings"),
                                    Err(e) => info!("Error updating User Settings: {e:?}"),
                                }
                            });
                            self.context.update_settings = true;
                        }
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
                        ui.add_space(20.0);

                        if let Some(usr) = self.context.shared_ctx.current_user.as_ref() {
                            let welcome_msg = RichText::new(format!("Welcome, {}", usr.name));
                            ui.menu_button(welcome_msg, |ui| {
                                if ui.add(Button::new("Modify Theme")).clicked() {
                                    self.context.shared_ctx.modify_theme = true;
                                    ui.close_menu();
                                }
                            });
                        }
                        if self.context.shared_ctx.current_user.is_none() {
                            if Button::new("Login").ui(ui).clicked() {
                                let _ = self
                                    .context
                                    .app_state_tx
                                    .send(crate::app_state::AppState::Login);
                            }
                        }
                        ui.add_space(20.0);

                        ui.colored_label(Color32::LIGHT_RED, self.context.computer_data.id.key().to_string());
                        ui.colored_label(Color32::WHITE, "Client ID: ");

                        let progress = self.context.progress;

                        let _ = ProgressBar::new(progress.0 / progress.1)
                            .fill(Color32::from_rgba_premultiplied(255, 77, 210, 20))
                            .desired_width(ui.available_width() / 4.0)
                            .show_percentage()
                            .animate(true)
                            .ui(ui);
                    }
                });
            })
        });

        CentralPanel::default() // When displaying a DockArea in another UI, it looks better
            .frame(Frame::central_panel(&ctx.style()).inner_margin(4.)) // to set inner margins to 0.
            .show(ctx, |ui| {
                let mut style = self
                    .context
                    .style
                    .get_or_insert(DockStyle::from_egui(ui.style()))
                    .clone();
                style.overlay.selection_color = Color32::from_rgb(92, 0, 87);
                style.separator.color_hovered = Color32::from_rgba_premultiplied(50, 93, 80, 77);
                style.separator.color_idle = Color32::from_rgba_premultiplied(17, 17, 33, 5);
                style.separator.color_dragged =
                    Color32::from_rgba_premultiplied(189, 189, 189, 130);
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
