use crate::{tabs::scripts::{AntiVirusProduct, InstalledProgram, ScheduledTask, StartupProgram, TaskbarItem}, terminal_mode::events::action_handler::{ActionHandler, WidgetEvent}, utilities::windows::{antivirus::check_antivirus, disable_notifications::{check_content_delivery_manager, check_explorer_advanced, check_push_notifications, get_installed_program_names}, install_windows_updates, net_adapter::{check_network_adapters, connect_to_wifi, get_wlan_status, scan_wifi_networks}, WindowsUpdates}};
use super::{script_checks::get_data_transfer_candidates, Reporter, ScriptsTab};

#[derive(Debug)]
pub enum WindowsUpdateEvent {
    UpdateLogs(String),
    ReturnedUpdates(WindowsUpdates),
}


impl<'a> ActionHandler for ScriptsTab<'a> {
    fn handle_event(&mut self, event: &WidgetEvent) {
        match event {
            WidgetEvent::ButtonClick { widget_id } => {
                match widget_id.0.as_str() {
                    "Tuneup" => {
                        self.current_reporter.replace(Reporter::Tuneup);
                        self.log_message("Starting Tuneup script...");
                        // Run the Tuneup script...
                        self.log_message("Tuneup script completed successfully.");
                    }
                    "Qc" => {
                        self.current_reporter.replace(Reporter::Qc);
                        self.log_message("Running QC checks...");
                        // Run QC script...
                        self.log_message("QC checks completed.");
                    }
                    "WindowsUpdates" => {
                        self.current_reporter.replace(Reporter::WindowsUpdates);
                        self.log_message("Checking for Windows updates...");
                        let tx = self.update_log_tx.clone();
                        // Spawn the Windows Update function in a new thread
                        std::thread::spawn(move || {
                            let _ = install_windows_updates(tx, false);
                        });

                        // Run Windows update script...
                        self.log_message("Windows update check finished.");
                    }
                    "GetAntivirus" => {
                        self.current_reporter.replace(Reporter::GetAntivirus);
                        self.log_message("Fetching installed antivirus products...");
                        match AntiVirusProduct::query_installed() {
                            Ok(products) => {
                                self.antivirus_products = products;
                                self.log_message("Antivirus data retrieved successfully.");
                            }
                            Err(err) => {
                                self.log_message(&format!("Failed to fetch antivirus products: {}", err));
                            }
                        }
                    }
                    "GetInstalledPrograms" => {
                        self.current_reporter.replace(Reporter::GetInstalledPrograms);
                        self.log_message("Fetching installed programs...");
                        match InstalledProgram::get_installed_programs() {
                            Ok(programs) => {
                                self.installed_programs = programs;
                                self.log_message("Installed programs retrieved successfully.");
                            }
                            Err(err) => {
                                self.log_message(&format!("Failed to fetch installed programs: {}", err));
                            }
                        }
                    }
                    "GetStartupItems" => {
                        self.current_reporter.replace(Reporter::GetStartupItems);
                        self.log_message("Fetching startup programs...");
                        match StartupProgram::get_startup_programs() {
                            Ok(startup_items) => {
                                self.startup_programs = startup_items;
                                self.log_message("Startup programs retrieved successfully.");
                            }
                            Err(err) => {
                                self.log_message(&format!("Failed to fetch startup programs: {}", err));
                            }
                        }
                    }
                    "GetScheduledTasks" => {
                        self.current_reporter.replace(Reporter::GetScheduledTasks);
                        self.log_message("Fetching scheduled tasks...");
                        match ScheduledTask::list_tasks() {
                            Ok(tasks) => {
                                self.scheduled_tasks = tasks;
                                self.log_message("Scheduled tasks retrieved successfully.");
                            }
                            Err(err) => {
                                self.log_message(&format!("Failed to fetch scheduled tasks: {}", err));
                            }
                        }
                    }
                    "GetTaskbarItems" => {
                        self.current_reporter.replace(Reporter::GetTaskbarItems);
                        self.log_message("Fetching taskbar items...");
                        match TaskbarItem::get_taskbar_items() {
                            Ok(items) => {
                                self.taskbar_items = items;
                                self.log_message("Taskbar items retrieved successfully.");
                            }
                            Err(err) => {
                                self.log_message(&format!("Failed to fetch taskbar items: {}", err));
                            }
                        }
                    }
                    "RunPrechecks" => {
                        self.current_reporter.replace(Reporter::RunPrechecks);
                        self.log_message("Running system prechecks...");
                        // self.run_prechecks();
                        let check_antivirus = check_antivirus();
                        let get_installed_program_names = get_installed_program_names();
                        let check_network_adapters = check_network_adapters();
                        let get_wlan_status = get_wlan_status();
                        // let scan_for_wifi = scan_wifi_networks();
                        // connect_to_wifi("PCLaptops2.4", Some("bestburger"), None)?;
                        self.log_message(&format!("Wlan Status: {get_wlan_status:?}"));
                        match check_network_adapters {
                            Ok(adapters) => self.log_message(&format!("Network Adapters => {adapters:?}")),
                            Err(e) => self.log_message(&format!("Error getting Network Adapter list => {e:?}")),
                        }

                        let tx = self.path_size_tx.clone();
                        std::thread::spawn(move || {
                            let paths = get_data_transfer_candidates();
                            match paths {
                                Ok(paths) => { let _ = tx.try_send(paths); },
                                Err(e) => log::info!("Error getting paths: {e:?}"),
                            };
                        });
                        match check_antivirus {
                            Ok(products) => self.log_message(&format!("Antivirus: {products:?}")),
                            Err(e) => self.log_message(&format!("ERR(Antivirus) => {e:?}")),
                        }
                    
                        // Check PushNotifications registry key
                        match check_push_notifications() {
                            Ok(status) => self.log_message(&format!("Push Notifications => {status}")),
                            Err(e) => self.log_message(&format!("Push Notifications => {e:?}")),
                        }
                    
                        // Check ContentDeliveryManager registry key
                        match check_content_delivery_manager() {
                            Ok(statuses) => {
                                for status in statuses.iter() {
                                    self.log_message(&format!("ContentDelivery => {status}"))
                                }
                            },
                            Err(e) => self.log_message(&format!("ContentDelivery => {e:?}")),
                        }
                    
                        // Check Explorer Advanced registry key
                        match check_explorer_advanced() {
                            Ok(status) => self.log_message(&format!("TaskBarAlignment => {status}")),
                            Err(e) => self.log_message(&format!("TaskBarAlignment => {e:?}")),
                        }

                        match get_installed_program_names {
                            Ok(x) => self.log_message(&format!("get_installed_program_names: {x:?}")),
                            Err(e) => self.log_message(&format!("ERR(get_installed_program_names) => {e:?}")),
                        }
                        // match is_push_notifications_disabled {
                        //     Ok(x) => self.log_message(&format!("is_push_notifications_disabled: {x:?}")),
                        //     Err(e) => self.log_message(&format!("ERR(is_push_notifications_disabled) => {e:?}")),
                        // }
                        // match is_windows_experience_disabled {
                        //     Ok(x) => self.log_message(&format!("is_windows_experience_disabled: {x:?}")),
                        //     Err(e) => self.log_message(&format!("ERR(is_windows_experience_disabled) => {e:?}")),
                        // }
                        // match is_tips_and_suggestions_disabled {
                        //     Ok(x) => self.log_message(&format!("is_tips_and_suggestions_disabled: {x:?}")),
                        //     Err(e) => self.log_message(&format!("ERR(is_tips_and_suggestions_disabled) => {e:?}")),
                        // }
                    }
                    _ => {
                        // self.current_reporter.replace(Reporter::Unknown);
                        // self.log_message("Unknown task triggered.");
                    }
                }
            }
            WidgetEvent::Api(_) => {},
            WidgetEvent::Active { widget_id } => {}
        }
    }
}
