use super::{filesystem::system_info::generate_client_id, utilities::load_encrypted_user_data, app_state::MasterTechApp, tabs::github::get_github_releases};
use displays::{app_state::AppState, pages::login_page::HASH, ui_tools::{decode_style, encode_style, theme_config::set_custom_style, toasts::{Toast, ToastKind, ToastOptions}}};
use database::{schema::{CustomerData, ExtendedSeb, LiveTaskPayload, LocalSebData, TicketData, CONNECTED_CLIENT_TABLE}, Database, WS_CLIENT_URL};
use eframe::{egui::{Context, ViewportCommand}, Frame};
use database::schema::GetKeysResponse;
use std::sync::{atomic::Ordering, Arc};
use surrealdb::RecordId;
use tokio::spawn;

impl MasterTechApp {
    pub fn first_run(&mut self, ctx: &Context, frame: &mut Frame) {
        self.context.shared_ctx.first_run = false;
        let custom_style = set_custom_style(&self.context.shared_ctx.theme_config);
        ctx.set_style((*custom_style).clone());

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

    pub fn load_data(&mut self, ctx: &Context) {
        if let Some(usr) = self.context.shared_ctx.current_user.clone() {
            match decode_style(&usr.get_color_scheme()) {
                Ok(color_settings) => self.context.shared_ctx.theme = Arc::new(color_settings),
                Err(e) => log::error!("Error setting theme config: {e:?}"),
            }
            ctx.request_repaint();
            self.context.connect(ctx.clone());
            self.context.show_ws_viewport.store(true, Ordering::Relaxed);
        }
    }

    pub fn receive(&mut self, frame: &mut eframe::Frame, ctx: &Context) {
        if self.context.shared_ctx.first_run { self.first_run(ctx, frame); }
        self.context.shared_ctx.receive_shared(frame, ctx);
        self.receive_prestashop(frame);
        self.receive_database(ctx, frame);
        self.receive_github();
        self.viewport_loader(ctx);
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
                            storage.set_string("user_settings", serde_json::to_string(&user.get_user_settings()).unwrap_or_default());
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
