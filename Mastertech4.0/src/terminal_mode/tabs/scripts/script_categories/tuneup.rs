use crate::{tabs::tur_sheet::get_ticket::SendRequest, terminal_mode::tabs::{checklist::Category, script_categories::{get_data_transfer_candidates, disable_hibernation_and_sleep}, scripts::Reporter, ScriptsTab}, utilities::{scripts::{antivirus::{kill_sas_processes, run_sas_quick_scan, start_webroot_scan}, install_sas, install_supereasybackup, install_webroot}, windows::windows_update::install_windows_updates}};
use crate::{utilities::{scripts::{install_program, disable_hkcu_startup_entries, onedrive_in_use, remove_copilot_appx}, windows::{registry::{align_taskbar_left, disable_account_notifications, disable_content_delivery_allowed, disable_copilot, disable_lockscreen_notifications, disable_notifications, disable_recent_items_tracking, disable_silent_installed_apps_enabled, disable_start_account_notifications, disable_subscribed_content_enabled, disable_system_pane_suggestions_enabled, enable_more_pins_layout, remove_chat_from_taskbar}}}};


impl <'a> ScriptsTab <'a> {
    /// When both "Activate SuperAnti" and "Change SuperAntiSpyware settings" are
    /// selected in the same run, we must combine them into a single sequential
    /// workflow so the settings change waits until activation finishes.
    /// This flag is set by `activate_superanti` when it detects the combo, and
    /// checked by `change_superantispyware_settings` to skip (it's handled in
    /// the combined flow).
    fn both_sas_scripts_selected(&self) -> bool {
        let selected = self.get_selected_scripts();
        let has_activate = selected.iter().any(|s| s.text == "Activate SuperAnti");
        let has_settings = selected.iter().any(|s| s.text == "Change SuperAntiSpyware settings");
        has_activate && has_settings
    }

    pub fn handle_tuneup(&mut self, item_text: &str, category: &Category){
        self.current_reporter.replace(Reporter::Tuneup);
        self.log_message(&format!("Starting Tuneup script: {}", item_text));
        match item_text {
            "Disable Sleep / Hibernation" => self.disable_sleep_hibernation(item_text, category),
            "Activate Webroot" => self.activate_webroot(item_text, category),
            "Activate SuperAnti" => self.activate_superanti(item_text, category),
            "Activate SEB" => self.activate_seb(item_text, category),
            "Run SuperAntiSpyware Scan" => self.run_superantispyware_scan(item_text, category),
            "Run Webroot Scan" => self.run_webroot_scan(item_text, category),
            "Run Junkware Category" => self.run_junkware_category(item_text, category),
            "Data Transfer" => self.data_transfer(item_text, category),
            "Install LibreOffice" => self.install_libreoffice(item_text, category),
            "Disable proxy settings" => self.disable_proxy_settings(item_text, category),
            "Disable Notifications" => self.disable_notifications(item_text, category),
            "Change SuperAntiSpyware settings" => self.change_superantispyware_settings(item_text, category),
            "Disable Startup Apps" => self.disable_startup_apps(item_text, category),
            "Unpin Copilot" => self.unpin_copilot(item_text, category),
            "Align Taskbar to left" => self.align_taskbar_left(item_text, category),
            "Check Updates" => self.check_updates(item_text, category),
            "Install Windows Updates" => self.install_windows_updates(item_text, category),
            "Change Timezone to Mountain" => self.change_timezone_to_mountain(item_text, category),
            "Disable BitLocker" => self.disable_bitlocker(item_text, category),
            _ => {
                self.log_message(&format!("Unknown Tuneup script: {}", item_text));
            }
        }
    }

