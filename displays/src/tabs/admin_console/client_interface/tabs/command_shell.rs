use eframe::egui::{epaint::Shadow, Align, Button, CentralPanel, Color32, Direction, Frame, Id, Key, KeyboardShortcut, Layout, Margin, Modifiers, RichText, ScrollArea, TextEdit, TopBottomPanel, Ui, Vec2, Widget};
use crate::{tabs::admin_console::WebSocketClient, PlatformSpawner, Spawner};
use egui_extras::syntax_highlighting::{highlight, CodeTheme};
use bincode::{config::standard, serde::*};
use crate::mcp::{CommandCompletion, DiagnosticResponse, ShellType};
use ewebsock::WsMessage;
use core::f32;
use crate::Cmd;

#[derive(Default, Clone, serde::Serialize, serde::Deserialize, Debug)]
pub struct History {
    pub from: String,
    pub message: String,
    pub timestamp: String
}


impl WebSocketClient {
    pub fn show_shell(&mut self, ui: &mut Ui) {
        let b_panel_marg = Margin::symmetric(5, 10);

        let id = ui.auto_id_with(format!("Chat {:?}", self.client.client_hash));

        TopBottomPanel::bottom(id)
            .default_height(ui.available_height()/1.2) // .resizable(false)
            .show_inside(ui, |ui| 
        {
            ui.visuals_mut().extreme_bg_color= Color32::BLACK;
            ui.visuals_mut().code_bg_color = Color32::BLACK;
            ui.style_mut().visuals.widgets.inactive.bg_fill = Color32::BLACK;
            
            let style = ui.style_mut();
            let default_rounding = eframe::egui::CornerRadius::same(2);
            style.visuals.widgets.inactive.corner_radius = default_rounding;
            style.visuals.widgets.active.corner_radius = default_rounding;
            style.visuals.widgets.hovered.corner_radius = default_rounding;
            
            let mut layouter = |ui: &Ui, buf: &dyn eframe::egui::TextBuffer, _: f32| {
                let mut layout_job: eframe::egui::text::LayoutJob = highlight(
                    ui.ctx(), 
                    &ui.style(), 
                    &CodeTheme::dark(12.), 
                    buf.as_str(), 
                    "bash".into()
                );
                layout_job.wrap.max_width = ui.available_width()/1.1;
                ui.fonts(|f| f.layout_job(layout_job))
            };

            ui.add_space(3.);

            // AI Command Completion Section
            ui.horizontal(|ui| {
                ui.label("👾");
                if ui.checkbox(&mut self.ai_completion_enabled, "AI Command Completion").changed() {
                    if self.ai_completion_enabled {
                        ui.ctx().request_repaint();
                    }
                }

                /*
                    Selectable labels for Command prompt, Powershell, etc.
                */
                
                ui.add_space(10.);
                
                if self.ai_completion_enabled && !self.command_suggestions.is_empty() {
                    ui.label(format!("💡 {} suggestions", self.command_suggestions.len()));
                    
                    if ui.button("🔄 Refresh").clicked() {
                        self.get_ai_command_completions();
                    }
                }
            });

            let text_edit = TextEdit::multiline(&mut self.input)
                .hint_text("Use Wisely.. (Press Tab for AI suggestions)")
                .margin(Margin::symmetric(10, 4))
                .desired_width(ui.available_width())
                .desired_rows(4)
                // .return_key(return_key)
                .layouter(&mut layouter)
                .ui(ui);
            
            // Handle AI command completion (live as you type)
            if self.ai_completion_enabled && text_edit.changed() {
                if self.input != self.last_partial_command && !self.input.is_empty() {
                    self.last_partial_command = self.input.clone();
                    // Reuse the same flow as the Refresh button
                    self.get_ai_command_completions();
                }
            }
            
            // Show AI suggestions if available
            if self.ai_completion_enabled && self.show_suggestions && !self.command_suggestions.is_empty() {
                ui.add_space(5.);
                ui.group(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new("👾 AI Command Suggestions:").strong().color(Color32::LIGHT_BLUE));
                        
                        ScrollArea::vertical().max_height(150.).show(ui, |ui| {
                            for (idx, suggestion) in self.command_suggestions.iter().enumerate() {
                                let selected = idx == self.selected_suggestion;
                                
                                ui.horizontal(|ui| {
                                    let response = ui.selectable_label(
                                        selected,
                                        RichText::new(&suggestion.completion)
                                            .monospace()
                                            .color(if selected { Color32::YELLOW } else { Color32::WHITE })
                                    );
                                    
                                    if response.clicked() {
                                        self.input = suggestion.completion.clone();
                                        self.show_suggestions = false;
                                        text_edit.request_focus();
                                    }
                                    
                                    ui.add_space(5.);
                                    
                                    if let Some(desc) = &suggestion.description {
                                        ui.label(RichText::new(desc).weak().italics());
                                    }
                                    
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        ui.label(RichText::new(format!("{:.0}%", suggestion.confidence * 100.))
                                            .small().weak());
                                    });
                                });
                            }
                        });
                        
                        ui.horizontal(|ui| {
                            if ui.button("❌ Close").clicked() {
                                self.show_suggestions = false;
                            }
                            ui.label(RichText::new("💡 Click suggestion to use, or press Tab to cycle").small().weak());
                        });
                    });
                });
            }

            let key_press = ui.input(|i| i.key_pressed(Key::Enter));
            let up_press = ui.input(|i| i.key_pressed(Key::ArrowUp));
            let down_press = ui.input(|i| i.key_pressed(Key::ArrowDown));
            let tab_press = ui.input(|i| i.key_pressed(Key::Tab));
            let escape_press = ui.input(|i| i.key_pressed(Key::Escape));
            let copy_key = ui.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(Modifiers::CTRL, Key::C)));
            let any_key = [key_press, up_press, down_press, tab_press, escape_press, copy_key];
            if any_key.iter().any(|a| *a) {
                ui.input_mut(|i| {
                    i.events = vec![];
                });
            }
            if copy_key && text_edit.has_focus() {
                self.input.clear();
            }

            // Handle AI suggestion navigation
            if self.ai_completion_enabled && self.show_suggestions && !self.command_suggestions.is_empty() {
                if tab_press {
                    self.selected_suggestion = (self.selected_suggestion + 1) % self.command_suggestions.len();
                }
                if escape_press {
                    self.show_suggestions = false;
                }
                if key_press {
                    // Use selected suggestion
                    if let Some(suggestion) = self.command_suggestions.get(self.selected_suggestion) {
                        self.input = suggestion.completion.clone();
                        self.show_suggestions = false;
                    }
                }
            } else {
                // Normal history navigation when suggestions not shown
                if down_press {
                    if self.history_idx <= self.my_command_history.len() {
                        self.history_idx += 1;
                    }
                    if let Some(history) = self.my_command_history.get(self.history_idx){
                        self.input = history.message.clone();
                    }
                } 
                if up_press {
                    if self.history_idx > 0 {
                        self.history_idx -= 1;
                    }
                    if let Some(history) = self.my_command_history.get(self.history_idx){
                        self.input = history.message.clone();
                    }
                }
                
                // Show suggestions on Tab if AI is enabled
                if tab_press && self.ai_completion_enabled && !self.input.is_empty() {
                    if !self.show_suggestions {
                        self.get_ai_command_completions();
                    }
                    self.show_suggestions = true;
                    self.selected_suggestion = 0;
                }
            }

            if text_edit.lost_focus() && key_press && !self.interactive{
                self.loading = true;
                text_edit.request_focus();

                self.history.push(History { 
                    from: "You".to_string(), 
                    message: self.input.clone(), 
                    timestamp:  chrono::Local::now().to_rfc3339()
                });

                self.notifications += 1;

                self.my_command_history.push(History { 
                    from: "You".to_string(), 
                    message: self.input.clone(), 
                    timestamp:  chrono::Local::now().to_rfc3339()
                });

                self.ws_sender.send(WsMessage::Text(std::mem::take(&mut self.input)));

            } else if text_edit.lost_focus() && key_press && self.interactive { 
                text_edit.request_focus();
                self.history.push(History { 
                    from: "You".to_string(), 
                    message: self.input.clone(), 
                    timestamp:  chrono::Local::now().to_rfc3339()
                });
                self.notifications += 1;

                self.my_command_history.push(History { 
                    from: "You".to_string(), 
                    message: self.input.clone(), 
                    timestamp:  chrono::Local::now().to_rfc3339()
                });

                match encode_to_vec(&Cmd::InteractiveInput(std::mem::take(&mut self.input)), standard()){
                    Ok(bytes) => self.ws_sender.send(WsMessage::Binary(bytes)),
                    Err(e) => self.history.push(History { 
                        from: "Client".to_string(), 
                        message: e.to_string(), 
                        timestamp:  chrono::Local::now().to_rfc3339()
                    }),
                }
            }
        
        });

        CentralPanel::default()
        .frame(
            Frame::new().fill(ui.style().visuals.widgets.inactive.weak_bg_fill)
            .stroke(ui.style().visuals.widgets.inactive.bg_stroke).outer_margin(b_panel_marg)
            .inner_margin(Margin::same(6))
        )
        .show_inside(ui, |ui| {
            let id = Id::new(format!("scroll_area-{:?}", self.client.client_hash));

            ScrollArea::vertical()
            .id_salt(id)
            .animated(true)
            .max_width(f32::INFINITY)
            .auto_shrink([false, false])
            .stick_to_bottom(true)
            .show(ui, |ui| {
                // ui.set_min_height(ui.available_height()/1.1);
                ui.set_width(ui.available_width());
                let max_msg_width = ui.available_width() / 2.2;

                // Display history messages
                let mut display_messages = self.history.clone();
                
                // If there's a buffer with ongoing output, show it as a temporary preview
                if !self.buffer.is_empty() {
                    display_messages.push(History {
                        from: "Client".to_string(),
                        message: format!("{}\n[Receiving...]", self.buffer.trim()),
                        timestamp: chrono::Local::now().to_rfc3339(),
                    });
                }

                // Render messages with improved styling
                for item in &display_messages {
                    let is_message_from_myself = if item.from.eq("You"){ true } else { false };
                    let username_txt_color = ui.style().visuals.hyperlink_color;
                    let from = if is_message_from_myself {
                        RichText::new("Command Sent:").strong().monospace().color(username_txt_color)
                    }else {
                        RichText::new("Client Response:").strong().monospace().color(username_txt_color)
                    };
                    // Messages from the user are right-aligned.
                    let layout = if is_message_from_myself {
                        Layout::top_down(Align::Max)
                    } else {
                        Layout::top_down(Align::Min)
                    };

                    let msg_color = if is_message_from_myself {
                        ui.style().visuals.widgets.active.bg_fill
                    } else {
                        ui.style().visuals.widgets.active.weak_bg_fill
                    };
    
                    ui.with_layout(layout, |ui| {
                        ui.set_max_width(max_msg_width);
                        let rounding = 8.;
                        let outer_margin = Margin { left: 1, right: 1, top: 4, bottom: 1 };
                        let inner_margin = Margin { left: 0, right: 0, top: 1, bottom: 0 };

                        let rnding = eframe::egui::CornerRadius {
                            ne: if is_message_from_myself { 0 } else { rounding as u8 },
                            nw: if is_message_from_myself { rounding as u8 } else { 0 },
                            se: rounding as u8,
                            sw: rounding as u8,
                        };

                        let style = ui.style().clone();

                        let (fill, stroke, shadow) = if self.hovered.contains(&item.timestamp) {
                            (
                                style.visuals.widgets.inactive.bg_fill + Color32::from_rgb(1, 1, 4),
                                style.visuals.widgets.hovered.fg_stroke,
                                style.visuals.window_shadow
                            )
                        } else {
                            (
                                msg_color,
                                style.visuals.widgets.open.bg_stroke,
                                Shadow::default()
                            )
                        };

                        // Add hover effect
                        if Frame::new()
                        .corner_radius(rnding)
                        .inner_margin(inner_margin)
                        .outer_margin(outer_margin)
                        .fill(fill)
                        .shadow(shadow)
                        .stroke(stroke)
                        .show(ui, |ui| { // NOTE FRAME SCOPED UI
                            ui.set_width(max_msg_width);
                            // Use a vertical layout to stack the name and message content
                            ui.vertical_centered(|ui| {
                                let btn_txt_color = ui.style().visuals.error_fg_color;
                                if is_message_from_myself {
                                    ui.horizontal(|ui| {
                                        let btn_txt_color = ui.style().visuals.error_fg_color;
                                        if Button::new(RichText::new("🗐").color(btn_txt_color))
                                        .ui(ui)
                                        .on_hover_text(RichText::new("Copy Message"))
                                        .clicked() {
                                            ui.ctx().copy_text(item.message.clone());
                                        }
                                        ui.add_space(5.);

                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui|{
                                            Button::new(from).fill(Color32::from_rgb(7, 7, 9)).min_size(Vec2::new(30., 35.)).ui(ui);
                                            ui.add_space(5.);
                                            ui.label(RichText::new(item.timestamp.clone()).weak()); // .format("%m/%d @ %I:%M%p").to_string()
                                        });
                                    });

                                } else {
                                    ui.horizontal(|ui| {
                                        Button::new(from).fill(Color32::from_rgb(7, 7, 9)).min_size(Vec2::new(30., 35.)).ui(ui);

                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                            if Button::new(RichText::new("🗐").color(btn_txt_color))
                                            .ui(ui)
                                            .clicked(){
                                                ui.ctx().copy_text(item.message.clone());
                                            }
                                        });
                                    });
                                }
                            
                                Frame::new() // Frame for the actual note text itself // or for modifying the note
                                    .fill(Color32::from_rgb(10,10,12))
                                    .stroke(style.visuals.widgets.inactive.bg_stroke)
                                    .outer_margin(Margin { top: 1, ..Default::default() })
                                    .inner_margin(Margin::symmetric(6, 10))
                                    .corner_radius(rnding)
                                    .show(ui, |ui| 
                                {
                                    ui.with_layout(Layout::from_main_dir_and_cross_align(
                                        Direction::TopDown,
                                        Align::Center,
                                    ), |ui| {
                                        ui.set_width(ui.available_width());
                                        crate::markdown_editor::viewer::easy_mark(ui, &item.message);
                                    });
                                });
                            });

                            let rm = &mut self.remove_hovered;
                            if rm.is_some() {
                                *rm = None;
                                self.hovered.remove(&item.timestamp);
                            }
                        })
                        .response
                        .hovered() {
                            self.hovered.insert(item.timestamp.clone());
                        } else {
                            self.remove_hovered = Some(item.timestamp.clone());
                        }
                    });
                };

                // After rendering, process the buffer
                // Note: Buffer processing is now handled in receive.rs when DONE is detected
            });
        });
    }

    /// Get AI-powered command completions (via MCP provider)
    fn get_ai_command_completions(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.input.is_empty() || !self.ai_completion_enabled {
                return;
            }

            // Clear old suggestions before fetching; they will be filled when response arrives
            self.command_suggestions.clear();

            let partial_command = self.input.clone();
            let shell_type = ShellType::PowerShell;
            let tx = self.diagnostic_tx.clone();
            PlatformSpawner::spawn(async move {
                // Map ShellType to tool helper's string dialect
                let shell = match shell_type {
                    ShellType::Cmd => "cmd",
                    ShellType::PowerShell => "powershell",
                    ShellType::Bash => "bash",
                    ShellType::Zsh => "zsh",
                }.to_string();

                match crate::mcp::tools::mcp_complete_command(partial_command, shell, None) {
                    Ok(payload) => {
                        // Extract completions array into Vec<CommandCompletion>
                        let mut completions: Vec<CommandCompletion> = Vec::new();
                        if let Some(arr) = payload.get("completions").and_then(|v| v.as_array()) {
                            for item in arr {
                                if let Some(comp) = item.get("completion").and_then(|v| v.as_str()) {
                                    let description = item.get("description").and_then(|v| v.as_str()).map(|s| s.to_string());
                                    let confidence = item.get("confidence").and_then(|v| v.as_f64()).unwrap_or(0.8) as f32;
                                    let category = item.get("category").and_then(|v| v.as_str()).map(|s| s.to_string());
                                    completions.push(CommandCompletion { completion: comp.to_string(), description, category, confidence });
                                }
                            }
                        }
                        if !completions.is_empty(){ 
                            log::warn!("New completions: {completions:?}");
                        }
                        let _ = tx.try_send(DiagnosticResponse::CommandCompletions { completions, context_info: None });
                    }
                    Err(e) => {
                        log::error!("Error generating completions: {e:?}");
                    }
                }
            });
        }
    }
}


