use crate::{tabs::scripts::{AntiVirusProduct, InstalledProgram, ScheduledTask, StartupProgram, TaskbarItem}, utilities::windows::{antivirus::check_antivirus, disable_notifications::{check_content_delivery_manager, check_explorer_advanced, check_push_notifications, get_installed_program_names}, install_windows_updates, net_adapter::{check_network_adapters, get_wlan_status, scan_wifi_networks}, WindowsUpdates}};
use super::{checklist::Category, render::Reporter, ScriptsTab};
use std::{collections::HashSet, path::{Path, PathBuf}};
use powershell_script::PsScriptBuilder;
use serde::Deserialize;
use walkdir::WalkDir;
use sysinfo::Disks;

impl <'a> ScriptsTab <'a> {
    pub fn run_selected_scripts(&mut self) {
        let selected = self.get_selected_scripts();
        if selected.is_empty() {
            self.log_message("No scripts selected to run.");
            return;
        }

        for item in selected {
            let category = item.category().clone();
            self.current_script.replace(Some((category.clone(), item.text.clone())));
            log::info!("Set current script: {:?}", *self.current_script.borrow());

            match category {
                Category::Tuneup => self.handle_tuneup(item.text.as_str(), &category),
                Category::Qc => self.handle_qc(item.text.as_str(), &category),
                Category::WindowsUpdates => self.handle_windows_updates(item.text.as_str(), &category),
                Category::RunPrechecks => self.handle_run_prechecks(item.text.as_str(), &category),
                Category::Informational => self.handle_informational(item.text.as_str(), &category),
                Category::JunkwareRemoval => self.handle_junkware_removal(item.text.as_str(), &category),
                Category::Custom(ref name) => self.handle_custom(&name, item.text.as_str(), &category),
            }

            self.current_reporter.replace(match category {
                Category::Tuneup => Reporter::Tuneup,
                Category::Qc => Reporter::Qc,
                Category::WindowsUpdates => Reporter::WindowsUpdates,
                Category::RunPrechecks => Reporter::RunPrechecks,
                Category::Informational => Reporter::Informational,
                Category::JunkwareRemoval => Reporter::JunkwareRemoval,
                Category::Custom(_) => Reporter::Unknown,
            });

            self.current_script.replace(None);
            log::info!("Cleared current script");
        }
        self.log_message("All selected scripts completed.");
    }

