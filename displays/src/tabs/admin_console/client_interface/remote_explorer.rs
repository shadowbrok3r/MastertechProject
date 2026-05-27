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
    self, Align, Align2, CentralPanel, Color32, FontId, Frame, Key, KeyboardShortcut, Layout,
    Margin, Response, RichText, ScrollArea, Sense, Stroke, TextEdit, Ui, Vec2, CornerRadius,
    scroll_area, Widget,
};
use crossbeam::channel::{Sender, Receiver};
use egui_data_table::{
    viewer::{default_hotkeys, DecodeErrorBehavior, RowCodec, UiActionContext, CustomActionContext, CustomActionEditor},
    CustomMenuItem, DataTable, Renderer, RowViewer, SelectionSnapshot, UiAction,
};
use egui_extras::Column as TableColumnConfig;
use serde::Serialize;
use crate::{Cmd, RemoteDirEntry, PlatformSpawner, Spawner};
use crate::ui_tools::icons::{self, menu_label};
use database::schema::file_storage::{self, FileEntry};

/// Common folder shortcuts
#[derive(Clone)]
pub struct FolderShortcut {
    pub name: String,
    pub icon: &'static str,
    pub path: String,
}

/// Full-width, **left-aligned** selectable row for the explorer's side
/// panels.
///
/// egui's [`Button`] and the deprecated `SelectableLabel` both center
/// their text inside the allocated rect with no public way to override
/// horizontal alignment. That makes them wrong for a Windows-style file
/// pane, where every entry's text needs to start at the left edge so the
/// eye can scan a vertical column of filenames without tracking past
/// centered whitespace.
///
/// This helper allocates an exact `[width, height]` rect, paints the
/// hover/selected background from the current style's
/// `interact_selectable` visuals, and draws the label at
/// `Align2::LEFT_CENTER` with an 8 px left inset. The returned
/// [`Response`] behaves like any other clickable widget — supports
/// `clicked()` / `double_clicked()` / `context_menu(...)` / `on_hover_*`.
fn sidebar_row(
    ui: &mut Ui,
    width: f32,
    height: f32,
    selected: bool,
    label: impl Into<String>,
) -> Response {
    let label = label.into();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());

    let visuals = ui.global_style().interact_selectable(&response, selected);

    // Only paint a background when there's something to indicate — keeps
    // the resting state of the panel clean.
    if selected || response.hovered() {
        ui.painter()
            .rect_filled(rect, visuals.corner_radius, visuals.weak_bg_fill);
    }

    let text_pos = rect.left_center() + Vec2::new(8.0, 0.0);
    ui.painter().text(
        text_pos,
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(13.0),
        visuals.text_color(),
    );

    response
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

/// Actions dispatched from the file table's context menu / clicks
#[derive(Debug, Clone)]
pub enum ExplorerAction {
    Navigate(String),
    Download(String),
    Execute(String),
    PreviewText(String),
    PreviewImage(String),
    CopyToTools(String),
    Delete(String),
    Refresh,
}

/* --------------------------------- egui-data-table viewer --------------------------------- */

/// Columns: 0 Icon, 1 Name, 2 Date Modified, 3 Size, 4 Type
const NUM_FILE_COLUMNS: usize = 5;

#[derive(Serialize)]
pub struct RemoteFileRowViewer {
    filter: String,
    #[serde(skip)]
    hotkeys: Vec<(KeyboardShortcut, UiAction)>,
    #[serde(skip)]
    pub action_tx: Option<Sender<ExplorerAction>>,
}

impl Default for RemoteFileRowViewer {
    fn default() -> Self {
        Self {
            filter: String::new(),
            hotkeys: Vec::new(),
            action_tx: None,
        }
    }
}

/* ------------------------------------ RowCodec ------------------------------------ */

pub struct RemoteFileCodec;

impl RowCodec<RemoteDirEntry> for RemoteFileCodec {
    type DeserializeError = &'static str;

