use displays::{app_state::AppState, tabs::ai_playground::ChatThread, ui_tools::{decode_style, encode_style, toasts::{Toast, ToastKind, ToastOptions}}};
use displays::{tabs::admin_console::AdminConsole, ui_tools::theme_config::set_custom_style};
use eframe::{egui::{Color32, Context, Margin, Stroke, Style, Vec2, Window}, Frame};
use crate::{app_state::MtechServer, webworker::decode_task_payload};
use std::{collections::HashMap, sync::Arc};
use wasm_bindgen_futures::spawn_local;
use egui_dock::DockState;
use database::DATABASE;
#[cfg(target_arch="wasm32")]
use crate::app_state::check_authentication;

impl MtechServer {
    const STYLE: &str = r#"{"override_text_style":null,"override_font_id":null,"override_text_valign":"Center","text_styles":{"Small":{"size":10.0,"family":"Proportional"},"Body":{"size":14.0,"family":"Proportional"},"Monospace":{"size":12.0,"family":"Monospace"},"Button":{"size":14.0,"family":"Proportional"},"Heading":{"size":18.0,"family":"Proportional"}},"drag_value_text_style":"Button","wrap":null,"wrap_mode":null,"spacing":{"item_spacing":{"x":3.0,"y":3.0},"window_margin":{"left":12,"right":12,"top":12,"bottom":12},"button_padding":{"x":5.0,"y":3.0},"menu_margin":{"left":12,"right":12,"top":12,"bottom":12},"indent":18.0,"interact_size":{"x":40.0,"y":20.0},"slider_width":100.0,"slider_rail_height":8.0,"combo_width":100.0,"text_edit_width":280.0,"icon_width":14.0,"icon_width_inner":8.0,"icon_spacing":6.0,"default_area_size":{"x":600.0,"y":400.0},"tooltip_width":600.0,"menu_width":400.0,"menu_spacing":2.0,"indent_ends_with_horizontal_line":false,"combo_height":200.0,"scroll":{"floating":true,"bar_width":6.0,"handle_min_length":12.0,"bar_inner_margin":4.0,"bar_outer_margin":0.0,"floating_width":2.0,"floating_allocated_width":0.0,"foreground_color":true,"dormant_background_opacity":0.0,"active_background_opacity":0.4,"interact_background_opacity":0.7,"dormant_handle_opacity":0.0,"active_handle_opacity":0.6,"interact_handle_opacity":1.0}},"interaction":{"interact_radius":5.0,"resize_grab_radius_side":5.0,"resize_grab_radius_corner":10.0,"show_tooltips_only_when_still":true,"tooltip_delay":0.5,"tooltip_grace_time":0.2,"selectable_labels":true,"multi_widget_text_select":true},"visuals":{"dark_mode":true,"text_alpha_from_coverage":"TwoCoverageMinusCoverageSq","override_text_color":[207,216,220,255],"weak_text_alpha":0.6,"weak_text_color":null,"widgets":{"noninteractive":{"bg_fill":[0,0,0,0],"weak_bg_fill":[61,61,61,232],"bg_stroke":{"width":1.0,"color":[71,71,71,247]},"corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"fg_stroke":{"width":1.0,"color":[207,216,220,255]},"expansion":0.0},"inactive":{"bg_fill":[58,51,106,0],"weak_bg_fill":[8,8,8,231],"bg_stroke":{"width":1.5,"color":[48,51,73,255]},"corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"fg_stroke":{"width":1.0,"color":[207,216,220,255]},"expansion":0.0},"hovered":{"bg_fill":[37,29,61,97],"weak_bg_fill":[95,62,97,69],"bg_stroke":{"width":1.7,"color":[106,101,155,255]},"corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"fg_stroke":{"width":1.5,"color":[83,87,88,35]},"expansion":2.0},"active":{"bg_fill":[12,12,15,255],"weak_bg_fill":[39,37,54,214],"bg_stroke":{"width":1.0,"color":[12,12,16,255]},"corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"fg_stroke":{"width":2.0,"color":[207,216,220,255]},"expansion":1.0},"open":{"bg_fill":[20,22,28,255],"weak_bg_fill":[17,18,22,255],"bg_stroke":{"width":1.8,"color":[42,44,93,165]},"corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"fg_stroke":{"width":1.0,"color":[109,109,109,255]},"expansion":0.0}},"selection":{"bg_fill":[23,64,53,27],"stroke":{"width":1.0,"color":[12,12,15,255]}},"hyperlink_color":[135,85,129,255],"faint_bg_color":[17,18,22,255],"extreme_bg_color":[9,12,15,83],"text_edit_bg_color":null,"code_bg_color":[30,31,35,255],"warn_fg_color":[61,185,157,255],"error_fg_color":[255,55,102,255],"window_corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"window_shadow":{"offset":[0,0],"blur":7,"spread":5,"color":[17,17,41,118]},"window_fill":[11,11,15,255],"window_stroke":{"width":1.0,"color":[77,94,120,138]},"window_highlight_topmost":true,"menu_corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"panel_fill":[12,12,15,255],"popup_shadow":{"offset":[0,0],"blur":8,"spread":3,"color":[19,18,18,96]},"resize_corner_size":18.0,"text_cursor":{"stroke":{"width":2.0,"color":[197,192,255,255]},"preview":true,"blink":true,"on_duration":0.5,"off_duration":0.5},"clip_rect_margin":3.0,"button_frame":true,"collapsing_header_frame":true,"indent_has_left_vline":true,"striped":true,"slider_trailing_fill":true,"handle_shape":{"Rect":{"aspect_ratio":0.5}},"interact_cursor":"Crosshair","image_loading_spinners":true,"numeric_color_space":"GammaByte","disabled_alpha":0.5},"animation_time":0.083333336,"debug":{"debug_on_hover":false,"debug_on_hover_with_all_modifiers":false,"hover_shows_next":false,"show_expand_width":false,"show_expand_height":false,"show_resize":false,"show_interactive_widgets":false,"show_widget_hits":false,"show_unaligned":true},"explanation_tooltips":false,"url_in_tooltip":false,"always_scroll_the_only_direction":true,"scroll_animation":{"points_per_second":1000.0,"duration":{"min":0.1,"max":0.3}},"compact_menu_style":true}"#;
    
