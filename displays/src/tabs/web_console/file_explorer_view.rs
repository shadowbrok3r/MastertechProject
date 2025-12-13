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
            loading: false,
            error: None,
        }
    }

    /// Initialize the explorer by requesting the current directory
    pub fn initialize(&mut self) {
        self.loading = true;
        let _ = self
            .send_cmd_tx
            .send(Cmd::FileSystemAction(FileSystemAction::EnterDirectory(
                "current".to_string(),
            )));
    }

    /// Navigate to a specific path
    pub fn navigate_to(&mut self, path: &str) {
        self.loading = true;
        let _ = self
            .send_cmd_tx
            .send(Cmd::FileSystemAction(FileSystemAction::EnterDirectory(
                path.to_string(),
            )));
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
        self.filesystem.receive();
        // Update loading state based on filesystem
        // The filesystem will update its entries when it receives data
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
                        "/"
                    } else {
                        &self.filesystem.current_prefix
                    };

                    self.path_input = current_path.to_string();

                    let path_edit = TextEdit::singleline(&mut self.path_input)
                        .desired_width(ui.available_width() - 100.0)
                        .font(eframe::egui::FontId::monospace(12.0));

                    let response = ui.add(path_edit);

                    if response.lost_focus()
                        && ui.input(|i| i.key_pressed(eframe::egui::Key::Enter))
                    {
                        self.navigate_to(&self.path_input.clone());
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
        let _ = self
            .send_cmd_tx
            .try_send(Cmd::FileSystemAction(action.clone()));
    }
}

