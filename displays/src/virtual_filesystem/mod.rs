use eframe::egui::{collapsing_header::CollapsingState, popup_below_widget, Align, CentralPanel, Color32, Direction, Frame, Id, Key, Layout, Margin, PopupCloseBehavior::CloseOnClickOutside, ProgressBar, RichText, ScrollArea, SidePanel, Stroke, TextEdit, TopBottomPanel, Ui, Vec2, Widget};
use rusty_s3::{Bucket, Credentials, S3Action, actions::{CompleteMultipartUpload, CreateMultipartUpload, UploadPart, GetObject}};
use crate::{channel_manager::ChannelManager, file_viewer::{FileViewer, ColorTheme, Syntax}, FileSystemAction, Spawner};
use database::schema::{buckets::{list_buckets, normalize_prefix}, Node, User, file_storage}; 
use reqwest::{header::{CONTENT_LENGTH, CONTENT_TYPE, ETAG}, Client, Url};
use std::{cell::RefCell, collections::{HashMap, HashSet}};
use crossbeam::channel::{Receiver, Sender};
use futures::{StreamExt, Future};
use zstd::zstd_safe::WriteBuf;
use anyhow::{Result, Error};
use crate::PlatformSpawner;
use mime_guess::from_path;
use database::STORAGE_URL;
use uuid::Uuid;
use rfd::FileHandle;
use bytes::Bytes;
use std::iter;
use log::info;
#[cfg(feature="tokio")]
use std::path::PathBuf;

pub const ONE_HOUR: web_time::Duration = web_time::Duration::from_secs(3600);

/// Storage backend type
#[derive(Debug, Clone, Default, PartialEq)]
pub enum StorageBackend {
    /// Use SurrealDB file storage (recommended for new implementations)
    #[default]
    SurrealDb,
    /// Use S3/MinIO storage (legacy)
    S3,
}

/// Fetcher implementation for SurrealDB file storage
#[derive(Debug, Clone)]
pub struct SurrealDbFetcher {
    bucket_name: String,
}

impl SurrealDbFetcher {
    /// Creates a new `SurrealDbFetcher` for the given bucket (typically the username)
    pub fn new(bucket_name: &str) -> Self {
        Self {
            bucket_name: bucket_name.to_string(),
        }
    }
    
    /// Request contents of a directory from SurrealDB bucket
    pub async fn request_bucket_contents(&self, prefix: &str) -> anyhow::Result<Node, anyhow::Error> {
        let entries = file_storage::list_files(&self.bucket_name, prefix).await?;
        
        // Build a Node tree from the file entries
        let mut children: HashMap<String, Node> = HashMap::new();
        
        for entry in entries {
            let key = entry.key.clone();
            // Extract the file/folder name from the full path
            let name = key.rsplit('/').next().unwrap_or(&key).to_string();
            
            if entry.is_directory {
                children.insert(name.clone(), Node::Folder(key, HashMap::new()));
            } else {
                children.insert(name.clone(), Node::File((key.clone(), name)));
            }
        }
        
        let folder_path = if prefix.is_empty() { "/" } else { prefix };
        Ok(Node::Folder(folder_path.to_string(), children))
    }
    
    /// Upload a file to SurrealDB bucket
    pub async fn upload_file(&self, path: &str, data: Vec<u8>) -> anyhow::Result<(), anyhow::Error> {
        file_storage::put_file(&self.bucket_name, path, data).await
    }
    
    /// Download a file from SurrealDB bucket
    pub async fn download_file(&self, path: &str) -> anyhow::Result<Option<Vec<u8>>, anyhow::Error> {
        file_storage::get_file(&self.bucket_name, path).await
    }
    
    /// Delete a file from SurrealDB bucket
    pub async fn delete_file(&self, path: &str) -> anyhow::Result<(), anyhow::Error> {
        file_storage::delete_file(&self.bucket_name, path).await
    }
    
    /// Check if a file exists
    pub async fn file_exists(&self, path: &str) -> anyhow::Result<bool, anyhow::Error> {
        file_storage::file_exists(&self.bucket_name, path).await
    }
}


// /// Fetcher implementation for S3.
#[derive(Debug, Clone)]
pub struct S3Fetcher {
    bucket: Bucket,
    credentials: Credentials,
}

impl S3Fetcher {
    /// Creates a new `S3Fetcher`.
    pub fn new(access_key: &str, secret_key: &str, bucket_name: &str) -> Self {
        let bucket = Bucket::new(
            STORAGE_URL.parse::<reqwest::Url>().unwrap(),
            rusty_s3::UrlStyle::Path,
            bucket_name.to_lowercase(),
            database::REGION
        ).unwrap();
        
        let credentials = Credentials::new(access_key.to_string(), secret_key.to_string());
        
        Self { bucket, credentials }
    }

    pub async fn request_bucket_contents(&mut self, prefix: &str) -> anyhow::Result<Node, anyhow::Error> {
        let res = list_buckets(self.credentials.clone(), self.bucket.clone(), Some(&prefix)).await?;
        Ok(res)
    }

}

pub trait FileSysHelper {
    fn handle_filesystem_action(&mut self, action: &FileSystemAction);
}

impl FileSysHelper for FileSystem {
    fn handle_filesystem_action(&mut self, action: &FileSystemAction) {
        log::warn!("FileSysHelper for FileSystem -> Action -> {action:?}");
        match action {
            FileSystemAction::Execute(label) => { self.execute_file = label.clone(); },
            FileSystemAction::Select((modifiers, label)) => {
                if self.selected_items.borrow().contains(label) {
                    // If the item was already selected, deselect it
                    self.selected_items.borrow_mut().remove(label);
                } 
                if modifiers.ctrl { 
                    self.selected_items.borrow_mut().insert(label.clone());
                } else { // If the control key is not down, clear previous selection and select the current item
                    self.selected_items.borrow_mut().clear();
                    self.selected_items.borrow_mut().insert(label.clone());
                }
                self.preview_selection(label.clone());
            },
            FileSystemAction::EnterDirectory(directory) => {
                if cfg!(target_os="linux") && !self.current_prefix.ends_with('/'){
                    self.current_prefix.push('/');
                } else if cfg!(target_os="windows") && !self.current_prefix.ends_with('\\') {
                    self.current_prefix.push('\\');
                }
                info!("directory double clicked: {directory:?}");
                self.double_click_folder(&directory);
            }
            FileSystemAction::ExpandDirectory(directory) => self.expand_folder(&directory),
            FileSystemAction::NavigateHome => {
                self.navigation_stack.clear();
                self.current_prefix.clear();
            },
            FileSystemAction::GetNode(new_node) => {
                log::info!("GetNode");
                let insert_node = self.insert_node(new_node.clone());
                info!("InsertNode: {insert_node:?}");
            },
            FileSystemAction::RequestNewContents(folder_prefix) => {
                let _ = self.request_contents(folder_prefix);
            },
            FileSystemAction::Delete(file_path) => {
                self.delete_selection(file_path.clone());
            },
            _ => {}
        }
    }
}

