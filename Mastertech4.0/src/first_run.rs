use super::{filesystem::system_info::{ComputerInfo, generate_client_id}, utilities::load_encrypted_user_data, app_state::MasterTechApp, tabs::github::get_github_releases};
use displays::{app_state::AppState, pages::login_page::HASH, ui_tools::{theme_config::ThemeConfig, toasts::{Toast, ToastKind, ToastOptions}}};
use database::{schema::{ComputerData, ExtendedSeb, LocalSebData, CONNECTED_CLIENT_TABLE}, Database, WS_CLIENT_URL};
use eframe::egui::{Context, ViewportCommand};
use database::schema::GetKeysResponse;
use std::sync::{Arc, Condvar, Mutex};
use std::sync::atomic::Ordering;
use log::{debug, error, info};
use surrealdb::RecordId;
use tokio::spawn;

impl MasterTechApp {
    pub fn first_run(&mut self) {
        self.context.shared_ctx.first_run = false;
        let github_tx = self.context.github_releases_channel.0.clone();
        let client = self.context.client.clone();
        spawn(async move {
            match get_github_releases(github_tx, client).await {
                Ok(_) => info!("get_github_releases ran ok"),
                Err(e) => error!("Error getting github releases: {e:?}"),
            }
        });
        
        let tx = self.context.shared_ctx.db_tx.clone();
        let pair = Arc::new(
            (Mutex::new(ComputerData::default()), Condvar::new())
        );
        let pair_clone = Arc::clone(&pair);

        spawn(async move {
            match ComputerData::default().get_computer_data().await {
                // sysinfo_tx
                Ok(data) => {
                    let (lock, cvar) = &*pair_clone;
                    let mut comp_data = lock.lock().unwrap();
                    *comp_data = data;
                    // info!("Computer Data: {comp_data:?}");
                    cvar.notify_one();
                }
                Err(e) => error!("Error getting specs: {e:?}"),
            }
        });

        // Wait for the spawned task to complete and notify the condition variable
        let (lock, cvar) = &*pair;
        let mut comp_data = lock.lock().unwrap();
        while comp_data.cpu.is_empty() {
            comp_data = cvar.wait(comp_data).unwrap();
        }
        // Access the shared data after notification
        self.context.computer_data = comp_data.clone();
        for disk in &self.context.computer_data.drives {
            self.context.disk_num += 1;
            if let Some(disks_arr) = self.context.disks.as_array_mut() {
                let disk_json = serde_json::to_value(&disk).unwrap_or_default();
                disks_arr.push(disk_json);
            } else {
                debug!("Expected self.context.drives to be an Array");
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

        let loaded_data = load_encrypted_user_data(HASH);
        match loaded_data {
            Some(login) => {
                spawn(async move {
                    let db = Database::new(login.username, login.password, None).await;
                    match tx.try_send(db) {
                        Ok(_) => drop(tx),
                        Err(e) => error!("Error sending specs: {e:?}"),
                    }
                });

                #[cfg(target_os = "windows")]
                {
                    let cps = &mut self.context.current_antivirus.clone();
                    let installed_antivirus = ComputerData::get_antivirus()
                        .map_err(|e| {
                            *cps += format!("Error checking antivirus: {e}\n").as_str()
                        })
                        .unwrap_or(Vec::new());

                    for (name, is_installed) in installed_antivirus {
                        match is_installed {
                            Some(true) => {
                                *cps += "\n";
                                *cps += &format!("{name}");
                            }
                            _ => {}
                        }
                    }
                }
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
            match serde_json::from_value::<ThemeConfig>(usr.get_color_scheme()) {
                Ok(color_settings) => self.context.shared_ctx.theme_config = color_settings.clone(),
                Err(e) => log::error!("Error setting theme config: {e:?}"),
            }
            ctx.request_repaint();
            self.context.connect(ctx.clone());
            self.context.show_ws_viewport.store(true, Ordering::Relaxed);
        }
    }

    pub fn receive(&mut self, ctx: &Context) {
        ctx.request_repaint_after_secs(0.5);
        while let Ok(message) = self.context.rx.try_recv() {
            if let Ok(info) = serde_json::from_str::<GetKeysResponse>(&message) {
                if !info.webroot_key.is_empty() || !info.superanti_key.is_empty() {
                    self.context.keys = info;
                }
                self.context.spinner = false;
            } else {
                self.context.spinner = false;
            }
        }

        // if let Some(dialog) = &mut self.context.open_file_dialog {
        //     if dialog.show(ctx).selected() {
        //         if let Some(file) = dialog.path() {
        //             self.context.opened_file = Some(file.to_path_buf());
        //         }
        //     }
        // }

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
    }
}
