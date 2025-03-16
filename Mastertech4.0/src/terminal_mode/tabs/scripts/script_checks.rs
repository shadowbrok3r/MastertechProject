use crate::{tabs::{scripts::{install_program, install_sas, install_supereasybackup, install_webroot, run_ps_script, AntiVirusProduct, InstalledProgram, ScheduledTask, StartupProgram, StartupState}, tur_sheet::get_ticket::SendRequest}, utilities::windows::{antivirus::check_antivirus, net_adapter::{check_network_adapters, get_wlan_status, scan_wifi_networks}, registry::{align_taskbar_left, disable_account_notifications, disable_content_delivery_allowed, disable_copilot, disable_lockscreen_notifications, disable_notifications, disable_recent_items_tracking, disable_silent_installed_apps_enabled, disable_start_account_notifications, disable_subscribed_content_enabled, disable_system_pane_suggestions_enabled, enable_more_pins_layout, remove_chat_from_taskbar}, windows_update::install_windows_updates}};
use super::{checklist::Category, render::Reporter, ScriptsTab};
use std::{path::{Path, PathBuf}, process::Command};
use powershell_script::PsScriptBuilder;
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;
use sysinfo::Disks;

/*
 FIGURED OUT HOW TO ******REACTIVATE****** SUPERANTISPYWARE.
 "C:\Program Files\SUPERAntiSpyware\SUPERAntiSpyware.exe" /autoregister:1HT2-ZJEA-VV0B5

 YOU HAVE TO *KILL* THE EXECUTABLE, THEN RELAUNCH THE EXECUTABLE WITH THAT COMMANDLINE FLAG
 
*/
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

            
            log::info!("Cleared current script");
        }
        self.log_message("All selected scripts completed.");
        self.current_script.replace(None);
    }

    fn remove_junkware(&mut self, item_text: Option<&str>) {
        if let Ok(programs) = InstalledProgram::get_installed_programs().as_mut() {
            for program in &mut *programs {
                if let Some(publisher) = &program.publisher {
                    let publisher = publisher.to_lowercase();
                    if let Some(txt) = item_text {
                        // if (txt.eq("") && publisher.contains("onelaunch"))
                        //     || (txt.eq("") && publisher.contains("webnavigator"))
                        //     || (txt.eq("") && publisher.contains("eset"))
                        //     || (txt.eq("") && publisher.contains("wavesor software"))
                        //     || (txt.eq("") && publisher.contains("clear browser"))
                        //     || (txt.eq("") && publisher.contains("shift technologies"))
                        //     || (txt.eq("") && publisher.contains("Avast Browser"))
                        //     || (txt.eq("") && publisher.contains("Mcaffee Safe"))
                        //     || (txt.eq("") && publisher.contains("driver support"))
                        //     || (txt.eq("") && publisher.contains("winzip"))
                        // {

                        // }
                        match txt {
                            "OneLaunch" if publisher.contains("onelaunch") => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled OneLaunch"),
                                Err(e) => self.log_message(&format!("Error uninstalling OneLaunch: {e:?}")),
                            }
                            "WebNavigator Browser" if publisher.contains("webnavigator") => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled Web Navigator Browser"),
                                Err(e) => self.log_message(&format!("Error uninstalling Web Navigator Browser: {e:?}")),
                            }
                            "ESET Security" if publisher.contains("eset") => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled ESET"),
                                Err(e) => self.log_message(&format!("Error uninstalling ESET: {e:?}")),
                            }
                            "Wave Browser" if publisher.contains("wavesor software") => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled Wave Browser"),
                                Err(e) => self.log_message(&format!("Error uninstalling Wave Browser: {e:?}")),
                            }
                            "Clear Browser" if publisher.contains("clear browser") => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled Clear Browser"),
                                Err(e) => self.log_message(&format!("Error uninstalling Clear Browser: {e:?}")),
                            }
                            "Shift Browser" if publisher.contains("shift technologies") => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled Shift Browser"),
                                Err(e) => self.log_message(&format!("Error uninstalling Shift Browser: {e:?}")),
                            }
                            "Avast Browser" if publisher.contains("Avast Browser") => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled Avast Browser"),
                                Err(e) => self.log_message(&format!("Error uninstalling Avast Browser: {e:?}")),
                            }
                            "Mcaffee Safe Search" if publisher.contains("Mcaffee Safe") => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled Mcaffee Safe Search"),
                                Err(e) => self.log_message(&format!("Error uninstalling Mcaffee Safe Search: {e:?}")),
                            }
                            "Driver Support" if publisher.contains("driver support") => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled Driver Support"),
                                Err(e) => self.log_message(&format!("Error uninstalling Driver Support: {e:?}")),
                            }
                            "Winzip" if publisher.contains("winzip") => match program.uninstall() {
                                Ok(_) => self.log_message("Uninstalled Winzip"),
                                Err(e) => self.log_message(&format!("Error uninstalling Winzip: {e:?}")),
                            }
                            "Webroot TEST" | "SuperAnti TEST" => {
                                for program in self.installed_programs.iter() {
                                    let display_name = program.display_name.clone().unwrap_or_default().to_lowercase();
                                    let publisher = program.publisher.clone().unwrap_or_default().to_lowercase();
                                    if display_name.contains("webroot")
                                        || display_name.contains("wrsa")
                                        || publisher.contains("webroot")
                                        || publisher.contains("wrsa")
                                        || display_name.contains("superantispyware")
                                        || publisher.contains("superantispyware")
                                    {
                                        self.log_message(&format!("Webroot or SAS found. attempting uninstall: {display_name:?}"));
                                        program.uninstall().unwrap();
                                    }
                                }
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
                            "Wave Browser" => match program.uninstall() {
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
        self.update_checklist(Category::JunkwareRemoval, "Wave Browser", true);
        self.update_checklist(Category::JunkwareRemoval, "Clear Browser", true);
        self.update_checklist(Category::JunkwareRemoval, "Shift Browser", true);
        self.update_checklist(Category::JunkwareRemoval, "Avast Browser", true);
        self.update_checklist(Category::JunkwareRemoval, "Mcaffee Safe", true);
        self.update_checklist(Category::JunkwareRemoval, "Driver Support", true);
        self.update_checklist(Category::JunkwareRemoval, "Winzip", true);
        self.update_checklist(Category::JunkwareRemoval, "OneLaunch", true);
        self.update_checklist(Category::JunkwareRemoval, "WebNavigator Browser", true);
    }

    // Category-specific handlers
    fn handle_tuneup(&mut self, item_text: &str, category: &Category){
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

        match disable_notifications() {
            Ok(results) => self.log_message(&format!("Push Notifications => {results:#?}")),
            Err(e) => self.log_message(&format!("Push Notifications => {e:?}")),
        }
        match align_taskbar_left() {
            Ok(messages) => for message in messages {
                self.log_message(&format!("TaskBarAlignment => {}", message.trim()));
            },
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

    fn handle_informational(&mut self, item_text: &str, category: &Category) {
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
            "Wave Browser" => self.remove_wavesor(),
            "Clear Browser" => self.remove_clearbrowser(),
            "Shift Browser" => self.remove_shiftbrowser(),
            "Avast Browser" => self.remove_avastbrowser(),
            "Mcaffee Safe" => self.remove_mcaffeesafe(),
            "Driver Support" => self.remove_driversupport(),
            "Winzip" => self.remove_winzip(),
            "Run Junkware Category" => {
                self.remove_junkware(Some("Webroot TEST"));
                self.remove_junkware(Some("SuperAnti TEST"));
                self.remove_junkware(Some("OneLaunch"));
                self.remove_junkware(Some("WebNavigator Browser"));
                self.remove_junkware(Some("ESET Security"));
                self.remove_junkware(Some("Wave Browser"));
                self.remove_junkware(Some("Clear Browser"));
                self.remove_junkware(Some("Shift Browser"));
                self.remove_junkware(Some("Avast Browser"));
                self.remove_junkware(Some("Mcaffee Safe Search"));
                self.remove_junkware(Some("Driver Support"));
                self.remove_junkware(Some("Winzip"));
                
            }
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
            let _ = install_windows_updates(tx, true, true);
        });
        self.log_message("Windows Updates initiated.");
        self.update_checklist(category.clone(), item_text, true);
    }

    fn activate_cps(&mut self, item_text: &str, category: &Category) {
        let service_number = self.service_number.clone();
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

        if service_number.is_empty() {
            self.log_message("CPS activation requires SO number.");
            return;
        }

        let so = service_number.clone();
        let tx = self.progress_tx.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            let cps_request = SendRequest::get_cps(so.clone(), client.clone());
            let cps_keys = cps_request.await.unwrap_or_default();
            // let res = install_webroot(cps_keys.webroot_key.clone(), client.clone(), tx.clone()).await;
            // log::info!("install_webroot Result: {res:?}");
            let res = install_sas(cps_keys.superanti_key.clone(), client.clone(), tx).await;
            log::info!("install_sas Result: {res:?}");
        });

        self.update_checklist(category.clone(), item_text, true);
    }

    fn activate_seb(&mut self, item_text: &str, category: &Category) {
        let service_number = self.service_number.clone();
        let email = self.customer_email.clone();
        if service_number.is_empty() || email.is_empty() {
            self.log_message("CPS activation requires SO number.");
            return;
        }
        let client = self.client.clone();
        let tx = self.progress_tx.clone();
        tokio::spawn(async move {
            match install_supereasybackup(email, client, tx).await {
                Ok(_) => log::info!("Installed SEB"),
                Err(e) => log::info!("Error Installing SEB: {e:?}"),
            }
        });
        
        self.log_message("SEB activation not implemented (requires SO number or email).");
        self.update_checklist(category.clone(), item_text, false);
    }

    fn run_tron(&mut self, item_text: &str, category: &Category) {
        self.log_message("Tron script not implemented yet.");
        self.update_checklist(category.clone(), item_text, false);
        
    }

    fn run_webroot_scan(&mut self, item_text: &str, category: &Category) {
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
        let download_url = "https://ninite.com/libreoffice/ninite.exe";
        let progress_tx = self.progress_tx.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            let res = install_program(download_url.to_string(), client, progress_tx).await;
            log::info!("Downloaded libre office: {res:?}");
        });
        self.update_checklist(category.clone(), item_text, false);
    }

    fn disable_proxy_settings(&mut self, item_text: &str, category: &Category) {
        self.log_message("Proxy settings disable not implemented.");
        self.update_checklist(category.clone(), item_text, false);
    }

    fn disable_notifications(&mut self, item_text: &str, category: &Category) {
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
    fn change_superantispyware_settings(&mut self, item_text: &str, category: &Category) {
        self.log_message("SuperAntiSpyware settings change not implemented.");
        self.update_checklist(category.clone(), item_text, false);
    }

    /// TODO: NOT YET IMPLEMENTED
    fn disable_startup_apps(&mut self, item_text: &str, category: &Category) {
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

    fn unpin_copilot(&mut self, item_text: &str, category: &Category) {
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

    fn align_taskbar_left(&mut self, item_text: &str, category: &Category) {
        match align_taskbar_left() {
            Ok(messages) => for message in messages {
                self.log_message(&format!("TaskBarAlignment => {}", message.trim()));
            },
            Err(e) => self.log_message(&format!("TaskBarAlignment => {e:?}")),
        }
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

    fn is_webroot_installed(&mut self, item_text: &str, category: &Category) {
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

    fn is_superantispyware_installed(&mut self, item_text: &str, category: &Category) {
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

    fn windows_version(&mut self, item_text: &str, category: &Category) {
        let win_ver = sysinfo::System::long_os_version().clone().unwrap_or_default();
        self.log_message(format!("Windows Version: {win_ver}"));
        self.update_checklist(category.clone(), item_text, false);
    }

    // JunkwareRemoval Items (assuming remove_junkware handles these)
    fn remove_onelaunch(&mut self) { self.remove_junkware(Some("OneLaunch")); }
    fn remove_webnavigator(&mut self) { self.remove_junkware(Some("WebNavigator Browser")); }
    fn remove_wavesor(&mut self) { self.remove_junkware(Some("Wave Browser")); }
    fn remove_clearbrowser(&mut self) { self.remove_junkware(Some("Clear Browser")); }
    fn remove_shiftbrowser(&mut self) { self.remove_junkware(Some("Shift Browser")); }
    fn remove_avastbrowser(&mut self) { self.remove_junkware(Some("Avast Browser")); }
    fn remove_mcaffeesafe(&mut self) { self.remove_junkware(Some("Mcaffee Safe")); }
    fn remove_driversupport(&mut self) { self.remove_junkware(Some("Driver Support")); }
    fn remove_winzip(&mut self) { self.remove_junkware(Some("Winzip")); }
}

#[derive(Debug, Clone, Deserialize)]
pub struct LicenseStatus {
    #[serde(rename = "Description")]
    pub _description: String,
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
        powercfg /change standby-timeout-ac 0
        powercfg /change standby-timeout-dc 0
        powercfg /change monitor-timeout-ac 0
        powercfg /change monitor-timeout-dc 0
        powercfg /change hibernate-timeout-ac 0
        powercfg /change hibernate-timeout-dc 0
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
            drive.to_path_buf(), 
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

pub fn read_folder(mut path: PathBuf, depth: usize, read_dirs_only: bool) -> Vec<PathBuf> {
    // Construct the expected "Users" prefix from the input path (e.g., "C:/Users/")
    path.push("Users");

    let mut result: Vec<PathBuf> = WalkDir::new(path)
        .min_depth(depth)
        .max_depth(depth)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|entry| !read_dirs_only || entry.path().is_dir())
        .map(|entry| entry.path().to_path_buf())
        .filter(|path| {
            let is_users_path = path.starts_with(&path);

            let exclude = path.file_name()
                .map(|name| {
                    let name_str = name.to_string_lossy();
                    name_str == "Public" || name_str.contains("Default") || name_str == "All Users"
                })
                .unwrap_or(false);

            is_users_path && !exclude
        })
        .collect();

    result.sort_by(|a, b| {
        let da = a.is_dir();
        let db = b.is_dir();
        match da == db {
            true => a.file_name().cmp(&b.file_name()),
            false => db.cmp(&da),
        }
    });

    result
}

fn _install_pc_health_check() -> anyhow::Result<String, anyhow::Error> {
    Ok(run_ps_script("winget install Microsoft.WindowsPCHealthCheck -h --accept-package-agreements --force")?)
}

fn _install_windbg() -> anyhow::Result<String, anyhow::Error> {
    Ok(run_ps_script("winget install Microsoft.WinDbg -h --accept-package-agreements --force")?)
}

pub fn _find_activation_keys() -> anyhow::Result<(), anyhow::Error> {

    Ok(())
}

pub fn _prompt_for_user_pw() -> anyhow::Result<(), anyhow::Error> {

    Ok(())
}

pub fn _checkdisk() -> anyhow::Result<String, anyhow::Error> { Ok(run_ps_script("chkdsk /f/x/r C:")?) }

pub fn _dism_scan() -> anyhow::Result<String, anyhow::Error> { Ok(run_ps_script("")?) }

pub fn _sfc_scan() -> anyhow::Result<String, anyhow::Error> { Ok(run_ps_script("sfc /scannow")?) }

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

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Process {
    #[serde(rename="ProcessId")]
    id: usize,
    #[serde(rename="Name")]
    process_name: String,
    #[serde(rename="ExecutablePath")]
    exe_path: Option<String>,
}

pub fn get_running_processes() -> Result<Vec<Process>, anyhow::Error> {
    let ps_script = r#"Get-WmiObject Win32_Process | Select-Object ProcessId, Name, ExecutablePath | ConvertTo-Json"#; // r#"Get-Process | Select-Object Id, ProcessName | ConvertTo-Json"#;

    let ps = PsScriptBuilder::new()
        .no_profile(true)
        .non_interactive(true)
        .hidden(true)
        .print_commands(false)
        .build();

    let output = ps.run(ps_script)?;
    if output.success() {
        let stdout = output.stdout().unwrap_or_default();
        Ok(serde_json::from_str(&stdout)?)
    } else {
        Err(anyhow::anyhow!("Failed to retrieve running processes"))
    }
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

#[test]
fn test_read_folder() {
    use std::fs;
    let temp_dir = tempfile::tempdir().unwrap();
    let users_dir = temp_dir.path().join("Users");
    let alice = users_dir.join("Alice");
    let other_user = users_dir.join("Another User");
    let mut bob = users_dir.join("Bob");

    fs::create_dir(&users_dir).unwrap();
    fs::create_dir(&alice).unwrap();
    fs::create_dir(&other_user).unwrap();
    fs::create_dir(users_dir.join("Public")).unwrap();
    fs::create_dir(users_dir.join("Default")).unwrap();
    fs::create_dir(&bob).unwrap();

    
    fs::File::create(&other_user.join("test.txt")).unwrap();
    fs::File::create(&other_user.join("test1.txt")).unwrap();
    fs::File::create(&other_user.join("test2.txt")).unwrap();

    fs::File::create(&alice.join("test.txt")).unwrap();
    fs::File::create(&alice.join("test1.txt")).unwrap();
    fs::File::create(&alice.join("test2.txt")).unwrap();

    let source_user_name = alice.file_name().clone().unwrap_or_default();
    let source1_user_name = other_user.file_name().clone().unwrap_or_default();

    bob.push("Desktop");
    let desktop_backup_folder = if bob.ends_with("UsersBackup") {
        bob.clone()
    } else {
        let new_bob = bob.join("UsersBackup");
        std::fs::create_dir_all(&new_bob).unwrap();
        new_bob
    };
    let user_folder = desktop_backup_folder.join(source_user_name);
    let user_folder1 = desktop_backup_folder.join(source1_user_name);
    println!("desktop_backup_folder: {desktop_backup_folder:?}\nuser: {user_folder:?}\nuser1 {user_folder1:?}");
    std::fs::create_dir_all(&user_folder).unwrap();
    std::fs::create_dir_all(&user_folder1).unwrap();

    // println!(
    //     "user_backup: {user_backup:?}\nuser_backup_1: {user_backup_1:?}"
    // );

    // assert_eq!(names, vec!["Alice", "Bob"]); // Excludes Public, Default
}