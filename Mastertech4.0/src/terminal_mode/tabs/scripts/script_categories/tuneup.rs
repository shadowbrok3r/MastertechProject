use std::process::Command;

use crate::{tabs::tur_sheet::get_ticket::SendRequest, terminal_mode::tabs::{checklist::Category, script_categories::disable_hibernation_and_sleep, scripts::Reporter, ScriptsTab}, utilities::{scripts::{get_running_processes, install_sas, install_supereasybackup, install_webroot, run_ps_script}, windows::windows_update::install_windows_updates}};


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

        if let Ok(processes) = get_running_processes() {
            for process in processes {
                let name = process.process_name.to_lowercase();
                let exe_path = process.exe_path.clone().unwrap_or_default().to_lowercase();
                if name.contains("sascore") 
                    || exe_path.contains("superanti") 
                    || name.contains("superanti")
                {
                    self.log_message(format!("PID {} found, attempting to kill SAS", process.id));
                    
                    let output = Command::new("taskkill")
                        .args(&["/PID", &format!("{}", process.id), "/F"])
                        .output();
                
                    self.log_message(format!("{:?}", output));
                }
            }
        }

        let so = self.service_number.clone();
        let tx = self.progress_tx.clone();
        let client = self.client.clone();
        
        tokio::spawn(async move {
            let cps_keys = SendRequest::get_cps(so.clone(), client.clone()).await.unwrap_or_default();
            log::info!("CPS Request: {cps_keys:?}");
            let key = cps_keys.get(0).cloned().unwrap_or_default();
            let res = install_webroot(key.webroot_key.clone(), client.clone(), tx.clone()).await;
            log::info!("install_webroot Result: {res:?}");
            let res = install_sas(key.superanti_key.clone(), client.clone(), tx).await;
            log::info!("install_sas Result: {res:?}");
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

}