    fn encode_column(&mut self, row: &RemoteDirEntry, column: usize, dst: &mut String) {
        match column {
            0 => dst.push_str(icons::file_icon(&row.name, row.is_directory)),
            1 => dst.push_str(&row.name),
            2 => dst.push_str(row.modified.as_deref().unwrap_or("")),
            3 => {
                if let Some(size) = row.size {
                    dst.push_str(&size.to_string());
                }
            }
            4 => {
                let ext = row.name.rsplit('.').next().unwrap_or("").to_lowercase();
                if row.is_directory { dst.push_str("Folder"); } else { dst.push_str(&ext); }
            }
            _ => {}
        }
    }

    fn decode_column(
        &mut self,
        src: &str,
        column: usize,
        row: &mut RemoteDirEntry,
    ) -> Result<(), DecodeErrorBehavior> {
        match column {
            1 => row.name = src.to_string(),
            2 => row.modified = if src.is_empty() { None } else { Some(src.to_string()) },
            3 => row.size = src.parse().ok(),
            _ => {}
        }
        Ok(())
    }

    fn create_empty_decoded_row(&mut self) -> RemoteDirEntry {
        RemoteDirEntry {
            name: String::new(),
            path: String::new(),
            is_directory: false,
            size: None,
            modified: None,
        }
    }
}

/* --------------------------------- RemoteExplorer --------------------------------- */

/// Remote filesystem explorer state
pub struct RemoteExplorer {
    /// Current path being viewed
    pub current_path: String,
    /// Navigation history stack
    pub navigation_stack: Vec<String>,
    /// Whether we're waiting for a response
    pub loading: bool,
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
    /// egui-data-table backing store for file entries
    pub file_table: DataTable<RemoteDirEntry>,
    /// Row viewer for the data table
    pub file_viewer: RemoteFileRowViewer,
    /// Receiver for actions dispatched from the table viewer
    pub action_rx: Receiver<ExplorerAction>,
    /// Sender kept so we can clone into the viewer on reset
    action_tx: Sender<ExplorerAction>,
}

impl Default for RemoteExplorer {
    fn default() -> Self {
        Self::new()
    }
}

