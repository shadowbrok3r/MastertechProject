//! Remote Explorer for browsing the filesystem on a remote Mastertech instance
//! 
//! This uses websocket commands to request directory listings and file operations
//! from the connected Mastertech client.
//! 
//! Features:
//! - Left sidebar with drives and shortcuts
//! - Right sidebar with "My Tools" from SurrealDB bucket
//! - Preview pane for text files and image thumbnails
//! - Double-click to execute files
//! - Context menu with download, copy to tools, delete options

use eframe::egui::{
    Align, CentralPanel, Color32, Direction, Frame, Key, Layout, Margin,
    RichText, ScrollArea, Sense, SidePanel, Stroke, TextEdit, TopBottomPanel, Ui,
    Vec2, CornerRadius,
};
use crossbeam::channel::Sender;
use crate::{Cmd, RemoteDirEntry, PlatformSpawner, Spawner};
use database::schema::file_storage::{self, FileEntry};

/// Common folder shortcuts
#[derive(Clone)]
pub struct FolderShortcut {
    pub name: String,
    pub icon: &'static str,
    pub path: String,
}

/// Tool entry from My Tools (SurrealDB bucket)
#[derive(Clone, Debug)]
pub struct ToolEntry {
    pub name: String,
    pub path: String,
    pub size: u64,
    pub is_text: bool,
}

impl From<FileEntry> for ToolEntry {
    fn from(entry: FileEntry) -> Self {
        let name = entry.filename();
        let is_text = is_text_file(&name);
        Self {
            name,
            path: entry.path(),
            size: entry.size.unwrap_or(0),
            is_text,
        }
    }
}

/// Preview state for displaying file contents
#[derive(Default)]
pub struct PreviewState {
    /// Path of the file being previewed
    pub path: String,
    /// Text content (for text files)
    pub text_content: Option<String>,
    /// Whether the content has been modified
    pub modified: bool,
    /// Image thumbnail data (PNG bytes)
    pub image_data: Option<Vec<u8>>,
    /// Texture handle for rendered image
    #[cfg(not(target_arch = "wasm32"))]
    pub texture: Option<eframe::egui::TextureHandle>,
    /// Whether we're loading content
    pub loading: bool,
    /// Error message if loading failed
    pub error: Option<String>,
}

impl PreviewState {
    /// Clear all preview state
    pub fn clear(&mut self) {
        self.path.clear();
        self.text_content = None;
        self.modified = false;
        self.image_data = None;
        #[cfg(not(target_arch = "wasm32"))]
        { self.texture = None; }
        self.loading = false;
        self.error = None;
    }
}

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
    /// Available drives on the remote system
    pub drives: Vec<String>,
    /// Common folder shortcuts
    pub shortcuts: Vec<FolderShortcut>,
    /// Whether left sidebar is visible
    pub sidebar_visible: bool,
    /// Filename of pending download (set when download is requested, cleared when complete)
    pub pending_download: Option<String>,
    /// Whether right sidebar (My Tools) is visible
    pub tools_sidebar_visible: bool,
    /// My Tools entries from SurrealDB bucket
    pub my_tools: Vec<ToolEntry>,
    /// Whether we're loading My Tools
    pub tools_loading: bool,
    /// Selected tool index
    pub selected_tool_idx: Option<usize>,
    /// Preview state for files
    pub preview: PreviewState,
    /// Whether preview pane is visible
    pub preview_visible: bool,
    /// Username for SurrealDB bucket
    pub bucket_name: String,
    /// Pending upload to My Tools (filename, data)
    pub pending_tool_upload: Option<(String, Vec<u8>)>,
    /// Channel receiver for My Tools updates
    #[cfg(not(target_arch = "wasm32"))]
    pub tools_rx: Option<crossbeam::channel::Receiver<Vec<ToolEntry>>>,
    /// Whether tools have been initially loaded
    pub tools_initialized: bool,
}

impl Default for RemoteExplorer {
    fn default() -> Self {
        Self::new()
    }
}

impl RemoteExplorer {
    pub fn new() -> Self {
        // Default shortcuts - the Mastertech client will resolve these using Windows API
        let shortcuts = vec![
            FolderShortcut { name: "Desktop".to_string(), icon: "🖥", path: "Desktop".to_string() },
            FolderShortcut { name: "Documents".to_string(), icon: "📄", path: "Documents".to_string() },
            FolderShortcut { name: "Downloads".to_string(), icon: "📥", path: "Downloads".to_string() },
            FolderShortcut { name: "Pictures".to_string(), icon: "🖼", path: "Pictures".to_string() },
            FolderShortcut { name: "Music".to_string(), icon: "🎵", path: "Music".to_string() },
            FolderShortcut { name: "Videos".to_string(), icon: "🎬", path: "Videos".to_string() },
            FolderShortcut { name: "AppData".to_string(), icon: "⚙", path: "AppData".to_string() },
            FolderShortcut { name: "LocalAppData".to_string(), icon: "💾", path: "LocalAppData".to_string() },
        ];
        
        Self {
            current_path: "current".to_string(),
            navigation_stack: Vec::new(),
            entries: Vec::new(),
            loading: false,
            selected_idx: None,
            path_input: "current".to_string(),
            error: None,
            drives: Vec::new(),
            shortcuts,
            sidebar_visible: true,
            pending_download: None,
            tools_sidebar_visible: true,
            my_tools: Vec::new(),
            tools_loading: false,
            selected_tool_idx: None,
            preview: PreviewState::default(),
            preview_visible: true,
            bucket_name: String::new(),
            pending_tool_upload: None,
            #[cfg(not(target_arch = "wasm32"))]
            tools_rx: None,
            tools_initialized: false,
        }
    }
    
