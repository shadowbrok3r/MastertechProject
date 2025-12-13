//! Remote file explorer view.
//!
//! Uses the existing FileSystem virtual filesystem component
//! to browse files on a connected Mastertech client.

use crate::{
    virtual_filesystem::{FileSysHelper, FileSystem},
    Cmd, FileSystemAction,
};
use crossbeam::channel::Sender;
use database::schema::ConnectedClient;
use eframe::egui::{
    Align, Button, Color32, CornerRadius, Frame, Layout, Margin, RichText, TextEdit, Ui, Vec2,
};

/// File explorer view wrapping the virtual filesystem
pub struct FileExplorerView {
    /// The client this explorer is connected to
    pub client: ConnectedClient,
    /// The filesystem component
    pub filesystem: FileSystem,
    /// Channel to send commands to the client
    send_cmd_tx: Sender<Cmd>,
    /// Current path input (for manual navigation)
    path_input: String,
    /// Last synced path (to detect when filesystem path changes)
    last_synced_path: String,
    /// Is the path input currently being edited?
    is_editing_path: bool,
    /// Is the explorer loading?
    pub loading: bool,
    /// Error message
    pub error: Option<String>,
}

impl FileExplorerView {
    pub fn new(client: ConnectedClient, send_cmd_tx: Sender<Cmd>) -> Self {
        let mut filesystem = FileSystem::new();

        // Set up the filesystem helper to send commands through our channel
        let helper = WebSocketFileSysHelper::new(send_cmd_tx.clone());
        filesystem.helper_delegate = Some(Box::new(helper));

        Self {
            client,
            filesystem,
            send_cmd_tx,
            path_input: String::new(),
            last_synced_path: String::new(),
            is_editing_path: false,
            loading: false,
            error: None,
        }
    }

    /// Initialize the explorer by requesting the current directory
    pub fn initialize(&mut self) {
        log::info!("FileExplorer: Initializing - requesting 'current' directory");
        self.loading = true;
        let cmd = Cmd::FileSystemAction(FileSystemAction::EnterDirectory("current".to_string()));
        match self.send_cmd_tx.send(cmd) {
            Ok(_) => log::info!("FileExplorer: Initialize command sent"),
            Err(e) => log::error!("FileExplorer: Failed to send initialize command: {:?}", e),
        }
    }

    /// Navigate to a specific path
    pub fn navigate_to(&mut self, path: &str) {
        log::info!("FileExplorer: Navigating to path: {}", path);
        self.loading = true;
        let cmd = Cmd::FileSystemAction(FileSystemAction::EnterDirectory(path.to_string()));
        match self.send_cmd_tx.send(cmd) {
            Ok(_) => log::info!("FileExplorer: Navigate command sent for path: {}", path),
            Err(e) => log::error!("FileExplorer: Failed to send navigate command: {:?}", e),
        }
    }

    /// Go up one directory level
    pub fn go_up(&mut self) {
        let current = self.filesystem.current_prefix.clone();
        if let Some(parent) = current.rsplit_once('\\').map(|(p, _)| p) {
            self.navigate_to(parent);
        } else if let Some(parent) = current.rsplit_once('/').map(|(p, _)| p) {
            self.navigate_to(parent);
        }
    }

    /// Refresh the current directory
    pub fn refresh(&mut self) {
        let current = self.filesystem.current_prefix.clone();
        if current.is_empty() {
            self.navigate_to("current");
        } else {
            self.navigate_to(&current);
        }
    }

    /// Process incoming updates
    pub fn receive(&mut self) {
        // Check if the filesystem received new data
        let had_action = self.filesystem.current_action.is_some();
        let prev_action = self.filesystem.current_action.clone();
        
        self.filesystem.receive();
        
        // Log if action changed (compare by debug string since FileSystemAction doesn't impl PartialEq)
        let action_changed = format!("{:?}", prev_action) != format!("{:?}", self.filesystem.current_action);
        if action_changed {
            log::info!("FileExplorer: Action changed from {:?} to {:?}", 
                prev_action, self.filesystem.current_action);
        }
        
        // If we had a pending action and the filesystem processed it, clear loading
        if had_action && self.filesystem.current_action.is_some() {
            // Action was processed, loading is done
            log::info!("FileExplorer: Action processed, clearing loading state");
            self.loading = false;
        }
        
        // Also check if the root has children (data was received)
        if self.loading {
            if let database::schema::Node::Folder(_, children) = &self.filesystem.root {
                if !children.is_empty() {
                    log::info!("FileExplorer: Root has {} children, clearing loading state", children.len());
                    self.loading = false;
                }
            }
        }
    }