    pub fn first_run(&mut self, ctx: &Context, frame: &mut Frame) {
        self.context.shared_ctx.first_run = false;
        let current_version = env!("CARGO_PKG_VERSION");
        match serde_json::from_str::<Style>(Self::STYLE) {
            Ok(theme) => {
                let style = Arc::new(theme);
                ctx.set_style(style);
            }
            Err(e) => log::error!("Error setting theme: {e:?}")
        };
        
        if let Some(storage) = frame.storage_mut() {
            gloo_console::info!("We have Storage Mut Access");
            // Get existing chats a user has with ChatGPT
            if let Some(chat_history) = storage.get_string("chat_history") {
                // info!("chat_history: {chat_history:?}");
                let chat_threads: HashMap<String, ChatThread> = serde_json::from_str(&chat_history).unwrap_or_default();
                // info!("chat_threads: {chat_threads:?}");
                if let Some((nth, _)) = chat_threads.iter().nth(0) {
                    self.context.shared_ctx.ai_playground.selected_thread = nth.to_string();
                }
                self.context.shared_ctx.ai_playground.set_threads(chat_threads);
            }

            // if let Some(service_map) = storage.get_string("service_data") {
            //     match serde_json::from_str::<HashMap<String, PrestashopPayload>>(&service_map) {
            //         Ok(map) => {
            //             for (key, v) in map.iter() {
            //                 if let Some(k) = self.context.shared_ctx.task_audit_table.service_map.get_mut(key) {
            //                     if !k.iter().contains(&v) {
            //                         log::info!("Order: {v:?}");
            //                         k.push(v.clone());
            //                     }
            //                 }
            //             }
            //         },
            //         Err(e) => log::error!("Error converting service_map: {e:?}"),
            //     }
            // }

            if let Some(user) = self.context.shared_ctx.current_user.as_ref() {
                ctx.set_style(decode_style(&user.get_color_scheme()).unwrap_or_default());
                gloo_console::info!("2 We have a user");
                let user_version = user.get_version();
                gloo_console::info!(format!("2 current_version: {current_version}\nuser_version: {user_version}"));
                if let Some(version) = storage.get_string("version") {
                    if (current_version != version) || (current_version != user_version) {
                        gloo_console::info!("1 Mismatched Cargo Version. Doing update");
                        self.invalidate();
                    } else {
                        let mut usr = user.clone();
                        let v = current_version;
                        wasm_bindgen_futures::spawn_local(async move {
                            let res = usr.save_version(v).await;
                            gloo_console::info!(format!("Saving user version: {res:?}"));
                        });
                    }
                } else {
                    if current_version != user_version {
                        gloo_console::info!("3 Mismatched Cargo Version. Doing update");
                        self.invalidate();
                    } else {
                        let mut usr = user.clone();
                        let v = current_version;
                        wasm_bindgen_futures::spawn_local(async move {
                            let res = usr.save_version(v).await;
                            gloo_console::info!(format!("Saving user version: {res:?}"));
                        });
                    }
                }
            } else {
                let custom_style = set_custom_style(&self.context.shared_ctx.theme_config);
                ctx.set_style((*custom_style).clone());
            }
            
            if let Some(version) = storage.get_string("version") {
                if current_version != version {
                    gloo_console::info!("1 Mismatched Cargo Version. Doing update");
                    self.invalidate();
                }
            }
            //     } else {
            //         if let Some(user) = self.context.shared_ctx.current_user.as_ref() {
            //             gloo_console::info!("1 We have a user");
            //             let current_version = env!("CARGO_PKG_VERSION");
            //             let user_version = user.get_version();
            //             gloo_console::info!(format!("1 current_version: {version}\nuser_version: {user_version}"));
            //             if current_version != user_version {
            //                 gloo_console::info!("2 Mismatched Cargo Version. Doing update");
            //                 self.invalidate();
            //             } else {
            //                 let mut usr = user.clone();
            //                 let v = current_version;
            //                 wasm_bindgen_futures::spawn_local(async move {
            //                     let res = usr.save_version(v).await;
            //                     gloo_console::info!(format!("Saving user version: {res:?}"));
            //                 });
            //             }
            //         }
            //     }
            // } // else {
            //     if let Some(user) = self.context.shared_ctx.current_user.as_ref() {
            //         gloo_console::info!("2 We have a user");
            //         let user_version = user.get_version();
            //         gloo_console::info!(format!("2 current_version: {current_version}\nuser_version: {user_version}"));
            //         if current_version != user_version {
            //             gloo_console::info!("3 Mismatched Cargo Version. Doing update");
            //             self.invalidate();
            //         } else {
            //             let mut usr = user.clone();
            //             let v = current_version;
            //             wasm_bindgen_futures::spawn_local(async move {
            //                 let res = usr.save_version(v).await;
            //                 gloo_console::info!(format!("Saving user version: {res:?}"));
            //             });
            //         }
            //     } else {
            //         gloo_console::error!("No user");
            //         storage.set_string(
            //             "version",
            //             env!("CARGO_PKG_VERSION").to_string()
            //         );
            //     }
            // }
        }

        #[cfg(target_arch="wasm32")]
        match check_authentication(self.context.shared_ctx.db_tx.clone()) {
            Ok(state) => {
                log::info!("1");
                if let AppState::NoAuth(reason) = &state {
                    let toast = &mut self.context.shared_ctx.toasts;
    
                    let error_toast = Toast {
                        kind: ToastKind::Error,
                        text: format!("Message from Database: {reason}").into(),
                        options: ToastOptions::default()
                            .show_progress(true)
                            .duration_in_seconds(6.0),
                    };
                    toast.add(error_toast);
                }else {
                    spawn_local(async move {
                        match DATABASE.health().await {
                            Ok(_) => log::info!("Healthy connection"),
                            Err(e) => log::error!("Database connection health: {e:?}"),
                        }
                    });
                }
                self.context.shared_ctx.app_state_tx.try_send(state);
            }
            Err(e) => {
                log::info!("2");
                log::error!("Error with auth: {e:?}");
                self.context.shared_ctx.state = AppState::NoAuth(e.to_string());
                self.context.shared_ctx.current_user = None;
            }
        };
    }

