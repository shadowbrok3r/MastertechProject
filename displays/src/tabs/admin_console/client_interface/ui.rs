use eframe::egui::{Align, Button, Color32, Id, Layout, RichText, TopBottomPanel, Ui, Widget};
use crate::Cmd;
use super::WebSocketClient;


pub enum WsDisplayState {
    LiveStats,
    Explorer,
    Shell,
    ToolBox,
    Terminal,
    EventLog,
    Services,
    TaskScheduler,
    Registry,
    StartupApps,
    Scripts,
}

impl WebSocketClient {
    pub fn show(&mut self, ui: &mut Ui) {
        self.receive(ui.ctx());
        ui.set_min_height(600.);

        eframe::egui::Panel::top(Id::new(format!("ClientTopPanel-{}", self.client.client_hash)))
        .exact_height(60.)
        .show_inside(ui, |ui| 
        {
            ui.add_space(2.);
            // Row 1: existing tabs
            ui.horizontal(|ui| {
                let btn_color = ui.style().visuals.error_fg_color;
                if Button::new(RichText::new("My Tools").color(btn_color)).ui(ui).clicked(){
                    let _ = self.display_state_channel.0.try_send(WsDisplayState::ToolBox);
                }

                if Button::new(RichText::new("Explorer").color(btn_color)).ui(ui).clicked(){
                    let _ = self.display_state_channel.0.try_send(WsDisplayState::Explorer);
                    self.notifications = 0;
                    if !self.interactive {
                        let path = if self.remote_explorer.current_path.is_empty() {
                            "current".to_string()
                        } else {
                            self.remote_explorer.current_path.clone()
                        };
                        let _ = self.send_cmd_tx.try_send(Cmd::ListDirectory(path));
                        self.remote_explorer.loading = true;
                        
                        if self.remote_explorer.drives.is_empty() {
                            let _ = self.send_cmd_tx.try_send(Cmd::GetDrives);
                        }
                    }
                }

                if self.live_stats_active {
                    if Button::new(RichText::new("■ Stop Charts").color(Color32::RED)).ui(ui).clicked(){
                        let _ = self.send_cmd_tx.try_send(Cmd::Quit);
                        self.live_stats_active = false;
                    }
                } else {
                    if Button::new(RichText::new("Charts").color(btn_color)).ui(ui).clicked(){
                        let _ = self.display_state_channel.0.try_send(WsDisplayState::LiveStats);
                        let _ = self.send_cmd_tx.try_send(Cmd::LiveData);
                        self.live_stats_active = true;
                    }
                }

                if Button::new(RichText::new("Mastertech TUI").color(btn_color)).ui(ui).clicked(){
                    let _ = self.display_state_channel.0.try_send(WsDisplayState::Terminal);
                }

                let notifs = if let WsDisplayState::Shell = self.state {
                    format!("Shell")
                } else {
                    if self.notifications > 0 {
                        format!("Shell   {}", self.notifications)
                    } else {
                        format!("Shell")
                    }
                };

                if Button::new(RichText::new(notifs).color(btn_color)).ui(ui).clicked(){
                    let _ = self.display_state_channel.0.try_send(WsDisplayState::Shell);
                }

                if self.interactive {
                    if Button::new(RichText::new("Quit").color(Color32::RED)).ui(ui).clicked(){
                        let _ = self.send_cmd_tx.try_send(Cmd::Quit);
                    }
                }

                ui.add_space(10.);
                
                let (status_color, status_text, status_tooltip) = if !self.client.connected {
                    (Color32::RED, "✖", "Disconnected")
                } else if let Some(last_activity) = &self.client.last_update {
                    let now = chrono::Utc::now();
                    let activity_time = last_activity.to_utc();
                    let elapsed_secs = (now - activity_time).num_seconds();
                    
                    if elapsed_secs < 30 {
                        (Color32::GREEN, "✔", "Active")
                    } else if elapsed_secs < 120 {
                        (Color32::YELLOW, "⚠", "Stale")
                    } else {
                        (Color32::LIGHT_RED, "⏳", "Inactive")
                    }
                } else if self.is_connected {
                    (Color32::from_rgb(100, 200, 100), "◯", "Connected (awaiting activity)")
                } else {
                    (Color32::RED, "✖", "Disconnected")
                };
                
                ui.colored_label(status_color, status_text).on_hover_text(status_tooltip);
                
                ui.add_space(10.);
                
                if self.persistent_shell_mode {
                    ui.colored_label(Color32::YELLOW, "🖳 Persistent Shell");
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let os_btn_color = Color32::from_rgb(180, 180, 200);
                    
                    if Button::new(RichText::new("Shutdown").color(os_btn_color).small())
                        .ui(ui).clicked() 
                    {
                        let _ = self.send_cmd_tx.try_send(Cmd::ShutdownSystem);
                    }
                    
                    if Button::new(RichText::new("🔄 Reboot").color(os_btn_color).small())
                        .ui(ui).clicked() 
                    {
                        let _ = self.send_cmd_tx.try_send(Cmd::RebootSystem { persist_mastertech: true });
                    }
                    
                    if Button::new(RichText::new("🚪 Log Off").color(os_btn_color).small())
                        .ui(ui).clicked() 
                    {
                        let _ = self.send_cmd_tx.try_send(Cmd::LogOffUser);
                    }
                    
                    if Button::new(RichText::new("🔒 Lock").color(os_btn_color).small())
                        .ui(ui).clicked() 
                    {
                        let _ = self.send_cmd_tx.try_send(Cmd::LockWorkstation);
                    }
                });
            });

            // Row 2: system management tabs
            ui.horizontal(|ui| {
                let sys_color = Color32::from_rgb(160, 200, 180);

                if Button::new(RichText::new("Event Log").color(sys_color).small()).ui(ui).clicked() {
                    let _ = self.display_state_channel.0.try_send(WsDisplayState::EventLog);
                    if self.event_log_viewer.entries.is_empty() {
                        let _ = self.send_cmd_tx.try_send(Cmd::ReadEventLog {
                            log_name: self.event_log_viewer.selected_log.clone(),
                            max_entries: self.event_log_viewer.max_entries,
                            level_filter: None,
                        });
                        self.event_log_viewer.loading = true;
                    }
                }

                if Button::new(RichText::new("Services").color(sys_color).small()).ui(ui).clicked() {
                    let _ = self.display_state_channel.0.try_send(WsDisplayState::Services);
                    if self.services_viewer.entries.is_empty() {
                        let _ = self.send_cmd_tx.try_send(Cmd::ListServices);
                        self.services_viewer.loading = true;
                    }
                }

                if Button::new(RichText::new("Task Scheduler").color(sys_color).small()).ui(ui).clicked() {
                    let _ = self.display_state_channel.0.try_send(WsDisplayState::TaskScheduler);
                    if self.task_scheduler_viewer.entries.is_empty() {
                        let _ = self.send_cmd_tx.try_send(Cmd::ListScheduledTasks { folder: None });
                        self.task_scheduler_viewer.loading = true;
                    }
                }

                if Button::new(RichText::new("Registry").color(sys_color).small()).ui(ui).clicked() {
                    let _ = self.display_state_channel.0.try_send(WsDisplayState::Registry);
                }

                if Button::new(RichText::new("Startup Apps").color(sys_color).small()).ui(ui).clicked() {
                    let _ = self.display_state_channel.0.try_send(WsDisplayState::StartupApps);
                    if self.startup_apps_viewer.entries.is_empty() {
                        let _ = self.send_cmd_tx.try_send(Cmd::ListStartupApps);
                        self.startup_apps_viewer.loading = true;
                    }
                }

                if Button::new(RichText::new("Scripts").color(sys_color).small()).ui(ui).clicked() {
                    let _ = self.display_state_channel.0.try_send(WsDisplayState::Scripts);
                    if self.remote_scripts_viewer.loading || self.remote_scripts_viewer.running {
                        // already loading or running
                    } else {
                        self.remote_scripts_viewer.loading = true;
                        let _ = self.send_cmd_tx.try_send(Cmd::GetRemoteScriptList);
                    }
                }
            });
            ui.add_space(2.);
        });

