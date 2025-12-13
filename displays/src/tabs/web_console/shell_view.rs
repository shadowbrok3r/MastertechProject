//! AI-enhanced shell view with MCP integration.
//!
//! Features:
//! - Shell type selector (PowerShell / CMD)
//! - AI command completion via MCP tools
//! - Command history
//! - Interactive shell mode

use crate::{mcp::McpService, Cmd, PlatformSpawner, Spawner};
use crossbeam::channel::{Receiver, Sender};
use database::schema::ConnectedClient;
use eframe::egui::{
    Align, Button, Color32, ComboBox, Context, CornerRadius, Frame, Key, Layout, Margin, RichText,
    ScrollArea, TextEdit, Ui, Vec2,
};
use serde::{Deserialize, Serialize};
use web_time::Instant;

/// Shell type selection
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ShellType {
    #[default]
    PowerShell,
    Cmd,
    Bash,
}

impl ShellType {
    pub fn as_str(&self) -> &str {
        match self {
            ShellType::PowerShell => "PowerShell",
            ShellType::Cmd => "CMD",
            ShellType::Bash => "Bash",
        }
    }

    pub fn prompt_prefix(&self) -> &str {
        match self {
            ShellType::PowerShell => "PS>",
            ShellType::Cmd => ">",
            ShellType::Bash => "$",
        }
    }
}

/// A single command history entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub command: String,
    pub output: String,
    pub timestamp: String,
    pub success: bool,
    pub shell_type: ShellType,
}

/// AI command completion suggestion
#[derive(Debug, Clone)]
pub struct CommandSuggestion {
    pub completion: String,
    pub description: Option<String>,
    pub confidence: f32,
}

/// Shell view state
pub struct ShellView {
    /// The client this shell is connected to
    pub client: ConnectedClient,
    /// Current shell type
    pub shell_type: ShellType,
    /// Current command input
    pub input: String,
    /// Command history
    pub history: Vec<HistoryEntry>,
    /// History navigation index
    history_index: usize,
    /// Channel to send commands to the client
    send_cmd_tx: Sender<Cmd>,
    /// Channel to receive command responses
    #[allow(dead_code)]
    receive_cmd_rx: Receiver<Cmd>,
    /// Channel to receive shell output text
    shell_output_rx: Option<Receiver<String>>,
    /// Output buffer for current command
    pub output_buffer: String,
    /// Is the shell waiting for a response?
    pub is_loading: bool,
    /// AI completion suggestions
    pub suggestions: Vec<CommandSuggestion>,
    /// Show suggestions popup
    pub show_suggestions: bool,
    /// Selected suggestion index
    pub selected_suggestion: usize,
    /// Last input for completion tracking
    last_completion_input: String,
    /// AI completion enabled
    pub ai_enabled: bool,
    /// MCP service for AI features
    #[cfg(not(target_arch = "wasm32"))]
    mcp_service: McpService,
    /// Completion cancel sender
    #[cfg(not(target_arch = "wasm32"))]
    completion_cancel_tx: Option<tokio::sync::oneshot::Sender<()>>,
    /// Last input change time for debouncing
    #[cfg(not(target_arch = "wasm32"))]
    last_input_change: Option<Instant>,
    /// Diagnostic response receiver
    #[cfg(not(target_arch = "wasm32"))]
    diagnostic_rx: Receiver<crate::mcp::DiagnosticResponse>,
    #[cfg(not(target_arch = "wasm32"))]
    diagnostic_tx: Sender<crate::mcp::DiagnosticResponse>,
    /// Interactive mode (persistent shell session)
    pub interactive_mode: bool,
}