    /// Set the bucket name (username) for My Tools
    pub fn set_bucket_name(&mut self, username: &str) {
        self.bucket_name = username.to_lowercase().replace(['.', ' ', '-'], "_");
    }
    
    /// Set available drives
    pub fn set_drives(&mut self, drives: Vec<String>) {
        self.drives = drives;
    }
    
    /// Load My Tools from SurrealDB bucket
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_my_tools(&mut self) {
        if self.bucket_name.is_empty() {
            log::warn!("Cannot load My Tools: bucket name not set");
            return;
        }
        
        self.tools_loading = true;
        self.tools_initialized = true;
        let bucket = self.bucket_name.clone();
        
        // Use a channel to receive the results
        let (tx, rx) = crossbeam::channel::bounded::<Vec<ToolEntry>>(1);
        
        // Store the receiver so we can poll it later
        self.tools_rx = Some(rx);
        
        log::info!("Loading My Tools from bucket: {}", bucket);
        
        PlatformSpawner::spawn(async move {
            // Ensure the bucket exists
            if let Err(e) = file_storage::init_user_bucket(&bucket).await {
                log::error!("Failed to init bucket: {}", e);
            }
            
            match file_storage::list_files(&bucket, "").await {
                Ok(entries) => {
                    let tools: Vec<ToolEntry> = entries.into_iter().map(ToolEntry::from).collect();
                    log::info!("Loaded {} tools from bucket", tools.len());
                    let _ = tx.send(tools);
                }
                Err(e) => {
                    log::error!("Failed to load My Tools: {}", e);
                    let _ = tx.send(Vec::new());
                }
            }
        });
    }
    
    /// Poll for My Tools updates - call this in the UI loop
    #[cfg(not(target_arch = "wasm32"))]
    pub fn poll_tools_updates(&mut self) {
        if let Some(rx) = &self.tools_rx {
            if let Ok(tools) = rx.try_recv() {
                log::info!("Received {} tools from channel", tools.len());
                self.my_tools = tools;
                self.tools_loading = false;
                self.tools_rx = None; // Clear the channel after receiving
            }
        }
    }
    
    #[cfg(target_arch = "wasm32")]
    pub fn poll_tools_updates(&mut self) {}
    
    #[cfg(target_arch = "wasm32")]
    pub fn load_my_tools(&mut self) {}
    
    /// Set My Tools from external source
    pub fn set_my_tools(&mut self, tools: Vec<ToolEntry>) {
        self.my_tools = tools;
        self.tools_loading = false;
    }
    
    /// Handle received file data and save to disk
    /// Accumulates chunks until is_last_chunk is true, then saves the complete file
    #[cfg(not(target_arch = "wasm32"))]
    pub fn handle_file_download(&mut self, data: Vec<u8>, is_last_chunk: bool, download_buffer: &mut Vec<u8>) -> Result<Option<String>, String> {
        // Get or verify we have a pending download
        let filename = match &self.pending_download {
            Some(f) => f.clone(),
            None => return Err("No pending download".to_string()),
        };
        
        // Accumulate data
        download_buffer.extend_from_slice(&data);
        log::info!("Accumulated {} bytes for download, is_last: {}", download_buffer.len(), is_last_chunk);
        
        if !is_last_chunk {
            // More chunks coming, just return success without message
            return Ok(None);
        }
        
        // This is the last chunk - save the file
        self.pending_download = None;
        let file_data = std::mem::take(download_buffer);
        
        // Use native file dialog to let user choose save location
        if let Some(save_path) = rfd::FileDialog::new()
            .set_file_name(&filename)
            .save_file()
        {
            match std::fs::write(&save_path, &file_data) {
                Ok(_) => {
                    let msg = format!("File saved to: {} ({} bytes)", save_path.display(), file_data.len());
                    log::info!("{}", msg);
                    Ok(Some(msg))
                }
                Err(e) => {
                    let msg = format!("Failed to save file: {}", e);
                    log::error!("{}", msg);
                    Err(msg)
                }
            }
        } else {
            // User cancelled the save dialog
            Err("Download cancelled".to_string())
        }
    }
    
    #[cfg(target_arch = "wasm32")]
    pub fn handle_file_download(&mut self, _data: Vec<u8>, _is_last_chunk: bool, _download_buffer: &mut Vec<u8>) -> Result<Option<String>, String> {
        Err("File download not supported in web browser".to_string())
    }
    
    /// Handle file preview content response
    pub fn handle_preview_content(&mut self, path: String, content: String) {
        self.preview.path = path;
        self.preview.text_content = Some(content);
        self.preview.image_data = None;
        self.preview.modified = false;
        self.preview.loading = false;
        self.preview.error = None;
        self.preview_visible = true;
    }
    
