use crate::{tabs::scripts::{AntiVirusProduct, InstalledProgram, ScheduledTask, StartupProgram, TaskbarItem}, utilities::windows::{antivirus::check_antivirus, disable_notifications::{check_content_delivery_manager, check_explorer_advanced, check_push_notifications, get_installed_program_names}, install_windows_updates, net_adapter::{check_network_adapters, get_wlan_status, scan_wifi_networks}, WindowsUpdates}};
use powershell_script::PsScriptBuilder;
use sysinfo::Disks;
use walkdir::WalkDir;
use std::{collections::HashSet, path::{Path, PathBuf}};
use super::{checklist::Category, render::Reporter, ScriptsTab};

impl <'a> ScriptsTab <'a> {
    pub fn _run_prechecks(&mut self) {
        self.log_message("Running system prechecks...");

        // Check Startup Apps
        // if let Ok(startup_apps) = StartupProgram::get_startup_programs() {
        //     self.update_checklist("Actionable", "Disabling Startup Apps", 
        //         !startup_apps.is_empty()
        //     );
        // }
    
        // // Check System Settings
        // self.update_checklist("Informational", "Are there any pending Windows updates?", 
        //     self.windows_updates.updates.len() > 0
        // );
    
        // self.update_checklist("Informational", "Is Windows Activated?", 
        //     _check_windows_activation()
        // );
    
        // self.update_checklist("Informational", "Is Sleep enabled?", 
        //     check_sleep_mode()
        // );
    

    
        self.log_message("Prechecks completed.");
    }

