use eframe::egui::{epaint::Shadow, Align, Button, CentralPanel, Color32, Direction, Frame, Id, Key, KeyboardShortcut, Layout, Margin, Modifiers, RichText, ScrollArea, TextEdit, Ui, Vec2, Widget};
use crate::{tabs::admin_console::WebSocketClient, PlatformSpawner, Spawner};
use egui_extras::syntax_highlighting::{highlight, CodeTheme};
use bincode::{config::standard, serde::*};
#[cfg(not(target_arch="wasm32"))]
use crate::mcp::{DiagnosticResponse, mcp::ShellType};
use database::SurrealValue;
use ewebsock::WsMessage;
use core::f32;
use crate::Cmd;

#[derive(Default, Clone, serde::Serialize, serde::Deserialize, Debug, SurrealValue)]
pub struct History {
    pub from: String,
    pub message: String,
    pub timestamp: String
}


impl WebSocketClient {
    // Replace current command token (or argument) intelligently with chosen completion.
    fn apply_command_suggestion(&mut self, completion: &str) {
        let current = self.input.clone();
        let trimmed = current.trim_end_matches(|c: char| c == '\n' || c == '\r');
        let ends_with_space = current.ends_with(char::is_whitespace);
        let mut parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.is_empty() {
            self.input = completion.to_string();
            return;
        }
        if parts.len() == 1 && !ends_with_space {
            // Replace the sole fragment (command fragment)
            self.input = completion.to_string();
            return;
        }
        // Argument mode: replace last token that starts with '-' OR append if ending with space
        if ends_with_space {
            // Append new argument (ensure space)
            if !self.input.ends_with(' ') { self.input.push(' '); }
            self.input.push_str(completion);
        } else {
            // Replace last token
            if let Some(last) = parts.last_mut() { *last = completion; }
            // Rebuild preserving original trailing whitespace (none, since not ends_with_space)
            let mut rebuilt = String::new();
            for (i, p) in parts.iter().enumerate() {
                if i > 0 { rebuilt.push(' '); }
                rebuilt.push_str(p);
            }
            self.input = rebuilt;
        }
    }

    pub fn show_shell(&mut self, ui: &mut Ui) {
        let b_panel_marg = Margin::symmetric(5, 10);

        let id = ui.auto_id_with(format!("Chat {:?}", self.client.client_hash));

        let text_response = &mut None;
        eframe::egui::Panel::bottom(id)
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
                ui.fonts_mut(|f| f.layout_job(layout_job))
            };

            ui.add_space(3.);