    /// Handle thumbnail response
    #[cfg(not(target_arch = "wasm32"))]
    pub fn handle_thumbnail(&mut self, path: String, png_data: Vec<u8>, ctx: &eframe::egui::Context) {
        self.preview.path = path;
        self.preview.text_content = None;
        self.preview.image_data = Some(png_data.clone());
        self.preview.loading = false;
        self.preview.error = None;
        self.preview_visible = true;
        
        // Create texture from PNG data using the image crate
        if let Ok(img) = ::image::load_from_memory(&png_data) {
            let rgba = img.to_rgba8();
            let size = [img.width() as usize, img.height() as usize];
            let pixels = rgba.into_raw();
            let color_image = eframe::egui::ColorImage::from_rgba_unmultiplied(size, &pixels);
            self.preview.texture = Some(ctx.load_texture(
                "preview_thumbnail",
                color_image,
                eframe::egui::TextureOptions::default(),
            ));
        }
    }
    
    #[cfg(target_arch = "wasm32")]
    pub fn handle_thumbnail(&mut self, _path: String, _png_data: Vec<u8>, _ctx: &eframe::egui::Context) {
        self.preview.error = Some("Thumbnails not supported in web browser".to_string());
    }
    
    /// Set the directory listing from response
    pub fn set_entries(&mut self, entries: Vec<RemoteDirEntry>, current_path: Option<String>) {
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
        
        // Update current path if provided (from server response)
        if let Some(path) = current_path {
            self.current_path = path.clone();
            self.path_input = path;
        } else {
            // Sync path_input with current_path
            self.path_input = self.current_path.clone();
        }
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
    
    /// Copy a file to My Tools (upload to SurrealDB bucket)
    #[cfg(not(target_arch = "wasm32"))]
    pub fn copy_to_my_tools(&mut self, path: &str, data: Vec<u8>) {
        if self.bucket_name.is_empty() {
            log::error!("Cannot copy to My Tools: bucket name not set");
            return;
        }
        
        let filename = std::path::Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unnamed".to_string());
        
        let bucket = self.bucket_name.clone();
        let dest_path = format!("/{}", filename);
        let data_len = data.len();
        
        log::info!("copy_to_my_tools: Copying {} ({} bytes) to bucket: {}", filename, data_len, bucket);
        
        // Create channel to signal completion
        let (tx, rx) = crossbeam::channel::bounded::<Vec<ToolEntry>>(1);
        self.tools_rx = Some(rx);
        self.tools_loading = true;
        
        let filename_for_log = filename.clone();
        log::info!("copy_to_my_tools: Spawning async task for {}", filename_for_log);
        PlatformSpawner::spawn(async move {
            log::info!("copy_to_my_tools: Inside async task for {}", filename);
            
            // Ensure the bucket exists first
            if let Err(e) = file_storage::init_user_bucket(&bucket).await {
                log::error!("copy_to_my_tools: Failed to init bucket: {}", e);
            }
            
            log::info!("copy_to_my_tools: Calling put_file for {}", filename);
            match file_storage::put_file(&bucket, &dest_path, data).await {
                Ok(_) => {
                    log::info!("copy_to_my_tools: Successfully uploaded {} to My Tools", filename);
                    
                    // Refresh the tools list after upload
                    log::info!("copy_to_my_tools: Refreshing tools list...");
                    match file_storage::list_files(&bucket, "").await {
                        Ok(entries) => {
                            let tools: Vec<ToolEntry> = entries.into_iter().map(ToolEntry::from).collect();
                            log::info!("copy_to_my_tools: Refreshed tools list: {} items", tools.len());
                            let _ = tx.send(tools);
                        }
                        Err(e) => {
                            log::error!("copy_to_my_tools: Failed to refresh My Tools after upload: {}", e);
                            let _ = tx.send(Vec::new());
                        }
                    }
                }
                Err(e) => {
                    log::error!("copy_to_my_tools: Failed to upload to My Tools: {}", e);
                    let _ = tx.send(Vec::new());
                }
            }
        });
        log::info!("copy_to_my_tools: Async task spawned for {}", filename_for_log);
    }
    
    /// Copy a tool from My Tools to the remote client
    pub fn copy_tool_to_client(&mut self, tool: &ToolEntry, destination: &str, cmd_tx: &Sender<Cmd>) {
        if self.bucket_name.is_empty() {
            log::error!("Cannot copy from My Tools: bucket name not set");
            return;
        }
        
        let bucket = self.bucket_name.clone();
        let tool_path = tool.path.clone();
        let dest = if destination.is_empty() {
            self.current_path.clone()
        } else {
            destination.to_string()
        };
        let filename = tool.name.clone();
        let cmd_tx = cmd_tx.clone();
        
        log::info!("Copying {} from My Tools to {}", filename, dest);
        
        PlatformSpawner::spawn(async move {
            match file_storage::get_file(&bucket, &tool_path).await {
                Ok(Some(data)) => {
                    let full_dest = if dest.ends_with('/') || dest.ends_with('\\') {
                        format!("{}{}", dest, filename)
                    } else {
                        format!("{}\\{}", dest, filename)
                    };
                    let _ = cmd_tx.try_send(Cmd::UploadToClient(full_dest, data));
                    log::info!("Sent UploadToClient command for {}", filename);
                }
                Ok(None) => {
                    log::error!("Tool file not found: {}", tool_path);
                }
                Err(e) => {
                    log::error!("Failed to get tool file: {}", e);
                }
            }
        });
    }
    
    /// Display the explorer UI
    pub fn display(&mut self, ui: &mut Ui, cmd_tx: &Sender<Cmd>) {
        let inner_margin = Margin::same(4);
        let stroke = Stroke::new(0.7, Color32::from_additive_luminance(100));
        let radius = CornerRadius::same(5);
        
        // Poll for My Tools updates
        self.poll_tools_updates();
        
        // Load My Tools on first display if bucket is set and not yet initialized
        if !self.tools_initialized && !self.bucket_name.is_empty() {
            self.load_my_tools();
        }
        
        // Top panel with navigation controls
        self.display_top_panel(ui, cmd_tx);
        
        // Show error if any
        if let Some(error) = &self.error {
            TopBottomPanel::bottom("RemoteExplorerError")
                .exact_height(25.)
                .show_inside(ui, |ui| {
                    ui.colored_label(Color32::RED, error.clone());
                });
        }
        
        // Left sidebar with drives and shortcuts
        if self.sidebar_visible {
            self.display_left_sidebar(ui, cmd_tx, stroke, radius);
        }
        
        // Right sidebar with My Tools (includes preview pane)
        if self.tools_sidebar_visible {
            self.display_tools_sidebar(ui, cmd_tx, stroke, radius);
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
                self.display_file_list(ui, cmd_tx);
            });
    }
    
