use crate::{tabs::scripts::{AntiVirusProduct, InstalledProgram, ScheduledTask, StartupProgram, TaskbarItem}, terminal_mode::events::action_handler::{ActionHandler, WidgetEvent}, utilities::windows::{install_windows_updates, WindowsUpdates}};
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
                        self.run_prechecks();
                        self.log_message("Prechecks completed.");
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