pub trait ClonableFileSysHelper: FileSysHelper {
    fn clone_box(&self) -> Box<dyn ClonableFileSysHelper>;
}

impl<T> ClonableFileSysHelper for T
where
    T: 'static + FileSysHelper + Clone, // + Send,
{
    fn clone_box(&self) -> Box<dyn ClonableFileSysHelper> {
        Box::new(self.clone())
    }
}

impl Clone for FileSystem {
    fn clone(&self) -> Self {
        FileSystem {
            helper_delegate: self.helper_delegate.as_ref().map(|helper| helper.clone_box()),
            scroll_id: self.scroll_id.clone(),
            root: self.root.clone(),
            bytes_tx: self.bytes_tx.clone(),
            bytes_rx: self.bytes_rx.clone(),
            fs_actions_channel: self.fs_actions_channel.clone(),
            paths_channel: self.paths_channel.clone(),
            selected_items: self.selected_items.clone(),
            paths: self.paths.clone(),
            total_size: self.total_size.clone(),
            progress: self.progress.clone(),
            execute_file: self.execute_file.clone(),
            user: self.user.clone(),
            current_prefix: self.current_prefix.clone(),
            navigation_stack: self.navigation_stack.clone(),
            current_action: self.current_action.clone(),
            file_preview_channel: self.file_preview_channel.clone(),
            previewed_file: self.previewed_file.clone(),
            file_editor: self.file_editor.clone(),
            storage_backend: self.storage_backend.clone(),
        }
    }
    
    fn clone_from(&mut self, source: &Self) {
        *self = source.clone()
    }
}

pub struct FileSystem {
    /// Persistent Scroll ID so we dont have any 
    /// clashes of ID's between multiple websocket clients
    pub scroll_id: Id,
    /// The Entire Hierarchy of Folders/Files
    pub root: Node,
    /// Sending progress of downloads/uploads to progress bar
    bytes_tx: Sender<(u64, u64)>,
    /// Receiving progress of downloads/uploads to progress bar
    pub bytes_rx: Receiver<(u64, u64)>,
    /// Receive / Send FileSystemAction's from the UI
    pub fs_actions_channel: (Sender<FileSystemAction>, Receiver<FileSystemAction>),
    pub paths_channel: (Sender<Node>, Receiver<Node>),
    pub file_preview_channel: (Sender<String>, Receiver<String>),
    /// Selected files/folders
    pub selected_items: RefCell<HashSet<String>>,
    /// All of our paths
    pub paths: Vec<String>,
    /// Total size of download/upload
    pub total_size: f32,
    /// Progress bar for file/folder download/upload
    pub progress: f32,
    /// File to execute on Mastertech
    pub execute_file: String,
    /// Credentials for API calls to Minio (legacy) or username for SurrealDB
    pub user: User,
    /// Editable Current path used by a 
    /// TextEdit for manual navigation
    pub current_prefix: String,
    /// Stack to track navigation history
    pub navigation_stack: Vec<String>,
    pub current_action: Option<FileSystemAction>,
    pub helper_delegate: Option<Box<dyn ClonableFileSysHelper>>,
    pub previewed_file: Option<String>,
    pub file_editor: FileViewer,
    /// Storage backend to use (SurrealDB or S3)
    pub storage_backend: StorageBackend,
}

impl FileSystem {
    pub fn new() -> Self {
        let (bytes_tx, bytes_rx) = crossbeam::channel::unbounded();
        let fs_actions_channel = <FileSystemAction>::create_unbounded_channel();
        let paths_channel = <Node>::create_unbounded_channel();
        let file_preview_channel = <String>::create_unbounded_channel();
        let file_editor = FileViewer::default()
            .id_source("Script Editor")
            .with_rows(48)
            .vscroll(true)
            .auto_shrink(false)
            .with_fontsize(14.0)
            .with_theme(ColorTheme::TOKYO_DARK)
            .with_syntax(Syntax::powershell())
            .with_numlines(true);

        Self {
            scroll_id: Id::new(format!("virtual_fs_scrollarea-{}", Uuid::new_v4())),
            bytes_tx, bytes_rx,
            fs_actions_channel,
            file_preview_channel,
            paths_channel,
            root: Node::Folder(String::new(), HashMap::new()),
            selected_items: RefCell::new(HashSet::new()),
            progress: 0.0,
            total_size: 0.0,
            paths: Vec::new(),
            execute_file: String::new(),
            user: User::default(),
            current_prefix: String::new(),
            navigation_stack: Vec::new(),
            current_action: None,
            helper_delegate: None,
            previewed_file: Default::default(),
            file_editor,
            storage_backend: StorageBackend::default(),
        }
    }
    
    /// Create a new FileSystem with SurrealDB backend
    pub fn new_surrealdb() -> Self {
        let mut fs = Self::new();
        fs.storage_backend = StorageBackend::SurrealDb;
        fs
    }
    
    /// Create a new FileSystem with S3 backend (legacy)
    pub fn new_s3() -> Self {
        let mut fs = Self::new();
        fs.storage_backend = StorageBackend::S3;
        fs
    }
    
    /// Set the storage backend
    pub fn with_backend(mut self, backend: StorageBackend) -> Self {
        self.storage_backend = backend;
        self
    }