    fn display_top_panel(&mut self, ui: &mut Ui, cmd_tx: &Sender<Cmd>) {
        TopBottomPanel::top("RemoteExplorerTop")
            .frame(Frame::default().outer_margin(Margin::symmetric(5, 2)))
            .exact_height(35.)
            .show_inside(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    // Navigation buttons
                    if ui.button(RichText::new("⬆").heading())
                        .on_hover_text("Parent Folder")
                        .clicked() 
                    {
                        self.navigate_up(cmd_tx);
                    }
                    
                    if ui.button(RichText::new("🏠").heading())
                        .on_hover_text("Home")
                        .clicked() 
                    {
                        self.navigate_to("current".to_string(), cmd_tx);
                    }
                    
                    if ui.button(RichText::new("⟲").heading())
                        .on_hover_text("Refresh")
                        .clicked() 
                    {
                        self.refresh(cmd_tx);
                    }
                    
                    // Toggle buttons for sidebars
                    ui.separator();
                    
                    if ui.selectable_label(self.sidebar_visible, "📁")
                        .on_hover_text("Toggle Navigation Sidebar")
                        .clicked()
                    {
                        self.sidebar_visible = !self.sidebar_visible;
                    }
                    
                    if ui.selectable_label(self.tools_sidebar_visible, "🧰")
                        .on_hover_text("Toggle My Tools")
                        .clicked()
                    {
                        self.tools_sidebar_visible = !self.tools_sidebar_visible;
                    }
                    
                    if ui.selectable_label(self.preview_visible, "👁")
                        .on_hover_text("Toggle Preview Pane")
                        .clicked()
                    {
                        self.preview_visible = !self.preview_visible;
                    }
                    
                    ui.add_space(10.);
                    
                    // Path input
                    let pre_modified_path = self.path_input.clone();
                    let response = ui.add(TextEdit::singleline(&mut self.path_input)
                        .desired_width(ui.available_width() - 10.)
                        .hint_text("Enter path..."));
                    
                    if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        if self.path_input != pre_modified_path {
                            self.navigate_to(self.path_input.clone(), cmd_tx);
                        }
                    }
                });
            });
    }
    
    fn display_left_sidebar(&mut self, ui: &mut Ui, cmd_tx: &Sender<Cmd>, stroke: Stroke, radius: CornerRadius) {
        let sidebar_frame = Frame::default()
            .fill(Color32::from_rgb(20, 20, 24))
            .inner_margin(Margin::same(8))
            .corner_radius(radius)
            .stroke(stroke);
        
        SidePanel::left("RemoteExplorerSidebar")
            .frame(sidebar_frame)
            .resizable(true)
            .default_width(150.)
            .min_width(120.)
            .max_width(250.)
            .show_inside(ui, |ui| {
                let mut navigate_to_path: Option<String> = None;
                
                ScrollArea::vertical().show(ui, |ui| {
                    // Drives section
                    if !self.drives.is_empty() {
                        ui.label(RichText::new("💾 Drives").strong().color(Color32::LIGHT_GRAY));
                        ui.add_space(4.);
                        
                        for drive in &self.drives {
                            let label = format!("💿 {}", drive);
                            if ui.selectable_label(false, label).clicked() {
                                navigate_to_path = Some(drive.clone());
                            }
                        }
                        
                        ui.add_space(12.);
                    }
                    
                    // Quick access shortcuts
                    ui.label(RichText::new("⭐ Quick Access").strong().color(Color32::LIGHT_GRAY));
                    ui.add_space(4.);
                    
                    for shortcut in &self.shortcuts {
                        let label = format!("{} {}", shortcut.icon, shortcut.name);
                        if ui.selectable_label(false, label).clicked() {
                            navigate_to_path = Some(shortcut.path.clone());
                        }
                    }
                });
                
                // Apply navigation after borrow ends
                if let Some(path) = navigate_to_path {
                    self.navigate_to(path, cmd_tx);
                }
            });
    }
    
    fn display_tools_sidebar(&mut self, ui: &mut Ui, cmd_tx: &Sender<Cmd>, stroke: Stroke, radius: CornerRadius) {
        let sidebar_frame = Frame::default()
            .fill(Color32::from_rgb(24, 20, 28))
            .inner_margin(Margin::same(8))
            .corner_radius(radius)
            .stroke(stroke);
        
        SidePanel::right("MyToolsSidebar")
            .frame(sidebar_frame)
            .resizable(true)
            .default_width(280.)
            .min_width(200.)
            .max_width(450.)
            .show_inside(ui, |ui| {
                // Calculate available height to split between tools list and preview
                let total_height = ui.available_height();
                let has_preview = self.preview_visible && 
                    (self.preview.text_content.is_some() || self.preview.image_data.is_some() || self.preview.loading);
                
                // Tools list section (top half when preview is shown)
                let tools_height = if has_preview { total_height * 0.4 } else { total_height };
                
                ui.allocate_ui_with_layout(
                    Vec2::new(ui.available_width(), tools_height),
                    Layout::top_down(Align::LEFT),
                    |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("🧰 My Tools").strong().color(Color32::from_rgb(200, 180, 255)));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.small_button("⟲").on_hover_text("Refresh").clicked() {
                                    self.refresh_tools_async();
                                }
                                if ui.small_button("+").on_hover_text("Upload file").clicked() {
                                    self.upload_tool_dialog();
                                }
                            });
                        });
                        ui.separator();
                        
                        if self.tools_loading {
                            ui.spinner();
                            ui.label("Loading tools...");
                        } else if self.my_tools.is_empty() {
                            ui.label(RichText::new("No tools yet").italics().color(Color32::GRAY));
                            ui.add_space(10.);
                            ui.label("Upload scripts and files here to easily transfer them to client machines.");
                        } else {
                            let mut copy_to_client: Option<usize> = None;
                            let mut delete_tool: Option<usize> = None;
                            
                            ScrollArea::vertical()
                                .id_salt("my_tools_scroll")
                                .max_height(tools_height - 40.)
                                .show(ui, |ui| {
                                for (idx, tool) in self.my_tools.iter().enumerate() {
                                    let is_selected = self.selected_tool_idx == Some(idx);
                                    let icon = if tool.is_text { "📜" } else { "📦" };
                                    let label = format!("{} {}", icon, tool.name);
                                    
                                    let response = ui.selectable_label(is_selected, label);
                                    
                                    response.clone().on_hover_text(format!("{} bytes", tool.size));
                                    
                                    if response.clicked() {
                                        self.selected_tool_idx = Some(idx);
                                    }
                                    
                                    // Double-click to copy to client
                                    if response.double_clicked() {
                                        copy_to_client = Some(idx);
                                    }
                                    
                                    response.context_menu(|ui| {
                                        ui.set_min_width(180.0);
                                        
                                        if ui.button("📤 Copy to Client").clicked() {
                                            copy_to_client = Some(idx);
                                            ui.close();
                                        }
                                        
                                        ui.separator();
                                        
                                        if ui.button("🗑 Delete from My Tools").clicked() {
                                            delete_tool = Some(idx);
                                            ui.close();
                                        }
                                    });
                                }
                            });
                            
                            // Handle deferred actions
                            if let Some(idx) = copy_to_client {
                                if let Some(tool) = self.my_tools.get(idx) {
                                    let tool_clone = tool.clone();
                                    self.copy_tool_to_client(&tool_clone, &self.current_path.clone(), cmd_tx);
                                }
                            }
                            
                            if let Some(idx) = delete_tool {
                                if let Some(tool) = self.my_tools.get(idx) {
                                    self.delete_tool(&tool.path.clone());
                                    self.my_tools.remove(idx);
                                }
                            }
                        }
                    }
                );
                
                // Preview pane (bottom half)
                if has_preview {
                    ui.separator();
                    self.display_inline_preview(ui, cmd_tx, total_height * 0.6 - 10.);
                }
            });
    }
    
    /// Display preview inline (used in the tools sidebar)
    fn display_inline_preview(&mut self, ui: &mut Ui, cmd_tx: &Sender<Cmd>, max_height: f32) {
        ui.horizontal(|ui| {
            let filename = std::path::Path::new(&self.preview.path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "Preview".to_string());
            
            ui.label(RichText::new(format!("📄 {}", filename)).strong());
            
            if self.preview.modified {
                ui.label(RichText::new("●").color(Color32::YELLOW));
            }
            
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.small_button("✖").on_hover_text("Close preview").clicked() {
                    self.preview_visible = false;
                    self.preview.clear();
                }
                
                // Save button for text content
                if self.preview.text_content.is_some() && self.preview.modified {
                    if ui.small_button("💾 Save").clicked() {
                        if let Some(content) = &self.preview.text_content {
                            let _ = cmd_tx.try_send(Cmd::SaveRemoteFile(
                                self.preview.path.clone(),
                                content.clone(),
                            ));
                        }
                    }
                }
            });
        });
        
        ui.separator();
        
        if self.preview.loading {
            ui.spinner();
            ui.label("Loading preview...");
        } else if let Some(error) = &self.preview.error {
            ui.colored_label(Color32::RED, error.clone());
        } else if let Some(content) = &mut self.preview.text_content.clone() {
            // Text preview with editing
            ScrollArea::both()
                .id_salt("preview_text_scroll")
                .max_height(max_height - 30.)
                .show(ui, |ui| {
                let response = ui.add(
                    TextEdit::multiline(&mut content.clone())
                        .font(eframe::egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                        .desired_rows(8)
                );
                
                // Update content if changed
                if response.changed() {
                    self.preview.text_content = Some(content.clone());
                    self.preview.modified = true;
                }
            });
        } else if let Some(texture) = &self.preview.texture {
            // Image preview - show at original size, scrollable
            ScrollArea::both()
                .id_salt("preview_image_scroll")
                .max_height(max_height - 30.)
                .show(ui, |ui| {
                    let tex_size = texture.size_vec2();
                    // Show at original size by default
                    ui.image((texture.id(), tex_size));
                });
        }
    }
    
    fn display_file_list(&mut self, ui: &mut Ui, cmd_tx: &Sender<Cmd>) {
        let available_height = ui.available_height();
        
        if self.loading {
            ui.vertical_centered(|ui| {
                ui.add_space(50.);
                ui.spinner();
                ui.label("Loading directory contents...");
            });
            return;
        }
        
        if self.entries.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(50.);
                ui.label(RichText::new("📂 Empty directory").heading());
                ui.add_space(10.);
                if ui.button("Refresh").clicked() {
                    self.refresh(cmd_tx);
                }
            });
            return;
        }
        
        // Track deferred actions
        let mut navigate_to_path: Option<String> = None;
        let mut should_refresh = false;
        let mut new_selected_idx: Option<usize> = None;
        let mut download_path: Option<String> = None;
        let mut execute_path: Option<String> = None;
        let mut preview_text_path: Option<String> = None;
        let mut preview_image_path: Option<String> = None;
        let mut copy_to_tools_path: Option<String> = None;
        
        // Column header
        ui.horizontal(|ui| {
            ui.set_min_height(22.);
            let header_color = Color32::from_rgb(180, 180, 180);
            ui.allocate_ui_with_layout(
                Vec2::new(ui.available_width() * 0.5, 20.),
                Layout::left_to_right(Align::Center),
                |ui| {
                    ui.label(RichText::new("Name").color(header_color).strong());
                }
            );
            ui.allocate_ui_with_layout(
                Vec2::new(140., 20.),
                Layout::left_to_right(Align::Center),
                |ui| {
                    ui.label(RichText::new("Date Modified").color(header_color).strong());
                }
            );
            ui.allocate_ui_with_layout(
                Vec2::new(80., 20.),
                Layout::right_to_left(Align::Center),
                |ui| {
                    ui.label(RichText::new("Size").color(header_color).strong());
                }
            );
        });
        ui.separator();
        
        ScrollArea::vertical()
            .max_height(available_height - 40.)
            .auto_shrink(false)
            .show(ui, |ui| {
                for (idx, entry) in self.entries.iter().enumerate() {
                    let is_selected = self.selected_idx == Some(idx);
                    let icon = get_file_icon(&entry.name, entry.is_directory);
                    
                    // Format size
                    let size_text = if entry.is_directory {
                        String::new()
                    } else if let Some(size) = entry.size {
                        format_file_size(size)
                    } else {
                        String::new()
                    };
                    
                    // Format date modified
                    let modified_text = format_modified_date(entry.modified.as_deref());
                    
                    // Row layout
                    let response = ui.horizontal(|ui| {
                        ui.set_min_height(22.);
                        
                        // Selection highlight
                        let fill_color = if is_selected {
                            Color32::from_rgba_unmultiplied(80, 80, 120, 80)
                        } else {
                            Color32::TRANSPARENT
                        };
                        
                        let rect = ui.available_rect_before_wrap();
                        ui.painter().rect_filled(rect, 0.0, fill_color);
                        
                        // Name column (50% width)
                        let name_response = ui.allocate_ui_with_layout(
                            Vec2::new(ui.available_width() * 0.5, 20.),
                            Layout::left_to_right(Align::Center),
                            |ui| {
                                let label_text = format!("{}  {}", icon, entry.name);
                                let name_color = if entry.is_directory {
                                    Color32::from_rgb(130, 170, 255)
                                } else {
                                    Color32::from_rgb(220, 220, 220)
                                };
                                ui.add(eframe::egui::Label::new(RichText::new(label_text).color(name_color)).sense(Sense::click()))
                            }
                        ).inner;
                        
                        // Date Modified column (140px)
                        ui.allocate_ui_with_layout(
                            Vec2::new(140., 20.),
                            Layout::left_to_right(Align::Center),
                            |ui| {
                                ui.label(RichText::new(&modified_text).color(Color32::GRAY).small());
                            }
                        );
                        
                        // Size column (80px, right-aligned)
                        ui.allocate_ui_with_layout(
                            Vec2::new(80., 20.),
                            Layout::right_to_left(Align::Center),
                            |ui| {
                                ui.label(RichText::new(&size_text).color(Color32::GRAY).small());
                            }
                        );
                        
                        name_response
                    }).inner;
                    
                    // Handle clicks
                    if response.clicked() {
                        new_selected_idx = Some(idx);
                        
                        // Single click on text files -> preview
                        if !entry.is_directory && is_text_file(&entry.name) {
                            preview_text_path = Some(entry.path.clone());
                        }
                    }
                    
                    // Double-click behavior
                    if response.double_clicked() {
                        if entry.is_directory {
                            navigate_to_path = Some(entry.path.clone());
                        } else if is_image_file(&entry.name) {
                            // Request thumbnail for images
                            preview_image_path = Some(entry.path.clone());
                        } else {
                            // Execute the file
                            execute_path = Some(entry.path.clone());
                        }
                    }
                    
                    // Context menu
                    let entry_path = entry.path.clone();
                    let entry_name = entry.name.clone();
                    let entry_is_directory = entry.is_directory;
                    
                    response.context_menu(|ui| {
                        ui.set_min_width(180.0);
                        
                        if entry_is_directory {
                            if ui.button("📂 Open").clicked() {
                                navigate_to_path = Some(entry_path.clone());
                                ui.close();
                            }
                        } else {
                            if ui.button("▶ Execute / Open").clicked() {
                                execute_path = Some(entry_path.clone());
                                ui.close();
                            }
                            
                            if ui.button("📥 Download").clicked() {
                                download_path = Some(entry_path.clone());
                                ui.close();
                            }
                            
                            if is_text_file(&entry_name) {
                                if ui.button("👁 Preview").clicked() {
                                    preview_text_path = Some(entry_path.clone());
                                    ui.close();
                                }
                            }
                            
                            if is_image_file(&entry_name) {
                                if ui.button("🖼 View Thumbnail").clicked() {
                                    preview_image_path = Some(entry_path.clone());
                                    ui.close();
                                }
                            }
                            
                            ui.separator();
                            
                            if ui.button("🧰 Copy to My Tools").clicked() {
                                copy_to_tools_path = Some(entry_path.clone());
                                ui.close();
                            }
                        }
                        
                        ui.separator();
                        
                        if ui.button("🗑 Delete").clicked() {
                            let _ = cmd_tx.try_send(Cmd::FileSystemAction(
                                crate::FileSystemAction::Delete(entry_path.clone())
                            ));
                            should_refresh = true;
                            ui.close();
                        }
                    });
                }
            });
        
        // Apply deferred actions
        if let Some(idx) = new_selected_idx {
            self.selected_idx = Some(idx);
        }
        if let Some(path) = navigate_to_path {
            self.navigate_to(path, cmd_tx);
        }
        if let Some(path) = download_path {
            let filename = std::path::Path::new(&path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "download".to_string());
            self.pending_download = Some(filename);
            let _ = cmd_tx.try_send(Cmd::DownloadRemoteFile(path));
        }
        if let Some(path) = execute_path {
            let _ = cmd_tx.try_send(Cmd::ExecuteRemoteFile(path));
        }
        if let Some(path) = preview_text_path {
            self.preview.loading = true;
            let _ = cmd_tx.try_send(Cmd::PreviewRemoteFile(path));
        }
        if let Some(path) = preview_image_path {
            self.preview.loading = true;
            let _ = cmd_tx.try_send(Cmd::RequestThumbnail(path));
        }
        if let Some(path) = copy_to_tools_path {
            // First download the file, then upload to My Tools
            // We'll need to handle this specially in receive.rs
            self.pending_tool_upload = Some((path.clone(), Vec::new()));
            let _ = cmd_tx.try_send(Cmd::DownloadRemoteFile(path));
        }
        if should_refresh {
            self.refresh(cmd_tx);
        }
    }
    
    /// Refresh My Tools asynchronously
    #[cfg(not(target_arch = "wasm32"))]
    fn refresh_tools_async(&mut self) {
        if self.bucket_name.is_empty() {
            return;
        }
        
        self.tools_loading = true;
        let bucket = self.bucket_name.clone();
        
        // Create channel and store the receiver so we can poll for results
        let (tx, rx) = crossbeam::channel::bounded::<Vec<ToolEntry>>(1);
        self.tools_rx = Some(rx);
        
        PlatformSpawner::spawn(async move {
            match file_storage::list_files(&bucket, "").await {
                Ok(entries) => {
                    let tools: Vec<ToolEntry> = entries.into_iter().map(ToolEntry::from).collect();
                    let _ = tx.send(tools);
                }
                Err(e) => {
                    log::error!("Failed to refresh My Tools: {}", e);
                }
            }
        });
    }
    
    #[cfg(target_arch = "wasm32")]
    fn refresh_tools_async(&mut self) {
        // Not supported in WASM
    }
    
    /// Delete a tool from My Tools
    #[cfg(not(target_arch = "wasm32"))]
    fn delete_tool(&self, path: &str) {
        if self.bucket_name.is_empty() {
            return;
        }
        
        let bucket = self.bucket_name.clone();
        let tool_path = path.to_string();
        
        PlatformSpawner::spawn(async move {
            if let Err(e) = file_storage::delete_file(&bucket, &tool_path).await {
                log::error!("Failed to delete tool: {}", e);
            } else {
                log::info!("Deleted tool: {}", tool_path);
            }
        });
    }
    
    #[cfg(target_arch = "wasm32")]
    fn delete_tool(&self, _path: &str) {}
    
    /// Open file dialog to upload a tool
    #[cfg(not(target_arch = "wasm32"))]
    fn upload_tool_dialog(&mut self) {
        if self.bucket_name.is_empty() {
            log::warn!("upload_tool_dialog: bucket_name is empty");
            return;
        }
        
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("All Files", &["*"])
            .add_filter("Scripts", &["ps1", "bat", "cmd", "sh", "py"])
            .pick_file()
        {
            let bucket = self.bucket_name.clone();
            
            if let Ok(data) = std::fs::read(&path) {
                let filename = path.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "unnamed".to_string());
                let dest_path = format!("/{}", filename);
                let data_len = data.len();
                
                log::info!("upload_tool_dialog: Uploading {} ({} bytes) to bucket: {}", filename, data_len, bucket);
                
                // Create channel to receive the refreshed tools list
                let (tx, rx) = crossbeam::channel::bounded::<Vec<ToolEntry>>(1);
                self.tools_rx = Some(rx);
                self.tools_loading = true;
                
                PlatformSpawner::spawn(async move {
                    // Ensure bucket exists
                    if let Err(e) = file_storage::init_user_bucket(&bucket).await {
                        log::error!("upload_tool_dialog: Failed to init bucket: {}", e);
                    }
                    
                    match file_storage::put_file(&bucket, &dest_path, data).await {
                        Ok(_) => {
                            log::info!("upload_tool_dialog: Uploaded tool: {}", filename);
                            
                            // Refresh the tools list
                            match file_storage::list_files(&bucket, "").await {
                                Ok(entries) => {
                                    let tools: Vec<ToolEntry> = entries.into_iter().map(ToolEntry::from).collect();
                                    log::info!("upload_tool_dialog: Refreshed tools list: {} items", tools.len());
                                    let _ = tx.send(tools);
                                }
                                Err(e) => {
                                    log::error!("upload_tool_dialog: Failed to refresh tools list: {}", e);
                                    let _ = tx.send(Vec::new());
                                }
                            }
                        }
                        Err(e) => {
                            log::error!("upload_tool_dialog: Failed to upload tool: {}", e);
                            let _ = tx.send(Vec::new());
                        }
                    }
                });
            }
        }
    }
    
    #[cfg(target_arch = "wasm32")]
    fn upload_tool_dialog(&mut self) {}
}