    // Tuneup Items
    pub fn disable_sleep_hibernation(&mut self, item_text: &str, category: &Category) {
        match disable_hibernation_and_sleep() {
            Ok(_) => {
                self.log_message("Sleep / display / hibernate timeouts set to never; hibernation off.");
                self.update_checklist(category.clone(), item_text, true);
            }
            Err(e) => {
                self.log_message(format!("Disable Sleep / Hibernation failed: {e:?}"));
                self.update_checklist(category.clone(), item_text, false);
            }
        }
    }
    
    pub fn install_windows_updates(&mut self, item_text: &str, category: &Category) {
        self.log_message("Checking internet before Windows Updates...");
        let tx = self.update_log_tx.clone();
        let log_tx = self.script_log_tx.clone();
        let checklist_tx = self.checklist_completion_tx.clone();
        let category_clone = category.clone();
        let item_clone = item_text.to_string();

        tokio::spawn(async move {
            if let Err(e) = crate::utilities::windows::net_adapter::ensure_internet_connected().await {
                let _ = log_tx.try_send(format!("No internet for Windows Updates: {e}"));
                let _ = checklist_tx.try_send((category_clone, item_clone, false));
                return;
            }
            let _ = log_tx.try_send("Internet confirmed, starting Windows Updates...".into());
            std::thread::spawn(move || {
                let _ = install_windows_updates(tx, true, true);
            });
            let _ = checklist_tx.try_send((category_clone, item_clone, true));
        });
    }

    pub fn activate_webroot(&mut self, item_text: &str, category: &Category) {
        if self.service_number.is_empty() {
            self.log_message("Webroot activation requires SO number.");
            return;
        }

        let so = self.service_number.clone();
        let tx = self.progress_tx.clone();
        let client = self.client.clone();
        let log_tx = self.script_log_tx.clone();
        let checklist_tx = self.checklist_completion_tx.clone();
        let category_clone = category.clone();
        let item_text_clone = item_text.to_string();

        tokio::spawn(async move {
            let cps_keys = SendRequest::get_cps(so, client.clone()).await.unwrap_or_default();
            let key = cps_keys.get(0).cloned().unwrap_or_default();
            let _ = log_tx.try_send(format!("Webroot key: {}", key.webroot_key));

            let success = match install_webroot(key.webroot_key, client, tx).await {
                Ok(outcome) => {
                    let _ = log_tx.try_send(format!("Webroot licensed and active ({outcome})"));
                    if outcome.reboot_recommended() {
                        let _ = log_tx.try_send(format!(
                            "{} Webroot changed on disk ({outcome}) — reboot to finalize activation",
                            displays::scripts::REBOOT_RECOMMENDED_MARKER
                        ));
                    }
                    true
                }
                Err(e) => { let _ = log_tx.try_send(format!("Webroot install error: {e}")); false }
            };

            let _ = checklist_tx.try_send((category_clone, item_text_clone, success));
        });
    }