    pub fn receive(&mut self) { // , ctx: &Context
        if let Ok(new_node) = self.paths_channel.1.try_recv() {
            info!("Filesystem received a new node");
            let _ = self.insert_node(new_node);
        }

        while let Ok(x) = self.bytes_rx.try_recv() {
            self.progress = x.0 as f32;
            self.total_size = x.1 as f32;

            log::info!("X: {x:?}");
            if self.progress > 0.0 && self.progress >= self.total_size {
                self.progress = 0.0;
                self.total_size = 0.0;
            }
        }

        if let Ok(action) = self.fs_actions_channel.1.try_recv() {
            if let Some(helper) = self.helper_delegate.as_mut() {
                info!("virtual_filesystem -> Using helper delegate to handle_filesystem_action");
                helper.handle_filesystem_action(&action);
            } else {
                info!("virtual_filesystem -> Using Self to handle_filesystem_action");
                self.handle_filesystem_action(&action);
            }
            
            self.current_action = Some(action);
            // ctx.request_repaint();
        }

        if let Ok(file_contents) = self.file_preview_channel.1.try_recv() {
            self.previewed_file = Some(file_contents);
        }
    
        // if self.selected_items.borrow().is_empty() {
        //     self.previewed_file = None;
        // }
    }
    
    pub fn set_user(&mut self, user: User) -> &mut Self {
        self.user = user;
        self
    }

    /// This is to build an actual filesystem structure for when we are working with Mastertech from the website
    /// - Builds a 'virtual' filesystem since wasm doesnt know anything about PathBuf's. This is used in 
    /// mastertech to build the actual filesystem hierarchy, then builds a 'Node' out of it, serializes it,
    /// then sends it over websocket to the website
    #[cfg(feature="tokio")]
    pub fn build_virtual_file_system(&mut self, base_path: PathBuf, paths: Vec<PathBuf>) -> Node {
        let mut root = Node::Folder(base_path.display().to_string(), HashMap::new());
        for path in paths {
            let mut current = &mut root;
            let mut current_path = base_path.clone();
    
            for part in path.iter().skip(current_path.components().count()) {
                let part_str = part.to_str().unwrap(); // safely assuming valid Unicode data
                current_path.push(part);
    
                if current_path.is_dir() {
                    if let Node::Folder(_, folder) = current {
                        let folder_path = current_path.display().to_string();
                        current = folder.entry(part_str.to_string()).or_insert_with(|| Node::Folder(folder_path, HashMap::new()));
                    }
                } else if current_path.is_file() {
                    if let Node::Folder(full_path, folder) = current {
                        let file_full_path = format!("{}/{}", full_path, part_str);
                        folder.insert(part_str.to_string(), Node::File((file_full_path, part_str.to_string())));
                    }
                }
            }
        }

        root 
    }

    pub fn display(&mut self, ui: &mut Ui){
        let size = ui.available_size_before_wrap();
        let mut inner_margin_top = Margin::default();
        inner_margin_top.bottom = 5;

        let btm_panel_frame = Frame::default()
            .inner_margin(inner_margin_top)
            .corner_radius(eframe::egui::CornerRadius::same(10));

        let top_panel_frame = Frame::default()
            .outer_margin(Margin::symmetric(5, 2));

        let panel_frame = Frame::default()
            .fill(Color32::from_rgb(12, 12, 14))
            .inner_margin(Margin::same(6))
            .corner_radius(eframe::egui::CornerRadius::same(10))
            .stroke(Stroke::new(1.0, Color32::from_additive_luminance(50)));
        
        ui.style_mut().spacing.button_padding = Vec2::new(10.0, 3.0);


        TopBottomPanel::top("FileBrowserTop")
            .frame(top_panel_frame)
            // .show_separator_line(false)
            .exact_height(50.)
            .show_inside(ui, |ui| 
        {
            ui.vertical_centered(|ui| {
                let pre_modified_path = self.current_prefix.clone();
                let response = TextEdit::singleline(&mut self.current_prefix)
                    .desired_width(size.x/1.2)
                    .ui(ui);

                if response.lost_focus() || ui.input(|i| i.key_pressed(Key::Enter)) {
                    info!("Lost focus on self.current_prefix TextEdit: {pre_modified_path} // curr {}", self.current_prefix);
                    let _ = self.fs_actions_channel.0.try_send(FileSystemAction::EnterDirectory(self.current_prefix.clone()));
                }

                ui.with_layout(Layout::right_to_left(eframe::egui::Align::Center), |ui| {
                    ui.add_space(5.);

                    let force_refresh = ui.button(RichText::new("⟲").heading()).on_hover_text("Refresh Current Directory Contents");
                    if force_refresh.clicked() {
                        let _ = self.fs_actions_channel.0.try_send(FileSystemAction::RequestNewContents(self.current_prefix.clone()));
                    }

                    ui.add_space(5.);
    
                    let home_res = ui.button(RichText::new("🏠").heading()).on_hover_text("Home");
                    if home_res.clicked(){
                        info!("Home clicked. root: {:?}", self.root);
                        let send = self.fs_actions_channel.0.try_send(FileSystemAction::NavigateHome);
                        info!("Sending FS Action: {send:?}");
                    }
    
                    ui.add_space(5.);

                    let parent_res = ui.button(RichText::new("⬆").heading()).on_hover_text("Parent Folder");
                    if parent_res.clicked() {
                        let navigate_up = self.navigate_up();
                        info!("Navigating up: {navigate_up:?}");
                    }
                    ui.add_space(5.);
                });
            });
        });

        TopBottomPanel::bottom("FileBrowserBottom")
            .frame(btm_panel_frame)
            .show_separator_line(false)
            .show_inside(ui, |ui| 
                ui.vertical_centered(|ui | 
                    self.show_progress(ui)
                )
            );

        if self.selected_items.borrow().is_empty() {
            self.previewed_file = None;
        }

        if let Some(file) = self.previewed_file.as_mut() {
            if size.x > 1000. {
                SidePanel::right(Id::new("FileBrowserSidePanel"))
                    .default_width(ui.available_width()/2.0)
                    .show_inside(ui, |ui| 
                {
                    self.file_editor.show(ui, file);
                });
            } else {
                TopBottomPanel::bottom(Id::new("FileBrowserBottomPanel"))
                    .default_height(ui.available_height()/2.0)
                    .show_inside(ui, |ui| 
                {
                    self.file_editor.show(ui, file);
                });
            }
        }
        ui.add_space(5.0);

        CentralPanel::default().frame(panel_frame)
            .show_inside(ui, |ui| 
        {
            ScrollArea::vertical()
                .id_salt(self.scroll_id)
                .max_width(size.x)
                .max_height(size.y)
                .auto_shrink(false)
                .show(ui, |ui| 
            {
                ui.with_layout(Layout::from_main_dir_and_cross_align(Direction::TopDown, Align::Center), |ui| {
                    self.display_directory_contents(
                        ui, 
                        self.get_current_folder().unwrap_or(&self.root)
                    );
                });
            });
        });
        
    }

