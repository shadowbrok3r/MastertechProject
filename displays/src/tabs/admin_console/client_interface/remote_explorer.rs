//! Remote Explorer for browsing the filesystem on a remote Mastertech instance
//! 
//! This uses websocket commands to request directory listings and file operations
//! from the connected Mastertech client.

use eframe::egui::{
    Align, Button, CentralPanel, Color32, Direction, Frame, Key, Layout, Margin,
    RichText, ScrollArea, Stroke, TextEdit, TopBottomPanel, Ui, Vec2, Widget,
};
use crossbeam::channel::Sender;
use crate::{Cmd, RemoteDirEntry};

/// Remote filesystem explorer state
pub struct RemoteExplorer {
    /// Current path being viewed
    pub current_path: String,
    /// Navigation history stack
    pub navigation_stack: Vec<String>,
    /// Current directory listing
    pub entries: Vec<RemoteDirEntry>,
    /// Whether we're waiting for a response
    pub loading: bool,
    /// Selected entry index
    pub selected_idx: Option<usize>,
    /// Path input for manual navigation
    path_input: String,
    /// Error message if any
    error: Option<String>,
}

impl Default for RemoteExplorer {
    fn default() -> Self {
        Self::new()
    }
}

impl RemoteExplorer {
    pub fn new() -> Self {
        Self {
            current_path: "current".to_string(),
            navigation_stack: Vec::new(),
            entries: Vec::new(),
            loading: false,
            selected_idx: None,
            path_input: String::new(),
            error: None,
        }
    }
    