    pub fn activate_superanti(&mut self, item_text: &str, category: &Category) {
        if self.service_number.is_empty() {
            self.log_message("SuperAnti activation requires SO number.");
            return;
        }

        let also_change_settings = self.both_sas_scripts_selected();

        let killed = kill_sas_processes();
        self.log_message(format!("Killed {killed} SAS processes before install"));
        Self::wait_until_sas_not_running(5);

        let so = self.service_number.clone();
        let tx = self.progress_tx.clone();
        let client = self.client.clone();
        let log_tx = self.script_log_tx.clone();
        let checklist_tx = self.checklist_completion_tx.clone();
        let category_clone = category.clone();
        let item_text_clone = item_text.to_string();

        tokio::spawn(async move {
            let cps_keys = SendRequest::get_cps(so, client.clone()).await.unwrap_or_default();
            let key = cps_keys.get(0).cloned().unwrap_or_default();
            let _ = log_tx.try_send(format!("SuperAnti key: {}", key.superanti_key));

            // install_sas handles activation itself: /REGCODE during silent install
            // for fresh installs, /autoregister:KEY against the existing exe when
            // SAS is already present.
            let success = match install_sas(key.superanti_key, client, tx).await {
                Ok(_) => { let _ = log_tx.try_send("SAS installed successfully".into()); true }
                Err(e) => { let _ = log_tx.try_send(format!("SAS install error: {e}")); false }
            };

            let killed = kill_sas_processes();
            let _ = log_tx.try_send(format!("Post-install killed {killed} SAS processes"));
            Self::wait_until_sas_not_running(10);

            if also_change_settings && success {
                let _ = log_tx.try_send("Combined flow: writing settings + scheduled tasks...".into());
                std::thread::sleep(std::time::Duration::from_secs(2));

                use crate::utilities::scripts::antivirus::sas_tasks::configure_sas_scheduled_tasks;
                match configure_sas_scheduled_tasks() {
                    Ok((update_guid, scan_guid)) => {
                        let _ = log_tx.try_send(format!("Created SAS update task: {update_guid}"));
                        let _ = log_tx.try_send(format!("Created SAS quick-scan task: {scan_guid}"));
                        let _ = log_tx.try_send("SAS settings + tasks applied.".into());
                        let _ = checklist_tx.try_send((category_clone.clone(), "Change SuperAntiSpyware settings".into(), true));
                    }
                    Err(e) => {
                        let _ = log_tx.try_send(format!("Failed to configure SAS: {e}"));
                        let _ = checklist_tx.try_send((category_clone.clone(), "Change SuperAntiSpyware settings".into(), false));
                    }
                }
            }

            const SAS_EXE: &str = r"C:\Program Files\SUPERAntiSpyware\SUPERAntiSpyware.exe";
            if std::path::Path::new(SAS_EXE).exists() {
                let _ = log_tx.try_send("Relaunching SUPERAntiSpyware...".into());
                if let Err(e) = std::process::Command::new(SAS_EXE).spawn() {
                    let _ = log_tx.try_send(format!("Failed to relaunch SAS: {e}"));
                } else {
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    if Self::is_sas_running() {
                        let _ = log_tx.try_send("SUPERAntiSpyware is running.".into());
                    } else {
                        let _ = log_tx.try_send("Warning: SAS was launched but process not detected.".into());
                    }
                }
            }

            let _ = checklist_tx.try_send((category_clone, item_text_clone, success));
        });
    }

    pub fn activate_seb(&mut self, item_text: &str, category: &Category) {
        let service_number = self.service_number.clone();
        let email = self.customer_email.clone();
        
        if service_number.is_empty() || email.is_empty() {
            self.log_message("SEB activation requires SO number or email.");
            return;
        }

        let client = self.client.clone();
        let tx = self.progress_tx.clone();
        tokio::spawn(async move {
            match install_supereasybackup(email, client, tx).await {
                Ok(_) => log::info!("Installed SEB"),
                Err(e) => log::error!("Error Installing SEB: {e:?}"),
            }
        });
        
        self.update_checklist(category.clone(), item_text, false);
    }

    pub fn run_webroot_scan(&mut self, item_text: &str, category: &Category) {
        match start_webroot_scan() {
            Ok(message) => {
                self.log_message(message);
                self.update_checklist(category.clone(), item_text, true);
            }
            Err(e) => {
                self.log_message(format!("Webroot scan failed: {e:?}"));
                self.update_checklist(category.clone(), item_text, false);
            }
        }
    }

    /// Starts SAS's own Quick Scan scheduled task, creating it first if needed.
    pub fn run_superantispyware_scan(&mut self, item_text: &str, category: &Category) {
        self.log_message("Starting SuperAntiSpyware quick scan...");
        let log_tx = self.script_log_tx.clone();
        let checklist_tx = self.checklist_completion_tx.clone();
        let category_clone = category.clone();
        let item_clone = item_text.to_string();
        std::thread::spawn(move || {
            match run_sas_quick_scan() {
                Ok(messages) => {
                    for message in messages {
                        let _ = log_tx.try_send(message);
                    }
                    let _ = checklist_tx.try_send((category_clone, item_clone, true));
                }
                Err(e) => {
                    let _ = log_tx.try_send(format!("SuperAntiSpyware scan failed: {e:?}"));
                    let _ = checklist_tx.try_send((category_clone, item_clone, false));
                }
            }
        });
    }