    pub fn run_selected_scripts(&mut self) {
        let selected = self.get_selected_scripts();
        if selected.is_empty() {
            self.log_message("No scripts selected to run.");
            return;
        }

        for item in selected {
            let category = item.category().clone(); // Clone to own the value
            self.current_script.replace(Some((category.clone(), item.text.clone())));
            match item.category() {
                Category::Tuneup => {
                    self.current_reporter.replace(Reporter::Tuneup);
                    self.log_message(&format!("Starting Tuneup script: {}", item.text));
                    match item.text.as_str() {
                        "Disable Sleep / Hibernation" => {
                            // Add logic to disable sleep/hibernation
                            self.log_message(
                                format!("Disabling Sleep / Hibernation: {:?}", disable_hibernation_and_sleep())
                            );
                            self.update_checklist(category, &item.text, true);
                        }
                        "Run Windows Updates" => {
                            self.log_message("Running Windows Updates...");
                            let tx = self.update_log_tx.clone();
                            std::thread::spawn(move || {
                                let _ = install_windows_updates(tx, false);
                            });
                            self.log_message("Windows Updates initiated.");
                            self.update_checklist(category, &item.text, true);
                        }
                        "Activate CPS" => {
                            // Add CPS activation logic
                            self.log_message("CPS activated.");
                            self.update_checklist(category, &item.text, true);
                        }
                        "Activate SEB" => {
                            // Add SEB activation logic
                            self.log_message("SEB activated.");
                            self.update_checklist(category, &item.text, true);
                        }
                        "Run Tron" => {
                            // Add Tron execution logic
                            self.log_message("Tron script completed.");
                            self.update_checklist(category, &item.text, true);
                        }
                        "Run SuperAntiSpyware Scan" => {
                            // Add SAS scan logic
                            self.log_message("SuperAntiSpyware scan completed.");
                            self.update_checklist(category, &item.text, true);
                        }
                        "Run Junkware Category" => {
                            self.remove_junkware(Some(item.text.as_str()));
                            self.log_message("Junkware category cleanup completed.");
                            self.update_checklist(category, &item.text, true);
                        }
                        _ => self.log_message(&format!("Unknown Tuneup script: {}", item.text)),
                    }
                }
                Category::Qc => {
                    self.current_reporter.replace(Reporter::Qc);
                    self.log_message(&format!("Running QC check: {}", item.text));
                    match item.text.as_str() {
                        "Data Transfer" => {
                            // Add data transfer logic
                            self.log_message("Data transfer completed.");
                            self.update_checklist(category, &item.text, true);
                        }
                        "Install LibreOffice" => {
                            // Add LibreOffice install logic
                            self.log_message("LibreOffice installed.");
                            self.update_checklist(category, &item.text, true);
                        }
                        "Disable Sleep / Hibernation" => {
                            // Add logic to disable sleep/hibernation
                            self.log_message("Disabled Sleep / Hibernation for QC.");
                            self.update_checklist(category, &item.text, true);
                        }
                        "Disable proxy settings" => {
                            // Add proxy disable logic
                            self.log_message("Proxy settings disabled.");
                            self.update_checklist(category, &item.text, true);
                        }
                        "Disable Notifications" => {
                            // Add notification disable logic
                            self.log_message("Notifications disabled.");
                            self.update_checklist(category, &item.text, true);
                        }
                        "Change SuperAntiSpyware settings" => {
                            // Add SAS settings logic
                            self.log_message("SuperAntiSpyware settings updated.");
                            self.update_checklist(category, &item.text, true);
                        }
                        "Disable Startup Apps" => {
                            // Add startup disable logic
                            self.log_message("Startup apps disabled.");
                            self.update_checklist(category, &item.text, true);
                        }
                        "Unpin Copilot" => {
                            // Add Copilot unpin logic
                            self.log_message("Copilot unpinned.");
                            self.update_checklist(category, &item.text, true);
                        }
                        "Align Taskbar to left" => {
                            // Add taskbar alignment logic
                            self.log_message("Taskbar aligned to left.");
                            self.update_checklist(category, &item.text, true);
                        }
                        _ => self.log_message(&format!("Unknown QC script: {}", item.text)),
                    }
                }
                Category::WindowsUpdates => {
                    self.current_reporter.replace(Reporter::WindowsUpdates);
                    self.log_message(&format!("Running Windows Updates script: {}", item.text));
                    match item.text.as_str() {
                        "Check Updates" => {
                            self.log_message("Checking for Windows updates...");
                            let tx = self.update_log_tx.clone();
                            std::thread::spawn(move || {
                                let _ = install_windows_updates(tx, false);
                            });
                            self.update_checklist(category, &item.text, true);
                            self.log_message("Windows update check finished.");
                        }
                        "Install Now" => {
                            self.log_message("Installing Windows updates...");
                            let tx = self.update_log_tx.clone();
                            std::thread::spawn(move || {
                                let _ = install_windows_updates(tx, true); // Assuming true installs
                            });
                            self.update_checklist(category, &item.text, true);
                            self.log_message("Windows update installation initiated.");
                        }
                        _ => self.log_message(&format!("Unknown Windows Updates script: {}", item.text)),
                    }
                }
                Category::RunPrechecks => {
                    self.current_reporter.replace(Reporter::RunPrechecks);
                    self.log_message(&format!("Running precheck: {}", item.text));
                    match item.text.as_str() {
                        "Run Prechecks" => {
                            self.log_message("Running system prechecks...");
                            let tx = self.path_size_tx.clone();
                            std::thread::spawn(move || {
                                let paths = get_data_transfer_candidates();
                                match paths {
                                    Ok(paths) => { let _ = tx.try_send(paths); },
                                    Err(e) => log::info!("Error getting paths: {e:?}"),
                                };
                            });

                            match get_wlan_status() {
                                Ok(_) => self.log_message("Wlan Status OK"),
                                Err(e) => {
                                    self.log_message(&format!("Wlan Status: {e:?}"));
                                    self.update_checklist(category, &item.text, true);
                                },
                            }
                            match check_network_adapters() {
                                Ok(adapters) => self.log_message(&format!("Network Adapters => {adapters:?}")),
                                Err(e) => self.log_message(&format!("Error getting Network Adapter list => {e:?}")),
                            }
                            match check_antivirus() {
                                Ok(products) => self.log_message(&format!("Antivirus: {products:?}")),
                                Err(e) => self.log_message(&format!("ERR(Antivirus) => {e:?}")),
                            }
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
                            match get_installed_program_names() {
                                Ok(x) => self.log_message(&format!("get_installed_program_names: {x:?}")),
                                Err(e) => self.log_message(&format!("ERR(get_installed_program_names) => {e:?}")),
                            }
                            match scan_wifi_networks() {
                                Ok(networks) => self.log_message(&format!("Wifi Networks: {networks:?}")),
                                Err(e) => self.log_message(&format!("Error Scanning Wifi Networks: {e:?}")),
                            }
                        }
                        _ => self.log_message(&format!("Unknown Precheck script: {}", item.text)),
                    }
                }
                Category::Informational => {
                    self.current_reporter.replace(Reporter::Informational); // Assuming Reporter::Informational exists
                    self.log_message(&format!("Fetching info: {}", item.text));
                    
                    /*  
                        //  check for screenconnect
                        get_running_processes()
                        if let Ok(programs) = InstalledProgram::get_installed_programs().as_mut() {
                            for program in &mut *programs {
                                if let Some(name) = &program.publisher {
                                    match name.as_str() {
                                        "OneLaunch" => match program.uninstall() {
                                            Ok(_) => self.log_message("Uninstalled OneLaunch"),
                                            Err(e) => self.log_message(&format!("Error uninstalling OneLaunch: {e:?}")),
                                        }
                                        "WebNavigatorBrowser" => {}
                                        "ESET Security" => {}
                                        //ccleaner browser, SAS browser extension
                                        "Wavesor" => {}
                                        "Clear Browser" => {}
                                        "Shift Browser" => {}
                                        "Avast Browser" => {}
                                        "Mcaffee Safe Search" => {}
                                        "Driver Support" => {}
                                        "Winzip" => {}
                                        "SuperAntiSpyware" => self.update_checklist("Informational", "Is SuperAntiSpyware installed?", true),
                                        "Webroot" => self.update_checklist("Informational", "Is Webroot installed?", true),
                                        _ => {}
                                    }
                                }
                            }
                            self.update_checklist("Informational", "Is SuperEasyBackup installed?", 
                                programs.iter().any(|p| p.display_name.as_deref() == Some("SuperEasyBackup"))
                            );
                        }
                        // Check Antivirus Products
                        if let Ok(av_products) = AntiVirusProduct::query_installed() {
                            let active_avs: Vec<String> = av_products.iter()
                                .filter(|av| av.decode_product_state().0)  // Check if AV is enabled
                                .map(|av| av.display_name.clone())
                                .collect();
                            
                            for av in av_products {
                                self.log_message(&format!("AV: {av:#?}"));
                            }
                            self.update_checklist("Informational", "If Webroot/SAS not installed, what AV is active?", 
                                !active_avs.is_empty()
                            );
                        }

                        // Check Scheduled Tasks
                        if let Ok(tasks) = ScheduledTask::list_tasks() {
                            self.update_checklist("Informational", "Are there scheduled tasks for SuperAntiSpyware?", 
                                tasks.iter().any(|t| t.task_name.as_deref() == Some("SuperAntiSpyware"))
                            );
                        }
                    */
                    match item.text.as_str() {
                        "Is SuperEasyBackup installed?" => {
                            match InstalledProgram::get_installed_programs() {
                                Ok(programs) => {
                                    let installed = programs.iter().any(|p| p.display_name.clone().unwrap_or_default().contains("SuperEasyBackup"));
                                    self.log_message(&format!("SuperEasyBackup installed: {}", installed));
                                }
                                Err(err) => self.log_message(&format!("Failed to fetch installed programs: {}", err)),
                            }
                        }
                        "Is Webroot installed?" => {
                            match AntiVirusProduct::query_installed() {
                                Ok(products) => {
                                    self.antivirus_products = products;
                                    let installed = self.antivirus_products.iter().any(|p| p.display_name.contains("Webroot"));
                                    self.log_message(&format!("Webroot installed: {}", installed));
                                }
                                Err(err) => self.log_message(&format!("Failed to fetch antivirus products: {}", err)),
                            }
                        }
                        "Is SuperAntiSpyware installed?" => {
                            match InstalledProgram::get_installed_programs() {
                                Ok(programs) => {
                                    let installed = programs.iter().any(|p| p.display_name.clone().unwrap_or_default().contains("SuperAntiSpyware"));
                                    self.log_message(&format!("SuperAntiSpyware installed: {}", installed));
                                }
                                Err(err) => self.log_message(&format!("Failed to fetch installed programs: {}", err)),
                            }
                        }
                        "Are there scheduled tasks for it?" => {
                            match ScheduledTask::list_tasks() {
                                Ok(tasks) => {
                                    self.scheduled_tasks = tasks;
                                    self.log_message("Scheduled tasks retrieved successfully.");
                                }
                                Err(err) => self.log_message(&format!("Failed to fetch scheduled tasks: {}", err)),
                            }
                        }
                        "If Webroot/SAS not installed, what AV is active?" => {
                            match AntiVirusProduct::query_installed() {
                                Ok(products) => {
                                    self.antivirus_products = products;
                                    self.log_message(&format!("Active AV: {:?}", self.antivirus_products));
                                }
                                Err(err) => self.log_message(&format!("Failed to fetch antivirus products: {}", err)),
                            }
                        }
                        "Are there any pending Windows updates?" => {
                            self.log_message("Checking for Windows updates...");
                            let tx = self.update_log_tx.clone();
                            std::thread::spawn(move || {
                                let _ = install_windows_updates(tx, false);
                            });
                            self.log_message("Windows update check finished.");
                        }
                        "Is Windows Activated?" => {
                            // Add Windows activation check logic
                            self.log_message("Windows activation check not implemented.");
                        }
                        "Is Hibernation/Sleep enabled?" => {
                            match check_power_options() {
                                Ok(_) => {
                                    self.update_checklist(
                                        Category::Tuneup, 
                                        &item.text, 
                                        true
                                    );
                                    self.log_message("Hibernation is disabled");
                                },
                                Err(e) => self.log_message(e.to_string()),
                            }
                        }
                        "Have there been any Blue Screens in the past 30 days?" => {
                            // Add BSOD check logic
                            self.log_message("BSOD check not implemented.");
                        }
                        "When Was The Last Service Date?" => {
                            // Add service date logic
                            self.log_message("Service date check not implemented.");
                        }
                        "Windows Version" => {
                            // Add Windows version check logic
                            self.log_message("Windows version check not implemented.");
                        }
                        _ => self.log_message(&format!("Unknown Informational script: {}", item.text)),
                    }
                }
                Category::JunkwareRemoval => {
                    self.current_reporter.replace(Reporter::JunkwareRemoval); // Assuming this exists
                    match item.text.as_str() {
                            "OneLaunch" 
                            | "WebNavigator Browser" 
                            | "Wavesor" 
                            | "Clear Browser" 
                            | "Shift Browser" 
                            | "Avast Browser" 
                            | "Mcaffee Safe" 
                            | "Driver Support" 
                            | "Winzip" => 
                        {
                            self.remove_junkware(Some(item.text.as_str()));
                            self.log_message(&format!("Found junkware: {}", item.text));
                        }
                        _ => self.log_message(&format!("Unknown Junkware script: {}", item.text)),
                    }
                }
                Category::Custom(name) => {
                    self.log_message(&format!("Running custom script '{}': {}", name, item.text));
                }
            }
        }

        // Clear the current script after running
        self.current_script.replace(None);
        
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

pub fn _check_windows_activation() -> bool {
    let script = r#"
        $status = (Get-ItemProperty "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\SoftwareProtectionPlatform").LicStatusArray
        if ($status -eq 1) { "Activated" } else { "Not Activated" }
    "#;

    let ps = PsScriptBuilder::new()
        .no_profile(true)
        .non_interactive(true)
        .hidden(false)
        .print_commands(false)
        .build();

    match ps.run(script) {
        Ok(output) => output.stdout().unwrap_or_default().trim() == "Activated",
        Err(_) => false,  // Assume not activated if an error occurs
    }
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

    let ps = PsScriptBuilder::new()
        .no_profile(true)
        .non_interactive(true)
        .hidden(true)
        .print_commands(false)
        .build();

    let output = ps.run(ps_script)?;
    Ok(!output.stdout().unwrap_or_default().trim().is_empty())
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