use crate::terminal_mode::tabs::checklist::Category;
use crate::terminal_mode::tabs::scripts::Reporter;
use crate::terminal_mode::tabs::script_categories::check_windows_activation;
use crate::terminal_mode::tabs::ScriptsTab;
use crate::utilities::scripts::{check_power_options, AntiVirusProduct, InstalledProgram, ScheduledTask};
use crate::utilities::windows::antivirus::check_antivirus;

impl <'a> ScriptsTab <'a> {
    pub fn handle_informational(&mut self, item_text: &str, category: &Category) {
        self.installed_programs = InstalledProgram::get_installed_programs().unwrap_or_default();
        self.antivirus_products = AntiVirusProduct::query_installed().unwrap_or_default();
        self.current_reporter.replace(Reporter::Informational);
        self.log_message(&format!("Fetching info: {}", item_text));
        match item_text {
            "Is SuperEasyBackup installed?" => self.is_supereasybackup_installed(item_text, category),
            "Is Webroot installed?" => self.is_webroot_installed(item_text, category),
            "Is SuperAntiSpyware installed?" => self.is_superantispyware_installed(item_text, category),
            "Are there scheduled tasks for it?" => self.are_scheduled_tasks_for_sas(item_text, category),
            "Is Windows Activated?" => self.is_windows_activated(item_text, category),
            "Is Hibernation/Sleep enabled?" => self.is_hibernation_sleep_enabled(item_text, category),
            "Any Recent Blue Screens?" => self.recent_blue_screens(item_text, category),
            "When Was The Last Service Date?" => self.last_service_date(item_text, category),
            "Windows Version" => self.windows_version(item_text, category),
            _ =>self.log_message(&format!("Unknown Informational script: {}", item_text)),
        }
    }
    // Informational Items
    pub fn is_supereasybackup_installed(&mut self, item_text: &str, category: &Category) {
        let mut installed = false;
        for program in self.installed_programs.iter() {
            let display_name = program.display_name.clone().unwrap_or_default().to_lowercase();
            let publisher = program.publisher.clone().unwrap_or_default().to_lowercase();
            if display_name.contains("supereasybackup")
                || publisher.contains("supereasybackup")
            {
                installed = true;
                self.log_message(&format!("SuperEasyBackup Found."));
                self.log_message(&format!("--> Display Name: {}", display_name));
                self.log_message(&format!("--> Uninstall Path: {}", program.uninstall_string.clone().unwrap_or_default()));
                self.log_message(&format!("--> Version: {}", program.display_version.clone().unwrap_or_default()));
            }
        }
        self.update_checklist(category.clone(), item_text, installed);
    }

    pub fn is_webroot_installed(&mut self, item_text: &str, category: &Category) {
        let mut installed = false;
        for program in self.installed_programs.iter() {
            let display_name = program.display_name.clone().unwrap_or_default().to_lowercase();
            let publisher = program.publisher.clone().unwrap_or_default().to_lowercase();
            if display_name.contains("webroot")
                || display_name.contains("wrsa")
                || publisher.contains("webroot")
                || publisher.contains("wrsa")
            {
                installed = true;
                self.log_message(&format!("Webroot Found."));
                self.log_message(&format!("--> Display Name: {}", display_name));
                self.log_message(&format!("--> Uninstall Path: {}", program.uninstall_string.clone().unwrap_or_default()));
                self.log_message(&format!("--> Version: {}", program.display_version.clone().unwrap_or_default()));
                // program.uninstall().unwrap();
            }
        }
        if !installed {
            self.log_message(&format!("Webroot not installed."));
            self.active_av_if_no_webroot_sas(item_text, category);
        }
        self.update_checklist(category.clone(), item_text, true);
    }