    fn remove_junkware(&mut self, item_text: Option<&str>) {
        if let Ok(programs) = InstalledProgram::get_installed_programs().as_mut() {
            for program in &mut *programs {
                if let Some(publisher) = &program.publisher {
                    if let Some(txt) = item_text {
                        match txt {
                            "OneLaunch" if publisher == "OneLaunch" => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled OneLaunch"),
                                Err(e) => self.log_message(&format!("Error uninstalling OneLaunch: {e:?}")),
                            }
                            "WebNavigator Browser" if publisher == "WebNavigator Browser" => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled Web Navigator Browser"),
                                Err(e) => self.log_message(&format!("Error uninstalling Web Navigator Browser: {e:?}")),
                            }
                            "ESET Security" if publisher == "ESET Security" => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled ESET"),
                                Err(e) => self.log_message(&format!("Error uninstalling ESET: {e:?}")),
                            }
                            "Wavesor" if publisher == "Wavesor" => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled Wave Browser"),
                                Err(e) => self.log_message(&format!("Error uninstalling Wave Browser: {e:?}")),
                            }
                            "Clear Browser" if publisher == "Clear Browser" => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled Clear Browser"),
                                Err(e) => self.log_message(&format!("Error uninstalling Clear Browser: {e:?}")),
                            }
                            "Shift Browser" if publisher == "Shift Browser" => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled Shift Browser"),
                                Err(e) => self.log_message(&format!("Error uninstalling Shift Browser: {e:?}")),
                            }
                            "Avast Browser" if publisher == "Avast Browser" => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled Avast Browser"),
                                Err(e) => self.log_message(&format!("Error uninstalling Avast Browser: {e:?}")),
                            }
                            "Mcaffee Safe Search" if publisher == "Mcaffee Safe" => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled Mcaffee Safe Search"),
                                Err(e) => self.log_message(&format!("Error uninstalling Mcaffee Safe Search: {e:?}")),
                            }
                            "Driver Support" if publisher == "Driver Support" => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled Driver Support"),
                                Err(e) => self.log_message(&format!("Error uninstalling Driver Support: {e:?}")),
                            }
                            "Winzip" if publisher == "Winzip" => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled Winzip"),
                                Err(e) => self.log_message(&format!("Error uninstalling Winzip: {e:?}")),
                            }
                            "SuperAntiSpyware" => {
                                self.update_checklist(Category::Informational, "Is SuperAntiSpyware installed?", true);
                            }
                            "Webroot" => {
                                self.update_checklist(Category::Informational, "Is Webroot installed?", true);
                            }
                            _ => {}
                        }
                    } else {
                        //ccleaner browser, SAS browser extension
                        match publisher.as_str() {
                            "OneLaunch" => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled OneLaunch"),
                                Err(e) => self.log_message(&format!("Error uninstalling OneLaunch: {e:?}")),
                            }
                            "WebNavigator Browser" => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled Web Navigator Browser"),
                                Err(e) => self.log_message(&format!("Error uninstalling Web Navigator Browser: {e:?}")),
                            }
                            "ESET Security" => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled ESET"),
                                Err(e) => self.log_message(&format!("Error uninstalling ESET: {e:?}")),
                            }
                            "Wavesor" => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled Wave Browser"),
                                Err(e) => self.log_message(&format!("Error uninstalling Wave Browser: {e:?}")),
                            }
                            "Clear Browser" => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled Clear Browser"),
                                Err(e) => self.log_message(&format!("Error uninstalling Clear Browser: {e:?}")),
                            }
                            "Shift Browser" => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled Shift Browser"),
                                Err(e) => self.log_message(&format!("Error uninstalling Shift Browser: {e:?}")),
                            }
                            "Avast Browser" => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled Avast Browser"),
                                Err(e) => self.log_message(&format!("Error uninstalling Avast Browser: {e:?}")),
                            }
                            "Mcaffee Safe Search" => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled Mcaffee Safe Search"),
                                Err(e) => self.log_message(&format!("Error uninstalling Mcaffee Safe Search: {e:?}")),
                            }
                            "Driver Support" => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled Driver Support"),
                                Err(e) => self.log_message(&format!("Error uninstalling Driver Support: {e:?}")),
                            }
                            "Winzip" => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled Winzip"),
                                Err(e) => self.log_message(&format!("Error uninstalling Winzip: {e:?}")),
                            }
                            "SuperAntiSpyware" =>  self.update_checklist(Category::Tuneup, "Is SuperAntiSpyware installed?", true),
                            "Webroot" => self.update_checklist(Category::Tuneup, "Is Webroot installed?", true),
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    // Category-specific handlers
    fn handle_tuneup(&mut self, item_text: &str, category: &Category){
        self.current_reporter.replace(Reporter::Tuneup);
        self.log_message(&format!("Starting Tuneup script: {}", item_text));
        match item_text {
            "Disable Sleep / Hibernation" => self.disable_sleep_hibernation(item_text, category),
            "Install Windows Updates" => self.install_windows_updates(item_text, category),
            "Activate CPS" => self.activate_cps(item_text, category),
            "Activate SEB" => self.activate_seb(item_text, category),
            "Run Tron" => self.run_tron(item_text, category),
            "Run SuperAntiSpyware Scan" => self.run_superantispyware_scan(item_text, category),
            "Run Junkware Category" => self.run_junkware_category(item_text, category),
            _ => {
                self.log_message(&format!("Unknown Tuneup script: {}", item_text));
            }
        }
    }

    fn handle_qc(&mut self, item_text: &str, category: &Category){
        self.current_reporter.replace(Reporter::Qc);
        self.log_message(&format!("Running QC check: {}", item_text));
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

    fn handle_windows_updates(&mut self, item_text: &str, category: &Category){
        self.current_reporter.replace(Reporter::WindowsUpdates);
        self.log_message(&format!("Running Windows Updates script: {}", item_text));
        match item_text {
            "Check Updates" => self.check_updates(item_text, category),
            "Install Windows Updates" => self.install_windows_updates(item_text, category),
            _ => {
                self.log_message(&format!("Unknown Windows Updates script: {}", item_text));
            }
        }
    }

    fn handle_run_prechecks(&mut self, item_text: &str, category: &Category){
        self.current_reporter.replace(Reporter::RunPrechecks);
        self.log_message(&format!("Running precheck: {}", item_text));

        match check_push_notifications() {
            Ok(status) => self.log_message(&format!("Push Notifications => {status}")),
            Err(e) => self.log_message(&format!("Push Notifications => {e:?}")),
        }
        match check_content_delivery_manager() {
            Ok(statuses) => {
                for status in statuses.iter() {
                    self.log_message(&format!("ContentDelivery => {status}"))
                }
            }
            Err(e) => self.log_message(&format!("ContentDelivery => {e:?}")),
        }
        match check_explorer_advanced() {
            Ok(status) => self.log_message(&format!("TaskBarAlignment => {status}")),
            Err(e) => self.log_message(&format!("TaskBarAlignment => {e:?}")),
        }
        match get_wlan_status() {
            Ok(_) => self.log_message("Wlan Status OK"),
            Err(e) => {
                self.log_message(&format!("Wlan Status: {e:?}"));
                self.update_checklist(category.clone(), item_text, true);
            },
        }
        match check_network_adapters() {
            Ok(adapters) => self.log_message(&format!("Network Adapters => {adapters:?}")),
            Err(e) => self.log_message(&format!("Error getting Network Adapter list => {e:?}")),
        }
        match scan_wifi_networks() {
            Ok(networks) => self.log_message(&format!("Wifi Networks: {networks:?}")),
            Err(e) => self.log_message(&format!("Error Scanning Wifi Networks: {e:?}")),
        }
    }

    fn handle_informational(&mut self, item_text: &str, category: &Category){
        self.current_reporter.replace(Reporter::Informational);
        self.log_message(&format!("Fetching info: {}", item_text));
        match item_text {
            "Is SuperEasyBackup installed?" => self.is_supereasybackup_installed(item_text, category),
            "Is Webroot installed?" => self.is_webroot_installed(item_text, category),
            "Is SuperAntiSpyware installed?" => self.is_superantispyware_installed(item_text, category),
            "Are there scheduled tasks for it?" => self.are_scheduled_tasks_for_sas(item_text, category),
            "If Webroot/SAS not installed, what AV is active?" => self.active_av_if_no_webroot_sas(item_text, category),
            "Are there any pending Windows updates?" => self.are_pending_windows_updates(item_text, category),
            "Is Windows Activated?" => self.is_windows_activated(item_text, category),
            "Is Hibernation/Sleep enabled?" => self.is_hibernation_sleep_enabled(item_text, category),
            "Have there been any Blue Screens in the past 30 days?" => self.recent_blue_screens(item_text, category),
            "When Was The Last Service Date?" => self.last_service_date(item_text, category),
            "Windows Version" => self.windows_version(item_text, category),
            "Check Windows Updates" => self.check_updates(item_text, category),
            _ => {
                self.log_message(&format!("Unknown Informational script: {}", item_text));
            }
        }
    }

    fn handle_junkware_removal(&mut self, item_text: &str, category: &Category){
        self.current_reporter.replace(Reporter::JunkwareRemoval);
        self.log_message(&format!("Removing junkware: {}", item_text));
        match item_text {
            "OneLaunch" => self.remove_onelaunch(),
            "WebNavigator Browser" => self.remove_webnavigator(),
            "Wavesor" => self.remove_wavesor(),
            "Clear Browser" => self.remove_clearbrowser(),
            "Shift Browser" => self.remove_shiftbrowser(),
            "Avast Browser" => self.remove_avastbrowser(),
            "Mcaffee Safe" => self.remove_mcaffeesafe(),
            "Driver Support" => self.remove_driversupport(),
            "Winzip" => self.remove_winzip(),
            _ => {
                self.log_message(&format!("Unknown Junkware script: {}: {:?}", item_text, category));
            }
        }
    }

    /// TODO: NOT YET IMPLEMENTED
    fn handle_custom(&mut self, name: &str, item_text: &str, category: &Category){
        self.current_reporter.replace(Reporter::Unknown);
        self.log_message(&format!("Running custom script '{}': {} category: {:?}", name, item_text, category));
    }

    // Tuneup Items
    fn disable_sleep_hibernation(&mut self, item_text: &str, category: &Category) {
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

    fn install_windows_updates(&mut self, item_text: &str, category: &Category) {
        self.log_message("Running Windows Updates...");
        let tx = self.update_log_tx.clone();
        std::thread::spawn(move || {
            let _ = install_windows_updates(tx, false, true);
        });
        self.log_message("Windows Updates initiated.");
        self.update_checklist(category.clone(), item_text, true);
    }

    /// TODO: NOT YET IMPLEMENTED
    fn activate_cps(&mut self, item_text: &str, category: &Category) {
        self.log_message("CPS activation not implemented (requires SO number).");
        self.update_checklist(category.clone(), item_text, false);
    }

    /// TODO: NOT YET IMPLEMENTED
    fn activate_seb(&mut self, item_text: &str, category: &Category) {
        self.log_message("SEB activation not implemented (requires SO number or email).");
        self.update_checklist(category.clone(), item_text, false);
    }

    fn run_tron(&mut self, item_text: &str, category: &Category) {
        self.log_message("Tron script not implemented yet.");
        self.update_checklist(category.clone(), item_text, false);
        
    }

    /// TODO: NOT YET IMPLEMENTED
    fn run_superantispyware_scan(&mut self, item_text: &str, category: &Category) {
        self.log_message("SuperAntiSpyware scan not implemented.");  
        self.update_checklist(category.clone(), item_text, false);
    }

    fn run_junkware_category(&mut self, item_text: &str, category: &Category) {
        self.remove_junkware(Some(item_text));
        self.log_message("Junkware category cleanup completed.");
        self.update_checklist(category.clone(), item_text, true);
    }

    // Qc Items
    fn data_transfer(&mut self, item_text: &str, category: &Category) {
        log::info!("Finding Data transfer candidates");
        let tx = self.path_size_tx.clone();
        std::thread::spawn(move || {
            let paths = get_data_transfer_candidates();
            match paths {
                Ok(paths) => { let _ = tx.try_send(paths); },
                Err(e) => log::info!("Error getting paths: {e:?}"),
            };
        });
        // self.log_message("Data transfer completed.");
        self.update_checklist(category.clone(), item_text, true);
    }

    fn install_libreoffice(&mut self, item_text: &str, category: &Category) {
        self.log_message("LibreOffice installation not implemented.");
        self.update_checklist(category.clone(), item_text, false);
    }

    fn disable_proxy_settings(&mut self, item_text: &str, category: &Category) {
        self.log_message("Proxy settings disable not implemented.");
        self.update_checklist(category.clone(), item_text, false);
    }

    fn disable_notifications(&mut self, item_text: &str, category: &Category) {
        match check_push_notifications() {
            Ok(status) => self.log_message(&format!("Push Notifications => {status}")),
            Err(e) => self.log_message(&format!("Push Notifications => {e:?}")),
        }
        match check_content_delivery_manager() {
            Ok(statuses) => {
                for status in statuses.iter() {
                    self.log_message(&format!("ContentDelivery => {status}"))
                }
            }
            Err(e) => self.log_message(&format!("ContentDelivery => {e:?}")),
        }
        match check_explorer_advanced() {
            Ok(status) => self.log_message(&format!("TaskBarAlignment => {status}")),
            Err(e) => self.log_message(&format!("TaskBarAlignment => {e:?}")),
        }
        self.update_checklist(category.clone(), item_text, true);
    }

    /// TODO: NOT YET IMPLEMENTED
    fn change_superantispyware_settings(&mut self, item_text: &str, category: &Category) {
        self.log_message("SuperAntiSpyware settings change not implemented.");
        self.update_checklist(category.clone(), item_text, false);
    }

    /// TODO: NOT YET IMPLEMENTED
    fn disable_startup_apps(&mut self, item_text: &str, category: &Category) {
        if let Ok(programs) = StartupProgram::get_startup_programs() {
            for program in programs {
                // let p = program.
                self.log_message(format!("startup program -> {program:?}")); 
            }
        }
        self.update_checklist(category.clone(), item_text, false);
    }

    /// TODO: NOT YET IMPLEMENTED
    fn unpin_copilot(&mut self, item_text: &str, category: &Category) {
        self.log_message("Copilot unpin not implemented.");
        self.update_checklist(category.clone(), item_text, false);
    }

    /// TODO: NOT YET IMPLEMENTED
    fn align_taskbar_left(&mut self, item_text: &str, category: &Category) {
        self.log_message("Taskbar alignment to left not implemented.");
        self.update_checklist(category.clone(), item_text, false);
    }

    // WindowsUpdates Items
    fn check_updates(&mut self, item_text: &str, category: &Category) {
        self.log_message("Checking for Windows updates...");
        let tx = self.update_log_tx.clone();
        std::thread::spawn(move || {
            let _ = install_windows_updates(tx, false, false); // Check only
        });
        self.log_message("Windows update check finished.");
        self.update_checklist(category.clone(), item_text, true);
    }

    // Informational Items
    fn is_supereasybackup_installed(&mut self, item_text: &str, category: &Category) {
        match InstalledProgram::get_installed_programs() {
            Ok(programs) => {
                let installed = programs.iter().any(|p| p.display_name.clone().unwrap_or_default().contains("SuperEasyBackup"));
                self.log_message(&format!("SuperEasyBackup installed: {}", installed));
            }
            Err(err) => self.log_message(&format!("Failed to fetch installed programs: {}", err)),
        }
        self.update_checklist(category.clone(), item_text, true);
    }

    fn is_webroot_installed(&mut self, item_text: &str, category: &Category) {
        match AntiVirusProduct::query_installed() {
            Ok(products) => {
                log::info!("Products: {products:?}");
                let mut wrsa = AntiVirusProduct::default();
                let mut installed = false;
                for product in products.iter() {
                    if product.display_name.contains("Webroot") {
                        wrsa = product.clone();
                        installed = true;
                    }
                }
                self.antivirus_products = products;

                if !installed {
                    self.log_message(&format!("Couldnt determine WRSA install, checking program list"));
                } else {
                    self.log_message(&format!("Webroot installed: {wrsa:?}"));
                }
                self.update_checklist(category.clone(), item_text, true);
            }
            Err(err) => self.log_message(&format!("Failed to fetch antivirus products: {}", err)),
        }
    }

    fn is_superantispyware_installed(&mut self, item_text: &str, category: &Category) {
        match InstalledProgram::get_installed_programs() {
            Ok(programs) => {
                log::info!("SuperAnti: {programs:?}");
                let installed = programs.iter().any(|p| 
                    p.display_name.clone().unwrap_or_default().contains("SUPERAntiSpyware")
                    || p.publisher.clone().unwrap_or_default().contains("SUPERAntiSpyware")
                );
                self.update_checklist(category.clone(), item_text, true);
                self.log_message(&format!("SuperAntiSpyware installed: {}", installed));
            }
            Err(err) => self.log_message(&format!("Failed to fetch installed programs: {}", err)),
        }
    }

    fn are_scheduled_tasks_for_sas(&mut self, item_text: &str, category: &Category) {
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

    fn active_av_if_no_webroot_sas(&mut self, item_text: &str, category: &Category) {
        match AntiVirusProduct::query_installed() {
            Ok(products) => {
                self.antivirus_products = products;
                self.update_checklist(category.clone(), item_text, true);
                self.log_message(&format!("Active AV: {:?}", self.antivirus_products));
            }
            Err(err) => self.log_message(&format!("Failed to fetch antivirus products: {}", err)),
        }
        match check_antivirus() {
            Ok(products) => self.log_message(&format!("Antivirus: {products:?}")),
            Err(e) => self.log_message(&format!("ERR(Antivirus) => {e:?}")),
        }
    }

    fn are_pending_windows_updates(&mut self, item_text: &str, category: &Category) {
        self.log_message("Checking for Windows updates...");
        let tx = self.update_log_tx.clone();
        std::thread::spawn(move || {
            let _ = install_windows_updates(tx, false, false);
        });
        self.log_message("Windows update check finished.");
        self.update_checklist(category.clone(), item_text, true);
    }

    fn is_windows_activated(&mut self, item_text: &str, category: &Category) {
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

    fn is_hibernation_sleep_enabled(&mut self, item_text: &str, category: &Category) {
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
    fn recent_blue_screens(&mut self, item_text: &str, category: &Category) {
        self.log_message("BSOD check not implemented.");
        self.update_checklist(category.clone(), item_text, false);
    }

    /// TODO: NOT YET IMPLEMENTED
    fn last_service_date(&mut self, item_text: &str, category: &Category) {
        self.log_message("Service date check not implemented.");
        self.update_checklist(category.clone(), item_text, false);
    }

    /// TODO: NOT YET IMPLEMENTED
    fn windows_version(&mut self, item_text: &str, category: &Category) {
        self.log_message("Windows version check not implemented.");
        self.update_checklist(category.clone(), item_text, false);
    }

    // JunkwareRemoval Items (assuming remove_junkware handles these)
    fn remove_onelaunch(&mut self) { self.remove_junkware(Some("OneLaunch")); }
    fn remove_webnavigator(&mut self) { self.remove_junkware(Some("WebNavigatorBrowser")); }
    fn remove_wavesor(&mut self) { self.remove_junkware(Some("Wavesor")); }
    fn remove_clearbrowser(&mut self) { self.remove_junkware(Some("ClearBrowser")); }
    fn remove_shiftbrowser(&mut self) { self.remove_junkware(Some("ShiftBrowser")); }
    fn remove_avastbrowser(&mut self) { self.remove_junkware(Some("AvastBrowser")); }
    fn remove_mcaffeesafe(&mut self) { self.remove_junkware(Some("McaffeeSafe")); }
    fn remove_driversupport(&mut self) { self.remove_junkware(Some("DriverSupport")); }
    fn remove_winzip(&mut self) { self.remove_junkware(Some("Winzip")); }
}

fn get_running_processes() -> Result<HashSet<String>, anyhow::Error> {
    let ps_script = r#"Get-Process | Select-Object -ExpandProperty ProcessName | ConvertTo-Json"#;

    let ps = PsScriptBuilder::new()
        .no_profile(true)
        .non_interactive(true)
        .hidden(true)
        .print_commands(false)
        .build();

    let output = ps.run(ps_script)?;
    if output.success() {
        let stdout = output.stdout().unwrap_or_default();
        let processes: Vec<String> = serde_json::from_str(&stdout)?;
        Ok(processes.into_iter().collect())
    } else {
        Err(anyhow::anyhow!("Failed to retrieve running processes"))
    }
}

fn _install_pc_health_check() -> String {
    format!("winget install Microsoft.WindowsPCHealthCheck -h --accept-package-agreements --force")
}

fn _install_windbg() -> String {
    format!("winget install Microsoft.WinDbg -h --accept-package-agreements --force")
}

fn _check_program_running(process_name: &str) -> String {
    format!(
        r#"
        $process = Get-Process -Name "{}" -ErrorAction SilentlyContinue
        if ($process) {{ "Running" }} else {{ "Not Running" }}
        "#,
        process_name
    )
}

#[derive(Debug, Clone, Deserialize)]
pub struct LicenseStatus {
    #[serde(rename = "Description")]
    pub description: String,
    #[serde(rename = "LicenseStatus")]
    pub license_status: i32
}

pub fn check_windows_activation() -> anyhow::Result<LicenseStatus, anyhow::Error> {
    let script = r#"
        Get-CimInstance SoftwareLicensingProduct -Filter "Name like 'Windows%'" | 
        where { $_.PartialProductKey } | select Description, LicenseStatus | ConvertTo-Json
    "#;

    let output = PsScriptBuilder::new()
        .no_profile(true)
        .non_interactive(true)
        .hidden(false)
        .print_commands(false)
        .build()
        .run(script)?;

    let result: LicenseStatus = serde_json::from_str(&output.stdout().unwrap_or_default())?;

    Ok(result)
}

fn disable_hibernation_and_sleep() -> anyhow::Result<bool, anyhow::Error> {
    let ps_script = r#"
        # Define GUIDs and aliases for power settings
        $settings = @(
            @{
                Name = "Turn off display after"
                SubgroupGUID = "7516b95f-f776-4464-8c53-06167f40cc99"  # SUB_VIDEO
                SettingGUID = "3c0bc021-c8a8-4e07-a973-6b14cbcb2b7e"   # VIDEOIDLE
                Units = "Seconds"
            },
            @{
                Name = "Sleep after"
                SubgroupGUID = "238c9fa8-0aad-41ed-83f4-97be242c8f20"  # SUB_SLEEP
                SettingGUID = "29f6c1db-86da-48c5-9fdb-f2b67b1f44da"   # STANDBYIDLE
                Units = "Seconds"
            },
            @{
                Name = "Allow hybrid sleep"
                SubgroupGUID = "238c9fa8-0aad-41ed-83f4-97be242c8f20"  # SUB_SLEEP
                SettingGUID = "94ac6d29-73ce-41a6-809f-6363ba21b47e"   # HYBRIDSLEEP
                Units = "On/Off"
            },
            @{
                Name = "Hibernate after"
                SubgroupGUID = "238c9fa8-0aad-41ed-83f4-97be242c8f20"  # SUB_SLEEP
                SettingGUID = "9d7815a6-7ee4-497e-8888-515a05f02364"   # HIBERNATEIDLE
                Units = "Seconds"
            }
        )

        # Function to convert hex value to decimal (for seconds) or interpret as On/Off
        function Convert-PowerSettingValue {
            param (
                [string]$HexValue,
                [string]$Units
            )
            if ($Units -eq "Seconds") {
                [uint32]("0x" + $HexValue)
            } elseif ($Units -eq "On/Off") {
                if ($HexValue -eq "0x00000000") { "Off" } else { "On" }
            }
        }

        # Function to check if any setting is enabled
        function Check-PowerSettingsEnabled {
            $anyEnabled = $false
            foreach ($setting in $settings) {
                # Query current AC and DC settings
                $acResult = powercfg /query SCHEME_CURRENT $setting.SubgroupGUID $setting.SettingGUID | Select-String "Current AC Power Setting Index: (0x[0-9a-fA-F]+)"
                $dcResult = powercfg /query SCHEME_CURRENT $setting.SubgroupGUID $setting.SettingGUID | Select-String "Current DC Power Setting Index: (0x[0-9a-fA-F]+)"
                
                $acValue = if ($acResult) { $acResult.Matches.Groups[1].Value } else { "0x00000000" }
                $dcValue = if ($dcResult) { $dcResult.Matches.Groups[1].Value } else { "0x00000000" }
                
                $acConverted = Convert-PowerSettingValue -HexValue $acValue -Units $setting.Units
                $dcConverted = Convert-PowerSettingValue -HexValue $dcValue -Units $setting.Units

                Write-Host "$($setting.Name): AC = $acConverted, DC = $dcConverted"

                # Check if enabled (non-zero for Seconds, "On" for On/Off)
                if (($setting.Units -eq "Seconds" -and ($acConverted -gt 0 -or $dcConverted -gt 0)) -or 
                    ($setting.Units -eq "On/Off" -and ($acConverted -eq "On" -or $dcConverted -eq "On"))) {
                    $anyEnabled = $true
                }
            }
            return $anyEnabled
        }


        # Main logic
        Write-Host "Checking power settings..."
        $enabled = Check-PowerSettingsEnabled

        if ($enabled) {
            Write-Host "One or more power settings are enabled. Disabling all..."
        }
    "#;

    let output = PsScriptBuilder::new()
        .no_profile(true)
        .non_interactive(true)
        .hidden(true)
        .print_commands(false)
        .build()
        .run(ps_script)?;

    let stdout = output.stdout();
    log::info!("disable_hibernation_and_sleep -> stdout: {stdout:?}");

    Ok(!stdout.unwrap_or_default().trim().is_empty())
}

use windows::Storage::{UserDataPaths, SystemDataPaths};
pub fn get_data_transfer_candidates() -> anyhow::Result<Vec<(String, String)>, anyhow::Error> {
    let user_data: UserDataPaths = UserDataPaths::GetDefault()?;
    let sys_data = SystemDataPaths::GetDefault()?;

    log::info!(
        "User data: {:?}\n {:?}",
        user_data.Desktop()?,
        sys_data.UserProfiles()?
    );

    // user_data.
    let disks = Disks::new_with_refreshed_list();
    let mount_points = disks
        .iter()
        .map(|d| d.mount_point())
        .collect::<Vec<&Path>>();

    let mut paths_with_sizes = Vec::new();

    for drive in mount_points {
        let results = read_folder(
            &drive.to_path_buf(), 
            1, 
            true
        );
        if !results.is_empty() {
            for path in results {
                let dir_size = get_directory_size(path.as_path());
                let formatted_size = format_size(dir_size);
        
                log::info!("Directory: {:>10} | Size: {}", path.display(), formatted_size);
                paths_with_sizes.push((path.to_string_lossy().to_string(), formatted_size));
            }
        }
    }
    

    Ok(paths_with_sizes)
}

/// Get the total size of a directory (recursive) in bytes
fn get_directory_size(path: &Path) -> u64 {
    WalkDir::new(path)
        .into_iter()
        .filter_map(|entry| entry.ok())
        .filter_map(|e| e.metadata().ok())
        .filter(|metadata| metadata.is_file()) // Only count file sizes
        .map(|metadata| metadata.len())
        .sum()
}

/// Convert bytes to human-readable MB/GB
fn format_size(bytes: u64) -> String {
    const MB: u64 = 1024 * 1024;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    }
}

pub fn read_folder(path: &PathBuf, depth: usize, read_dirs_only: bool) -> Vec<PathBuf> {
    let mut result: Vec<PathBuf> = WalkDir::new(path)
        .min_depth(depth)
        .max_depth(depth)
        .into_iter()
        .filter_map(|e| e.ok()) // Only retrieve the resulted items
        .filter(|entry| !read_dirs_only || entry.path().is_dir()) // Include only directories if read_dirs_only is true
        .map(|entry| entry.path().to_path_buf()) // Iterate through each DirEntry
        .collect();

    // log::info!("path: {path:?} \nresult: {result:?}");
    result.sort_by(|a, b| {
        let da = a.is_dir();
        let db = b.is_dir();
        match da == db {
            true => a.file_name().cmp(&b.file_name()),
            false => db.cmp(&da),
        }
    });

    let result = result
        .into_iter()
        .filter(|path| {
            if read_dirs_only && !path.is_dir() {
                return false;
            }

            // log::info!("path.file_name(): {:?}", path.file_name());
            // Only include the "Users" directory if it exists in the path
            path.file_name()
                .map(|name| name == "Users")
                .unwrap_or(false)
        })
        .collect::<Vec<PathBuf>>();

    result
}

pub fn _activate_seb(activation_code: &str) -> anyhow::Result<(), anyhow::Error> {
    let _install_cmd = format!(r#"
        msiexec /i SuperEasyBackup.msi /qn Silent=1 ActivationURL=https://blue.mysecuredatavault.com ActivationCode={}
    "#, activation_code);
    Ok(())
}

pub fn _install_libre_office() -> anyhow::Result<(), anyhow::Error> {

    Ok(())
}

pub fn _find_activation_keys() -> anyhow::Result<(), anyhow::Error> {

    Ok(())
}

pub fn _prompt_for_user_pw() -> anyhow::Result<(), anyhow::Error> {

    Ok(())
}

pub fn _checkdisk() -> anyhow::Result<(), anyhow::Error> {

    Ok(())
}

pub fn _dism_scan() -> anyhow::Result<(), anyhow::Error> {

    Ok(())
}

pub fn _sfc_scan() -> anyhow::Result<(), anyhow::Error> {

    Ok(())
}

pub fn check_power_options() -> anyhow::Result<(), anyhow::Error> {

    let ps = PsScriptBuilder::new()
        .no_profile(true)
        .non_interactive(true)
        .hidden(true)
        .print_commands(false)
        .build();

    let output = ps.run(CHECK_POWER_OPTIONS)?;
    log::info!("output.stdout(): {:?}", output.stdout());
    Ok(())
}

pub const CHECK_POWER_OPTIONS: &str = r#"
# Define GUIDs and aliases for power settings
$settings = @(
    @{
        Name = "Turn off display after"
        SubgroupGUID = "7516b95f-f776-4464-8c53-06167f40cc99"  # SUB_VIDEO
        SettingGUID = "3c0bc021-c8a8-4e07-a973-6b14cbcb2b7e"   # VIDEOIDLE
        Units = "Seconds"
    },
    @{
        Name = "Sleep after"
        SubgroupGUID = "238c9fa8-0aad-41ed-83f4-97be242c8f20"  # SUB_SLEEP
        SettingGUID = "29f6c1db-86da-48c5-9fdb-f2b67b1f44da"   # STANDBYIDLE
        Units = "Seconds"
    },
    @{
        Name = "Allow hybrid sleep"
        SubgroupGUID = "238c9fa8-0aad-41ed-83f4-97be242c8f20"  # SUB_SLEEP
        SettingGUID = "94ac6d29-73ce-41a6-809f-6363ba21b47e"   # HYBRIDSLEEP
        Units = "On/Off"
    },
    @{
        Name = "Hibernate after"
        SubgroupGUID = "238c9fa8-0aad-41ed-83f4-97be242c8f20"  # SUB_SLEEP
        SettingGUID = "9d7815a6-7ee4-497e-8888-515a05f02364"   # HIBERNATEIDLE
        Units = "Seconds"
    }
)

# Function to convert hex value to decimal (for seconds) or interpret as On/Off
function Convert-PowerSettingValue {
    param (
        [string]$HexValue,
        [string]$Units
    )
    if ($Units -eq "Seconds") {
        [uint32]("0x" + $HexValue)
    } elseif ($Units -eq "On/Off") {
        if ($HexValue -eq "0x00000000") { "Off" } else { "On" }
    }
}

# Function to check if any setting is enabled
function Check-PowerSettingsEnabled {
    $anyEnabled = $false
    foreach ($setting in $settings) {
        # Query current AC and DC settings
        $acResult = powercfg /query SCHEME_CURRENT $setting.SubgroupGUID $setting.SettingGUID | Select-String "Current AC Power Setting Index: (0x[0-9a-fA-F]+)"
        $dcResult = powercfg /query SCHEME_CURRENT $setting.SubgroupGUID $setting.SettingGUID | Select-String "Current DC Power Setting Index: (0x[0-9a-fA-F]+)"
        
        $acValue = if ($acResult) { $acResult.Matches.Groups[1].Value } else { "0x00000000" }
        $dcValue = if ($dcResult) { $dcResult.Matches.Groups[1].Value } else { "0x00000000" }
        
        $acConverted = Convert-PowerSettingValue -HexValue $acValue -Units $setting.Units
        $dcConverted = Convert-PowerSettingValue -HexValue $dcValue -Units $setting.Units

        Write-Host "$($setting.Name): AC = $acConverted, DC = $dcConverted"

        # Check if enabled (non-zero for Seconds, "On" for On/Off)
        if (($setting.Units -eq "Seconds" -and ($acConverted -gt 0 -or $dcConverted -gt 0)) -or 
            ($setting.Units -eq "On/Off" -and ($acConverted -eq "On" -or $dcConverted -eq "On"))) {
            $anyEnabled = $true
        }
    }
    return $anyEnabled
}


# Main logic
Write-Host "Checking power settings..."
Write-output Check-PowerSettingsEnabled
"#;