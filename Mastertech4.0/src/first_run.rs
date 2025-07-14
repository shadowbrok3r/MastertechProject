use super::{filesystem::system_info::generate_client_id, utilities::load_encrypted_user_data, app_state::MasterTechApp, tabs::github::get_github_releases};
use displays::{app_state::AppState, pages::login_page::HASH, ui_tools::{encode_style, toasts::{Toast, ToastKind, ToastOptions}}};
use database::{schema::{CustomerData, ExtendedSeb, LiveTaskPayload, LocalSebData, TicketData, CONNECTED_CLIENT_TABLE}, Database, WS_CLIENT_URL};
use eframe::{egui::{Context, ViewportCommand}, Frame};
use database::schema::GetKeysResponse;
use surrealdb::RecordId;
use std::sync::Arc;
use tokio::spawn;
use egui::Style;

impl MasterTechApp {
    const STYLE: &str = r#"{"override_text_style":null,"override_font_id":null,"override_text_valign":"Center","text_styles":{"Small":{"size":10.0,"family":"Proportional"},"Body":{"size":14.0,"family":"Proportional"},"Monospace":{"size":12.0,"family":"Monospace"},"Button":{"size":14.0,"family":"Proportional"},"Heading":{"size":18.0,"family":"Proportional"}},"drag_value_text_style":"Button","wrap":null,"wrap_mode":null,"spacing":{"item_spacing":{"x":3.0,"y":3.0},"window_margin":{"left":12,"right":12,"top":12,"bottom":12},"button_padding":{"x":5.0,"y":3.0},"menu_margin":{"left":12,"right":12,"top":12,"bottom":12},"indent":18.0,"interact_size":{"x":40.0,"y":20.0},"slider_width":100.0,"slider_rail_height":8.0,"combo_width":100.0,"text_edit_width":280.0,"icon_width":14.0,"icon_width_inner":8.0,"icon_spacing":6.0,"default_area_size":{"x":600.0,"y":400.0},"tooltip_width":600.0,"menu_width":400.0,"menu_spacing":2.0,"indent_ends_with_horizontal_line":false,"combo_height":200.0,"scroll":{"floating":true,"bar_width":6.0,"handle_min_length":12.0,"bar_inner_margin":4.0,"bar_outer_margin":0.0,"floating_width":2.0,"floating_allocated_width":0.0,"foreground_color":true,"dormant_background_opacity":0.0,"active_background_opacity":0.4,"interact_background_opacity":0.7,"dormant_handle_opacity":0.0,"active_handle_opacity":0.6,"interact_handle_opacity":1.0}},"interaction":{"interact_radius":5.0,"resize_grab_radius_side":5.0,"resize_grab_radius_corner":10.0,"show_tooltips_only_when_still":true,"tooltip_delay":0.5,"tooltip_grace_time":0.2,"selectable_labels":true,"multi_widget_text_select":true},"visuals":{"dark_mode":true,"text_alpha_from_coverage":"TwoCoverageMinusCoverageSq","override_text_color":[207,216,220,255],"weak_text_alpha":0.6,"weak_text_color":null,"widgets":{"noninteractive":{"bg_fill":[0,0,0,0],"weak_bg_fill":[61,61,61,232],"bg_stroke":{"width":1.0,"color":[71,71,71,247]},"corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"fg_stroke":{"width":1.0,"color":[207,216,220,255]},"expansion":0.0},"inactive":{"bg_fill":[58,51,106,0],"weak_bg_fill":[8,8,8,231],"bg_stroke":{"width":1.5,"color":[48,51,73,255]},"corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"fg_stroke":{"width":1.0,"color":[207,216,220,255]},"expansion":0.0},"hovered":{"bg_fill":[37,29,61,97],"weak_bg_fill":[95,62,97,69],"bg_stroke":{"width":1.7,"color":[106,101,155,255]},"corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"fg_stroke":{"width":1.5,"color":[83,87,88,35]},"expansion":2.0},"active":{"bg_fill":[12,12,15,255],"weak_bg_fill":[39,37,54,214],"bg_stroke":{"width":1.0,"color":[12,12,16,255]},"corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"fg_stroke":{"width":2.0,"color":[207,216,220,255]},"expansion":1.0},"open":{"bg_fill":[20,22,28,255],"weak_bg_fill":[17,18,22,255],"bg_stroke":{"width":1.8,"color":[42,44,93,165]},"corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"fg_stroke":{"width":1.0,"color":[109,109,109,255]},"expansion":0.0}},"selection":{"bg_fill":[23,64,53,27],"stroke":{"width":1.0,"color":[12,12,15,255]}},"hyperlink_color":[135,85,129,255],"faint_bg_color":[17,18,22,255],"extreme_bg_color":[9,12,15,83],"text_edit_bg_color":null,"code_bg_color":[30,31,35,255],"warn_fg_color":[61,185,157,255],"error_fg_color":[255,55,102,255],"window_corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"window_shadow":{"offset":[0,0],"blur":7,"spread":5,"color":[17,17,41,118]},"window_fill":[11,11,15,255],"window_stroke":{"width":1.0,"color":[77,94,120,138]},"window_highlight_topmost":true,"menu_corner_radius":{"nw":6,"ne":6,"sw":6,"se":6},"panel_fill":[12,12,15,255],"popup_shadow":{"offset":[0,0],"blur":8,"spread":3,"color":[19,18,18,96]},"resize_corner_size":18.0,"text_cursor":{"stroke":{"width":2.0,"color":[197,192,255,255]},"preview":true,"blink":true,"on_duration":0.5,"off_duration":0.5},"clip_rect_margin":3.0,"button_frame":true,"collapsing_header_frame":true,"indent_has_left_vline":true,"striped":true,"slider_trailing_fill":true,"handle_shape":{"Rect":{"aspect_ratio":0.5}},"interact_cursor":"Crosshair","image_loading_spinners":true,"numeric_color_space":"GammaByte","disabled_alpha":0.5},"animation_time":0.083333336,"debug":{"debug_on_hover":false,"debug_on_hover_with_all_modifiers":false,"hover_shows_next":false,"show_expand_width":false,"show_expand_height":false,"show_resize":false,"show_interactive_widgets":false,"show_widget_hits":false,"show_unaligned":true},"explanation_tooltips":false,"url_in_tooltip":false,"always_scroll_the_only_direction":true,"scroll_animation":{"points_per_second":1000.0,"duration":{"min":0.1,"max":0.3}},"compact_menu_style":true}"#;
    