    pub fn run_junkware_category(&mut self, item_text: &str, category: &Category) {
        self.remove_junkware(Some(item_text));
        self.log_message("Junkware category cleanup completed.");
        self.update_checklist(category.clone(), item_text, true);
    }

    pub fn data_transfer(&mut self, item_text: &str, category: &Category) {
        self.loading = true;
        self.data_path_buttons.clear();
        self.log_message("Finding Data transfer candidates");
        let tx = self.path_size_tx.clone();
        std::thread::spawn(move || {
            let paths = get_data_transfer_candidates();
            match paths {
                Ok(paths) => { let _ = tx.try_send(paths); },
                Err(e) => log::error!("Error getting paths: {e:?}"),
            };
        });
        // self.log_message("Data transfer completed.");
        self.update_checklist(category.clone(), item_text, true);
    }

    pub fn install_libreoffice(&mut self, item_text: &str, category: &Category) {
        let download_url = "https://ninite.com/libreoffice/ninite.exe";
        let progress_tx = self.progress_tx.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            let res = install_program(download_url.to_string(), client, progress_tx).await;
            log::info!("Downloaded libre office: {res:?}");
        });
        self.update_checklist(category.clone(), item_text, false);
    }

    pub fn disable_proxy_settings(&mut self, item_text: &str, category: &Category) {
        self.log_message("Proxy settings disable not implemented.");
        self.update_checklist(category.clone(), item_text, false);
    }

    pub fn disable_notifications(&mut self, item_text: &str, category: &Category) {
        match disable_notifications() {
            Ok(results) => self.log_message(&format!("Push Notifications => {results:#?}")),
            Err(e) => self.log_message(&format!("Push Notifications => {e:?}")),
        }
        match disable_lockscreen_notifications() {
            Ok(results) => self.log_message(&format!("disable_lockscreen_notifications => {results:#?}")),
            Err(e) => self.log_message(&format!("Error with disable_lockscreen_notifications => {e:?}")),
        }
        // match disable_copilot() {
        //     Ok(results) => self.log_message(&format!("disable_copilot => {results:#?}")),
        //     Err(e) => self.log_message(&format!("Error with disable_copilot => {e:?}")),
        // }
        match disable_content_delivery_allowed() {
            Ok(results) => self.log_message(&format!("disable_content_delivery_allowed => {results:#?}")),
            Err(e) => self.log_message(&format!("Error with disable_content_delivery_allowed => {e:?}")),
        }
        match disable_silent_installed_apps_enabled() {
            Ok(results) => self.log_message(&format!("disable_silent_installed_apps_enabled => {results:#?}")),
            Err(e) => self.log_message(&format!("Error with disable_silent_installed_apps_enabled => {e:?}")),
        }
        match disable_subscribed_content_enabled() {
            Ok(results) => self.log_message(&format!("disable_subscribed_content_enabled => {results:#?}")),
            Err(e) => self.log_message(&format!("Error with disable_subscribed_content_enabled => {e:?}")),
        }
        match disable_system_pane_suggestions_enabled() {
            Ok(results) => self.log_message(&format!("disable_system_pane_suggestions_enabled => {results:#?}")),
            Err(e) => self.log_message(&format!("Error with disable_system_pane_suggestions_enabled => {e:?}")),
        }
        match disable_account_notifications() {
            Ok(results) => self.log_message(&format!("disable_account_notifications => {results:#?}")),
            Err(e) => self.log_message(&format!("Error with disable_account_notifications => {e:?}")),
        }
        match enable_more_pins_layout() {
            Ok(results) => self.log_message(&format!("enable_more_pins_layout => {results:#?}")),
            Err(e) => self.log_message(&format!("Error with enable_more_pins_layout => {e:?}")),
        }
        match disable_start_account_notifications() {
            Ok(results) => self.log_message(&format!("disable_start_account_notifications => {results:#?}")),
            Err(e) => self.log_message(&format!("Error with disable_start_account_notifications => {e:?}")),
        }
        match disable_recent_items_tracking() {
            Ok(results) => self.log_message(&format!("disable_recent_items_tracking => {results:#?}")),
            Err(e) => self.log_message(&format!("Error with disable_recent_items_tracking => {e:?}")),
        }
        match remove_chat_from_taskbar() {
            Ok(results) => self.log_message(&format!("remove_chat_from_taskbar => {results:#?}")),
            Err(e) => self.log_message(&format!("Error with remove_chat_from_taskbar => {e:?}")),
        }

        self.update_checklist(category.clone(), item_text, true);
    }

    pub fn change_superantispyware_settings(&mut self, item_text: &str, category: &Category) {
        // If both scripts were selected, activate_superanti already handles settings
        // in its combined flow — skip here to avoid a race.
        if self.both_sas_scripts_selected() {
            self.log_message("Settings will be applied by the Activate SuperAnti combined flow.");
            return;
        }

        use crate::utilities::scripts::antivirus::sas_tasks::configure_sas_scheduled_tasks;
        const SAS_EXE_PATH: &str = r"C:\Program Files\SUPERAntiSpyware\SUPERAntiSpyware.exe";

        if !std::path::Path::new(SAS_EXE_PATH).exists() {
            self.log_message("SUPERAntiSpyware not installed yet. Waiting for it (polling up to 5 min)...");
            let log_tx = self.script_log_tx.clone();
            let checklist_tx = self.checklist_completion_tx.clone();
            let category_clone = category.clone();
            let item_text_clone = item_text.to_string();
            std::thread::spawn(move || {
                const POLL_INTERVAL_SECS: u64 = 2;
                const TIMEOUT_SECS: u64 = 300;
                let start = std::time::Instant::now();
                while !std::path::Path::new(SAS_EXE_PATH).exists() {
                    if start.elapsed().as_secs() >= TIMEOUT_SECS {
                        let _ = log_tx.try_send("Timeout waiting for SUPERAntiSpyware to be installed.".into());
                        let _ = checklist_tx.try_send((category_clone, item_text_clone, false));
                        return;
                    }
                    std::thread::sleep(std::time::Duration::from_secs(POLL_INTERVAL_SECS));
                }
                let _ = log_tx.try_send("SUPERAntiSpyware detected; applying settings...".into());
                let killed = kill_sas_processes();
                let _ = log_tx.try_send(format!("Killed {killed} SAS processes"));
                Self::wait_until_sas_not_running(10);
                std::thread::sleep(std::time::Duration::from_secs(2));

                let success = match configure_sas_scheduled_tasks() {
                    Ok((update_guid, scan_guid)) => {
                        let _ = log_tx.try_send(format!("Created SAS update task: {update_guid}"));
                        let _ = log_tx.try_send(format!("Created SAS quick-scan task: {scan_guid}"));
                        let _ = log_tx.try_send("SAS scheduled tasks configured. Relaunching SAS.".into());
                        if let Err(e) = std::process::Command::new(SAS_EXE_PATH).spawn() {
                            let _ = log_tx.try_send(format!("Failed to relaunch SAS: {e}"));
                        } else {
                            std::thread::sleep(std::time::Duration::from_secs(3));
                            if Self::is_sas_running() {
                                let _ = log_tx.try_send("SUPERAntiSpyware is running.".into());
                            } else {
                                let _ = log_tx.try_send("Warning: SAS launched but process not detected.".into());
                            }
                        }
                        true
                    }
                    Err(e) => {
                        let _ = log_tx.try_send(format!("Failed to configure SAS tasks: {e}"));
                        false
                    }
                };
                let _ = checklist_tx.try_send((category_clone, item_text_clone, success));
            });
            return;
        }

        // SAS is already installed — run standalone settings change
        self.log_message("Killing SAS processes before modifying settings...");
        let killed = kill_sas_processes();
        self.log_message(format!("Killed {killed} SAS processes"));
        Self::wait_until_sas_not_running(10);
        std::thread::sleep(std::time::Duration::from_secs(2));

        match configure_sas_scheduled_tasks() {
            Ok((update_guid, scan_guid)) => {
                self.log_message(format!("Created SAS update task: {update_guid}"));
                self.log_message(format!("Created SAS quick-scan task: {scan_guid}"));
                self.log_message("SAS scheduled tasks configured. Relaunching SAS.");
                if let Err(e) = std::process::Command::new(SAS_EXE_PATH).spawn() {
                    self.log_message(format!("Failed to relaunch SAS: {e}"));
                } else {
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    if Self::is_sas_running() {
                        self.log_message("SUPERAntiSpyware is running.");
                    } else {
                        self.log_message("Warning: SAS launched but process not detected.");
                    }
                }
                self.update_checklist(category.clone(), item_text, true);
            }
            Err(e) => {
                self.log_message(format!("Failed to configure SAS tasks: {e}"));
                self.update_checklist(category.clone(), item_text, false);
            }
        }
    }

    /// Disables Edge auto-launch startup entries, and the OneDrive startup
    /// entry when no OneDrive account is signed in.
    pub fn disable_startup_apps(&mut self, item_text: &str, category: &Category) {
        let mut all_ok = true;

        match disable_hkcu_startup_entries("msedge") {
            Ok(messages) => for message in messages {
                self.log_message(format!("Edge: {message}"));
            },
            Err(e) => {
                all_ok = false;
                self.log_message(format!("Edge startup: {e:?}"));
            }
        }

        if onedrive_in_use() {
            self.log_message("OneDrive has a signed-in account; leaving its startup entry enabled.");
        } else {
            match disable_hkcu_startup_entries("onedrive") {
                Ok(messages) => for message in messages {
                    self.log_message(format!("OneDrive: {message}"));
                },
                Err(e) => {
                    all_ok = false;
                    self.log_message(format!("OneDrive startup: {e:?}"));
                }
            }
            // Stop the running instance so sign-in prompts end immediately.
            use std::os::windows::process::CommandExt;
            let _ = std::process::Command::new("taskkill")
                .args(["/F", "/IM", "OneDrive.exe"])
                .creation_flags(0x08000000)
                .output();
            self.log_message("OneDrive not signed in: killed OneDrive.exe");
        }

        self.update_checklist(category.clone(), item_text, all_ok);
    }

    pub fn unpin_copilot(&mut self, item_text: &str, category: &Category) {
        let mut all_ok = true;
        match disable_copilot() {
            Ok(results) => {
                for result in results.iter() {
                    self.log_message(result);
                }
            },
            Err(e) => {
                all_ok = false;
                self.log_message(format!("Error disabling copilot: {e:?}"));
            }
        }
        match remove_copilot_appx() {
            Ok(messages) => for message in messages {
                self.log_message(message);
            },
            Err(e) => {
                all_ok = false;
                self.log_message(format!("Error removing Copilot app: {e:?}"));
            }
        }
        self.update_checklist(category.clone(), item_text, all_ok);
    }

    pub fn align_taskbar_left(&mut self, item_text: &str, category: &Category) {
        match align_taskbar_left() {
            Ok(messages) => {
                for message in messages {
                    self.log_message(&format!("TaskBarAlignment => {}", message.trim()));
                }
                self.update_checklist(category.clone(), item_text, true);
            },
            Err(e) => {
                self.log_message(&format!("TaskBarAlignment => {e:?}"));
                self.update_checklist(category.clone(), item_text, false);
            }
        }
    }

    // WindowsUpdates Items
    pub fn check_updates(&mut self, item_text: &str, category: &Category) {
        self.log_message("Checking for Windows updates...");
        let tx = self.update_log_tx.clone();
        std::thread::spawn(move || {
            let _ = install_windows_updates(tx, false, false); // Check only
        });
        self.log_message("Windows update check finished.");
        self.update_checklist(category.clone(), item_text, true);
    }

    /// Returns true if any SUPERAntiSpyware-related process is currently running.
    fn is_sas_running() -> bool {
        use crate::utilities::scripts::get_running_processes;
        if let Ok(processes) = get_running_processes() {
            return processes.iter().any(|p| {
                let name = p.process_name.to_lowercase();
                let exe = p.exe_path.clone().unwrap_or_default().to_lowercase();
                name.contains("sascore")
                    || name.contains("sastask")
                    || name.contains("superanti")
                    || exe.contains("superanti")
            });
        }
        false
    }

    /// Polls until no SAS processes are detected, up to `max_attempts` iterations
    /// (1 second apart). Ensures taskkill has fully terminated the processes
    /// before proceeding.
    fn wait_until_sas_not_running(max_attempts: u32) {
        for _ in 0..max_attempts {
            if !Self::is_sas_running() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
        log::warn!("SAS processes still detected after {max_attempts}s of waiting");
    }

    pub fn change_timezone_to_mountain(&mut self, item_text: &str, category: &Category) {
        use powershell_script::PsScriptBuilder;
        self.log_message("Setting timezone to Mountain Standard Time...");
        let ps = PsScriptBuilder::new()
            .no_profile(true)
            .non_interactive(true)
            .hidden(true)
            .print_commands(false)
            .build();
        match ps.run("tzutil /s \"Mountain Standard Time\"") {
            Ok(_) => {
                self.log_message("Timezone set to Mountain Standard Time");
                self.update_checklist(category.clone(), item_text, true);
            },
            Err(e) => self.log_message(&format!("Failed to set timezone: {e:?}")),
        }
    }

    pub fn disable_bitlocker(&mut self, item_text: &str, category: &Category) {
        use powershell_script::PsScriptBuilder;
        self.log_message("Checking BitLocker status...");
        let ps = PsScriptBuilder::new()
            .no_profile(true)
            .non_interactive(true)
            .hidden(true)
            .print_commands(false)
            .build();
        let check = r#"
            $volumes = Get-BitLockerVolume -ErrorAction SilentlyContinue
            if ($volumes) {
                $volumes | ForEach-Object { "$($_.MountPoint): $($_.VolumeStatus) / $($_.ProtectionStatus)" }
            } else { "No BitLocker volumes found" }
        "#;
        if let Ok(out) = ps.run(check) {
            self.log_message(&format!("BitLocker status:\n{}", out.stdout().unwrap_or_default()));
        }
        let disable = r#"
            $volumes = Get-BitLockerVolume -ErrorAction SilentlyContinue |
                Where-Object { $_.ProtectionStatus -eq 'On' -or $_.VolumeStatus -ne 'FullyDecrypted' }
            if ($volumes) {
                foreach ($vol in $volumes) {
                    Disable-BitLocker -MountPoint $vol.MountPoint -ErrorAction SilentlyContinue | Out-Null
                    "Disabling BitLocker on $($vol.MountPoint)"
                }
            } else { "No BitLocker-protected volumes to disable" }
        "#;
        let ps2 = PsScriptBuilder::new()
            .no_profile(true)
            .non_interactive(true)
            .hidden(true)
            .print_commands(false)
            .build();
        match ps2.run(disable) {
            Ok(out) => {
                self.log_message(&out.stdout().unwrap_or_default());
                self.update_checklist(category.clone(), item_text, true);
            },
            Err(e) => self.log_message(&format!("Failed to disable BitLocker: {e:?}")),
        }
    }
}