    pub fn invalidate(&mut self) {
        gloo_console::info!("Invalidating");
        #[cfg(target_arch = "wasm32")]
        {
            wasm_cookies::delete("user");
            wasm_cookies::delete("jwt");
        }

        spawn_local(async move {
            let invalidation = DATABASE.invalidate().await;
            gloo_console::info!(format!("invalidated connection: {:?}", invalidation));
        });

        if let Some(window) = web_sys::window() {
            if let Ok(Some(storage)) = window.local_storage() {
                let clear = storage.clear();
                gloo_console::info!(format!("Clearing storage: {clear:?}"));
            }
            if let Ok(caches) = window.caches() {
                gloo_console::error!(format!("Caches: {:?}", caches.keys().as_string()));
                // for cache in caches.keys().then(cb)
                //     let success_closure = Closure::wrap(Box::new(move |_value: JsValue| {
                //         gloo_console::info!(format!("Initialized worker with {} threads", num_threads));
                //     }) as Box<dyn FnMut(JsValue)>);
            }
            let reload = window.location().reload();
            gloo_console::info!(format!("Reloading window: {reload:?}"));
        } else {
            gloo_console::info!(format!("No window"));
        }
        let logout_msg = "Logged out".to_string();
        self.context.shared_ctx.state = AppState::NoAuth(logout_msg.clone());
        let _ = self.context.shared_ctx.app_state_tx.try_send(AppState::NoAuth(logout_msg));
        let toast = &mut self.context.shared_ctx.toasts;

        let error_toast = Toast {
            kind: ToastKind::Error,
            text: format!("Detected older crate version").into(),
            options: ToastOptions::default().show_progress(true).duration_in_seconds(10.0),
        };
        toast.add(error_toast);
    }