    pub fn first_run(&mut self, ctx: &Context, frame: &mut Frame) {
        self.context.shared_ctx.first_run = false;
        // let custom_style = set_custom_style(&self.context.shared_ctx.theme_config);
        match serde_json::from_str::<Style>(Self::STYLE) {
            Ok(theme) => {
                let style = Arc::new(theme);
                ctx.set_style(style);
            }
            Err(e) => log::error!("Error setting theme: {e:?}")
        };

        if let Some(storage) = frame.storage() {
            self.context.ticket_data = storage.get_string("ticket_data").map_or(TicketData::default(), |f| serde_json::from_str(&f).unwrap_or_default());
            self.context.task_data = storage.get_string("task_data").map_or(LiveTaskPayload::default(), |f| serde_json::from_str(&f).unwrap_or_default());
            self.context.customer_data = storage.get_string("customer_data").map_or(CustomerData::default(), |f| serde_json::from_str(&f).unwrap_or_default());
            self.context.seb_info = storage.get_string("seb_info").map_or(vec![], |f| serde_json::from_str(&f).unwrap_or_default());
        }
        
        let github_tx = self.context.github_releases_channel.0.clone();
        let client = self.context.client.clone();
        let tx = self.context.shared_ctx.db_tx.clone();

        spawn(async move {
            match get_github_releases(github_tx, client).await {
                Ok(_) => log::info!("get_github_releases ran ok"),
                Err(e) => log::error!("Error getting github releases: {e:?}"),
            }
        });

        match load_encrypted_user_data(HASH) {
            Some(login) => {
                if cfg!(debug_assertions) {
                    log::error!("loaded data: {login:?}");
                }
                spawn(async move {
                    let db = Database::new(login.username, login.password, None).await;
                    match tx.try_send(db) {
                        Ok(_) => drop(tx),
                        Err(e) => log::error!("Error sending specs: {e:?}"),
                    }
                });
            }
            None => {
                let toast = &mut self.context.shared_ctx.toasts;

                let error_toast = Toast {
                    kind: ToastKind::Error,
                    text: "Could not get login from encoded data".into(),
                    options: ToastOptions::default()
                        .show_progress(true)
                        .duration_in_seconds(6.0),
                };
                toast.add(error_toast);
                let _ = self.context.shared_ctx.app_state_tx.try_send(AppState::NoAuth("Needs Login".to_string()));
            }
        }
    }

