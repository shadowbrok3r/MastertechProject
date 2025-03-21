use crate::{filesystem::system_info::generate_client_id, tabs::tur_sheet::scaffold::AsanaResponse};
use displays::ui_tools::{theme_config::ThemeConfig, toasts::{Toast, ToastKind, ToastOptions}};
use database::{schema::{ComputerData, CONNECTED_CLIENT_TABLE}, Database, WS_CLIENT_URL};
use super::utilities::crypto::pass_hash::load_encrypted_user_data;
use super::filesystem::system_info::ComputerInfo;
use super::app_state::{AppState, MasterTechApp};
use eframe::egui::{Context, ViewportCommand};
use database::schema::GetKeysResponse;
use std::sync::{Arc, Condvar, Mutex};
use super::pages::login_page::HASH;
use std::sync::atomic::Ordering;
use log::{debug, error, info};
use surrealdb::RecordId;
use tokio::spawn;

impl MasterTechApp {
    pub fn first_run(&mut self) {
        self.context.first_run = false;
        // let x = std::env::current_exe().unwrap();
        // std::fs::rename( x, "Mastertech1").unwrap();
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
                    info!("Computer Data: {comp_data:?}");
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
            self.context.output_text += &format!("{:#?}", &seb_inf);
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
                let _ = self.context.app_state_tx.try_send(AppState::Login);
            }
        }
    }

    pub fn load_data(&mut self, ctx: &Context) {
        if let Some(usr) = self.context.shared_ctx.current_user.clone() {
            match serde_json::from_value::<ThemeConfig>(usr.user_settings.color_scheme.clone()) {
                Ok(color_settings) => self.context.shared_ctx.theme_config = color_settings.clone(),
                Err(e) => info!("Error setting theme config: {e:?}"),
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
            } else if let Ok(info) = serde_json::from_str::<AsanaResponse>(&message) {
                if let Some(e) = info.status {
                    self.context.output_text = format!("Status Code: {e:#?}");
                };
                self.context.output_text = format!("{:#?}", info.gid);
            } else {
                self.context.output_text = format!("{}", message);
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
            self.context.output_text = format!("Downloaded Bytes: {}/{}", &res.0, &res.1);
            self.context.progress.1 = res.1 as f32;
            self.context.progress.0 += res.0 as f32;
            if res.0 == res.1 {
                self.context.progress = (0.0, 0.0);
                self.context.output_text += "\nFinished";
                let current_path = std::env::current_dir().unwrap();
                // let linux_path = std::env::current_dir().unwrap();
                let mtech_path = current_path.join("git-MasterTech.exe");
                // let mtech_linux_path = linux_path.join("git-MasterTech");

                if mtech_path.exists() {
                    info!("Mastertech does exist at {:?}", mtech_path);
                    let mut mtech_cmd = std::process::Command::new(mtech_path);
                    if mtech_cmd.status().is_ok() {
                        info!("Mtech opened, closing current window");
                        ctx.send_viewport_cmd(ViewportCommand::Close);
                    }
                }
                // else if mtech_linux_path.exists() {
                //     let mut mtech_cmd = std::process::Command::new(mtech_linux_path);
                //     if mtech_cmd.status().is_ok() {
                //         info!("Mtech opened, closing current window");
                //         ctx.send_viewport_cmd(ViewportCommand::Close);
                //     }
                // }
            }
        }

        if let Ok(keys) = self.context.cps_keys_rx.try_recv() {
            ctx.request_repaint();
            if keys.webroot_key.contains("Error") {
                let toast = &mut self.context.shared_ctx.toasts;
                self.context.output_text =
                    "Error fetching Keys. Is SW\\/PCLCPS\\/O on ticket?".to_string();
                let error_toast = Toast {
                    kind: ToastKind::Error,
                    text: "Error fetching Keys. Is SW\\/PCLCPS\\/O on ticket?".into(),
                    options: ToastOptions::default()
                        .show_progress(true)
                        .duration_in_seconds(6.0),
                };
                toast.add(error_toast);
            }
            self.context.keys = keys;
        }

        if let Ok(state) = self.context.app_state_rx.try_recv() {
            info!("Got a new state: {state:?}");
            self.state = state;
            ctx.request_repaint();
        }

        while let Ok(copied_items) = self.context.copied_items_rx.try_recv() {
            self.context.output_text += &format!("{copied_items}\n");
            ctx.request_repaint();
        }

        if let Ok(seb) = self.context.seb_channel.1.try_recv() {
            self.context.json_editor.set_value(seb.clone()).unwrap();
            ctx.request_repaint();
        }
    }
}