        match self.state {
            WsDisplayState::LiveStats => self.show_live_stats(ui),
            WsDisplayState::Explorer => {
                let cmd_tx = self.send_cmd_tx.clone();
                ui.group(|ui| self.remote_explorer.display(ui, &cmd_tx)).inner
            },
            WsDisplayState::ToolBox => ui.group(|ui| self.toolbox.display(ui)).inner,
            WsDisplayState::Shell => self.show_shell(ui),
            WsDisplayState::Terminal => {
                #[cfg(feature="tokio")]
                self.remote_terminal.ui(ui)
            },
            WsDisplayState::EventLog => {
                let cmd_tx = self.send_cmd_tx.clone();
                self.event_log_viewer.display(ui, &cmd_tx);
            },
            WsDisplayState::Services => {
                let cmd_tx = self.send_cmd_tx.clone();
                self.services_viewer.display(ui, &cmd_tx);
            },
            WsDisplayState::TaskScheduler => {
                let cmd_tx = self.send_cmd_tx.clone();
                self.task_scheduler_viewer.display(ui, &cmd_tx);
            },
            WsDisplayState::Registry => {
                let cmd_tx = self.send_cmd_tx.clone();
                self.registry_editor.display(ui, &cmd_tx);
            },
            WsDisplayState::StartupApps => {
                let cmd_tx = self.send_cmd_tx.clone();
                self.startup_apps_viewer.display(ui, &cmd_tx);
            },
            WsDisplayState::Scripts => {
                let cmd_tx = self.send_cmd_tx.clone();
                self.remote_scripts_viewer.display(ui, &cmd_tx);
            },
        };
    }
}