    pub fn receive(&mut self, frame: &mut eframe::Frame, ctx: &eframe::egui::Context) {
        // do some initial setting up
        if self.context.shared_ctx.first_run { self.first_run(ctx, frame); }
        self.receive_database(frame, ctx);
        self.context.shared_ctx.receive_shared(frame, ctx);

        // most important part of the whole app.. setting up our styling
        // currently this just sets the style of the app, but in the near
        // future i will be making this the setup to allow user customization
        // to the style of any part of the app
        let theme_res = Window::new("Theme Configuration")
        .open(&mut self.context.shared_ctx.modify_theme)
        .max_height(600.)
        .min_width(700.)
        .title_bar(true)
        .show(ctx, |ui| {
            self.context.shared_ctx.theme_config.edit_ui(ui, ctx, self.context.shared_ctx.settings_sender.clone())
        });
        
        if let Some(window_res) = theme_res {
            if let Some(r) = window_res.inner {
                if r.0 {
                    if let Some(user) = self.context.shared_ctx.current_user.clone().as_mut() {
                        user.set_color_scheme(encode_style(&r.1.clone()).unwrap_or_default());
                        ctx.set_style(r.1.clone());
                        if let Some(storage) = frame.storage_mut() {
                            storage.set_string("user_settings", serde_json::to_string(&user.get_user_settings()).unwrap_or_default());
                        }
                        #[cfg(target_arch = "wasm32")]
                        {
                            wasm_cookies::delete("user");
                            let duration = web_time::Duration::from_secs(172800);
                            let usr = serde_json::to_string(&user.clone()).unwrap();
                            let cookie_opts = wasm_cookies::CookieOptions::default()
                                .with_same_site(wasm_cookies::SameSite::Strict)
                                .secure()
                                .expires_after(duration);
                        
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

                            let compressed: Vec<u8> = compress_string(&usr);
                            let encoded: String = general_purpose::STANDARD.encode(&compressed);
                            log::info!("Compressed data: {}\nEncoded: {}\nOriginal: {}", compressed.len(), encoded.len(), usr.len());
                            wasm_cookies::set("user", &encoded, &cookie_opts);
                        }
                    }
                    self.context.shared_ctx.theme = r.1;
                    self.context.shared_ctx.modify_theme = false;
                }
            }
        }

        // if !self.context.shared_ctx.modify_theme {
        //     let custom_style = set_custom_style(&self.context.shared_ctx.theme_config);
        //     ctx.set_style((custom_style).clone());
        // }

        // Getting responses from our webworker
        if let Some(items) = self.context.data_update.take() {
            let tx = self.context.shared_ctx.initial_tasks_tx.clone();
            wasm_bindgen_futures::spawn_local(async move {
                // log::info!("Got data update from webworker: {:?}", items.len());
                let _ = tx.try_send(decode_task_payload(&items).unwrap_or_default());
            });
        }

        // if let Some(decompressed_data) = self.context.admin_console_data_helper.deser_data_update.take() {
        //     if let Some(sysinfo) = deserializer::<SystemInformation>(&decompressed_data){
        //         info!("Got sysinfo from admin console");
        //         self.context.shared_ctx. resource_mon.set_sysinfo(sysinfo);
        //     }
        // }

        if self.context.shared_ctx.web_console_layout.wants_to_undock {
            let layout = &mut self.context.shared_ctx.web_console_layout;
            let undock_client = layout.undock_client.clone();
            for client in self.context.shared_ctx.clients.clone() {
                let should_we_undock = if let Some(undock) = undock_client.get(&client.connection_string)
                {
                    undock
                } else {
                    &false
                };

                if *should_we_undock {
                    let color = if client.connected {
                        Color32::LIGHT_BLUE
                    } else {
                        Color32::LIGHT_RED
                    };

                    let column_frame = eframe::egui::Frame::default()
                        .fill(Color32::from_rgb(12, 12, 14))
                        .inner_margin(Margin::same(4))
                        .outer_margin(Margin::symmetric(5, 3))
                        .corner_radius(eframe::egui::CornerRadius::same(10))
                        .stroke(Stroke::new(1.0, color));

                    Window::new(&client.connection_string)
                        .frame(column_frame)
                        .min_size(Vec2::new(700., 400.))
                        .max_size(Vec2::new(1500., 900.))
                        .default_size(Vec2::new(1000., 900.))
                        .show(ctx, |ui| {
                            ui.vertical_centered_justified(|ui| {
                                
                                let tx = layout.ui_actions_channel.0.clone();
                                
                                ui.horizontal(|ui| AdminConsole::client_header(ui, tx, &client.clone(), undock_client.clone()));
                                if let Some(ws_client) =
                                    layout.ws_clients.get_mut(&client.connection_string)
                                {
                                    ws_client.show(ui);
                                }
                            });
                        });
                }
            }
        }

        // Get User settings from local storage
        if let Some(user) = &self.context.shared_ctx.current_user {
            if self.context.get_settings {
                self.context.get_settings = false;
                match serde_json::from_value::<DockState<String>>(user.get_user_settings().get_ui_layout_mtechserver()){
                    Ok(tree) => self.tree = tree,
                    Err(e) => log::error!("Could not get UI layout from user: {e:?}: {:#?}", user.get_user_settings().get_ui_layout_mtechserver()),
                }
            } 
        }

        // Get User settings from local storage
        // this bool gets switched via clicking
        // the submit button in the crate::tabs::json_viewer
        // module
        if self.context.update_settings {
            self.context.update_settings = false;
            log::info!("Saving settings: {:?}", self.context.user_settings.clone());
            frame.storage_mut().unwrap().set_string(
                "user_settings",
                serde_json::to_string(&self.context.user_settings).unwrap(),
            );
        }

        if self.context.shared_ctx.ai_playground.save_chats {
            self.context.shared_ctx.ai_playground.save_chats = false;
            if let Some(_usr) = &self.context.shared_ctx.current_user {
                let threads = self.context.shared_ctx.ai_playground.get_threads();
                // for (id, thread) in threads {
                    // thread.messages
                // }
                // info!("Saving chats: {:?}", threads);
                frame.storage_mut().unwrap().set_string(
                    "chat_history",
                    serde_json::to_string(&threads).unwrap(),
                );
            }
        }
    }
}