    pub fn display_directory_contents(&self, ui: &mut Ui, node: &Node){
        let tx = self.fs_actions_channel.0.clone();
        ui.vertical(|ui| 
        {
            let mut count = 0;
            if let Node::Folder(_, children) = node {
                // Collect entries into a vector for sorting
                let mut entries: Vec<(&String, &Node)> = children.iter().collect();
                entries.sort_by(|a, b| {
                    let a_is_dir = matches!(a.1, Node::Folder(_, _));
                    let b_is_dir = matches!(b.1, Node::Folder(_, _));
                    match a_is_dir == b_is_dir {
                        true => a.0.cmp(b.0), // Sort alphabetically if both are files or both are directories
                        false => b_is_dir.cmp(&a_is_dir), // Directories first
                    }
                });

                for (label, node) in entries {
                    let modifiers = ui.input(|i| i.modifiers); // Get the current modifiers

                    if let Node::Folder(full_path, _) = node {
                        count+=1;
                        
                        let id = ui.make_persistent_id(format!("virtual_fs_collapsing-header-{:?}-{}-{}", self.scroll_id, label, count));

                        let res = CollapsingState::load_with_default_open(
                            ui.ctx(), 
                            id, 
                            false
                        )
                        .show_header(ui, |ui| 
                        {
                            let is_selected = self.selected_items.borrow().contains(label);
                            let selectable_label = ui.selectable_label(is_selected, RichText::new(format!("🗀   {}", label)));

                            if selectable_label.clicked() { // If the item was already selected, deselect it
                                let _ = tx.try_send(FileSystemAction::Select((modifiers, normalize_prefix(full_path))));
                            }

                            if selectable_label.double_clicked(){
                                let _ = tx.try_send(FileSystemAction::EnterDirectory(full_path.clone()));
                            }

                            if selectable_label.secondary_clicked(){
                                ui.memory_mut(|mem| mem.open_popup(
                                    ui.make_persistent_id(format!("sub_menu-{:?}", full_path))
                                ));
                            }

                            popup_below_widget(
                                ui, 
                                ui.make_persistent_id(format!("sub_menu-{:?}", full_path)), 
                                &selectable_label, 
                                CloseOnClickOutside, 
                                |ui| 
                            {
                                ui.vertical_centered_justified(|ui| {
                                    ui.set_width(200.0);

                                    if ui.button("Download").clicked(){
                                        info!("Path: {:?}", full_path.clone());
                                        self.download_selection(full_path.to_string(), label.clone());
                                    }

                                    ui.add_space(5.0);

                                    if ui.button("Upload").clicked(){
                                        info!("Dir: {:?}", full_path.clone());
                                        self.upload(full_path.to_string());
                                    }

                                    ui.add_space(5.0);

                                    if ui.button("Upload Folder").clicked(){
                                        // if cfg!(target_os="windows") || cfg!(target_os="linux"){
                                        //     #[cfg(target_os="windows")]
                                        //     self.upload_folder(dir);
                                        // }
                                    }
                                }).inner;
                            });
                        })
                        .body(|ui| 
                            self.display_directory_contents(ui, &node)
                        );

                        if res.0.clicked() {
                            let _ = tx.try_send(FileSystemAction::ExpandDirectory(full_path.clone()));
                        }

                        if res.0.secondary_clicked(){
                            ui.memory_mut(|mem| mem.open_popup(
                                ui.make_persistent_id(format!("upload_file_menu"))
                            ));
                        }

                        popup_below_widget(
                            ui, 
                            ui.make_persistent_id(format!("upload_file_menu")), 
                            &res.0, 
                            CloseOnClickOutside, 
                            |ui| 
                        {
                            ui.vertical_centered_justified(|ui| {
                                ui.set_width(200.0);

                                if ui.button("Upload").clicked(){
                                    // self.upload(dir);
                                }

                                ui.add_space(5.0);

                                if ui.button("Upload Folder").clicked(){
                                    // self.upload_folder(dir);
                                }
                            }).inner;
                        });

                    } else if let Node::File((full_path, label)) = node {

                        // let id = ui.make_persistent_id(format!("sub_menu-{:?}", full_path));
                        let file_selected = self.selected_items.borrow().contains(full_path);
                        let selectable_label = ui.selectable_label(file_selected, RichText::new(format!("🗋   {}", label)));

                        if selectable_label.clicked() {
                            let path = normalize_prefix(full_path);
                            let _ = tx.try_send(FileSystemAction::Select((modifiers, path.clone())));
                        }

                        if selectable_label.secondary_clicked(){
                            ui.memory_mut(|mem| mem.open_popup(
                                ui.make_persistent_id(format!("sub_menu-{:?}", full_path))
                            ));
                        }

                        if selectable_label.double_clicked(){
                            let _ = tx.try_send(FileSystemAction::Execute(full_path.clone()));
                        }

                        popup_below_widget(
                            ui, 
                            ui.make_persistent_id(format!("sub_menu-{:?}", full_path)), 
                            &selectable_label, 
                            CloseOnClickOutside, 
                            |ui| 
                        {
                            ui.vertical_centered_justified(|ui| {
                                ui.set_width(200.0);
                                if ui.button("Download").clicked(){
                                    self.download_selection(full_path.clone(), label.clone());
                                }

                                ui.add_space(5.0);

                                if ui.button("Copy To Client").clicked(){
                                    let _ = tx.try_send(FileSystemAction::CopyToClient(full_path.clone()));
                                }

                                ui.add_space(5.0);

                                if ui.button("Copy From Client").clicked(){
                                    let _ = tx.try_send(FileSystemAction::CopyFromClient(full_path.clone()));
                                }

                                ui.add_space(5.0);

                                if ui.button("Delete File").clicked(){
                                    info!("Deleting file: {full_path}-{label}");
                                    let _ = tx.try_send(FileSystemAction::Delete(full_path.clone()));
                                }
                            }).inner
                        });
                    };
                }
            }
        });
    }