            // AI Command Completion Section
            ui.horizontal(|ui| {
                #[cfg(not(target_arch="wasm32"))]
                if self.ai_completion_enabled {
                    ui.label("👾");
                    if ui.checkbox(&mut self.ai_completion_enabled, "AI Command Completion").changed() {
                        if !self.ai_completion_enabled {
                            self.command_suggestions.clear();
                            self.show_suggestions = false;
                            // Also cancel any in-flight request
                            if let Some(cancel) = self.completion_cancel_tx.take() {
                                let _ = cancel.send(());
                            }
                        }
                    }
                    if self.completion_cancel_tx.is_some() {
                        ui.spinner();
                        ui.label(RichText::new("Thinking...").weak());
                    }
                } else {
                    ui.label("👾");
                    ui.checkbox(&mut self.ai_completion_enabled, "AI Command Completion");
                }

                /*
                    Selectable labels for Command prompt, Powershell, etc.
                */
                
                ui.add_space(10.);
                
                #[cfg(not(target_arch="wasm32"))]
                if self.ai_completion_enabled && !self.command_suggestions.is_empty() {
                    ui.label(format!("💡 {} suggestions", self.command_suggestions.len()));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("🔄 Refresh").clicked() {
                            self.get_ai_command_completions();
                        }
                    });
                }
            });

            let text_edit_out = TextEdit::singleline(&mut self.input)
                .hint_text("Use Wisely.. (Press Tab for AI suggestions)")
                .margin(Margin::symmetric(10, 4))
                .desired_width(ui.available_width())
                .desired_rows(4)
                .layouter(&mut layouter)
                .show(ui);

            // text_edit_out.response.c

            let text_edit = text_edit_out.response;

            *text_response = Some(text_edit.clone());

            let tab_press = ui.input(|i| i.key_pressed(Key::Tab));

            #[cfg(not(target_arch="wasm32"))]
            if tab_press && self.command_suggestions.is_empty() {
                text_edit.request_focus();
            } else if cfg!(not(target_arch="wasm32")) && tab_press && text_edit.lost_focus() {
                text_edit.request_focus();
            }

            if tab_press && text_edit.lost_focus() {
                text_edit.request_focus();
            }

            // Handle AI command completion with debouncing
            #[cfg(not(target_arch="wasm32"))]
            if self.ai_completion_enabled && text_edit.changed() {
                self.last_input_change_time = Some(web_time::Instant::now());
                self.command_suggestions.clear(); // Clear old suggestions on new input
                self.show_suggestions = false;
            }

            #[cfg(not(target_arch="wasm32"))]
            if self.ai_completion_enabled {
                if let Some(last_change) = self.last_input_change_time {
                    if last_change.elapsed() > std::time::Duration::from_millis(100) {
                        // Timer has expired, clear it and fire the request.
                        self.last_input_change_time = None;
                        if !self.input.is_empty() {
                            self.get_ai_command_completions();
                        }
                    }
                }
            }

            // Show AI suggestions if available
            #[cfg(not(target_arch="wasm32"))]
            #[cfg(not(target_arch="wasm32"))]
            if self.ai_completion_enabled && !self.command_suggestions.is_empty() && self.show_suggestions {
                ui.add_space(5.);
                ui.group(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new("👾 AI Command Suggestions:").strong().color(Color32::LIGHT_BLUE));
                        
                        ScrollArea::vertical().max_height(150.).show(ui, |ui| {
                            let len = self.command_suggestions.len();
                            for idx in 0..len {
                                let suggestion = &self.command_suggestions[idx];
                                let selected = idx == self.selected_suggestion;
                                
                                ui.horizontal(|ui| {
                                    let response = ui.selectable_label(
                                        selected,
                                        RichText::new(&suggestion.completion)
                                            .monospace()
                                            .color(if selected { Color32::CYAN } else { Color32::WHITE })
                                    );
                                    
                                    if response.clicked() {
                                        let completion_str = suggestion.completion.clone();
                                        // Defer mutation after UI borrow scope
                                        self.pending_completion = Some(completion_str);
                                        self.show_suggestions = false;
                                        text_edit.request_focus();
                                    }
                                    
                                    ui.add_space(5.);
                                    
                                    if let Some(desc) = &suggestion.description {
                                        ui.label(RichText::new(desc).weak().italics());
                                    }
                                    
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        ui.label(RichText::new(format!("{:.0}%", suggestion.confidence * 100.)).weak());
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

            // Apply any deferred completion outside of the borrowing loops (native only)
            #[cfg(not(target_arch="wasm32"))]
            {
                if let Some(done) = self.pending_completion.take() {
                    self.apply_command_suggestion(&done);
                    // Move caret to end after applying suggestion so user can continue typing.
                    let text_id = text_edit.id;
                    if let Some(mut state) = eframe::egui::widgets::text_edit::TextEditState::load(ui.ctx(), text_id) {
                        use eframe::egui::text::{CCursor, CCursorRange};
                        let end = CCursor::new(self.input.chars().count());
                        let mut cursor = state.cursor.clone();
                        cursor.set_char_range(Some(CCursorRange::one(end)));
                        state.cursor = cursor;
                        state.store(ui.ctx(), text_id);
                    }
                    text_edit.request_focus();
                }
            }

            let key_press = ui.input(|i| i.key_pressed(Key::Enter));
            let up_press = ui.input(|i| i.key_pressed(Key::ArrowUp));
            let down_press = ui.input(|i| i.key_pressed(Key::ArrowDown));
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

            // Handle AI suggestion navigation (native) or fallback to history navigation (wasm)
            #[cfg(not(target_arch="wasm32"))]
            {
                if self.ai_completion_enabled && !self.command_suggestions.is_empty() && self.show_suggestions {
                    if tab_press {
                        self.selected_suggestion = (self.selected_suggestion + 1) % self.command_suggestions.len();
                    }
                    if escape_press { self.show_suggestions = false; }
                    if key_press {
                        if let Some(suggestion) = self.command_suggestions.get(self.selected_suggestion) {
                            self.pending_completion = Some(suggestion.completion.clone());
                            self.show_suggestions = false;
                        }
                    }
                } else {
                    // Normal history navigation when suggestions not shown
                    if down_press {
                        if self.history_idx <= self.my_command_history.len() { self.history_idx += 1; }
                        if let Some(history) = self.my_command_history.get(self.history_idx){ self.input = history.message.clone(); }
                    }
                    if up_press {
                        if self.history_idx > 0 { self.history_idx -= 1; }
                        if let Some(history) = self.my_command_history.get(self.history_idx){ self.input = history.message.clone(); }
                    }
                    if tab_press && self.ai_completion_enabled && !self.input.is_empty() {
                        if !self.show_suggestions && self.completion_cancel_tx.is_none() { self.get_ai_command_completions(); }
                        self.show_suggestions = true;
                        self.selected_suggestion = 0;
                    }
                }
            }
            #[cfg(target_arch="wasm32")]
            {
                // WASM: Only history navigation
                if down_press {
                    if self.history_idx <= self.my_command_history.len() { self.history_idx += 1; }
                    if let Some(history) = self.my_command_history.get(self.history_idx){ self.input = history.message.clone(); }
                }
                if up_press {
                    if self.history_idx > 0 { self.history_idx -= 1; }
                    if let Some(history) = self.my_command_history.get(self.history_idx){ self.input = history.message.clone(); }
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

            let tab_press = ui.input(|i| i.key_pressed(Key::Tab));
            if tab_press {
                ui.input_mut(|i| {
                    i.events = vec![];
                });
                if let Some(res) = text_response {
                    if res.lost_focus() {
                        res.request_focus();
                    }
                }
            }

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
                                        let btn = Button::new(from).fill(Color32::from_rgb(7, 7, 9)).min_size(Vec2::new(30., 35.)).ui(ui);

                                        if btn.has_focus() {
                                            btn.surrender_focus();
                                            if let Some(res) = text_response {
                                                res.request_focus();
                                            }
                                        }

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
                                    ui.with_layout(Layout::left_to_right(
                                        // Direction::TopDown,
                                        Align::Min,
                                    ), |ui| {
                                        ui.set_width(ui.available_width());
                                        // crate::markdown_editor::viewer::easy_mark(ui, &item.message);
                                        ui.label(RichText::new(&item.message).monospace());
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
            let partial_command = self.input.clone();
            if partial_command.is_empty() || !self.ai_completion_enabled {
                return;
            }

            // If a request for the same command is already in flight, do nothing.
            if self.completion_cancel_tx.is_some() && partial_command == self.last_partial_command {
                return;
            }

            // Cancel any previous, different request.
            if let Some(cancel_tx) = self.completion_cancel_tx.take() {
                let _ = cancel_tx.send(());
            }

            // Set state for the new request *before* spawning.
            self.last_partial_command = partial_command.clone();
            self.command_suggestions.clear();

            let shell_type = ShellType::PowerShell; // TODO: make user selectable
            let tx = self.diagnostic_tx.clone();
            let openai_session = self.mcp_service.openai_session.clone();
            
            let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
            self.completion_cancel_tx = Some(cancel_tx);

            PlatformSpawner::spawn(async move {
                if let Ok(guard) = openai_session.try_lock() {
                    if let Some(session) = guard.as_ref() {
                        // Use the new streaming completion method with cancellation
                        match session.stream_command_completions(&partial_command, &shell_type, cancel_rx, tx.clone()).await {
                            Ok(()) => {
                                // Completion stream finished; nothing else to do.
                            }
                            Err(e) => {
                                if e.to_string() != "Request cancelled" {
                                    log::error!("AI streaming completion error: {e:?}");
                                    let _ = tx.try_send(DiagnosticResponse::Error { message: "AI completion error".to_string(), details: Some(e.to_string()) });
                                }
                            }
                        }
                    }
                }
            });
        }
    }
}

