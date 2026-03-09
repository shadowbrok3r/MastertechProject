use crate::{tabs::tur_sheet::get_ticket::SendRequest, terminal_mode::tabs::{checklist::Category, script_categories::{get_data_transfer_candidates, disable_hibernation_and_sleep}, scripts::Reporter, ScriptsTab}, utilities::{scripts::{antivirus::kill_sas_processes, install_sas, install_supereasybackup, install_webroot, run_ps_script}, windows::windows_update::install_windows_updates}};
use crate::{utilities::{scripts::{install_program, StartupProgram, StartupState}, windows::{registry::{align_taskbar_left, disable_account_notifications, disable_content_delivery_allowed, disable_copilot, disable_lockscreen_notifications, disable_notifications, disable_recent_items_tracking, disable_silent_installed_apps_enabled, disable_start_account_notifications, disable_subscribed_content_enabled, disable_system_pane_suggestions_enabled, enable_more_pins_layout, remove_chat_from_taskbar}}}};


impl <'a> ScriptsTab <'a> {
    pub fn handle_tuneup(&mut self, item_text: &str, category: &Category){
        self.current_reporter.replace(Reporter::Tuneup);
        self.log_message(&format!("Starting Tuneup script: {}", item_text));
        match item_text {
            "Disable Sleep / Hibernation" => self.disable_sleep_hibernation(item_text, category),
            "Activate CPS" => self.activate_cps(item_text, category),
            "Activate SEB" => self.activate_seb(item_text, category),
            "Run Tron" => self.run_tron(item_text, category),
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
            _ => {
                self.log_message(&format!("Unknown Tuneup script: {}", item_text));
            }
        }
    }
    
    // Tuneup Items
    pub fn disable_sleep_hibernation(&mut self, item_text: &str, category: &Category) {
        match disable_hibernation_and_sleep() {
            Ok(disabled) => {
                if disabled {
                    self.log_message(format!("Disabled Sleep / Hibernation"));
                    self.update_checklist(category.clone(), item_text, disabled);
                } else {
                    self.log_message(format!("Sleep / Hibernation already disabled"));
                }
            }
            Err(e) => self.log_message(format!("Sleep / Hibernation already disabled? {e:?}")),
        }
    }
    
    pub fn install_windows_updates(&mut self, item_text: &str, category: &Category) {
        self.log_message("Running Windows Updates...");
        let tx = self.update_log_tx.clone();
        std::thread::spawn(move || {
            let _ = install_windows_updates(tx, true, true);
        });
        self.log_message("Windows Updates initiated.");
        self.update_checklist(category.clone(), item_text, true);
    }

    pub fn activate_cps(&mut self, item_text: &str, category: &Category) {
        if self.service_number.is_empty() {
            self.log_message("CPS activation requires SO number.");
            return;
        }

        let killed = kill_sas_processes();
        self.log_message(format!("Killed {killed} SAS processes before install"));

        let so = self.service_number.clone();
        let tx = self.progress_tx.clone();
        let client = self.client.clone();
        
        tokio::spawn(async move {
            let cps_keys = SendRequest::get_cps(so.clone(), client.clone()).await.unwrap_or_default();
            log::info!("CPS Request: {cps_keys:?}");
            let key = cps_keys.get(0).cloned().unwrap_or_default();
            let res = install_webroot(key.webroot_key.clone(), client.clone(), tx.clone()).await;
            log::info!("install_webroot Result: {res:?}");

            // install_sas now waits for the installer and re-kills + autoregisters internally
            let res = install_sas(key.superanti_key.clone(), client.clone(), tx).await;
            log::info!("install_sas Result: {res:?}");

            // Final kill after everything settles so the key sticks
            let killed = kill_sas_processes();
            log::info!("Post-install killed {killed} SAS processes");
        });

        self.update_checklist(category.clone(), item_text, true);
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

    pub fn run_tron(&mut self, item_text: &str, category: &Category) {
        self.log_message("Tron script not implemented yet.");
        self.update_checklist(category.clone(), item_text, false);   
    }

    pub fn run_webroot_scan(&mut self, item_text: &str, category: &Category) {
        for program in self.installed_programs.iter() {
            let display_name = program.display_name.clone().unwrap_or_default().to_lowercase();
            let publisher = program.publisher.clone().unwrap_or_default().to_lowercase();
            if display_name.contains("webroot")
                || display_name.contains("wrsa")
                || publisher.contains("webroot")
                || publisher.contains("wrsa")
            {
                let install_path = program.install_location.clone().unwrap_or_default();
                let res = run_ps_script(&format!("{install_path} -scan=\"C:\""));
                match res {
                    Ok(out) => self.log_message(format!("Webroot scan: {out}")),
                    Err(e) => self.log_message(format!("Error running Webroot scan: {e:?}")),
                }
            }
        }
        self.update_checklist(category.clone(), item_text, false);
    }

    /// TODO: NOT YET IMPLEMENTED
    pub fn run_superantispyware_scan(&mut self, item_text: &str, category: &Category) {
        self.log_message("SuperAntiSpyware scan not implemented.");  
        self.update_checklist(category.clone(), item_text, false);
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
        use crate::utilities::scripts::antivirus::sas_tasks::configure_sas_scheduled_tasks;

        let sas_exe = std::path::Path::new(r"C:\Program Files\SUPERAntiSpyware\SUPERAntiSpyware.exe");
        if !sas_exe.exists() {
            self.log_message("SUPERAntiSpyware is not installed yet. Install via Activate CPS first.");
            self.update_checklist(category.clone(), item_text, false);
            return;
        }

        self.log_message("Killing SAS processes before modifying database...");
        let killed = kill_sas_processes();
        self.log_message(format!("Killed {killed} SAS processes"));

        // Brief pause to let file locks release
        std::thread::sleep(std::time::Duration::from_secs(2));

        match configure_sas_scheduled_tasks() {
            Ok((update_guid, scan_guid)) => {
                self.log_message(format!("Created SAS update task: {update_guid}"));
                self.log_message(format!("Created SAS quick-scan task: {scan_guid}"));
                self.log_message("SAS scheduled tasks configured successfully.");
                self.update_checklist(category.clone(), item_text, true);
            }
            Err(e) => {
                self.log_message(format!("Failed to configure SAS tasks: {e}"));
                self.update_checklist(category.clone(), item_text, false);
            }
        }
    }

    /// TODO: NOT YET IMPLEMENTED
    pub fn disable_startup_apps(&mut self, item_text: &str, category: &Category) {
        if let Ok(programs) = StartupProgram::get_startup_programs() {
            for program in programs {
                log::info!("startup program -> {program:?}");
                if let Some(StartupState::Enabled) = program.decoded_state {
                    self.log_message(format!("startup program -> {program:?}")); 
                }
            }
        }
        self.update_checklist(category.clone(), item_text, false);
    }

    pub fn unpin_copilot(&mut self, item_text: &str, category: &Category) {
        match disable_copilot() {
            Ok(results) => {
                for result in results.iter() {
                    self.log_message(result);
                }
            },
            Err(e) => self.log_message(format!("Error disabling copilot: {e:?}")),
        }
        self.update_checklist(category.clone(), item_text, true);
    }

    pub fn align_taskbar_left(&mut self, item_text: &str, category: &Category) {
        match align_taskbar_left() {
            Ok(messages) => for message in messages {
                self.log_message(&format!("TaskBarAlignment => {}", message.trim()));
            },
            Err(e) => self.log_message(&format!("TaskBarAlignment => {e:?}")),
        }
        self.update_checklist(category.clone(), item_text, false);
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

}