    pub fn request_contents(&self, folder_prefix: &str) -> Result<(), Error> {
        let folder_pref = folder_prefix.trim_start_matches('/').to_string();
        let tx = self.paths_channel.0.clone();
        let name = self.user.get_username().to_string();
        let backend = self.storage_backend.clone();
        
        match backend {
            StorageBackend::SurrealDb => {
                PlatformSpawner::spawn(async move {
                    let fetcher = SurrealDbFetcher::new(&name);
                    match fetcher.request_bucket_contents(&folder_pref).await {
                        Ok(node) => { let _ = tx.send(node); },
                        Err(e) => log::warn!("Error getting node from SurrealDB: {e:?}"),
                    }
                });
            }
            StorageBackend::S3 => {
                let access_key = self.user.get_minio_access_key().unwrap_or_default();
                let secret_key = self.user.get_minio_secret_key().unwrap_or_default();
                PlatformSpawner::spawn(async move {
                    let mut s3_fetcher = S3Fetcher::new(&access_key, &secret_key, &name);
                    match s3_fetcher.request_bucket_contents(&folder_pref).await {
                        Ok(node) => { let _ = tx.send(node); },
                        Err(e) => log::warn!("Error getting node from S3: {e:?}"),
                    }
                });
            }
        }

        Ok(())
    }

    pub fn expand_folder(&self, folder_prefix: &str) {
        let normalized_prefix = normalize_prefix(folder_prefix);
        let _ = self.fs_actions_channel.0.try_send(FileSystemAction::RequestNewContents(normalized_prefix));
    }

    pub fn double_click_folder(&mut self, folder_prefix: &str) {
        let normalized_prefix = normalize_prefix(folder_prefix);
    
        // Navigate to the new prefix
        self.navigate_to(normalized_prefix.clone());
    
        // Check if the folder's contents have already been fetched
        if let Some(Node::Folder(_, children)) = self.root.find_folder(&normalized_prefix) {
            if !children.is_empty() && children.len() > 1 {
                info!("Folder '{}' already fetched. No need to re-fetch.", normalized_prefix);
            } else {
                let _ = self.fs_actions_channel.0.try_send(FileSystemAction::RequestNewContents(normalized_prefix.clone()));
            }
        } else {
            let _ = self.fs_actions_channel.0.try_send(FileSystemAction::RequestNewContents(normalized_prefix));
        }
    }
    
    /// Merges a new `Node` into the existing file system.
    ///
    /// - **new_node**: The `Node` fetched from `list_directory` to merge.
    ///
    /// Returns `Ok(())` on success or an error if the merge fails.
    pub fn insert_node(&mut self, new_node: Node) -> Result<(), Error> {
        self.root.merge_node(new_node)
    }

    /// Sets the current view to the specified prefix.
    ///
    /// - **prefix**: The folder prefix to navigate to.
    ///
    /// This method updates `current_prefix` and manages the navigation stack.
    pub fn navigate_to(&mut self, prefix: String) {
        let normalized_prefix = normalize_prefix(&prefix);
    
        if normalized_prefix != self.current_prefix {
            if !self.current_prefix.is_empty()
                && self.current_prefix != "/"
                && !self.current_prefix.ends_with(":/")
            {
                self.navigation_stack.push(self.current_prefix.clone());
            }
            self.current_prefix = normalized_prefix;
            info!("Navigated to prefix: '{}'", self.current_prefix);
        } else {
            info!("Attempted to navigate to the same prefix: '{}'. No action taken.", normalized_prefix);
        }
    }
    
    /// Navigates back to the previous directory.
    ///
    /// Returns `Ok(true)` if navigation was successful, `Ok(false)` if already at root,
    /// or an error if something goes wrong.
    pub fn navigate_up(&mut self) -> Result<bool, Error> {
        info!(
            "self.navigation_stack: {:?}\nself.current_prefix: {}",
            self.navigation_stack, self.current_prefix
        );
    
        if let Some(previous_prefix) = self.navigation_stack.pop() {
            info!(
                "previous_prefix: '{previous_prefix}' -- self.current_prefix: {}",
                self.current_prefix
            );
            self.current_prefix = previous_prefix;
            info!("Navigated up to prefix: '{}'", self.current_prefix);
            Ok(true)
        } else if !self.current_prefix.is_empty() && self.current_prefix != "/" {
            info!(
                "Stack is empty, but navigating up to root from '{}'.",
                self.current_prefix
            );
    
            // Remove trailing slash first
            if self.current_prefix.ends_with('/') {
                self.current_prefix.pop();
            }
    
            // Truncate to the last '/'
            if let Some(pos) = self.current_prefix.rfind('/') {
                self.current_prefix.truncate(pos);
            } else {
                // If no `/` is found, directly set to "/"
                self.current_prefix = "/".to_string();
            }
    
            info!("Navigated up to root: '{}'", self.current_prefix);
            Ok(true)
        } else {
            info!(
                "Already at root directory. Cannot navigate up. {}",
                self.current_prefix
            );
            Ok(false)
        }
    }
    
    /// Retrieves the `Node::Folder` corresponding to `current_prefix`.
    ///
    /// Returns a reference to the folder node if found.
    pub fn get_current_folder(&self) -> Option<&Node> {
        self.root.find_folder(&self.current_prefix)
    }

    /// Retrieves a mutable reference to the `Node::Folder` corresponding to `current_prefix`.
    ///
    /// Returns a mutable reference to the folder node if found.
    pub fn get_current_folder_mut(&mut self) -> Option<&mut Node> {
        let node = self.root.find_or_create_folder_mut(&self.current_prefix);
        info!("Node? {node:?}");
        Some(node)
    }

    pub fn show_progress(&mut self, ui: &mut Ui) {
        if self.progress.round() == self.total_size.round()
            // || (self.progress / self.total_size).round() == 1.0
        {
            self.progress = 0.0;
            self.total_size = 0.0;
        }

        if self.progress > 0. {
            ProgressBar::new(self.progress / self.total_size)
                .show_percentage()
                .fill(
                    Color32::from_rgba_premultiplied(50, 10, 50, 65)
                )
                .ui(ui);
        }
    }