    /// Render the file explorer
    pub fn show(&mut self, ui: &mut Ui) {
        ui.vertical(|ui| {
            // Header with navigation controls
            self.render_header(ui);

            ui.add_space(8.0);

            // File listing
            self.render_files(ui);
        });
    }

    fn render_header(&mut self, ui: &mut Ui) {
        Frame::NONE
            .fill(Color32::from_rgb(25, 28, 35))
            .inner_margin(Margin::symmetric(8, 12))
            .corner_radius(CornerRadius::same(6))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    // Back button
                    let back_btn = Button::new(
                        RichText::new("⬅")
                            .size(14.0)
                            .color(Color32::from_rgb(200, 205, 215)),
                    )
                    .min_size(Vec2::new(32.0, 28.0));

                    if ui.add(back_btn).on_hover_text("Go Up").clicked() {
                        self.go_up();
                    }

                    ui.add_space(4.0);

                    // Refresh button
                    let refresh_btn = Button::new(
                        RichText::new("🔄")
                            .size(14.0)
                            .color(Color32::from_rgb(100, 180, 255)),
                    )
                    .min_size(Vec2::new(32.0, 28.0));

                    if ui.add(refresh_btn).on_hover_text("Refresh").clicked() {
                        self.refresh();
                    }

                    ui.add_space(8.0);

                    // Path display/input
                    let current_path = if self.filesystem.current_prefix.is_empty() {
                        "/".to_string()
                    } else {
                        self.filesystem.current_prefix.clone()
                    };

                    // Only sync path_input from filesystem when:
                    // 1. Not currently editing
                    // 2. The filesystem path has changed
                    if !self.is_editing_path && current_path != self.last_synced_path {
                        self.path_input = current_path.clone();
                        self.last_synced_path = current_path;
                    }

                    let path_edit = TextEdit::singleline(&mut self.path_input)
                        .desired_width(ui.available_width() - 100.0)
                        .font(eframe::egui::FontId::monospace(12.0));

                    let response = ui.add(path_edit);

                    // Track editing state
                    if response.has_focus() {
                        self.is_editing_path = true;
                    }

                    if response.lost_focus() {
                        self.is_editing_path = false;
                        if ui.input(|i| i.key_pressed(eframe::egui::Key::Enter)) {
                            log::info!("FileExplorer: Navigating to path: {}", self.path_input);
                            self.navigate_to(&self.path_input.clone());
                        }
                    }

                    // Go button
                    let go_btn = Button::new(
                        RichText::new("→")
                            .size(14.0)
                            .color(Color32::from_rgb(50, 205, 50)),
                    )
                    .min_size(Vec2::new(32.0, 28.0));

                    if ui.add(go_btn).on_hover_text("Go").clicked() {
                        self.navigate_to(&self.path_input.clone());
                    }
                });

                // Second row: client info and loading indicator
                ui.horizontal(|ui| {
                    let name = self
                        .client
                        .friendly_name
                        .clone()
                        .unwrap_or_else(|| self.client.connection_string.clone());

                    ui.label(
                        RichText::new(format!("Client: {}", name))
                            .size(10.0)
                            .color(Color32::from_rgb(51, 255, 189)),
                    );

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if self.loading {
                            ui.spinner();
                        }

                        if let Some(error) = &self.error {
                            ui.colored_label(Color32::RED, error);
                        }
                    });
                });
            });
    }

    fn render_files(&mut self, ui: &mut Ui) {
        Frame::NONE
            .fill(Color32::from_rgb(18, 20, 25))
            .inner_margin(Margin::same(8))
            .corner_radius(CornerRadius::same(6))
            .show(ui, |ui| {
                // Use the filesystem's display method
                self.filesystem.display(ui);
            });
    }
}

/// Helper delegate to route filesystem commands through the WebSocket.
/// Implements `FileSysHelper` trait to integrate with the existing FileSystem.
#[derive(Clone)]
pub struct WebSocketFileSysHelper {
    send_cmd_tx: Sender<Cmd>,
}

impl WebSocketFileSysHelper {
    pub fn new(send_cmd_tx: Sender<Cmd>) -> Self {
        Self { send_cmd_tx }
    }
}

impl FileSysHelper for WebSocketFileSysHelper {
    fn handle_filesystem_action(&mut self, action: &FileSystemAction) {
        log::info!("WebSocketFileSysHelper: Forwarding action to client: {:?}", action);
        match self.send_cmd_tx.try_send(Cmd::FileSystemAction(action.clone())) {
            Ok(_) => log::info!("WebSocketFileSysHelper: Action forwarded successfully"),
            Err(e) => log::error!("WebSocketFileSysHelper: Failed to forward action: {:?}", e),
        }
    }
}