    /// Set the directory listing from response
    pub fn set_entries(&mut self, entries: Vec<RemoteDirEntry>) {
        self.entries = entries;
        // Sort: directories first, then alphabetically
        self.entries.sort_by(|a, b| {
            match (a.is_directory, b.is_directory) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            }
        });
        self.loading = false;
        self.selected_idx = None;
    }
    
    /// Set error message
    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
        self.loading = false;
    }
    
    /// Navigate to a new path
    pub fn navigate_to(&mut self, path: String, cmd_tx: &Sender<Cmd>) {
        if path != self.current_path {
            self.navigation_stack.push(self.current_path.clone());
        }
        self.current_path = path.clone();
        self.path_input = path.clone();
        self.loading = true;
        self.entries.clear();
        let _ = cmd_tx.try_send(Cmd::ListDirectory(path));
    }
    
    /// Navigate up one directory level
    pub fn navigate_up(&mut self, cmd_tx: &Sender<Cmd>) {
        // Try to get parent from navigation stack first
        if let Some(previous) = self.navigation_stack.pop() {
            self.current_path = previous.clone();
            self.path_input = previous.clone();
            self.loading = true;
            self.entries.clear();
            let _ = cmd_tx.try_send(Cmd::ListDirectory(previous));
        } else if !self.current_path.is_empty() {
            // Compute parent directory
            let path = std::path::Path::new(&self.current_path);
            if let Some(parent) = path.parent() {
                let parent_str = parent.to_string_lossy().to_string();
                self.current_path = parent_str.clone();
                self.path_input = parent_str.clone();
                self.loading = true;
                self.entries.clear();
                let _ = cmd_tx.try_send(Cmd::ListDirectory(parent_str));
            }
        }
    }
    
    /// Refresh current directory
    pub fn refresh(&mut self, cmd_tx: &Sender<Cmd>) {
        self.loading = true;
        self.entries.clear();
        let _ = cmd_tx.try_send(Cmd::ListDirectory(self.current_path.clone()));
    }
    
    /// Display the explorer UI
    pub fn display(&mut self, ui: &mut Ui, cmd_tx: &Sender<Cmd>) {
        let size = ui.available_size_before_wrap();
        let inner_margin = Margin::same(4);
        let stroke = Stroke::new(0.7, Color32::from_additive_luminance(100));
        let radius = eframe::egui::CornerRadius::same(5);
        
        // Top panel with navigation controls
        TopBottomPanel::top("RemoteExplorerTop")
            .frame(Frame::default().outer_margin(Margin::symmetric(5, 2)))
            .exact_height(45.)
            .show_inside(ui, |ui| {
                ui.vertical_centered(|ui| {
                    // Path input
                    let pre_modified_path = self.path_input.clone();
                    let response = TextEdit::singleline(&mut self.path_input)
                        .desired_width(size.x / 1.2)
                        .hint_text("Enter path...")
                        .ui(ui);
                    
                    if response.lost_focus() || ui.input(|i| i.key_pressed(Key::Enter)) {
                        if self.path_input != pre_modified_path {
                            self.navigate_to(self.path_input.clone(), cmd_tx);
                        }
                    }
                    
                    // Navigation buttons
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_space(5.);
                        
                        // Refresh button
                        if ui.button(RichText::new("⟲").heading())
                            .on_hover_text("Refresh")
                            .clicked() 
                        {
                            self.refresh(cmd_tx);
                        }
                        
                        ui.add_space(5.);
                        
                        // Home button
                        if ui.button(RichText::new("🏠").heading())
                            .on_hover_text("Home")
                            .clicked() 
                        {
                            self.navigate_to("current".to_string(), cmd_tx);
                        }
                        
                        ui.add_space(5.);
                        
                        // Up button
                        if ui.button(RichText::new("⬆").heading())
                            .on_hover_text("Parent Folder")
                            .clicked() 
                        {
                            self.navigate_up(cmd_tx);
                        }
                    });
                });
            });
        
        // Show error if any
        if let Some(error) = &self.error {
            TopBottomPanel::bottom("RemoteExplorerError")
                .exact_height(25.)
                .show_inside(ui, |ui| {
                    ui.colored_label(Color32::RED, error.clone());
                });
        }
        
        // Main content area
        let panel_frame = Frame::default()
            .fill(Color32::from_rgb(12, 12, 14))
            .inner_margin(inner_margin)
            .corner_radius(radius)
            .stroke(stroke);
        
        CentralPanel::default()
            .frame(panel_frame)
            .show_inside(ui, |ui| {
                if self.loading {
                    ui.vertical_centered(|ui| {
                        ui.add_space(50.);
                        ui.spinner();
                        ui.label("Loading directory contents...");
                    });
                } else if self.entries.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(50.);
                        ui.label(RichText::new("📂 Empty directory").heading());
                        ui.add_space(10.);
                        if ui.button("Refresh").clicked() {
                            self.refresh(cmd_tx);
                        }
                    });
                } else {
                    ScrollArea::vertical()
                        .max_height(size.y - 80.)
                        .auto_shrink(false)
                        .show(ui, |ui| {
                            ui.with_layout(
                                Layout::from_main_dir_and_cross_align(Direction::TopDown, Align::Min),
                                |ui| {
                                    for (idx, entry) in self.entries.iter().enumerate() {
                                        let is_selected = self.selected_idx == Some(idx);
                                        let icon = if entry.is_directory { "📁" } else { "📄" };
                                        let size_text = if let Some(size) = entry.size {
                                            format_file_size(size)
                                        } else {
                                            String::new()
                                        };
                                        
                                        let label_text = format!("{}  {}", icon, entry.name);
                                        
                                        let response = ui.selectable_label(is_selected, label_text);
                                        
                                        // Show size on hover for files
                                        if !size_text.is_empty() {
                                            response.clone().on_hover_text(format!("Size: {}", size_text));
                                        }
                                        
                                        if response.clicked() {
                                            self.selected_idx = Some(idx);
                                        }
                                        
                                        // Double-click to navigate into directories
                                        if response.double_clicked() {
                                            if entry.is_directory {
                                                self.navigate_to(entry.path.clone(), cmd_tx);
                                            }
                                        }
                                        
                                        // Context menu
                                        response.context_menu(|ui| {
                                            ui.set_min_width(150.0);
                                            
                                            if entry.is_directory {
                                                if ui.button("📂 Open").clicked() {
                                                    self.navigate_to(entry.path.clone(), cmd_tx);
                                                    ui.close();
                                                }
                                            } else {
                                                if ui.button("📥 Download").clicked() {
                                                    let _ = cmd_tx.try_send(Cmd::DownloadRemoteFile(entry.path.clone()));
                                                    ui.close();
                                                }
                                            }
                                            
                                            ui.separator();
                                            
                                            if ui.button("🗑 Delete").clicked() {
                                                let _ = cmd_tx.try_send(Cmd::FileSystemAction(
                                                    crate::FileSystemAction::Delete(entry.path.clone())
                                                ));
                                                self.refresh(cmd_tx);
                                                ui.close();
                                            }
                                        });
                                    }
                                }
                            );
                        });
                }
            });
    }
}

/// Format file size in human-readable format
fn format_file_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    
    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}