impl RemoteExplorer {
    pub fn new() -> Self {
        // Default shortcuts - the Mastertech client will resolve these using Windows API
        let shortcuts = [
            ("Desktop", "Desktop"),
            ("Documents", "Documents"),
            ("Downloads", "Downloads"),
            ("Pictures", "Pictures"),
            ("Music", "Music"),
            ("Videos", "Videos"),
            ("AppData", "AppData"),
            ("LocalAppData", "LocalAppData"),
        ]
        .into_iter()
        .map(|(name, path)| FolderShortcut {
            name: name.to_string(),
            icon: icons::folder_shortcut_icon(path),
            path: path.to_string(),
        })
        .collect();
        
        let (action_tx, action_rx) = crossbeam::channel::unbounded();
        let mut file_viewer = RemoteFileRowViewer::default();
        file_viewer.action_tx = Some(action_tx.clone());

        Self {
            current_path: "current".to_string(),
            navigation_stack: Vec::new(),
            loading: false,
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
            file_table: DataTable::new(),
            file_viewer,
            action_rx,
            action_tx,
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
    pub fn set_entries(&mut self, mut entries: Vec<RemoteDirEntry>, current_path: Option<String>) {
        entries.sort_by(|a, b| {
            match (a.is_directory, b.is_directory) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            }
        });
        self.file_table.replace(entries);
        self.loading = false;
        
        if let Some(path) = current_path {
            self.current_path = path.clone();
            self.path_input = path;
        } else {
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
        self.file_table.clear();
        let _ = cmd_tx.try_send(Cmd::ListDirectory(path));
    }
    
    /// Navigate up one directory level
    pub fn navigate_up(&mut self, cmd_tx: &Sender<Cmd>) {
        if let Some(previous) = self.navigation_stack.pop() {
            self.current_path = previous.clone();
            self.path_input = previous.clone();
            self.loading = true;
            self.file_table.clear();
            let _ = cmd_tx.try_send(Cmd::ListDirectory(previous));
        } else if !self.current_path.is_empty() {
            let path = std::path::Path::new(&self.current_path);
            if let Some(parent) = path.parent() {
                let parent_str = parent.to_string_lossy().to_string();
                self.current_path = parent_str.clone();
                self.path_input = parent_str.clone();
                self.loading = true;
                self.file_table.clear();
                let _ = cmd_tx.try_send(Cmd::ListDirectory(parent_str));
            }
        }
    }
    
    /// Refresh current directory
    pub fn refresh(&mut self, cmd_tx: &Sender<Cmd>) {
        self.loading = true;
        self.file_table.clear();
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
        let stroke = Stroke::new(0.7_f32, Color32::from_additive_luminance(100));
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
            eframe::egui::Panel::bottom("RemoteExplorerError")
                .exact_size(25.)
                .show_inside(ui, |ui| {
                    ui.colored_label(ui.style().visuals.error_fg_color, error.clone());
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
        // Windows File Explorer toolbar layout:
        //
        //   [⬆][🏠][⟲]   <path input fills remainder>           [View ▾]
        //
        // Navigation is a tight three-button group at the left (no
        // separator between them — Windows Explorer doesn't either).
        // The three sidebar/preview toggles used to sit between the nav
        // buttons and the path bar; they've moved into the right-aligned
        // **View** menu so the toolbar reads as a single intent (where
        // to go, what to see) instead of a jumble of selectable_labels.
        //
        // Uniform button size: each nav button is a square `NAV_BTN_W ×
        // NAV_BTN_W`, matching Explorer's chrome.
        const NAV_BTN_W: f32 = 28.0;

        eframe::egui::Panel::top("RemoteExplorerTop")
            .frame(Frame::default().outer_margin(Margin::symmetric(5, 2)))
            .exact_size(36.)
            .show_inside(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    // Up — parent directory.
                    if ui
                        .add_sized(
                            [NAV_BTN_W, NAV_BTN_W],
                            egui::Button::new(RichText::new(icons::UP).size(16.0)),
                        )
                        .on_hover_text("Up to parent folder (Alt+Up)")
                        .clicked()
                    {
                        self.navigate_up(cmd_tx);
                    }

                    // Home — back to the remote machine's current dir.
                    // The BMP house glyph U+2302 (⌂) sounded safe but
                    // isn't in the loaded proportional font and rendered
                    // as a missing-glyph box. The supplementary-plane
                    // 🏠 emoji *is* covered by egui's bundled fallback,
                    // so we use it directly — same as the original.
                    if ui
                        .add_sized(
                            [NAV_BTN_W, NAV_BTN_W],
                            egui::Button::new(RichText::new(icons::HOME).size(16.0)),
                        )
                        .on_hover_text("Home")
                        .clicked()
                    {
                        self.navigate_to("current".to_string(), cmd_tx);
                    }

                    // Refresh.
                    if ui
                        .add_sized(
                            [NAV_BTN_W, NAV_BTN_W],
                            egui::Button::new(RichText::new(icons::REFRESH).size(16.0)),
                        )
                        .on_hover_text("Refresh (F5)")
                        .clicked()
                    {
                        self.refresh(cmd_tx);
                    }

                    // Path input — fills the remaining space between the
                    // nav group on the left and the View menu pinned to
                    // the right edge. We reserve ~90 px for the View
                    // button at the right so the TextEdit doesn't shove
                    // it off-screen on narrow windows.
                    const VIEW_BTN_RESERVED: f32 = 90.0;
                    let path_width = (ui.available_width() - VIEW_BTN_RESERVED).max(120.0);
                    let pre_modified_path = self.path_input.clone();
                    let response = ui.add(
                        TextEdit::singleline(&mut self.path_input)
                            .desired_width(path_width)
                            .hint_text("Enter path..."),
                    );
                    if response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                        if self.path_input != pre_modified_path {
                            self.navigate_to(self.path_input.clone(), cmd_tx);
                        }
                    }

                    // View menu — pane toggles, right-aligned.
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.menu_button(RichText::new(menu_label("View")), |ui| {
                            if ui
                                .add(
                                    egui::Button::new("Navigation sidebar")
                                        .selected(self.sidebar_visible),
                                )
                                .clicked()
                            {
                                self.sidebar_visible = !self.sidebar_visible;
                                ui.close();
                            }
                            if ui
                                .add(
                                    egui::Button::new("My Tools sidebar")
                                        .selected(self.tools_sidebar_visible),
                                )
                                .clicked()
                            {
                                self.tools_sidebar_visible = !self.tools_sidebar_visible;
                                ui.close();
                            }
                            if ui
                                .add(
                                    egui::Button::new("Preview pane")
                                        .selected(self.preview_visible),
                                )
                                .clicked()
                            {
                                self.preview_visible = !self.preview_visible;
                                ui.close();
                            }
                        });
                    });
                });
            });
    }
    
    fn display_left_sidebar(&mut self, ui: &mut Ui, cmd_tx: &Sender<Cmd>, stroke: Stroke, radius: CornerRadius) {
        // Sidebar entries (drives and quick-access shortcuts) used to
        // be plain `selectable_label` calls, which size to their text and
        // produced a ragged left column. Every entry is now sized to the
        // sidebar's full width with the same row height so the panel
        // reads as a uniform list — Windows Explorer's navigation pane
        // behavior.
        const ENTRY_H: f32 = 24.0;

        let sidebar_frame = Frame::default()
            .fill(Color32::from_rgb(20, 20, 24))
            .inner_margin(Margin::same(8))
            .corner_radius(radius)
            .stroke(stroke);

        eframe::egui::Panel::left("RemoteExplorerSidebar")
            .frame(sidebar_frame)
            .resizable(true)
            .default_size(160.)
            .min_size(140.)
            .max_size(260.)
            .show_inside(ui, |ui| {
                let mut navigate_to_path: Option<String> = None;

                ScrollArea::vertical().show(ui, |ui| {
                    let entry_w = ui.available_width();

                    // Quick access first — matches Windows File Explorer's
                    // navigation-pane ordering. ⭐ is BMP (U+2B50) and
                    // renders in the default proportional font.
                    ui.label(RichText::new(format!("{} Quick Access", icons::STAR)).strong().color(Color32::LIGHT_GRAY));
                    ui.add_space(4.);

                    for shortcut in &self.shortcuts {
                        let label = format!("{} {}", shortcut.icon, shortcut.name);
                        if sidebar_row(ui, entry_w, ENTRY_H, false, label).clicked() {
                            navigate_to_path = Some(shortcut.path.clone());
                        }
                    }

                    if !self.drives.is_empty() {
                        ui.add_space(12.);
                        // Plain "Drives" header (no supplementary-plane
                        // emoji prefix that may fall back to a missing-
                        // glyph box).
                        ui.label(RichText::new("Drives").strong().color(Color32::LIGHT_GRAY));
                        ui.add_space(4.);

                        for drive in &self.drives {
                            let label = format!("{} {drive}", icons::HARD_DRIVE);
                            if sidebar_row(ui, entry_w, ENTRY_H, false, label).clicked() {
                                navigate_to_path = Some(drive.clone());
                            }
                        }
                    }
                });

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
        
        eframe::egui::Panel::right("MyToolsSidebar")
            .frame(sidebar_frame)
            .resizable(true)
            .default_size(280.)
            .min_size(200.)
            .max_size(450.)
            .show_inside(ui, |ui| {
                // Calculate available height to split between tools list and preview
                let total_height = ui.available_height();
                let has_preview = self.preview_visible && 
                    (self.preview.text_content.is_some() || self.preview.image_data.is_some() || self.preview.loading);
                
                // Tools list section (top half when preview is shown)
                let tools_height = if has_preview { total_height * 0.4 } else { total_height };
                
                // Per-tool-row height — kept uniform with the left
                // sidebar's `ENTRY_H` so both side panels feel like the
                // same widget family.
                const TOOL_ENTRY_H: f32 = 24.0;

                ui.allocate_ui_with_layout(
                    Vec2::new(ui.available_width(), tools_height),
                    Layout::top_down(Align::LEFT),
                    |ui| {
                        ui.horizontal(|ui| {
                            // Plain "My Tools" text — the previous 🧰
                            // toolbox emoji (U+1F9F0) is in the
                            // supplementary plane and isn't covered by
                            // egui's bundled emoji fallback font, so it
                            // rendered as a missing-glyph box.
                            ui.label(
                                RichText::new("My Tools")
                                    .strong()
                                    .color(Color32::from_rgb(200, 180, 255)),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.small_button(icons::REFRESH).on_hover_text("Refresh").clicked() {
                                    self.refresh_tools_async();
                                }
                                if ui.small_button(icons::PLUS).on_hover_text("Upload file").clicked() {
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
                                .max_width(tools_height - 40.)
                                .show(ui, |ui| {
                                let entry_w = ui.available_width();
                                for (idx, tool) in self.my_tools.iter().enumerate() {
                                    let is_selected = self.selected_tool_idx == Some(idx);
                                    // 📜 / 📦 are supplementary-plane
                                    // emoji too — swap to BMP markers so
                                    // every entry reliably gets an icon
                                    // (◈ = text doc, ◆ = binary blob).
                                    let icon = if tool.is_text {
                                        icons::FILE_TEXT
                                    } else {
                                        icons::PACKAGE
                                    };
                                    let label = format!("{} {}", icon, tool.name);

                                    let response =
                                        sidebar_row(ui, entry_w, TOOL_ENTRY_H, is_selected, label);
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

                                        if ui.button("Copy to Client").clicked() {
                                            copy_to_client = Some(idx);
                                            ui.close();
                                        }

                                        ui.separator();

                                        if ui.button("Delete from My Tools").clicked() {
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
            
            ui.label(RichText::new(format!("{} {}", icons::FILE, filename)).strong());
            
            if self.preview.modified {
                ui.label(RichText::new(icons::STATUS_DOT).color(Color32::YELLOW));
            }
            
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.small_button(icons::CLOSE).on_hover_text("Close preview").clicked() {
                    self.preview_visible = false;
                    self.preview.clear();
                }
                
                // Save button for text content
                if self.preview.text_content.is_some() && self.preview.modified {
                    if ui.small_button(format!("{} Save", icons::SAVE)).clicked() {
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
            ui.colored_label(ui.style().visuals.error_fg_color, error.clone());
        } else if let Some(content) = &mut self.preview.text_content.clone() {
            // Text preview with editing
            ScrollArea::both()
                .id_salt("preview_text_scroll")
                .max_width(max_height - 30.)
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
        } 
        // Image preview (native only - texture field is cfg-gated)
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(texture) = &self.preview.texture {
            // Image preview - show at original size, scrollable
            ScrollArea::both()
                .id_salt("preview_image_scroll")
                .max_width(max_height - 30.)
                .show(ui, |ui| {
                    let tex_size = texture.size_vec2();
                    // Show at original size by default
                    ui.image((texture.id(), tex_size));
                });
        }
    }
    
    fn display_file_list(&mut self, ui: &mut Ui, cmd_tx: &Sender<Cmd>) {
        if self.loading {
            ui.vertical_centered(|ui| {
                ui.add_space(50.);
                ui.spinner();
                ui.label("Loading directory contents...");
            });
            return;
        }
        
        if self.file_table.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(50.);
                ui.label(RichText::new("Empty directory").heading());
                ui.add_space(10.);
                if ui.button("Refresh").clicked() {
                    self.refresh(cmd_tx);
                }
            });
            return;
        }

        // Drain actions dispatched by the RowViewer
        while let Ok(action) = self.action_rx.try_recv() {
            match action {
                ExplorerAction::Navigate(path) => {
                    self.navigate_to(path, cmd_tx);
                }
                ExplorerAction::Download(path) => {
                    let filename = std::path::Path::new(&path)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| "download".to_string());
                    self.pending_download = Some(filename);
                    let _ = cmd_tx.try_send(Cmd::DownloadRemoteFile(path));
                }
                ExplorerAction::Execute(path) => {
                    let _ = cmd_tx.try_send(Cmd::ExecuteRemoteFile(path));
                }
                ExplorerAction::PreviewText(path) => {
                    self.preview.loading = true;
                    let _ = cmd_tx.try_send(Cmd::PreviewRemoteFile(path));
                }
                ExplorerAction::PreviewImage(path) => {
                    self.preview.loading = true;
                    let _ = cmd_tx.try_send(Cmd::RequestThumbnail(path));
                }
                ExplorerAction::CopyToTools(path) => {
                    self.pending_tool_upload = Some((path.clone(), Vec::new()));
                    let _ = cmd_tx.try_send(Cmd::DownloadRemoteFile(path));
                }
                ExplorerAction::Delete(path) => {
                    let _ = cmd_tx.try_send(Cmd::FileSystemAction(
                        crate::FileSystemAction::Delete(path),
                    ));
                    self.refresh(cmd_tx);
                }
                ExplorerAction::Refresh => {
                    self.refresh(cmd_tx);
                }
            }
        }

        // Filter bar
        ui.horizontal(|ui| {
            ui.label("Filter:");
            egui::TextEdit::singleline(&mut self.file_viewer.filter)
                .hint_text("Search files...")
                .desired_width(ui.available_width() - 10.)
                .show(ui);
        });
        ui.add_space(2.);

        ScrollArea::horizontal()
            .auto_shrink(false)
            .show(ui, |ui|
                Renderer::new(&mut self.file_table, &mut self.file_viewer)
                    .with_style_modify(|s| {
                        s.scroll_bar_visibility = scroll_area::ScrollBarVisibility::AlwaysVisible;
                        s.single_click_edit_mode = true;
                        s.auto_shrink = [false, false].into();
                    })
                    .ui(ui)
            );
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

/* ========================== RowViewer implementation ========================== */

impl RowViewer<RemoteDirEntry> for RemoteFileRowViewer {
    fn try_create_codec(&mut self, _copy_full_row: bool) -> Option<impl RowCodec<RemoteDirEntry>> {
        Some(RemoteFileCodec)
    }

    fn num_columns(&mut self) -> usize { NUM_FILE_COLUMNS }

    fn column_name(&mut self, column: usize) -> std::borrow::Cow<'static, str> {
        ["", "Name", "Date Modified", "Size", "Type"][column].into()
    }

    fn is_sortable_column(&mut self, column: usize) -> bool { column > 0 }

    fn row_filter_hash(&mut self) -> &impl std::hash::Hash { &self.filter }

    fn filter_row(&mut self, row: &RemoteDirEntry) -> bool {
        if self.filter.trim().is_empty() { return true; }
        let f = self.filter.to_lowercase();
        row.name.to_lowercase().contains(&f)
    }

    fn hotkeys(&mut self, context: &UiActionContext) -> Vec<(KeyboardShortcut, UiAction)> {
        let hot = default_hotkeys(context);
        self.hotkeys.clone_from(&hot);
        hot
    }

    fn is_editable_cell(&mut self, _column: usize, _row: usize, _row_value: &RemoteDirEntry) -> bool { false }

    fn show_cell_view(&mut self, ui: &mut egui::Ui, row: &RemoteDirEntry, column: usize) {
        let style = ui.style_mut();
        style.interaction.multi_widget_text_select = false;
        style.interaction.selectable_labels = false;
        match column {
            0 => {
                let glyph = icons::file_icon(&row.name, row.is_directory);
                let color = if row.is_directory {
                    Color32::from_rgb(130, 170, 255)
                } else {
                    ui.style().visuals.text_color()
                };
                ui.label(icons::icon_sized(glyph, 14.0).color(color));
            }
            1 => {
                let name_color = if row.is_directory {
                    Color32::from_rgb(130, 170, 255)
                } else {
                    Color32::from_rgb(220, 220, 220)
                };
                
                ui.add(egui::Label::new(RichText::new(&row.name).color(name_color).underline()).sense(Sense::click()));
            }
            2 => {
                ui.label(
                    RichText::new(format_modified_date(row.modified.as_deref()))
                        .color(Color32::GRAY)
                        .small(),
                );
            }
            3 => {
                if row.is_directory {
                    ui.label("");
                } else if let Some(size) = row.size {
                    ui.label(RichText::new(format_file_size(size)).color(Color32::GRAY).small());
                } else {
                    ui.label("");
                }
            }
            4 => {
                if row.is_directory {
                    ui.label(RichText::new("Folder").color(Color32::from_rgb(130, 170, 255)).small());
                } else {
                    let ext = row.name.rsplit('.').next().unwrap_or("").to_lowercase();
                    ui.label(RichText::new(ext).color(Color32::GRAY).small());
                }
            }
            _ => {}
        }
    }

    fn show_cell_editor(
        &mut self,
        _ui: &mut egui::Ui,
        _row: &mut RemoteDirEntry,
        _column: usize,
    ) -> Option<egui::Response> {
        None
    }

    fn on_cell_view_response(
        &mut self,
        row: &RemoteDirEntry,
        column: usize,
        resp: &egui::Response,
    ) -> Option<Box<RemoteDirEntry>> {
        if let Some(tx) = &self.action_tx {
            if resp.clicked() {
                if !row.is_directory && is_text_file(&row.name) {
                    let _ = tx.try_send(ExplorerAction::PreviewText(row.path.clone()));
                } else if row.is_directory {
                    let _ = tx.try_send(ExplorerAction::Navigate(row.path.clone()));
                } else if is_image_file(&row.name) {
                    let _ = tx.try_send(ExplorerAction::PreviewImage(row.path.clone()));
                } else {
                    let _ = tx.try_send(ExplorerAction::Execute(row.path.clone()));
                }
            }
        }

        if column == 1 {
            resp.clone().on_hover_text(&row.path);
        }

        None
    }

    fn set_cell_value(&mut self, src: &RemoteDirEntry, dst: &mut RemoteDirEntry, column: usize) {
        match column {
            1 => dst.name = src.name.clone(),
            2 => dst.modified = src.modified.clone(),
            3 => dst.size = src.size,
            4 => dst.is_directory = src.is_directory,
            _ => {}
        }
    }

    fn compare_cell(&self, l: &RemoteDirEntry, r: &RemoteDirEntry, column: usize) -> std::cmp::Ordering {
        use std::cmp::Ordering::*;
        // Directories always sort before files regardless of column
        match (l.is_directory, r.is_directory) {
            (true, false) => return Less,
            (false, true) => return Greater,
            _ => {}
        }
        match column {
            0 => Equal,
            1 => l.name.to_lowercase().cmp(&r.name.to_lowercase()),
            2 => l.modified.cmp(&r.modified),
            3 => l.size.unwrap_or(0).cmp(&r.size.unwrap_or(0)),
            4 => {
                let l_ext = l.name.rsplit('.').next().unwrap_or("").to_lowercase();
                let r_ext = r.name.rsplit('.').next().unwrap_or("").to_lowercase();
                l_ext.cmp(&r_ext)
            }
            _ => Equal,
        }
    }

    fn new_empty_row(&mut self) -> RemoteDirEntry {
        RemoteDirEntry {
            name: String::new(),
            path: String::new(),
            is_directory: false,
            size: None,
            modified: None,
        }
    }

    fn column_render_config(&mut self, column: usize, _is_editing: bool) -> TableColumnConfig {
        let base = TableColumnConfig::auto();
        match column {
            0 => base.at_least(30.).at_most(35.),
            1 => TableColumnConfig::remainder().at_least(120.).clip(true).resizable(true),
            2 => base.at_least(140.).at_most(160.).resizable(true),  // modified
            3 => base.at_least(80.).at_most(100.),                   // size
            4 => base.at_least(60.).at_most(80.),                    // type
            _ => base,
        }
    }

    fn custom_context_menu_items(
        &mut self,
        _context: &UiActionContext,
        selection: &SelectionSnapshot<'_, RemoteDirEntry>,
    ) -> Vec<CustomMenuItem> {
        let has_selection = !selection.selected_rows.is_empty();
        let first_is_dir = selection.selected_rows.first().map(|(_, r)| r.is_directory).unwrap_or(false);
        let first_is_text = selection.selected_rows.first().map(|(_, r)| is_text_file(&r.name)).unwrap_or(false);
        let first_is_image = selection.selected_rows.first().map(|(_, r)| is_image_file(&r.name)).unwrap_or(false);

        let mut items = Vec::new();

        if first_is_dir {
            items.push(CustomMenuItem::new("open_dir", "Open").icon(icons::FOLDER_OPEN).enabled(has_selection));
        } else {
            items.push(CustomMenuItem::new("execute", "Execute / Open").icon(icons::PLAY).enabled(has_selection));
            items.push(CustomMenuItem::new("download", "Download").icon(icons::DOWNLOAD).enabled(has_selection));
            if first_is_text {
                items.push(CustomMenuItem::new("preview_text", "Preview").icon(icons::EYE).enabled(true));
            }
            if first_is_image {
                items.push(CustomMenuItem::new("preview_image", "View Thumbnail").icon(icons::IMAGE).enabled(true));
            }
            items.push(CustomMenuItem::new("copy_to_tools", "Copy to My Tools").icon(icons::UPLOAD).enabled(has_selection));
        }
        items.push(CustomMenuItem::new("delete", "Delete").icon(icons::CLOSE).enabled(has_selection));
        items.push(CustomMenuItem::new("refresh", "Refresh").icon(icons::REFRESH).enabled(true));
        items
    }

    fn on_custom_action_ex(
        &mut self,
        action_id: &'static str,
        ctx: &CustomActionContext<'_, RemoteDirEntry>,
        _editor: &mut CustomActionEditor<RemoteDirEntry>,
    ) {
        let Some(tx) = &self.action_tx else { return };
        let first = ctx.selection.selected_rows.first().map(|(_, r)| r);

        match action_id {
            "open_dir" => {
                if let Some(row) = first {
                    let _ = tx.try_send(ExplorerAction::Navigate(row.path.clone()));
                }
            }
            "execute" => {
                if let Some(row) = first {
                    let _ = tx.try_send(ExplorerAction::Execute(row.path.clone()));
                }
            }
            "download" => {
                for (_, row) in ctx.selection.selected_rows.iter() {
                    if !row.is_directory {
                        let _ = tx.try_send(ExplorerAction::Download(row.path.clone()));
                    }
                }
            }
            "preview_text" => {
                if let Some(row) = first {
                    let _ = tx.try_send(ExplorerAction::PreviewText(row.path.clone()));
                }
            }
            "preview_image" => {
                if let Some(row) = first {
                    let _ = tx.try_send(ExplorerAction::PreviewImage(row.path.clone()));
                }
            }
            "copy_to_tools" => {
                if let Some(row) = first {
                    let _ = tx.try_send(ExplorerAction::CopyToTools(row.path.clone()));
                }
            }
            "delete" => {
                for (_, row) in ctx.selection.selected_rows.iter() {
                    let _ = tx.try_send(ExplorerAction::Delete(row.path.clone()));
                }
            }
            "refresh" => {
                let _ = tx.try_send(ExplorerAction::Refresh);
            }
            _ => {}
        }
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