impl ShellView {
    pub fn new(
        client: ConnectedClient, 
        shell_type: ShellType, 
        send_cmd_tx: Sender<Cmd>,
        shell_output_rx: Option<Receiver<String>>,
    ) -> Self {
        let (_receive_cmd_tx, receive_cmd_rx) = crossbeam::channel::unbounded();
        
        #[cfg(not(target_arch = "wasm32"))]
        let (diagnostic_tx, diagnostic_rx) = crossbeam::channel::unbounded();
        
        #[cfg(not(target_arch = "wasm32"))]
        let mcp_service = McpService::default();

        Self {
            client,
            shell_type,
            input: String::new(),
            history: Vec::new(),
            history_index: 0,
            send_cmd_tx,
            receive_cmd_rx,
            shell_output_rx,
            output_buffer: String::new(),
            is_loading: false,
            suggestions: Vec::new(),
            show_suggestions: false,
            selected_suggestion: 0,
            last_completion_input: String::new(),
            ai_enabled: true,
            #[cfg(not(target_arch = "wasm32"))]
            mcp_service,
            #[cfg(not(target_arch = "wasm32"))]
            completion_cancel_tx: None,
            #[cfg(not(target_arch = "wasm32"))]
            last_input_change: None,
            #[cfg(not(target_arch = "wasm32"))]
            diagnostic_rx,
            #[cfg(not(target_arch = "wasm32"))]
            diagnostic_tx,
            interactive_mode: false,
        }
    }

    /// Process incoming messages
    pub fn receive(&mut self, ctx: &Context) {
        // Receive shell output from the connection manager
        if let Some(rx) = &self.shell_output_rx {
            let mut received_count = 0;
            while let Ok(output) = rx.try_recv() {
                received_count += 1;
                log::info!("ShellView: Received output ({} bytes): {}", 
                    output.len(),
                    if output.len() > 200 { &output[..200] } else { &output }
                );
                
                // Check for DONE marker indicating command completion
                let is_done = output.contains("DONE");
                
                // Clean up the output (remove DONE marker and trim)
                let cleaned = output.replace("DONE", "").trim().to_string();
                
                if !cleaned.is_empty() {
                    // Append to the current output buffer
                    if !self.output_buffer.is_empty() {
                        self.output_buffer.push('\n');
                    }
                    self.output_buffer.push_str(&cleaned);
                    log::info!("ShellView: Output buffer now {} bytes", self.output_buffer.len());
                }
                
                if is_done {
                    log::info!("ShellView: DONE marker received, completing command");
                    // Command completed - move output to history
                    self.is_loading = false;
                    if let Some(last_entry) = self.history.last_mut() {
                        last_entry.output = std::mem::take(&mut self.output_buffer);
                        last_entry.success = !last_entry.output.to_lowercase().contains("error");
                        log::info!("ShellView: Command completed, output: {} bytes", last_entry.output.len());
                    }
                }
                
                ctx.request_repaint();
            }
            if received_count > 0 {
                log::info!("ShellView: Processed {} messages this frame", received_count);
            }
        } else {
            log::warn!("ShellView: No shell_output_rx channel available!");
        }
        
        // Receive diagnostic responses (AI completions)
        #[cfg(not(target_arch = "wasm32"))]
        while let Ok(response) = self.diagnostic_rx.try_recv() {
            if let crate::mcp::DiagnosticResponse::CommandCompletions { completions, .. } = response {
                self.suggestions = completions
                    .into_iter()
                    .map(|c| CommandSuggestion {
                        completion: c.completion,
                        description: c.description,
                        confidence: c.confidence,
                    })
                    .collect();
                self.show_suggestions = !self.suggestions.is_empty();
                self.selected_suggestion = 0;
                ctx.request_repaint();
            }
        }

        // Check for debounced completion requests
        #[cfg(not(target_arch = "wasm32"))]
        if self.ai_enabled {
            if let Some(last_change) = self.last_input_change {
                if last_change.elapsed().as_millis() > 300 && self.input != self.last_completion_input {
                    self.request_completions();
                }
            }
        }
    }