    pub fn upload(&self, path: String) {
        let task = rfd::AsyncFileDialog::new().pick_files();
        let name = self.user.get_username().to_string();
        let backend = self.storage_backend.clone();

        match backend {
            StorageBackend::SurrealDb => {
                PlatformSpawner::spawn(async move {
                    if let Some(files) = task.await {
                        let fetcher = SurrealDbFetcher::new(&name);
                        for file_handle in files {
                            let file_name = file_handle.file_name();
                            let file_path = if path.ends_with('/') {
                                format!("{}{}", path, file_name)
                            } else {
                                format!("{}/{}", path, file_name)
                            };
                            let data = file_handle.read().await;
                            match fetcher.upload_file(&file_path, data).await {
                                Ok(_) => info!("Uploaded {} to SurrealDB", file_path),
                                Err(e) => log::error!("Error uploading to SurrealDB: {e:?}"),
                            }
                        }
                    }
                });
            }
            StorageBackend::S3 => {
                let access_key = self.user.get_minio_access_key().unwrap_or_default();
                let secret_key = self.user.get_minio_secret_key().unwrap_or_default();
                PlatformSpawner::spawn(async move {
                    let result = Self::perform_upload(
                        &name.clone(),
                        &access_key.clone(),
                        &secret_key.clone(),
                        path.clone(),
                        task
                    ).await;
                    info!("Result: {result:?}");
                });
            }
        }
    }

    #[cfg(feature="tokio")]
    pub fn upload_folder(&self, _path: String) {
        let _task = rfd::AsyncFileDialog::new().pick_folders();
        let _access_key = self.user.get_minio_access_key().unwrap_or_default();
        let _secret_key = self.user.get_minio_secret_key().unwrap_or_default();
        let _name = self.user.get_username().to_string();
        // tokio::spawn(async move {
        //     let result = Self::perform_upload(
        //         &name.clone(),
        //         &access_key.clone(),
        //         &secret_key.clone(),
        //         &path.clone(),
        //         task
        //     ).await;
        //     info!("Result: {result:?}");
        // });
    }

    fn download_selection(&self, path: String, filename: String) {
        let task = rfd::AsyncFileDialog::new().set_file_name(filename.clone()).save_file();
        let tx = self.bytes_tx.clone();
        let name = self.user.get_username().to_string();
        let backend = self.storage_backend.clone();
        
        match backend {
            StorageBackend::SurrealDb => {
                PlatformSpawner::spawn(async move {
                    let fetcher = SurrealDbFetcher::new(&name);
                    match fetcher.download_file(&path).await {
                        Ok(Some(data)) => {
                            // Report progress
                            let total = data.len() as u64;
                            let _ = tx.send((0, total));
                            
                            // Save to file using rfd
                            if let Some(file_handle) = task.await {
                                match file_handle.write(&data).await {
                                    Ok(_) => {
                                        info!("Downloaded {} bytes to {}", total, filename);
                                        let _ = tx.send((total, total));
                                    }
                                    Err(e) => log::error!("Error writing file: {e:?}"),
                                }
                            }
                        }
                        Ok(None) => log::warn!("File not found in SurrealDB: {path}"),
                        Err(e) => log::warn!("Error downloading file from SurrealDB: {e:?}"),
                    }
                });
            }
            StorageBackend::S3 => {
                let access_key = self.user.get_minio_access_key().unwrap_or_default();
                let secret_key = self.user.get_minio_secret_key().unwrap_or_default();
                PlatformSpawner::spawn(async move {
                    let result = Self::perform_download(
                        &name.clone(),
                        &access_key,
                        &secret_key,
                        tx.clone(),
                        &path,
                        &filename,
                        task
                    ).await;
                    info!("Result: {result:?}");
                });
            }
        }
    }

    pub fn preview_selection(&self, path: String) {
        let tx = self.bytes_tx.clone();
        let preview_tx = self.file_preview_channel.0.clone();
        let name = self.user.get_username().to_string();
        let backend = self.storage_backend.clone();
        
        match backend {
            StorageBackend::SurrealDb => {
                PlatformSpawner::spawn(async move {
                    let fetcher = SurrealDbFetcher::new(&name);
                    match fetcher.download_file(&path).await {
                        Ok(Some(data)) => {
                            // Try to convert bytes to UTF-8 string for preview
                            match String::from_utf8(data) {
                                Ok(content) => { let _ = preview_tx.send(content); },
                                Err(e) => log::warn!("Error converting file to string: {e:?}"),
                            }
                        }
                        Ok(None) => log::warn!("File not found in SurrealDB: {path}"),
                        Err(e) => log::warn!("Error getting file from SurrealDB: {e:?}"),
                    }
                });
            }
            StorageBackend::S3 => {
                let access_key = self.user.get_minio_access_key().unwrap_or_default();
                let secret_key = self.user.get_minio_secret_key().unwrap_or_default();
                PlatformSpawner::spawn(async move {
                    let result = Self::preview_file(
                        tx.clone(),
                        &name.clone(),
                        &access_key,
                        &secret_key,
                        &path
                    ).await;

                    match result {
                        Ok(file) => { let _ = preview_tx.send(file); },
                        Err(e) => log::warn!("Error getting file to preview: {e:?}"),
                    }
                });
            }
        }
    }

    fn delete_selection(&self, path: String) {
        let name = self.user.get_username().to_string();
        let backend = self.storage_backend.clone();
        let fs_tx = self.fs_actions_channel.0.clone();
        let current_prefix = self.current_prefix.clone();
    
        match backend {
            StorageBackend::SurrealDb => {
                PlatformSpawner::spawn(async move {
                    let fetcher = SurrealDbFetcher::new(&name);
                    match fetcher.delete_file(&path).await {
                        Ok(_) => info!("File '{path}' successfully deleted from SurrealDB."),
                        Err(e) => log::warn!("Error deleting '{path}' from SurrealDB: {e:?}"),
                    }
                    let _ = fs_tx.try_send(FileSystemAction::RequestNewContents(current_prefix));
                });
            }
            StorageBackend::S3 => {
                let access_key = self.user.get_minio_access_key().unwrap_or_default();
                let secret_key = self.user.get_minio_secret_key().unwrap_or_default();
                let parsed = name.split_once('@').unwrap_or_default().0.to_string();
            
                PlatformSpawner::spawn(async move {
                    let region = database::REGION;
                    let bucket = Bucket::new(
                        STORAGE_URL.to_string().parse::<Url>().unwrap(), 
                        rusty_s3::UrlStyle::Path, 
                        parsed, 
                        region,
                    )
                    .expect("Url has a valid scheme and host");
            
                    let credentials = Credentials::new(access_key, secret_key);
            
                    // Create the DeleteObject action
                    let action = rusty_s3::actions::DeleteObject::new(
                        &bucket, 
                        Some(&credentials), 
                        &path
                    );
                    let signed_url = action.sign(ONE_HOUR);
            
                    let client = Client::new();
                    match client.delete(signed_url).send().await {
                        Ok(response) if response.status().is_success() => {
                            info!("File '{path}' successfully deleted from S3.");
                        }
                        Ok(response) => {
                            log::warn!(
                                "Failed to delete file '{}': {}",
                                path,
                                response.status()
                            );
                        }
                        Err(err) => {log::warn!("Error deleting '{path}': {}", err);}
                    }
                    let _ = fs_tx.try_send(FileSystemAction::RequestNewContents(current_prefix));
                });
            }
        }
    }