    pub fn receive(&mut self, frame: &mut eframe::Frame, ctx: &Context) {
        if self.context.shared_ctx.first_run { self.first_run(ctx, frame); }
        self.context.shared_ctx.receive_shared(frame, ctx);
        self.receive_prestashop(frame);
        self.receive_database(ctx, frame);
        self.receive_github();
        self.viewport_loader(ctx);
        // self.context.file_browser.try_lock()
        // ctx.request_repaint_after_secs(0.5);

        // most important part of the whole app.. setting up our styling
        let theme_res = eframe::egui::Window::new("Theme Configuration")
        .open(&mut self.context.shared_ctx.modify_theme)
        .max_height(600.)
        .min_width(700.)
        .title_bar(true)
        .show(ctx, |ui|
            self.context.shared_ctx.theme_config.edit_ui(ui, ctx, self.context.shared_ctx.settings_sender.clone())
        );
        
        if let Some(window_res) = theme_res {
            if let Some(r) = window_res.inner {
                if r.0 {
                    if let Some(user) = self.context.shared_ctx.current_user.clone().as_mut() {
                        user.set_color_scheme(encode_style(&r.1).unwrap_or_default());
                        if let Some(storage) = frame.storage_mut() {
                            storage.set_string("user_settings", serde_json::to_string(&r.1).unwrap_or_default());
                        }
                    }
                    self.context.shared_ctx.theme = r.1;
                    self.context.shared_ctx.modify_theme = false;
                }
            }
        }
        
        // Get User settings from local storage
        if let Some(user) = &self.context.shared_ctx.current_user {
            if self.context.get_settings {
                self.context.get_settings = false;
                match serde_json::from_value::<egui_dock::DockState<String>>(user.get_user_settings().get_ui_layout_mastertech()){
                    Ok(tree) => self.tree = tree,
                    Err(e) => log::error!("Could not get UI layout from user: {e:?}"),
                }
            } 
        }
        
        while let Ok(message) = self.context.rx.try_recv() {
            if let Ok(keys) = serde_json::from_str::<GetKeysResponse>(&message) {
                if !keys.webroot_key.is_empty() || !keys.superanti_key.is_empty() {
                    self.context.keys = keys;
                }
                self.context.spinner = false;
            } else {
                self.context.spinner = false;
            }
        }

        if let Ok(computer_data) = self.context.computer_data_rx.try_recv() {
            self.context.computer_data = computer_data.clone();
            for disk in &self.context.computer_data.drives {
                self.context.disk_num += 1;
                if let Some(disks_arr) = self.context.disks.as_array_mut() {
                    let disk_json = serde_json::to_value(&disk).unwrap_or_default();
                    disks_arr.push(disk_json);
                } else {
                    log::debug!("Expected self.context.drives to be an Array");
                }
            }
            if let Some(seb_inf) = &self.context.computer_data.seb_info {
                log::info!("SEB: {seb_inf:#?}");
            }

            let client_hash = generate_client_id(
                self.context.computer_data.hostname.clone(), 
                self.context.computer_data.cpu.trim().to_string()
            );

            let url_string = format!(
                "{}:{}", 
                self.context.computer_data.hostname.clone(), 
                client_hash.split_at(9).0
            );

            self.context.client_title = url_string.clone();

            self.context.url = Some(
                format!(
                    "{WS_CLIENT_URL}&room_id={}",
                    url_string.clone()
                )
            );
            
            self.context.client_uuid = RecordId::from_table_key(
                CONNECTED_CLIENT_TABLE.to_string(), 
                url_string.clone().as_str()
            );
        }

        if let Ok(antivirus) = self.context.current_antivirus_rx.try_recv() {
            let cps = &mut self.context.current_antivirus.clone();
            for (name, is_installed) in antivirus {
                match is_installed {
                    Some(true) => {
                        *cps += "\n";
                        *cps += &format!("{name}");
                    }
                    _ => {}
                }
            }
        }

        while let Ok(res) = self.context.bytes_rx.try_recv() {
            ctx.request_repaint();
            self.context.progress.1 = res.1 as f32;
            self.context.progress.0 += res.0 as f32;
            if res.0 == res.1 {
                self.context.progress = (0.0, 0.0);
                let current_exe = std::env::current_dir().unwrap().join("git-MasterTech.exe");
                #[cfg(target_os = "windows")]
                {
                    use std::os::windows::process::CommandExt;
                    std::process::Command::new("cmd")
                        .arg("/C")
                        .arg(&current_exe)
                        .creation_flags(0x00000010) // CREATE_NEW_CONSOLE flag
                        .creation_flags(0x00000008) // DETACHED_PROCESS flag
                        .spawn()
                        .unwrap();
                }
                let replacement = self_replace::self_replace(&current_exe);
                log::info!("Replacement: {replacement:?}");
                let rm = std::fs::remove_file(&current_exe);
                log::info!("Removal: {rm:?}");

                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
        }

        if let Ok(keys) = self.context.cps_keys_rx.try_recv() {
            ctx.request_repaint();
            let key = keys.get(0).cloned().unwrap_or_default();
            if key.webroot_key.contains("Error") {
                let toast = &mut self.context.shared_ctx.toasts;
                let error_toast = Toast {
                    kind: ToastKind::Error,
                    text: "Error fetching Keys. Is SW\\/PCLCPS\\/O on ticket?".into(),
                    options: ToastOptions::default()
                        .show_progress(true)
                        .duration_in_seconds(6.0),
                };
                toast.add(error_toast);
            }
            self.context.keys = key;
        }

        while let Ok(copied_items) = self.context.copied_items_rx.try_recv() {
            log::info!("{copied_items}\n");
            ctx.request_repaint();
        }

        if let Ok(seb) = self.context.seb_channel.1.try_recv() {
            self.context.seb_info = seb.clone();
            let carbonite = seb.get(0).cloned().unwrap_or_default();
            self.context.computer_data.seb_info = Some(LocalSebData {
                InstalledDeviceId: carbonite.device_id.clone(),
                InstallInstanceId: carbonite.device_id.clone(),
                ActivationCode: carbonite.activation_code.clone(),
                InstallVersion: carbonite.client_version.clone(),
                MachineName: carbonite.device_name.clone(),
                ExtendedSeb: Some(ExtendedSeb {
                    email: carbonite.email.clone(),
                    phone: carbonite.phone.clone(),
                    userid: carbonite.userid.clone(),
                    device_name: carbonite.device_name.clone(),
                    device_id: carbonite.device_id.clone(),
                    state: carbonite.state.clone(),
                    usage_gb: carbonite.usage_gb.clone(),
                    date_device_created: carbonite.date_device_created.clone(),
                    activated: carbonite.activated.clone(),
                    activation_code: carbonite.activation_code.clone(),
                    last_complete_backup: carbonite.last_complete_backup.clone(),
                    last_client_status_update: carbonite.last_client_status_update.clone(),
                    id_recurly_account: carbonite.id_recurly_account.clone(),
                    date_last_scan: carbonite.date_last_scan.clone(),
                    date_email_sent: carbonite.date_email_sent.clone(),
                    date_canceled_account: carbonite.date_canceled_account.clone(),
                    date_deleted_account: carbonite.date_deleted_account.clone(),
                    current_period_ends_at: carbonite.current_period_ends_at.clone(),
                    date_modified: carbonite.date_modified.clone(),
                    date_created: carbonite.date_created.clone(),
                }),
                ..Default::default()
            });
            ctx.request_repaint();
        }
        
        // if let Some(dialog) = &mut self.context.open_file_dialog {
        //     if dialog.show(ctx).selected() {
        //         if let Some(file) = dialog.path() {
        //             self.context.opened_file = Some(file.to_path_buf());
        //         }
        //     }
        // }
    }
}