    /// Request AI completions for current input
    #[cfg(not(target_arch = "wasm32"))]
    fn request_completions(&mut self) {
        if self.input.trim().is_empty() {
            self.suggestions.clear();
            self.show_suggestions = false;
            return;
        }

        // Cancel any pending completion request
        if let Some(cancel_tx) = self.completion_cancel_tx.take() {
            let _ = cancel_tx.send(());
        }

        let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
        self.completion_cancel_tx = Some(cancel_tx);

        let input = self.input.clone();
        let shell_type = self.shell_type;
        let progress_tx = self.diagnostic_tx.clone();
        let session = self.mcp_service.openai_session.clone();

        self.last_completion_input = input.clone();

        PlatformSpawner::spawn(async move {
            let guard = session.lock().await;
            if let Some(session) = guard.as_ref() {
                let shell = match shell_type {
                    ShellType::PowerShell => crate::mcp::mcp::ShellType::PowerShell,
                    ShellType::Cmd => crate::mcp::mcp::ShellType::Cmd,
                    ShellType::Bash => crate::mcp::mcp::ShellType::Bash,
                };
                let _ = session
                    .stream_command_completions(&input, &shell, cancel_rx, progress_tx)
                    .await;
            }
        });
    }

    /// Execute the current command
    fn execute_command(&mut self) {
        let command = self.input.trim().to_string();
        if command.is_empty() {
            return;
        }

        // Add to history
        self.history.push(HistoryEntry {
            command: command.clone(),
            output: String::new(),
            timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
            success: true,
            shell_type: self.shell_type,
        });
        self.history_index = self.history.len();

        // NOTE: The Mastertech client currently uses a PersistentShell that:
        // - On Windows: uses PowerShell
        // - On Linux: uses sh (not bash directly)
        // All commands are sent as InteractiveInput to this persistent shell.
        // The shell type selector here is for future use when the client
        // supports starting different shell types.
        let cmd = Cmd::InteractiveInput(command.clone());
        
        log::info!("ShellView: Executing command: {}", command);
        log::info!("ShellView: Sending Cmd::InteractiveInput to client");

        match self.send_cmd_tx.send(cmd) {
            Ok(_) => log::info!("ShellView: Command sent successfully"),
            Err(e) => log::error!("ShellView: Failed to send command: {:?}", e),
        }

        // Clear input
        self.input.clear();
        self.suggestions.clear();
        self.show_suggestions = false;
        self.is_loading = true;
    }

    /// Navigate command history
    fn navigate_history(&mut self, up: bool) {
        if self.history.is_empty() {
            return;
        }

        if up {
            if self.history_index > 0 {
                self.history_index -= 1;
                self.input = self.history[self.history_index].command.clone();
            }
        } else {
            if self.history_index < self.history.len() - 1 {
                self.history_index += 1;
                self.input = self.history[self.history_index].command.clone();
            } else {
                self.history_index = self.history.len();
                self.input.clear();
            }
        }
    }

    /// Apply selected suggestion
    fn apply_suggestion(&mut self) {
        if let Some(suggestion) = self.suggestions.get(self.selected_suggestion) {
            // Check if we're completing a command name or parameter
            let trimmed = self.input.trim_end();
            if trimmed.contains(' ') {
                // Completing a parameter - append to existing input
                let parts: Vec<&str> = self.input.split_whitespace().collect();
                let last = parts.last().unwrap_or(&"");
                
                if last.starts_with('-') || suggestion.completion.starts_with('-') {
                    // Replace partial parameter
                    let prefix: String = parts[..parts.len().saturating_sub(1)].join(" ");
                    self.input = format!("{} {}", prefix, suggestion.completion);
                } else {
                    self.input = format!("{} {}", trimmed, suggestion.completion);
                }
            } else {
                // Completing command name
                self.input = suggestion.completion.clone();
            }
            
            self.suggestions.clear();
            self.show_suggestions = false;
        }
    }

    /// Render the shell view
    pub fn show(&mut self, ui: &mut Ui) {
        ui.vertical(|ui| {
            // Header with shell type selector
            self.render_header(ui);

            ui.add_space(8.0);

            // Command history and output
            self.render_history(ui);

            ui.add_space(8.0);

            // Input area with suggestions
            self.render_input(ui);
        });
    }