    async fn perform_upload(
        name: &String, 
        access_key: &String, 
        secret_key: &String, 
        mut path: String,
        task: impl Future<Output = Option<Vec<FileHandle>>>
    ) -> Result<(), Error> {

        let name = name.clone();
        let region = database::REGION;
        let client = Client::new();
        let credentials = Credentials::new(access_key, secret_key);
        let mut bytes: Bytes = Bytes::new();
        let files = task.await.unwrap();
        let mut file_name = String::new();

        let bucket = Bucket::new(
            STORAGE_URL.to_string().parse::<Url>()?, 
            rusty_s3::UrlStyle::Path, name, region
        )?;
        if !path.ends_with('/') {
            path.push_str("/");
        }
        for file_handle in files {

            file_name = format!("{path}{}", file_handle.file_name());
            bytes = Bytes::copy_from_slice(file_handle.read().await.as_slice());
        }

        let action = CreateMultipartUpload::new(&bucket, Some(&credentials), &file_name);
        let url = action.sign(ONE_HOUR);
        let resp = client.post(url).send().await?.error_for_status()?;
        let body = resp.text().await?;
        let multipart = CreateMultipartUpload::parse_response(&body)?;
    
        info!(
            "multipart upload created - upload id: {}",
            multipart.upload_id()
        );
    
        let part_upload = UploadPart::new(
            &bucket,
            Some(&credentials),
            &file_name,
            1,
            multipart.upload_id(),
        );

        let url = part_upload.sign(ONE_HOUR);
        // let x = Bytes::from(bytes.as_slice()).clone();

        let resp = client
            .put(url)
            .body(bytes)
            .send()
            .await?
            .error_for_status()?;

        let etag = resp
            .headers()
            .get(ETAG)
            .expect("every UploadPart request returns an Etag");
    
        info!("etag: {}", etag.to_str()?);
    
        let action = CompleteMultipartUpload::new(
            &bucket,
            Some(&credentials),
            &file_name,
            multipart.upload_id(),
            iter::once(etag.to_str()?),
        );
        let url = action.sign(ONE_HOUR);
    
        let resp = client
            .post(url)
            .body(action.body())
            .send()
            .await?
            .error_for_status()?;

        let body = resp.text().await?;

        info!("it worked! {body}");
        Ok(())
    }

    pub fn upload_script(&self, file_name: String, script_contents: String) {
        let access_key = self.user.get_minio_access_key().unwrap_or_default();
        let secret_key = self.user.get_minio_secret_key().unwrap_or_default();
        let name = self.user.get_username().to_string();
        let bytes = Bytes::copy_from_slice(script_contents.as_bytes());

        let new_name = if file_name.contains(' ') {
            file_name.replace(' ', "_")
        } else {
            file_name
        };

        PlatformSpawner::spawn(async move {
            let result = Self::perform_upload_script(
                &name.clone(),
                &access_key.clone(),
                &secret_key.clone(),
                bytes,
                &new_name.clone(),
            ).await;

            info!("Result: {result:?}");
        });
        let _ = self.fs_actions_channel.0.try_send(FileSystemAction::RequestNewContents(self.current_prefix.clone()));
    }

    pub async fn perform_upload_script(
        name: &String, 
        access_key: &String, 
        secret_key: &String,
        bytes: Bytes,
        file_name: &String
    ) -> Result<(), Error> {
        let path = format!("Scripts/{file_name}");
        let name = name.clone();
        let region = database::REGION;
        let client = Client::new();
        let credentials = Credentials::new(access_key, secret_key);

        let bucket = Bucket::new(
            STORAGE_URL.to_string().parse::<Url>()?, 
            rusty_s3::UrlStyle::Path, name, region
        )?;

        // Step 1: Create the "folder" if it doesn't exist
        let folder_path = "Scripts/"; // The "folder" key in S3
        let create_folder_action = rusty_s3::actions::PutObject::new(&bucket, Some(&credentials), folder_path);
        let create_folder_url = create_folder_action.sign(ONE_HOUR);

        let folder_response = client
            .put(create_folder_url)
            .header(CONTENT_LENGTH, 0) // Empty object for the folder
            .send()
            .await?;

        if !folder_response.status().is_success() {
            return Err(anyhow::anyhow!(format!(
                "Failed to create folder '{}': {}",
                folder_path,
                folder_response.status()
            )));
        }

        info!("Folder '{}' ensured in the bucket.", folder_path);

        let action = CreateMultipartUpload::new(&bucket, Some(&credentials), &path);
        let url = action.sign(ONE_HOUR);
        let resp = client.post(url).send().await?.error_for_status()?;
        let body = resp.text().await?;
        let multipart = CreateMultipartUpload::parse_response(&body)?;
    
        info!(
            "multipart upload created - upload id: {}",
            multipart.upload_id()
        );
    
        let part_upload = UploadPart::new(
            &bucket,
            Some(&credentials),
            &path,
            1,
            multipart.upload_id(),
        );

        let url = part_upload.sign(ONE_HOUR);

        let resp = client
            .put(url)
            .body(bytes)
            .send()
            .await?
            .error_for_status()?;

        let etag = resp
            .headers()
            .get(ETAG)
            .expect("every UploadPart request returns an Etag");
    
        info!("etag: {}", etag.to_str()?);
    
        let action = CompleteMultipartUpload::new(
            &bucket,
            Some(&credentials),
            &path,
            multipart.upload_id(),
            iter::once(etag.to_str()?),
        );
        let url = action.sign(ONE_HOUR);
    
        let resp = client
            .post(url)
            .body(action.body())
            .send()
            .await?
            .error_for_status()?;

        let body = resp.text().await?;

        info!("it worked! {body}");
        Ok(())
    }
    