/// Get an appropriate icon for a file based on its extension
fn get_file_icon(filename: &str, is_directory: bool) -> &'static str {
    if is_directory {
        return "📁";
    }
    
    let ext = filename.rsplit('.').next().unwrap_or("").to_lowercase();
    
    match ext.as_str() {
        // Scripts
        "ps1" | "psm1" | "psd1" => "📜",
        "bat" | "cmd" => "📜",
        "sh" | "bash" | "zsh" => "📜",
        "py" | "pyw" => "🐍",
        "rb" => "💎",
        "js" | "ts" | "mjs" => "📜",
        
        // Documents
        "txt" | "log" | "md" | "rst" => "📝",
        "doc" | "docx" => "📘",
        "pdf" => "📕",
        "xls" | "xlsx" | "csv" => "📊",
        "ppt" | "pptx" => "📽",
        
        // Code
        "rs" => "🦀",
        "c" | "cpp" | "h" | "hpp" => "⚙",
        "java" | "kt" => "☕",
        "go" => "🔵",
        "cs" => "💜",
        "html" | "htm" => "🌐",
        "css" | "scss" | "sass" => "🎨",
        "json" | "yaml" | "yml" | "toml" | "xml" => "📋",
        
        // Images
        "jpg" | "jpeg" | "png" | "gif" | "bmp" | "webp" | "ico" => "🖼",
        "svg" => "🎨",
        "psd" | "ai" => "🎨",
        "raw" | "arw" | "cr2" | "nef" | "dng" => "📷",
        
        // Audio/Video
        "mp3" | "wav" | "flac" | "ogg" | "m4a" => "🎵",
        "mp4" | "mkv" | "avi" | "mov" | "wmv" | "webm" => "🎬",
        
        // Archives
        "zip" | "rar" | "7z" | "tar" | "gz" | "bz2" => "📦",
        
        // Executables
        "exe" | "msi" => "⚡",
        "dll" | "so" | "dylib" => "🔧",
        
        // Config
        "ini" | "cfg" | "conf" | "config" => "⚙",
        "reg" => "📋",
        
        // Misc
        "iso" | "img" => "💿",
        "db" | "sqlite" | "mdb" => "🗃",
        "lnk" => "🔗",
        
        _ => "📄",
    }
}