    fn render_header(&mut self, ui: &mut Ui) {
        Frame::NONE
            .fill(Color32::from_rgb(25, 28, 35))
            .inner_margin(Margin::symmetric(8, 12))
            .corner_radius(CornerRadius::same(6))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    // Shell type selector
                    ui.label(
                        RichText::new("Shell:")
                            .size(12.0)
                            .color(Color32::from_rgb(160, 165, 175)),
                    );

                    ComboBox::from_id_salt("shell_type")
                        .selected_text(self.shell_type.as_str())
                        .width(100.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.shell_type,
                                ShellType::PowerShell,
                                "PowerShell",
                            );
                            ui.selectable_value(&mut self.shell_type, ShellType::Cmd, "CMD");
                            ui.selectable_value(&mut self.shell_type, ShellType::Bash, "Bash");
                        });

                    ui.add_space(16.0);

                    // AI toggle
                    let ai_color = if self.ai_enabled {
                        Color32::from_rgb(100, 200, 255)
                    } else {
                        Color32::GRAY
                    };
                    let ai_btn = Button::new(RichText::new("🤖").size(14.0).color(ai_color))
                        .fill(Color32::TRANSPARENT);

                    if ui
                        .add(ai_btn)
                        .on_hover_text(if self.ai_enabled {
                            "AI Completion: ON"
                        } else {
                            "AI Completion: OFF"
                        })
                        .clicked()
                    {
                        self.ai_enabled = !self.ai_enabled;
                        if !self.ai_enabled {
                            self.suggestions.clear();
                            self.show_suggestions = false;
                        }
                    }

                    ui.add_space(8.0);

                    // Interactive mode toggle
                    let int_color = if self.interactive_mode {
                        Color32::YELLOW
                    } else {
                        Color32::GRAY
                    };
                    let int_btn = Button::new(RichText::new("🖥").size(14.0).color(int_color))
                        .fill(Color32::TRANSPARENT);

                    if ui
                        .add(int_btn)
                        .on_hover_text(if self.interactive_mode {
                            "Interactive Mode: ON"
                        } else {
                            "Interactive Mode: OFF"
                        })
                        .clicked()
                    {
                        self.interactive_mode = !self.interactive_mode;
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        // Client info
                        let name = self
                            .client
                            .friendly_name
                            .clone()
                            .unwrap_or_else(|| self.client.connection_string.clone());
                        ui.label(
                            RichText::new(&name)
                                .size(11.0)
                                .color(Color32::from_rgb(51, 255, 189)),
                        );

                        // Loading indicator
                        if self.is_loading {
                            ui.spinner();
                        }
                    });
                });
            });
    }

    fn render_history(&mut self, ui: &mut Ui) {
        let available_height = ui.available_height() - 80.0; // Reserve space for input

        Frame::NONE
            .fill(Color32::from_rgb(15, 17, 22))
            .inner_margin(Margin::same(8))
            .corner_radius(CornerRadius::same(6))
            .show(ui, |ui| {
                ScrollArea::vertical()
                    .max_height(available_height)
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for entry in &self.history {
                            // Command line
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(entry.shell_type.prompt_prefix())
                                        .size(12.0)
                                        .color(Color32::from_rgb(51, 255, 189))
                                        .monospace(),
                                );
                                ui.label(
                                    RichText::new(&entry.command)
                                        .size(12.0)
                                        .color(Color32::from_rgb(200, 205, 215))
                                        .monospace(),
                                );
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    ui.label(
                                        RichText::new(&entry.timestamp)
                                            .size(9.0)
                                            .color(Color32::from_rgb(100, 105, 115)),
                                    );
                                });
                            });

                            // Output
                            if !entry.output.is_empty() {
                                let output_color = if entry.success {
                                    Color32::from_rgb(180, 185, 195)
                                } else {
                                    Color32::from_rgb(255, 120, 120)
                                };
                                ui.label(
                                    RichText::new(&entry.output)
                                        .size(11.0)
                                        .color(output_color)
                                        .monospace(),
                                );
                            }

                            ui.add_space(4.0);
                        }

                        // Current output buffer
                        if !self.output_buffer.is_empty() {
                            ui.label(
                                RichText::new(&self.output_buffer)
                                    .size(11.0)
                                    .color(Color32::from_rgb(180, 185, 195))
                                    .monospace(),
                            );
                        }
                    });
            });
    }

    fn render_input(&mut self, ui: &mut Ui) {
        Frame::NONE
            .fill(Color32::from_rgb(25, 28, 35))
            .inner_margin(Margin::same(8))
            .corner_radius(CornerRadius::same(6))
            .show(ui, |ui| {
                // Suggestions popup (rendered above input)
                if self.show_suggestions && !self.suggestions.is_empty() {
                    Frame::NONE
                        .fill(Color32::from_rgb(35, 38, 48))
                        .corner_radius(CornerRadius::same(4))
                        .inner_margin(Margin::same(4))
                        .show(ui, |ui| {
                            for (i, suggestion) in self.suggestions.iter().enumerate() {
                                let is_selected = i == self.selected_suggestion;
                                let bg = if is_selected {
                                    Color32::from_rgb(60, 100, 150)
                                } else {
                                    Color32::TRANSPARENT
                                };

                                Frame::NONE.fill(bg).show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(&suggestion.completion)
                                                .size(12.0)
                                                .color(Color32::WHITE)
                                                .monospace(),
                                        );

                                        if let Some(desc) = &suggestion.description {
                                            ui.label(
                                                RichText::new(desc)
                                                    .size(10.0)
                                                    .color(Color32::from_rgb(150, 155, 165)),
                                            );
                                        }

                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                            let conf_color = if suggestion.confidence > 0.7 {
                                                Color32::from_rgb(50, 205, 50)
                                            } else if suggestion.confidence > 0.4 {
                                                Color32::YELLOW
                                            } else {
                                                Color32::GRAY
                                            };
                                            ui.label(
                                                RichText::new(format!(
                                                    "{:.0}%",
                                                    suggestion.confidence * 100.0
                                                ))
                                                .size(9.0)
                                                .color(conf_color),
                                            );
                                        });
                                    });
                                });
                            }
                        });
                    ui.add_space(4.0);
                }

                // Input line
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(self.shell_type.prompt_prefix())
                            .size(14.0)
                            .color(Color32::from_rgb(51, 255, 189))
                            .monospace(),
                    );

                    let text_edit = TextEdit::singleline(&mut self.input)
                        .font(eframe::egui::FontId::monospace(13.0))
                        .desired_width(ui.available_width() - 60.0)
                        .frame(false);

                    let response = ui.add(text_edit);

                    // Handle keyboard events
                    if response.has_focus() {
                        let ctx = ui.ctx();

                        // Enter to execute
                        if ctx.input(|i| i.key_pressed(Key::Enter)) {
                            self.execute_command();
                        }

                        // Up/Down for history
                        if ctx.input(|i| i.key_pressed(Key::ArrowUp)) {
                            if self.show_suggestions && !self.suggestions.is_empty() {
                                self.selected_suggestion =
                                    self.selected_suggestion.saturating_sub(1);
                            } else {
                                self.navigate_history(true);
                            }
                        }

                        if ctx.input(|i| i.key_pressed(Key::ArrowDown)) {
                            if self.show_suggestions && !self.suggestions.is_empty() {
                                self.selected_suggestion = (self.selected_suggestion + 1)
                                    .min(self.suggestions.len().saturating_sub(1));
                            } else {
                                self.navigate_history(false);
                            }
                        }

                        // Tab to apply suggestion
                        if ctx.input(|i| i.key_pressed(Key::Tab)) {
                            if self.show_suggestions && !self.suggestions.is_empty() {
                                self.apply_suggestion();
                            }
                        }

                        // Escape to hide suggestions
                        if ctx.input(|i| i.key_pressed(Key::Escape)) {
                            self.show_suggestions = false;
                        }
                    }

                    // Track input changes for completion debouncing
                    if response.changed() {
                        #[cfg(not(target_arch = "wasm32"))]
                        {
                            self.last_input_change = Some(Instant::now());
                        }
                    }

                    // Execute button
                    let exec_btn = Button::new(
                        RichText::new("▶")
                            .size(14.0)
                            .color(Color32::from_rgb(50, 205, 50)),
                    )
                    .min_size(Vec2::new(32.0, 24.0));

                    if ui.add(exec_btn).on_hover_text("Execute (Enter)").clicked() {
                        self.execute_command();
                    }
                });
            });
    }
}

