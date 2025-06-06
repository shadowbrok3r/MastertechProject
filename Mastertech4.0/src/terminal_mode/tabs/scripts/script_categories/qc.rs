use crate::{terminal_mode::tabs::{checklist::Category, script_categories::get_data_transfer_candidates, ScriptsTab}, utilities::{scripts::{install_program, StartupProgram, StartupState}, windows::{registry::{align_taskbar_left, disable_account_notifications, disable_content_delivery_allowed, disable_copilot, disable_lockscreen_notifications, disable_notifications, disable_recent_items_tracking, disable_silent_installed_apps_enabled, disable_start_account_notifications, disable_subscribed_content_enabled, disable_system_pane_suggestions_enabled, enable_more_pins_layout, remove_chat_from_taskbar}, windows_update::install_windows_updates}}};

impl <'a> ScriptsTab <'a> {
    pub fn handle_qc(&mut self, item_text: &str, category: &Category){        
        match item_text {
            "Data Transfer" => self.data_transfer(item_text, category),
            "Install LibreOffice" => self.install_libreoffice(item_text, category),
            "Disable Sleep / Hibernation" => self.disable_sleep_hibernation(item_text, category),
            "Disable proxy settings" => self.disable_proxy_settings(item_text, category),
            "Disable Notifications" => self.disable_notifications(item_text, category),
            "Change SuperAntiSpyware settings" => self.change_superantispyware_settings(item_text, category),
            "Disable Startup Apps" => self.disable_startup_apps(item_text, category),
            "Unpin Copilot" => self.unpin_copilot(item_text, category),
            "Align Taskbar to left" => self.align_taskbar_left(item_text, category),
            _ => {
                self.log_message(&format!("Unknown QC script: {}", item_text));
            }
        }
    }

    pub fn data_transfer(&mut self, item_text: &str, category: &Category) {
        log::info!("Finding Data transfer candidates");
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

    /// TODO: NOT YET IMPLEMENTED
    pub fn change_superantispyware_settings(&mut self, item_text: &str, category: &Category) {
        self.log_message("SuperAntiSpyware settings change not implemented.");
        self.update_checklist(category.clone(), item_text, false);
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