/// Check if a file is likely a text file based on extension
fn is_text_file(filename: &str) -> bool {
    let ext = filename.rsplit('.').next().unwrap_or("").to_lowercase();
    
    matches!(ext.as_str(),
        "txt" | "log" | "md" | "rst" | "csv" |
        "ps1" | "psm1" | "psd1" | "bat" | "cmd" | "sh" | "bash" | "zsh" |
        "py" | "pyw" | "rb" | "js" | "ts" | "mjs" | "jsx" | "tsx" |
        "rs" | "c" | "cpp" | "h" | "hpp" | "java" | "kt" | "go" | "cs" |
        "html" | "htm" | "css" | "scss" | "sass" |
        "json" | "yaml" | "yml" | "toml" | "xml" | "ini" | "cfg" | "conf" |
        "sql" | "reg" | "gitignore" | "editorconfig"
    )
}

/// Check if a file is an image
fn is_image_file(filename: &str) -> bool {
    let ext = filename.rsplit('.').next().unwrap_or("").to_lowercase();
    
    matches!(ext.as_str(),
        "jpg" | "jpeg" | "png" | "gif" | "bmp" | "webp" | "ico" |
        "svg" | "tiff" | "tif" |
        "raw" | "arw" | "cr2" | "cr3" | "nef" | "dng" | "orf" | "raf"
    )
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

/// Format a modified date string for display
/// Input is ISO 8601 format (RFC 3339), output is a user-friendly format
fn format_modified_date(modified: Option<&str>) -> String {
    match modified {
        Some(s) => {
            // Try to parse the RFC 3339 datetime
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
                // Format as "Jan 18, 2026  2:30 PM"
                dt.format("%b %d, %Y  %I:%M %p").to_string()
            } else {
                // Fallback: just show the raw string, truncated
                s.chars().take(19).collect()
            }
        }
        None => String::new(),
    }
}