/// Generate mock command completions (placeholder for MCP integration)
fn generate_mock_completions(partial: &str, shell_type: &ShellType) -> Vec<CommandCompletion> {
    let mut suggestions = Vec::new();

    match shell_type {
        #[cfg(not(target_arch = "wasm32"))]
        ShellType::Cmd => {
            if partial.starts_with("d") {
                suggestions.push(CommandCompletion {
                    category: None,
                    completion: "dir".to_string(),
                    description: Some("List directory contents".to_string()),
                    confidence: 0.95,
                });
                suggestions.push(CommandCompletion {
                    category: None,
                    completion: "dir /a".to_string(),
                    description: Some("List all files including hidden".to_string()),
                    confidence: 0.90,
                });
                suggestions.push(CommandCompletion {
                    category: None,
                    completion: "dir /s".to_string(),
                    description: Some("List files recursively".to_string()),
                    confidence: 0.85,
                });
            }
            if partial.starts_with("s") {
                suggestions.push(CommandCompletion {
                    category: None,
                    completion: "systeminfo".to_string(),
                    description: Some("Display system configuration information".to_string()),
                    confidence: 0.95,
                });
                suggestions.push(CommandCompletion {
                    category: None,
                    completion: "sfc /scannow".to_string(),
                    description: Some("System File Checker - scan and repair".to_string()),
                    confidence: 0.88,
                });
            }
            if partial.starts_with("t") {
                suggestions.push(CommandCompletion {
                    category: None,
                    completion: "tasklist".to_string(),
                    description: Some("Display running processes".to_string()),
                    confidence: 0.92,
                });
                suggestions.push(CommandCompletion {
                    category: None,
                    completion: "taskkill /im".to_string(),
                    description: Some("Terminate process by image name".to_string()),
                    confidence: 0.80,
                });
            }
            if partial.starts_with("i") {
                suggestions.push(CommandCompletion {
                    category: None,
                    completion: "ipconfig /all".to_string(),
                    description: Some("Display complete network configuration".to_string()),
                    confidence: 0.93,
                });
                suggestions.push(CommandCompletion {
                    category: None,
                    completion: "ipconfig /release".to_string(),
                    description: Some("Release IP address configuration".to_string()),
                    confidence: 0.75,
                });
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        ShellType::PowerShell => {
            if partial.to_lowercase().starts_with("get-") {
                suggestions.push(CommandCompletion {
                    category: None,
                    completion: "Get-Process".to_string(),
                    description: Some("Get running processes".to_string()),
                    confidence: 0.95,
                });
                suggestions.push(CommandCompletion {
                    category: None,
                    completion: "Get-Service".to_string(),
                    description: Some("Get system services".to_string()),
                    confidence: 0.93,
                });
                suggestions.push(CommandCompletion {
                    category: None,
                    completion: "Get-EventLog".to_string(),
                    description: Some("Get Windows event logs".to_string()),
                    confidence: 0.90,
                });
            }
            if partial.starts_with("Set-") {
                suggestions.push(CommandCompletion {
                    category: None,
                    completion: "Set-ExecutionPolicy".to_string(),
                    description: Some("Set PowerShell execution policy".to_string()),
                    confidence: 0.88,
                });
            }
        }
        _ => {}
    }

    // Add context-aware suggestions based on current working directory or previous commands
    if partial.contains("log") {
        suggestions.push(CommandCompletion {
            category: None,
            completion: format!("{} | tail -f", partial),
            description: Some("Follow log file in real-time".to_string()),
            confidence: 0.75,
        });
    }

    // Sort by confidence
    suggestions.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));
    
    // Limit to top 8 suggestions
    suggestions.truncate(8);
    
    suggestions
}