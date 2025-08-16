use eframe::egui::{
    CentralPanel, Color32, ComboBox, Frame, Key, Margin, RichText, ScrollArea, SidePanel, TextEdit, TopBottomPanel, Ui
};
use crate::{
    PlatformSpawner, Spawner,
    tabs::ai_playground::{ChatMessage, ChatThread, SentFrom, ChatMessageType},
};

use std::collections::HashMap;
use crossbeam::channel::{Receiver, Sender};
use serde::{Deserialize, Serialize};
use log::info;

/// Enhanced AI playground with diagnostic capabilities
#[derive(Serialize)]
pub struct EnhancedAiPlayground {
    pub selected_thread: String,
    pub chat_title: HashMap<String, String>,
    pub edit_title: bool,
    pub threads: HashMap<String, ChatThread>,
    #[serde(skip)]
    pub response_tx: Sender<ChatMessage>,
    #[serde(skip)]
    pub response_rx: Receiver<ChatMessage>,
    pub save_chats: bool,
    pub image_id: String,
    pub open_modal: bool,
    
    // Enhanced features
    pub current_mode: AiMode,
    pub show_provider_config: bool,
    
    // Diagnostic features
    pub pending_approvals: Vec<ScriptApprovalRequest>,
    pub completion_suggestions: Vec<String>,
    pub last_partial_command: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum AiMode {
    Chat,           // Regular chat mode
    Diagnostics,    // Computer diagnostics mode
    Shell,          // Shell command completion mode
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptApprovalRequest {
    pub id: String,
    pub script: String,
    pub description: String,
    pub risk_level: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl Default for EnhancedAiPlayground {
    fn default() -> Self {
        let (response_tx, response_rx) = crossbeam::channel::unbounded::<ChatMessage>();

        Self {
            selected_thread: String::new(),
            threads: HashMap::new(),
            chat_title: HashMap::new(),
            edit_title: false,
            response_tx,
            response_rx,
            save_chats: false,
            image_id: String::new(),
            open_modal: false,
            
            current_mode: AiMode::Chat,
            show_provider_config: false,
            
            pending_approvals: Vec::new(),
            completion_suggestions: Vec::new(),
            last_partial_command: String::new(),
        }
    }
}

impl EnhancedAiPlayground {
    pub fn enhanced_ai_playground(&mut self, ui: &mut Ui) {
        // Top panel with mode selection and provider config
        TopBottomPanel::top("enhanced_ai_top")
            .frame(Frame::default().inner_margin(Margin::same(8)))
            .exact_height(60.)
            .show_inside(ui, |ui| {
                ui.horizontal(|ui| {
                    // Mode selection
                    ui.label(RichText::new("Mode:").strong());
                    ComboBox::from_label("")
                        .selected_text(format!("{:?}", self.current_mode))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.current_mode, AiMode::Chat, "💬 Chat");
                            ui.selectable_value(&mut self.current_mode, AiMode::Diagnostics, "🔧 Diagnostics");
                            ui.selectable_value(&mut self.current_mode, AiMode::Shell, "⚡ Shell Assistant");
                        });

                    ui.add_space(20.);

                    // Provider configuration
                    if ui.button("⚙ Provider Config").clicked() {
                        self.show_provider_config = !self.show_provider_config;
                    }

                    ui.add_space(ui.available_width() - 200.);

                    // Status indicator
                    match self.current_mode {
                        AiMode::Chat => ui.label(RichText::new("💬 Ready for conversation").color(Color32::LIGHT_GREEN)),
                        AiMode::Diagnostics => ui.label(RichText::new("🔧 Diagnostic tools available").color(Color32::LIGHT_BLUE)),
                        AiMode::Shell => ui.label(RichText::new("⚡ Command completion active").color(Color32::YELLOW)),
                    };
                });

                // Provider configuration row
                if self.show_provider_config {
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label("Provider:");
                        ui.label("OpenAI GPT-4 (Default)");
                        ui.label("⭕ Connected");
                    });
                }
            });

        // Left sidebar - mode-specific panels
        SidePanel::left("enhanced_ai_sidebar")
            .frame(Frame::default().inner_margin(Margin::same(8)))
            .exact_width(220.)
            .show_inside(ui, |ui| {
                match self.current_mode {
                    AiMode::Chat => self.show_chat_sidebar(ui),
                    AiMode::Diagnostics => self.show_diagnostics_sidebar(ui),
                    AiMode::Shell => self.show_shell_sidebar(ui),
                }
            });

        // Bottom panel - input area
        TopBottomPanel::bottom("enhanced_ai_input")
            .frame(Frame::default().inner_margin(Margin::same(8)))
            .exact_height(80.)
            .show_inside(ui, |ui| {
                match self.current_mode {
                    AiMode::Chat => self.show_chat_input(ui),
                    AiMode::Diagnostics => self.show_diagnostics_input(ui),
                    AiMode::Shell => self.show_shell_input(ui),
                }
            });

        // Central panel - main content
        CentralPanel::default()
            .frame(Frame::dark_canvas(ui.style()))
            .show_inside(ui, |ui| {
                match self.current_mode {
                    AiMode::Chat => self.show_chat_content(ui),
                    AiMode::Diagnostics => self.show_diagnostics_content(ui),
                    AiMode::Shell => self.show_shell_content(ui),
                }
            });

        // Handle events
        self.handle_enhanced_ai_events(ui);
    }

    fn show_chat_sidebar(&mut self, ui: &mut Ui) {
        ui.vertical_centered(|ui| {
            ui.heading("💬 Chat Threads");
            ui.separator();

            // Show existing threads
            let selected_thread = self.selected_thread.clone();
            for (thread_id, _) in self.threads.iter() {
                let title = self.chat_title
                    .get(thread_id)
                    .unwrap_or(thread_id);

                if ui.selectable_label(
                    selected_thread.eq(thread_id), 
                    RichText::new(title)
                ).clicked() {
                    self.selected_thread = thread_id.clone();
                }
            }

            ui.add_space(10.);

            if ui.button("➕ New Chat").clicked() {
                self.create_new_chat_thread();
            }
        });
    }

    fn show_diagnostics_sidebar(&mut self, ui: &mut Ui) {
        ui.vertical_centered(|ui| {
            ui.heading("🔧 Diagnostic Tools");
            ui.separator();

            if ui.button("🕱 Analyze BSOD Dumps").clicked() {
                self.run_diagnostic_tool("analyze_bsod", serde_json::json!({"include_recent": true}));
            }

            if ui.button("📋 Check Event Logs").clicked() {
                self.run_diagnostic_tool("analyze_event_logs", serde_json::json!({
                    "log_name": "System",
                    "hours_back": 24
                }));
            }

            if ui.button("📊 Performance Report").clicked() {
                self.run_diagnostic_tool("generate_performance_report", serde_json::json!({
                    "duration_hours": 24,
                    "include_processes": true,
                    "include_hardware": true
                }));
            }

            if ui.button("💻 System Summary").clicked() {
                self.run_diagnostic_tool("get_system_summary", serde_json::json!({
                    "include_hardware": true,
                    "include_software": true,
                    "include_network": true
                }));
            }

            ui.separator();
            ui.heading("⏳ Pending Approvals");

            for approval in &self.pending_approvals {
                ui.group(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new(&approval.description).strong());
                        ui.label(format!("Risk: {}", approval.risk_level));
                        ui.horizontal(|ui| {
                            if ui.button("✅ Approve").clicked() {
                                // Handle approval
                            }
                            if ui.button("❌ Deny").clicked() {
                                // Handle denial
                            }
                        });
                    });
                });
            }
        });
    }

    fn show_shell_sidebar(&mut self, ui: &mut Ui) {
        ui.vertical_centered(|ui| {
            ui.heading("⚡ Shell Assistant");
            ui.separator();

            ui.label("Recent Commands:");
            ScrollArea::vertical().max_height(200.).show(ui, |ui| {
                for suggestion in &self.completion_suggestions {
                    if ui.button(suggestion).clicked() {
                        // Use this suggestion
                        if let Some(thread) = self.threads.get_mut(&self.selected_thread) {
                            thread.input = suggestion.clone();
                        }
                    }
                }
            });

            ui.separator();

            ui.label("Quick Actions:");
            if ui.button("🔍 System Info").clicked() {
                self.suggest_command("systeminfo");
            }
            if ui.button("📂 List Processes").clicked() {
                self.suggest_command("tasklist");
            }
            if ui.button("🌐 Network Status").clicked() {
                self.suggest_command("ipconfig /all");
            }
            if ui.button("💾 Disk Usage").clicked() {
                self.suggest_command("dir C:\\ /s");
            }
        });
    }

    fn show_chat_input(&mut self, ui: &mut Ui) {
        if let Some(thread) = self.threads.get_mut(&self.selected_thread) {
            // Move input to a local variable to avoid borrow issues
            let mut input = thread.input.clone();
            let mut send = false;
            ui.horizontal(|ui| {
                let text_edit = TextEdit::multiline(&mut input)
                    .desired_width(ui.available_width() - 80.)
                    .hint_text("Ask me anything...");
                let response = ui.add(text_edit);
                if ui.button("Send").clicked() || (response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter))) {
                    send = true;
                }
            });
            // Write back input if changed
            if thread.input != input {
                thread.input = input;
            }
            if send {
                self.send_chat_message();
            }
        }
    }

    fn show_diagnostics_input(&mut self, ui: &mut Ui) {
        if let Some(thread) = self.threads.get_mut(&self.selected_thread) {
            let mut input = thread.input.clone();
            let mut analyze = false;
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    let text_edit = TextEdit::multiline(&mut input)
                        .desired_width(ui.available_width() - 80.)
                        .hint_text("Describe the issue you're experiencing...");
                    ui.add(text_edit);
                    if ui.button("Analyze").clicked() {
                        analyze = true;
                    }
                });
                ui.label(RichText::new("💡 Try: 'My computer is running slow', 'Blue screen occurred', 'Check for errors'")
                    .italics().weak());
            });
            if thread.input != input {
                thread.input = input;
            }
            if analyze {
                self.send_diagnostic_request();
            }
        }
    }

    fn show_shell_input(&mut self, ui: &mut Ui) {
        if let Some(thread) = self.threads.get_mut(&self.selected_thread) {
            let mut input = thread.input.clone();
            let mut changed = false;
            let mut complete = false;
            ui.horizontal(|ui| {
                let text_edit = TextEdit::singleline(&mut input)
                    .desired_width(ui.available_width() - 80.)
                    .hint_text("Type a partial command...");
                let response = ui.add(text_edit);
                if response.changed() {
                    changed = true;
                }
                if ui.button("Complete").clicked() {
                    complete = true;
                }
            });
            if thread.input != input {
                thread.input = input.clone();
            }
            if changed {
                self.last_partial_command = input;
                self.get_command_completions();
            }
            if complete {
                self.get_command_completions();
            }
        }
    }

    fn show_chat_content(&mut self, ui: &mut Ui) {
        // Regular chat display (similar to existing implementation)
        let messages = if let Some(thread) = self.threads.get(&self.selected_thread) {
            thread.messages.clone()
        } else {
            Vec::new()
        };
        ScrollArea::vertical().show(ui, |ui| {
            for message in messages.iter() {
                self.render_chat_message(ui, message);
            }
        });
    }

    fn show_diagnostics_content(&mut self, ui: &mut Ui) {
        let messages = if let Some(thread) = self.threads.get(&self.selected_thread) {
            thread.messages.clone()
        } else {
            Vec::new()
        };
        let threads_empty = self.threads.is_empty();
        ScrollArea::vertical().show(ui, |ui| {
            ui.heading("🔧 Computer Diagnostics");
            ui.separator();
            for message in messages.iter() {
                self.render_diagnostic_message(ui, message);
            }
            if threads_empty {
                ui.vertical_centered(|ui| {
                    ui.add_space(100.);
                    ui.heading("Welcome to Computer Diagnostics");
                    ui.label("Select a diagnostic tool from the sidebar or describe an issue you're experiencing.");
                    ui.add_space(20.);
                    ui.label("I can help you with:");
                    ui.label("• Analyzing Blue Screen crashes (BSOD)");
                    ui.label("• Checking Windows Event Logs for errors");
                    ui.label("• Generating performance reports");
                    ui.label("• Providing system health summaries");
                    ui.label("• Recommending solutions for common issues");
                });
            }
        });
    }

    fn show_shell_content(&mut self, ui: &mut Ui) {
        let suggestions: Vec<String> = self.completion_suggestions.clone();
        ScrollArea::vertical().show(ui, |ui| {
            ui.heading("⚡ Shell Command Assistant");
            ui.separator();
            if !suggestions.is_empty() {
                ui.label("Command Completions:");
                for suggestion in &suggestions {
                    ui.horizontal(|ui| {
                        if ui.button("▶").clicked() {
                            // Execute command suggestion
                        }
                        ui.label(suggestion);
                    });
                }
            } else {
                ui.vertical_centered(|ui| {
                    ui.add_space(100.);
                    ui.heading("Shell Command Assistant");
                    ui.label("Start typing a command in the input box below for intelligent completions.");
                    ui.add_space(20.);
                    ui.label("Features:");
                    ui.label("• Smart command completion for CMD, PowerShell, and Bash");
                    ui.label("• Context-aware suggestions");
                    ui.label("• Common administrative commands");
                    ui.label("• Safety warnings for potentially dangerous commands");
                });
            }
        });
    }

    fn handle_enhanced_ai_events(&mut self, ui: &mut Ui) {
        while let Ok(response) = self.response_rx.try_recv() {
            ui.ctx().request_repaint();
            
            let current_thread = self.threads
                .entry(response.thread_id.clone())
                .or_insert_with(|| ChatThread {
                    id: response.thread_id.clone(),
                    messages: Vec::new(),
                    images: Vec::new(),
                    input: String::new(),
                });

            current_thread.messages.push(response);
        }
    }

    // Helper methods
    fn create_new_chat_thread(&mut self) {
        let thread_id = uuid::Uuid::new_v4().to_string();
        self.selected_thread = thread_id.clone();
        self.threads.insert(thread_id.clone(), ChatThread {
            id: thread_id,
            messages: Vec::new(),
            images: Vec::new(),
            input: String::new(),
        });
    }

    fn send_chat_message(&mut self) {
        // Implementation similar to existing chat functionality
        info!("Sending chat message in enhanced AI playground");
    }

    fn send_diagnostic_request(&mut self) {
        if let Some(thread) = self.threads.get(&self.selected_thread) {
            let input = thread.input.clone();
            let response_tx = self.response_tx.clone();
            
            PlatformSpawner::spawn(async move {
                // This would integrate with diagnostic tools
                let response = ChatMessage {
                    id: uuid::Uuid::new_v4().to_string(),
                    thread_id: "diagnostic".to_string(),
                    ts: chrono::Utc::now().timestamp() as i32,
                    from: SentFrom::Gpt,
                    content: ChatMessageType::Text(format!("Analyzing: {}", input)),
                };
                let _ = response_tx.send(response);
            });
        }
    }

    fn get_command_completions(&mut self) {
        let partial_command = self.last_partial_command.clone();
        if !partial_command.is_empty() {
            // This would use command completion
            self.completion_suggestions = vec![
                format!("{} /?", partial_command),
                format!("{}list", partial_command),
                format!("{}info", partial_command),
            ];
        }
    }

    fn suggest_command(&mut self, command: &str) {
        if let Some(thread) = self.threads.get_mut(&self.selected_thread) {
            thread.input = command.to_string();
        }
    }

    fn run_diagnostic_tool(&mut self, tool_name: &str, _params: serde_json::Value) {
        let response_tx = self.response_tx.clone();
        let tool_name = tool_name.to_string();
        
        PlatformSpawner::spawn(async move {
            // This would run the actual diagnostic tool
            let response = ChatMessage {
                id: uuid::Uuid::new_v4().to_string(),
                thread_id: "diagnostic".to_string(),
                ts: chrono::Utc::now().timestamp() as i32,
                from: SentFrom::Gpt,
                content: ChatMessageType::Text(format!("Running {}...", tool_name)),
            };
            let _ = response_tx.send(response);
        });
    }

    fn render_chat_message(&self, ui: &mut Ui, message: &ChatMessage) {
        // Implementation similar to existing chat message rendering
        ui.label(format!("{:?}: {:?}", message.from, message.content));
    }

    fn render_diagnostic_message(&self, ui: &mut Ui, message: &ChatMessage) {
        // Enhanced rendering for diagnostic messages with special formatting
        ui.group(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(format!("{:?}", message.from)).strong());
                match &message.content {
                    ChatMessageType::Text(text) => {
                        ui.label(text);
                    }
                    _ => {
                        ui.label(format!("{:?}", message.content));
                    }
                }
            });
        });
    }
}