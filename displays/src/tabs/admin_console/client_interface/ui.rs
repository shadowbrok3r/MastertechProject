use eframe::egui::{Align, Button, Color32, Id, Layout, RichText, TopBottomPanel, Ui, Widget};
use crate::Cmd;
use super::WebSocketClient;


pub enum WsDisplayState {
    LiveStats,
    Explorer,
    Shell,
    ToolBox,
    Terminal
}

impl WebSocketClient {
    pub fn show(&mut self, ui: &mut Ui) {
        self.receive(ui.ctx());
        ui.set_min_height(600.);

        TopBottomPanel::top(Id::new(format!("ClientTopPanel-{}", self.client.client_hash)))
        .exact_height(35.)
        .show_inside(ui, |ui| 
        {
            ui.add_space(2.);
            ui.horizontal(|ui| {
                let btn_color = ui.style().visuals.error_fg_color;
                if Button::new(RichText::new("My Tools").color(btn_color)).ui(ui).clicked(){
                    let _ = self.display_state_channel.0.try_send(WsDisplayState::ToolBox);
                }

                if Button::new(RichText::new("Explorer").color(btn_color)).ui(ui).clicked(){
                    let _ = self.display_state_channel.0.try_send(WsDisplayState::Explorer);
                    self.notifications = 0;
                    // Request directory listing using the new websocket-based explorer
                    if !self.interactive {
                        let path = if self.remote_explorer.current_path.is_empty() {
                            "current".to_string()
                        } else {
                            self.remote_explorer.current_path.clone()
                        };
                        let _ = self.send_cmd_tx.try_send(Cmd::ListDirectory(path));
                        self.remote_explorer.loading = true;
                        
                        // Also request drives if we don't have them yet
                        if self.remote_explorer.drives.is_empty() {
                            let _ = self.send_cmd_tx.try_send(Cmd::GetDrives);
                        }
                    }
                }

                // Show "Charts" or "Stop Charts" based on live stats state
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
                
                // Connection status indicator
                let (status_color, status_text) = if self.is_connected {
                    (Color32::GREEN, "✔")
                } else {
                    (Color32::RED, "✖")
                };
                
                ui.colored_label(status_color, status_text);
                
                ui.add_space(10.);
                
                // Persistent shell indicator
                if self.persistent_shell_mode {
                    ui.colored_label(Color32::YELLOW, "🖳 Persistent Shell");
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    // OS command buttons (right-aligned)
                    let os_btn_color = Color32::from_rgb(180, 180, 200);
                    
                    if Button::new(RichText::new("⏻ Shutdown").color(os_btn_color).small())
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
        };
    }
}