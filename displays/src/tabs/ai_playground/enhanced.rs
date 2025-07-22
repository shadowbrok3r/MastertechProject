use eframe::egui::{
    Align, Button, CentralPanel, Color32, ComboBox, Frame, Layout, Margin, RichText, 
    ScrollArea, SidePanel, TextEdit, TopBottomPanel, Ui, Vec2
};
use crate::{
    app_state::SharedContext, 
    PlatformSpawner, Spawner,
    tabs::ai_playground::{ChatMessage, ChatThread, SentFrom, ChatMessageType},
};

#[cfg(not(target_arch = "wasm32"))]
use crate::mcp::{McpService, LlmProvider, DiagnosticCommand, DiagnosticResponse};

use std::collections::HashMap;
use crossbeam::channel::{Receiver, Sender};
use serde::{Deserialize, Serialize};
use log::info;

/// Enhanced AI playground with MCP diagnostic capabilities
pub struct EnhancedAiPlayground {
    pub selected_thread: String,
    pub chat_title: HashMap<String, String>,
    pub edit_title: bool,
    pub threads: HashMap<String, ChatThread>,
    pub response_tx: Sender<ChatMessage>,
    pub response_rx: Receiver<ChatMessage>,
    pub save_chats: bool,
    pub image_id: String,
    pub open_modal: bool,
    
    // MCP-specific fields
    #[cfg(not(target_arch = "wasm32"))]
    pub mcp_service: McpService,
    pub current_mode: AiMode,
    pub llm_provider: LlmProvider,
    pub show_provider_config: bool,
    
    // Diagnostic features
    pub pending_approvals: Vec<ScriptApprovalRequest>,
    pub completion_suggestions: Vec<String>,
    pub last_partial_command: String,
}

#[derive(Debug, Clone, PartialEq)]
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
            
            #[cfg(not(target_arch = "wasm32"))]
            mcp_service: McpService::default(),
            current_mode: AiMode::Chat,
            llm_provider: LlmProvider::default(),
            show_provider_config: false,
            
            pending_approvals: Vec::new(),
            completion_suggestions: Vec::new(),
            last_partial_command: String::new(),
        }
    }
}