    pub fn is_superantispyware_installed(&mut self, item_text: &str, category: &Category) {
        let mut installed = false;
        for program in self.installed_programs.iter() {
            if program.display_name.clone().unwrap_or_default().contains("SUPERAntiSpyware")
                || program.publisher.clone().unwrap_or_default().contains("SUPERAntiSpyware")
            {
                installed = true;
                self.log_message(&format!("SuperAntiSpyware Found."));
                self.log_message(&format!("--> Display Name: {}", program.display_name.clone().unwrap_or_default()));
                self.log_message(&format!("--> Uninstall Path: {}", program.uninstall_string.clone().unwrap_or_default()));
                self.log_message(&format!("--> Version: {}", program.display_version.clone().unwrap_or_default()));
            }
        }
        if !installed {
            self.active_av_if_no_webroot_sas(item_text, category);
        }
        self.update_checklist(category.clone(), item_text, true);
    }

    pub fn are_scheduled_tasks_for_sas(&mut self, item_text: &str, category: &Category) {
        match ScheduledTask::list_tasks() {
            Ok(tasks) => {
                let mut sas_task = ScheduledTask::default();
                let mut active_task = false;
                for task in tasks.iter() {
                    active_task = task.task_name.clone().unwrap_or_default().contains("SUPERAntiSpyware");
                    if active_task {
                        sas_task = task.clone();
                    }
                }
                self.scheduled_tasks = tasks;
                self.update_checklist(category.clone(), item_text, active_task);
                self.log_message(format!("Scheduled tasks for SAS: {sas_task:?}"));
            }
            Err(err) => self.log_message(&format!("Failed to fetch scheduled tasks: {}", err)),
        }
    }

    pub fn active_av_if_no_webroot_sas(&mut self, item_text: &str, category: &Category) {
        let antivirus = &self.antivirus_products;
        if !antivirus.is_empty() {
            for product in antivirus.iter() {
                self.log_message(&format!("Active antivirus: {}", product.display_name));
            }
            self.update_checklist(category.clone(), item_text, true);
            self.log_message(&format!("Active AV: {:?}", self.antivirus_products));
        } else {
            match check_antivirus() {
                Ok(products) => self.log_message(&format!("Antivirus: {products:?}")),
                Err(e) => self.log_message(&format!("ERR(Antivirus) => {e:?}")),
            }
        }
    }

    pub fn is_windows_activated(&mut self, item_text: &str, category: &Category) {
        let activation_result  = check_windows_activation();
        match activation_result {
            Ok(license_status) => {
                if license_status.license_status == 1 {
                    self.log_message(format!("Windows is active: {license_status:?}"));
                } else {
                    self.log_message(format!("Windows is not active: {license_status:?}"));
                }
                self.update_checklist(category.clone(), item_text, true);
            },
            Err(e) => self.log_message(format!("Error running activation check: {e:?}")),
        }
    }

    pub fn is_hibernation_sleep_enabled(&mut self, item_text: &str, category: &Category) {
        match check_power_options() {
            Ok(_) => {
                self.update_checklist(
                    category.clone(), 
                    item_text, 
                    true
                );
                self.log_message("Hibernation is disabled");
                self.update_checklist(category.clone(), item_text, true);
            },
            Err(e) => self.log_message(e.to_string()),
        }
    }

    /// TODO: NOT YET IMPLEMENTED
    pub fn recent_blue_screens(&mut self, item_text: &str, category: &Category) {
        self.log_message("BSOD check not implemented.");
        self.update_checklist(category.clone(), item_text, false);
    }

    /// TODO: NOT YET IMPLEMENTED
    pub fn last_service_date(&mut self, item_text: &str, category: &Category) {
        self.log_message("Service date check not implemented.");
        self.update_checklist(category.clone(), item_text, false);
    }

    pub fn windows_version(&mut self, item_text: &str, category: &Category) {
        let win_ver = sysinfo::System::long_os_version().clone().unwrap_or_default();
        self.log_message(format!("Windows Version: {win_ver}"));
        self.update_checklist(category.clone(), item_text, false);
    }

}