    async fn preview_file(
        tx: Sender<(u64, u64)>, // Sender for progress reporting
        name: &String, 
        access_key: &String, 
        secret_key: &String, 
        path: &String,
    ) -> Result<String, Error> { // Return the downloaded content as a String
        let name = name.clone();
        let region = database::REGION;
        let bucket = Bucket::new(
            STORAGE_URL.to_string().parse::<Url>()?, 
            rusty_s3::UrlStyle::Path, 
            name, 
            region,
        )?;
        log::error!("BUCKET: {bucket:?}");
        let credentials = Credentials::new(access_key, secret_key);
    
        // Create the GET request action
        let mut action = GetObject::new(&bucket, Some(&credentials), &path);
        action.query_mut().insert("response-cache-control", "no-cache, no-store");
        let signed_url = action.sign(ONE_HOUR);
    
        let client = Client::new();
        let mime = from_path(path).first_or_octet_stream();
        let resp = client.get(signed_url).header(CONTENT_TYPE, mime.essence_str()).send().await?;
        let content_length = resp.content_length().unwrap_or(0);
    
        let mut downloaded_bytes: u64 = 0;
        let mut byte_stream = resp.bytes_stream();
        let mut byte_vec = Vec::new(); // Collect all bytes here
    
        info!("Content length: {content_length}");
    
        // Process the byte stream
        while let Some(item) = byte_stream.next().await {
            let chunk = item?;
            downloaded_bytes += chunk.len() as u64;
    
            // Push the chunk into the vector
            byte_vec.extend_from_slice(&chunk);
            #[cfg(feature="tokio")]
            tokio::time::sleep(web_time::Duration::from_millis(500)).await; // 100ms delay between chunks    
            // Report progress via the Sender
            let _ = tx.send((downloaded_bytes, content_length));

        }
    
        if downloaded_bytes == content_length {
            info!("Downloaded: {downloaded_bytes}");
    
            // Attempt to convert the bytes into a UTF-8 string
            let content = String::from_utf8(byte_vec.clone()).map_err(|e| {
                log::error!("E: {e:?}");
                anyhow::anyhow!(format!("Failed to decode bytes as UTF-8: {}", e))
            })?;
    
            // Return the content as a String
            return Ok(content);
        }
    
        Err(anyhow::anyhow!("Downloaded bytes do not match content length"))
    }
    
    async fn perform_download(
        name: &String, 
        access_key: &String, 
        secret_key: &String, 
        tx: Sender<(u64, u64)>,
        path: &String,
        filename: &String,
        task: impl Future<Output = Option<FileHandle>>
    ) -> Result<(), Error> {
        let name = name.clone();
        let region = database::REGION;
        let bucket = Bucket::new(
            STORAGE_URL.to_string().parse::<Url>()?, 
            rusty_s3::UrlStyle::Path, name, region
        )?;

        let credentials = Credentials::new(access_key, secret_key);
        let mut action = GetObject::new(&bucket, Some(&credentials), &path);
        action.query_mut().insert("response-cache-control", "no-cache, no-store");
        let signed_url = action.sign(ONE_HOUR);

        let client = Client::new();
        let mime = from_path(filename).first_or_octet_stream();
        let resp = client.get(signed_url).header(CONTENT_TYPE, mime.essence_str()).send().await?;
        let content_length = resp.content_length().unwrap();
        let mut downloaded_bytes: u64 = 0;
        // let bytes = resp.await.unwrap();
        let mut byte_stream = resp.bytes_stream();
        info!("Content length: {content_length}");
        let file = task.await;
        let mut _bytes = Bytes::new();
        let mut byte_vec = Vec::new();

        while let Some(item) = byte_stream.next().await{
            let chunk = item?;
            // _bytes = _bytes + chunk.clone();
            downloaded_bytes += chunk.len() as u64;
            byte_vec.extend_from_slice(&chunk.as_slice());
            let _ = tx.send((downloaded_bytes, content_length));
            #[cfg(feature="tokio")]
            tokio::time::sleep(web_time::Duration::from_millis(500)).await; // 100ms delay between chunks
        }

        if downloaded_bytes == content_length {
            info!("Downloaded: {downloaded_bytes}");
            // let x = byte_vec.concat();
            if let Some(ref file) = file {
                file.write(&byte_vec.as_slice()).await?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_navigation() {
        let mut fs = FileSystem::new();
        assert_eq!(fs.current_prefix, "");
        assert!(fs.navigation_stack.is_empty());

        // Navigate to "1-TUNEUP/"
        fs.navigate_to("1-TUNEUP/".to_string());
        assert_eq!(fs.current_prefix, "1-TUNEUP/");
        assert_eq!(fs.navigation_stack, vec![""]);

        // Navigate to "1-TUNEUP/Subfolder/"
        fs.navigate_to("1-TUNEUP/Subfolder/".to_string());
        assert_eq!(fs.current_prefix, "1-TUNEUP/Subfolder/");
        assert_eq!(fs.navigation_stack, vec!["", "1-TUNEUP/"]);

        // Navigate up
        let result = fs.navigate_up();
        assert!(result.unwrap());
        assert_eq!(fs.current_prefix, "1-TUNEUP/");
        assert_eq!(fs.navigation_stack, vec![""]);

        // Navigate up again
        let result = fs.navigate_up();
        assert!(result.unwrap());
        assert_eq!(fs.current_prefix, "");
        assert!(fs.navigation_stack.is_empty());

        // Attempt to navigate up from root
        let result = fs.navigate_up();
        assert!(result.unwrap());
        assert_eq!(fs.current_prefix, "");
        assert!(fs.navigation_stack.is_empty());
    }
}


#[test]
fn test_merge_node_with_normalized_subfolders() {
    let mut root = Node::Folder("/".to_string(), HashMap::new());

    let subfolders = vec![
        ("1-TUNEUP/".to_string(), Node::Folder("1-TUNEUP".to_string(), HashMap::new())),
        ("2-DIAGNOSTIC/".to_string(), Node::Folder("2-DIAGNOSTIC".to_string(), HashMap::new())),
    ];

    let mut map = HashMap::new();
    for (key, value) in subfolders {
        map.insert(key, value);
    }

    let new_node = Node::Folder("/".to_string(), map);
    root.merge_node(new_node).unwrap();

    if let Some(Node::Folder(_, children)) = root.find_folder("/") {
        assert!(children.contains_key("1-TUNEUP"));
        assert!(children.contains_key("2-DIAGNOSTIC"));
        assert!(!children.contains_key(""));
    } else {
        panic!("Root folder not found or invalid structure.");
    }
}
