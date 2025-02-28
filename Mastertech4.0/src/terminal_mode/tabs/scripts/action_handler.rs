use crate::{tabs::scripts::{AntiVirusProduct, InstalledProgram, ScheduledTask, StartupProgram, TaskbarItem}, terminal_mode::events::action_handler::{ActionHandler, WidgetEvent}, utilities::windows::{antivirus::check_antivirus, disable_notifications::{is_push_notifications_disabled, is_tips_and_suggestions_disabled, is_windows_experience_disabled}, install_windows_updates, installed_programs::get_installed_program_names, WindowsUpdates}};
use super::{Reporter, ScriptsTab};

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
                        let is_push_notifications_disabled = is_push_notifications_disabled();
                        let is_windows_experience_disabled = is_windows_experience_disabled();
                        let is_tips_and_suggestions_disabled = is_tips_and_suggestions_disabled();

                        match check_antivirus {
                            Ok(_) => self.log_message(&format!("check_antivirus OK")),
                            Err(e) => self.log_message(&format!("ERR(check_antivirus) => {e:?}")),
                        }
                        match get_installed_program_names {
                            Ok(x) => self.log_message(&format!("get_installed_program_names: {x:?}")),
                            Err(e) => self.log_message(&format!("ERR(get_installed_program_names) => {e:?}")),
                        }
                        match is_push_notifications_disabled {
                            Ok(x) => self.log_message(&format!("is_push_notifications_disabled: {x:?}")),
                            Err(e) => self.log_message(&format!("ERR(is_push_notifications_disabled) => {e:?}")),
                        }
                        match is_windows_experience_disabled {
                            Ok(x) => self.log_message(&format!("is_windows_experience_disabled: {x:?}")),
                            Err(e) => self.log_message(&format!("ERR(is_windows_experience_disabled) => {e:?}")),
                        }
                        match is_tips_and_suggestions_disabled {
                            Ok(x) => self.log_message(&format!("is_tips_and_suggestions_disabled: {x:?}")),
                            Err(e) => self.log_message(&format!("ERR(is_tips_and_suggestions_disabled) => {e:?}")),
                        }
                    }
                    _ => {
                        // self.current_reporter.replace(Reporter::Unknown);
                        // self.log_message("Unknown task triggered.");
                    }
                }
            }
            &WidgetEvent::Api(_) => {}
        }
    }
}
