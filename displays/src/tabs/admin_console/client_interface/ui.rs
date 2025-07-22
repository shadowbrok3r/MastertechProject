use eframe::egui::{Align, Button, Color32, Id, Layout, RichText, TopBottomPanel, Ui, Widget};
use crate::{Cmd, FileSystemAction};
use super::WebSocketClient;


pub enum WsDisplayState {
    LiveStats,
    Explorer,
    Shell,
    ToolBox,
    Terminal
}

impl WebSocketClient {
    pub fn show(&mut self, ui: &mut Ui) { // , add_contents: impl FnOnce(&mut Ui)
        self.receive(ui.ctx());
        ui.set_min_height(600.);

        TopBottomPanel::top(Id::new(format!("ClientTopPanel-{}", self.client.client_hash)))
        .exact_height(26.)// .frame(top_frame)
        .show_inside(ui, |ui| 
        {
            ui.add_space(2.);
            ui.horizontal(|ui| {
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| { 
                    let btn_color = ui.style().visuals.error_fg_color;
                    if Button::new(RichText::new("My Tools").color(btn_color)).ui(ui).clicked(){
                        let _ = self.display_state_channel.0.try_send(WsDisplayState::ToolBox);
                    }

                    if Button::new(RichText::new("Explorer").color(btn_color)).ui(ui).clicked(){
                        let _ = self.display_state_channel.0.try_send(WsDisplayState::Explorer);
                        self.notifications = 0;
                        // if we are already in an interactive mode, then we dont want to quit that session,
                        if !self.interactive {
                            if self.explorer.current_prefix.is_empty() {
                                let _ = self.send_cmd_tx.try_send(Cmd::FileSystemAction(FileSystemAction::EnterDirectory("current".to_string())));
                            } else {
                                let _ = self.send_cmd_tx.try_send(Cmd::FileSystemAction(FileSystemAction::EnterDirectory(self.explorer.current_prefix.clone())));
                            }
                        }
                    }

                    if Button::new(RichText::new("Charts").color(btn_color)).ui(ui).clicked(){
                        let _ = self.display_state_channel.0.try_send(WsDisplayState::LiveStats);
                        let _ = self.send_cmd_tx.try_send(Cmd::LiveData);
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
                        (Color32::GREEN, "●")
                    } else {
                        (Color32::RED, "●")
                    };
                    
                    ui.colored_label(status_color, status_text);
                    
                    // Last pong time
                    if let Some(last_pong) = self.last_pong_time {
                        let elapsed = last_pong.elapsed().as_secs();
                        ui.label(format!("Last pong: {}s ago", elapsed));
                    } else {
                        ui.label("No pong received");
                    }
                    
                    ui.add_space(10.);
                    
                    // Persistent shell indicator
                    if self.persistent_shell_mode {
                        ui.colored_label(Color32::YELLOW, "🔗 Persistent Shell");
                    }
                });

            });
            ui.add_space(2.);
        });

        match self.state {
            WsDisplayState::LiveStats => self.show_live_stats(ui),
            WsDisplayState::Explorer => ui.group(|ui| self.explorer.display(ui)).inner,
            WsDisplayState::ToolBox => ui.group(|ui| self.toolbox.display(ui)).inner,
            WsDisplayState::Shell => self.show_shell(ui),
            WsDisplayState::Terminal => {
                #[cfg(feature="tokio")]
                self.remote_terminal.ui(ui)
            },
        };
    }
}