impl SharedContext {
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
                        .selected_text(format!("{:?}", self.enhanced_ai_playground.current_mode))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.enhanced_ai_playground.current_mode, AiMode::Chat, "💬 Chat");
                            ui.selectable_value(&mut self.enhanced_ai_playground.current_mode, AiMode::Diagnostics, "🔧 Diagnostics");
                            ui.selectable_value(&mut self.enhanced_ai_playground.current_mode, AiMode::Shell, "⚡ Shell Assistant");
                        });

                    ui.add_space(20.);

                    // Provider configuration
                    if ui.button("⚙️ Provider Config").clicked() {
                        self.enhanced_ai_playground.show_provider_config = !self.enhanced_ai_playground.show_provider_config;
                    }

                    ui.add_space(ui.available_width() - 200.);

                    // Status indicator
                    match self.enhanced_ai_playground.current_mode {
                        AiMode::Chat => ui.label(RichText::new("💬 Ready for conversation").color(Color32::LIGHT_GREEN)),
                        AiMode::Diagnostics => ui.label(RichText::new("🔧 Diagnostic tools available").color(Color32::LIGHT_BLUE)),
                        AiMode::Shell => ui.label(RichText::new("⚡ Command completion active").color(Color32::YELLOW)),
                    };
                });

                // Provider configuration row
                if self.enhanced_ai_playground.show_provider_config {
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label("Provider:");
                        match &mut self.enhanced_ai_playground.llm_provider {
                            LlmProvider::OpenAI { model, .. } => {
                                ComboBox::from_label("OpenAI Model")
                                    .selected_text(model.as_str())
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(model, "gpt-4".to_string(), "GPT-4");
                                        ui.selectable_value(model, "gpt-4-turbo".to_string(), "GPT-4 Turbo");
                                        ui.selectable_value(model, "gpt-3.5-turbo".to_string(), "GPT-3.5 Turbo");
                                    });
                            }
                            _ => {
                                ui.label("Other providers coming soon...");
                            }
                        }
                    });
                }
            });

        // Left sidebar - mode-specific panels
        SidePanel::left("enhanced_ai_sidebar")
            .frame(Frame::default().inner_margin(Margin::same(8)))
            .exact_width(220.)
            .show_inside(ui, |ui| {
                match self.enhanced_ai_playground.current_mode {
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
                match self.enhanced_ai_playground.current_mode {
                    AiMode::Chat => self.show_chat_input(ui),
                    AiMode::Diagnostics => self.show_diagnostics_input(ui),
                    AiMode::Shell => self.show_shell_input(ui),
                }
            });

        // Central panel - main content
        CentralPanel::default()
            .frame(Frame::dark_canvas(ui.style()))
            .show_inside(ui, |ui| {
                match self.enhanced_ai_playground.current_mode {
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
            let selected_thread = self.enhanced_ai_playground.selected_thread.clone();
            for (thread_id, _) in self.enhanced_ai_playground.threads.iter() {
                let title = self.enhanced_ai_playground.chat_title
                    .get(thread_id)
                    .unwrap_or(thread_id);

                if ui.selectable_label(
                    selected_thread.eq(thread_id), 
                    RichText::new(title)
                ).clicked() {
                    self.enhanced_ai_playground.selected_thread = thread_id.clone();
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

            if ui.button("🟦 Analyze BSOD Dumps").clicked() {
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

            if ui.button("🖥️ System Summary").clicked() {
                self.run_diagnostic_tool("get_system_summary", serde_json::json!({
                    "include_hardware": true,
                    "include_software": true,
                    "include_network": true
                }));
            }

            ui.separator();
            ui.heading("⏳ Pending Approvals");

            for approval in &self.enhanced_ai_playground.pending_approvals {
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
                for suggestion in &self.enhanced_ai_playground.completion_suggestions {
                    if ui.button(suggestion).clicked() {
                        // Use this suggestion
                        if let Some(thread) = self.enhanced_ai_playground.threads.get_mut(&self.enhanced_ai_playground.selected_thread) {
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
            if ui.button("🗂️ List Processes").clicked() {
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
        if let Some(thread) = self.enhanced_ai_playground.threads.get_mut(&self.enhanced_ai_playground.selected_thread) {
            ui.horizontal(|ui| {
                let text_edit = TextEdit::multiline(&mut thread.input)
                    .desired_width(ui.available_width() - 80.)
                    .hint_text("Ask me anything...");
                
                let response = ui.add(text_edit);
                
                if ui.button("Send").clicked() || (response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                    self.send_chat_message();
                }
            });
        }
    }

    fn show_diagnostics_input(&mut self, ui: &mut Ui) {
        if let Some(thread) = self.enhanced_ai_playground.threads.get_mut(&self.enhanced_ai_playground.selected_thread) {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    let text_edit = TextEdit::multiline(&mut thread.input)
                        .desired_width(ui.available_width() - 80.)
                        .hint_text("Describe the issue you're experiencing...");
                    
                    ui.add(text_edit);
                    
                    if ui.button("Analyze").clicked() {
                        self.send_diagnostic_request();
                    }
                });
                
                ui.label(RichText::new("💡 Try: 'My computer is running slow', 'Blue screen occurred', 'Check for errors'")
                    .italics().weak());
            });
        }
    }

    fn show_shell_input(&mut self, ui: &mut Ui) {
        if let Some(thread) = self.enhanced_ai_playground.threads.get_mut(&self.enhanced_ai_playground.selected_thread) {
            ui.horizontal(|ui| {
                let text_edit = TextEdit::singleline(&mut thread.input)
                    .desired_width(ui.available_width() - 80.)
                    .hint_text("Type a partial command...");
                
                let response = ui.add(text_edit);
                
                if response.changed() {
                    self.enhanced_ai_playground.last_partial_command = thread.input.clone();
                    self.get_command_completions();
                }
                
                if ui.button("Complete").clicked() {
                    self.get_command_completions();
                }
            });
        }
    }

    fn show_chat_content(&mut self, ui: &mut Ui) {
        // Regular chat display (similar to existing implementation)
        ScrollArea::vertical().show(ui, |ui| {
            if let Some(thread) = self.enhanced_ai_playground.threads.get(&self.enhanced_ai_playground.selected_thread) {
                for message in &thread.messages {
                    self.render_chat_message(ui, message);
                }
            }
        });
    }

    fn show_diagnostics_content(&mut self, ui: &mut Ui) {
        ScrollArea::vertical().show(ui, |ui| {
            ui.heading("🔧 Computer Diagnostics");
            ui.separator();
            
            if let Some(thread) = self.enhanced_ai_playground.threads.get(&self.enhanced_ai_playground.selected_thread) {
                for message in &thread.messages {
                    self.render_diagnostic_message(ui, message);
                }
            }
            
            if self.enhanced_ai_playground.threads.is_empty() {
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
        ScrollArea::vertical().show(ui, |ui| {
            ui.heading("⚡ Shell Command Assistant");
            ui.separator();
            
            if !self.enhanced_ai_playground.completion_suggestions.is_empty() {
                ui.label("Command Completions:");
                for suggestion in &self.enhanced_ai_playground.completion_suggestions {
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
        while let Ok(response) = self.enhanced_ai_playground.response_rx.try_recv() {
            ui.ctx().request_repaint();
            
            let current_thread = self.enhanced_ai_playground.threads
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
        self.enhanced_ai_playground.selected_thread = thread_id.clone();
        self.enhanced_ai_playground.threads.insert(thread_id.clone(), ChatThread {
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
        #[cfg(not(target_arch = "wasm32"))]
        {
            if let Some(thread) = self.enhanced_ai_playground.threads.get(&self.enhanced_ai_playground.selected_thread) {
                let input = thread.input.clone();
                let response_tx = self.enhanced_ai_playground.response_tx.clone();
                
                PlatformSpawner::spawn(async move {
                    // This would integrate with MCP diagnostic tools
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
    }

    fn get_command_completions(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let partial_command = self.enhanced_ai_playground.last_partial_command.clone();
            if !partial_command.is_empty() {
                // This would use MCP command completion
                self.enhanced_ai_playground.completion_suggestions = vec![
                    format!("{} /?", partial_command),
                    format!("{}list", partial_command),
                    format!("{}info", partial_command),
                ];
            }
        }
    }

    fn suggest_command(&mut self, command: &str) {
        if let Some(thread) = self.enhanced_ai_playground.threads.get_mut(&self.enhanced_ai_playground.selected_thread) {
            thread.input = command.to_string();
        }
    }

    fn run_diagnostic_tool(&mut self, tool_name: &str, params: serde_json::Value) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let response_tx = self.enhanced_ai_playground.response_tx.clone();
            let tool_name = tool_name.to_string();
            
            PlatformSpawner::spawn(async move {
                // This would run the actual MCP diagnostic tool
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