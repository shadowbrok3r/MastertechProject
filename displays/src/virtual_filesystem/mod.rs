use eframe::egui::{collapsing_header::CollapsingState, popup_below_widget, Align, CentralPanel, Color32, Context, Direction, Frame, Id, Layout, Margin, PopupCloseBehavior::CloseOnClickOutside, ProgressBar, RichText, Rounding, ScrollArea, Stroke, TextEdit, TopBottomPanel, Ui, Vec2, Widget};
use rusty_s3::{Bucket, Credentials, S3Action, actions::{CompleteMultipartUpload, CreateMultipartUpload, UploadPart, GetObject}};
use std::{cell::RefCell, collections::{HashMap, HashSet}};
use reqwest::{header::{CONTENT_TYPE, ETAG}, Client, Url};
use crate::{channel_manager::ChannelManager, Spawner, FileSystemAction};
use crossbeam::channel::{Receiver, Sender};
use database::schema::{buckets::list_buckets, Node, User}; // buckets::list_buckets, 
use futures::{StreamExt, Future};
use anyhow::{Result, Error};
use crate::PlatformSpawner;
use mime_guess::from_path;
use database::STORAGE_URL;
use surrealdb::sql::Uuid;
use rfd::FileHandle;
// use regex::Regex;
use bytes::Bytes;
use std::iter;
use log::info;
#[cfg(feature="tokio")]
use std::path::PathBuf;

pub const ONE_HOUR: web_time::Duration = web_time::Duration::from_secs(3600);


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
            "us-west"
        ).unwrap();
        
        let credentials = Credentials::new(access_key.to_string(), secret_key.to_string());
        
        Self { bucket, credentials }
    }

    pub async fn request_bucket_contents(&mut self, prefix: &str) -> anyhow::Result<Node, anyhow::Error> {
        let res = list_buckets(self.credentials.clone(), self.bucket.clone(), Some(&prefix)).await?;
        Ok(res)
    }

}


#[derive(Debug, Clone)]
pub struct FileSystem {
    /// Persistent Scroll ID so we dont have any 
    /// clashes of ID's between multiple websocket clients
    pub scroll_id: Id,
    /// The Entire Hierarchy of Folders/Files
    pub root: Node,
    /// Sending progress of downloads/uploads to progress bar
    bytes_tx: Sender<(Vec<u8>, u64)>,
    /// Receiving progress of downloads/uploads to progress bar
    pub bytes_rx: Receiver<(Vec<u8>, u64)>,
    /// Receive / Send FileSystemAction's from the UI
    pub fs_actions_channel: (Sender<FileSystemAction>, Receiver<FileSystemAction>),
    pub paths_channel: (Sender<Node>, Receiver<Node>),
    /// Selected files/folders
    selected_items: RefCell<HashSet<String>>,
    /// All of our paths
    pub paths: Vec<String>,
    /// Total size of download/upload
    total_size: f32,
    /// Progress bar for file/folder download/upload
    progress: f32,
    /// File to execute on Mastertech
    pub execute_file: String,
    /// Credentials for API calls to Minio
    pub user: User,
    /// Editable Current path used by a 
    /// TextEdit for manual navigation
    pub current_prefix: String,
    /// Stack to track navigation history
    pub navigation_stack: Vec<String>,
}

impl FileSystem {
    pub fn new() -> Self {
        let (bytes_tx, bytes_rx) = crossbeam::channel::unbounded();
        let fs_actions_channel = <FileSystemAction>::create_unbounded_channel();
        let paths_channel = <Node>::create_unbounded_channel();

        Self {
            scroll_id: Id::new(format!("virtual_fs_scrollarea-{}", Uuid::new_v4())),
            bytes_tx, bytes_rx,
            fs_actions_channel,
            paths_channel,
            root: Node::Folder(String::new(), HashMap::new()),
            selected_items: RefCell::new(HashSet::new()),
            progress: 0.0,
            total_size: 0.0,
            paths: Vec::new(),
            execute_file: String::new(),
            user: User::default(),
            current_prefix: "/".to_string(),
            navigation_stack: Vec::new(),
        }
    }

    pub fn receive(&mut self, ctx: &Context) { // , requester: &mut dyn FnMut(&str)
        if let Ok(action) = self.fs_actions_channel.1.try_recv() {
            self.handle_filesystem_action(&action, None);
            ctx.request_repaint();
        }
    }
    
    pub fn set_user(&mut self, user: User) -> &mut Self {
        self.user = user;
        self
    }

    pub fn handle_filesystem_action(
        &mut self, 
        action: &FileSystemAction, 
        requester: Option<Box<dyn Fn(&str, Sender<Node>, FileSystemAction)>>
    ) {
        log::info!("Action: {action:?}");
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
                log::info!("Files: {new_node:?}");
                let insert_node = self.insert_node(new_node.clone());
                info!("InsertNode: {insert_node:?}");
            },
            FileSystemAction::RequestNewContents(folder_prefix) => {
                if let Some(mut requester) = requester {
                    requester(&folder_prefix, self.paths_channel.0.clone(), action.clone());
                } else {
                    let _ = self.request_contents(folder_prefix);
                }
            },
        }
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
                    if let Node::Folder(_, ref mut folder) = current {
                        let folder_path = current_path.display().to_string();
                        current = folder.entry(part_str.to_string()).or_insert_with(|| Node::Folder(folder_path, HashMap::new()));
                    }
                } else if current_path.is_file() {
                    if let Node::Folder(ref full_path, ref mut folder) = current {
                        let file_full_path = format!("{}/{}", full_path, part_str);
                        folder.insert(part_str.to_string(), Node::File((file_full_path, part_str.to_string())));
                    }
                }
            }
        }

        root 
    }

    pub fn display(&mut self, ui: &mut Ui){
        // self.receive(ui.ctx(), |_|);

        let size = ui.available_size_before_wrap();
        let mut inner_margin_top = Margin::default();
        inner_margin_top.bottom = 5.0;

        let btm_panel_frame = Frame::default()
            .inner_margin(inner_margin_top.clone())
            .rounding(Rounding::same(10.0));

        let top_panel_frame = Frame::default()
            .outer_margin(Margin::symmetric(5., 7.));

        let panel_frame = Frame::default()
            .fill(Color32::from_rgb(12, 12, 14))
            .inner_margin(Margin::same(6.))
            .rounding(Rounding::same(10.0))
            .stroke(Stroke::new(1.0, Color32::from_additive_luminance(50)));
        
        ui.style_mut().spacing.button_padding = Vec2::new(10.0, 3.0);

        TopBottomPanel::top("FileBrowserTop")
            .frame(top_panel_frame)
            .show_separator_line(false)
            .exact_height(38.)
            .show_inside(ui, |ui| 
        {
            ui.with_layout(Layout::left_to_right(eframe::egui::Align::Center), |ui| {
                let pre_modified_path = self.current_prefix.clone();
                let response = TextEdit::singleline(&mut self.current_prefix)
                .desired_width(ui.available_width() / 2.)
                .ui(ui);

                if response.lost_focus() && self.current_prefix.ne(&pre_modified_path) {
                    info!("Lost focus on self.current_prefix TextEdit");
                    let _ = self.fs_actions_channel.0.try_send(FileSystemAction::EnterDirectory(self.current_prefix.clone()));
                }

                let home_res = ui.button("🏠").on_hover_text("Home");
                if home_res.clicked(){
                    let _ = self.fs_actions_channel.0.try_send(FileSystemAction::NavigateHome);
                }

                let parent_res = ui.button("⬆").on_hover_text("Parent Folder");
                if parent_res.clicked() {
                    let navigate_up = self.navigate_up();
                    info!("Navigating up: {navigate_up:?}");
                }
            });
        });

        TopBottomPanel::bottom("FileBrowserBottom")
            .frame(btm_panel_frame)
            .show_separator_line(false)
            .show_inside(ui, |ui| 
        {
            ui.vertical_centered(|ui | self.show_progress(ui));
        });

        ui.add_space(10.0);

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
                                let _ = tx.try_send(FileSystemAction::Select((modifiers, full_path.clone())));
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
                                    //     info!("Dir: {:?}", dir.clone());
                                    //     if cfg!(target_os="windows") || cfg!(target_os="linux"){
                                    //         #[cfg(target_os="windows")]
                                    //         self.upload_folder(dir);
                                    //     }
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
                            let _ = tx.try_send(FileSystemAction::Select((modifiers, full_path.clone())));
                        }

                        if selectable_label.secondary_clicked(){
                            ui.memory_mut(|mem| mem.open_popup(
                                ui.make_persistent_id(format!("sub_menu-{:?}", full_path))
                            ));
                        }

                        if selectable_label.double_clicked(){
                            let _ = tx.try_send(FileSystemAction::EnterDirectory(full_path.clone()));
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
                            }).inner
                        });
                    };
                }
            }
        });
    }

    fn request_contents(&self, folder_prefix: &str) -> Result<(), Error> {
        let folder_pref = folder_prefix.to_string();
        let tx = self.paths_channel.0.clone();
        let access_key = self.user.minio_access_key.clone().unwrap_or_default();
        let secret_key = self.user.minio_secret_key.clone().unwrap_or_default();
        let name = self.user.email.clone();
        PlatformSpawner::spawn(async move {
            let parsed = name.split_once('@').unwrap().0.to_string().clone();
            let mut s3_fetcher = S3Fetcher::new(&access_key, &secret_key, &parsed);
            match s3_fetcher.request_bucket_contents(&folder_pref).await {
                Ok(node) => { let _ = tx.try_send(node); },
                Err(e) => log::warn!("Error getting node: {e:?}"),
            }
        });

        Ok(())
    }

    fn expand_folder(&self, folder_prefix: &str) {
        let _ = self.fs_actions_channel.0.try_send(FileSystemAction::RequestNewContents(folder_prefix.to_string()));
    }

    fn double_click_folder(&mut self, folder_prefix: &str) {
        // Navigate to the new prefix
        self.navigate_to(folder_prefix.to_string());

        // Check if the folder's contents have already been fetched
        if let Some(Node::Folder(_, ref children)) = self.root.find_folder(folder_prefix) {
            // If children are already loaded (non-empty), no need to fetch
            if !children.is_empty() {
                info!("Folder '{}' already fetched. No need to re-fetch.", folder_prefix);
            } else {
                let _ = self.fs_actions_channel.0.try_send(FileSystemAction::RequestNewContents(folder_prefix.to_string()));
            }
        } else {
            let _ = self.fs_actions_channel.0.try_send(FileSystemAction::RequestNewContents(folder_prefix.to_string()));
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
        if prefix != self.current_prefix {
            // Push the current prefix onto the navigation stack before navigating
            if !self.current_prefix.is_empty() {
                self.navigation_stack.push(self.current_prefix.clone());
            }
            self.current_prefix = prefix;
            info!("Navigated to prefix: '{}'", self.current_prefix);
        } else {
            info!("Attempted to navigate to the same prefix: '{prefix}'. No action taken. self.current_prefix: '{}'", self.current_prefix);
        }
    }

    /// Navigates back to the previous directory.
    ///
    /// Returns `Ok(true)` if navigation was successful, `Ok(false)` if already at root,
    /// or an error if something goes wrong.
    pub fn navigate_up(&mut self) -> Result<bool, Error> {
        info!("self.navigation_stack: {:?}\nself.current_prefix: {}", self.navigation_stack, self.current_prefix);

        if let Some(previous_prefix) = self.navigation_stack.pop() {
            // Set the dispayed
            info!("previous_prefix: '{previous_prefix}' -- self.current_prefix: {}", self.current_prefix);
            self.current_prefix = previous_prefix;
            info!("Navigated up to prefix: '{}'", self.current_prefix);
            Ok(true)
        } else {
            // Already at root
            info!("Already at root directory. Cannot navigate up. {}", self.current_prefix);
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
        self.root.find_folder_mut(&self.current_prefix)
    }

    pub fn show_progress(&mut self, ui: &mut Ui) {
        while let Ok(x) = self.bytes_rx.try_recv() {
            self.total_size = x.1 as f32;
            for y in x.0 {
                self.progress += y as f32;
            }
        }
        if self.progress == self.total_size {
            self.progress = 0.0;
            self.total_size = 0.0;
        }
        ProgressBar::new(self.progress / self.total_size).show_percentage().fill(Color32::from_rgba_premultiplied(50, 10, 50, 65)).ui(ui);
    }

    pub fn upload(&self, path: String) {
        let task = rfd::AsyncFileDialog::new().pick_files();
        let secret_key = self.user.minio_secret_key.clone().unwrap_or_default();
        let access_key = self.user.minio_access_key.clone().unwrap_or_default();
        let name = self.user.email.clone();
        let parsed = name.split_once('@').unwrap().0.to_string().clone();

        PlatformSpawner::spawn(async move {
            let result = Self::perform_upload(
                &parsed.clone(),
                &access_key.clone(),
                &secret_key.clone(),
                &path.clone(),
                task
            ).await;

            info!("Result: {result:?}");
        });
    }

    #[cfg(feature="tokio")]
    pub fn upload_folder(&self, _path: String) {
        let _task = rfd::AsyncFileDialog::new().pick_folders();
        let _secret_key = self.user.minio_secret_key.clone().unwrap_or_default();
        let _access_key = self.user.minio_access_key.clone().unwrap_or_default();
        let name = self.user.email.clone();
        let _parsed = name.split_once('@').unwrap().0.to_string().clone();
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
        let secret_key = self.user.minio_secret_key.clone().unwrap_or_default();
        let access_key = self.user.minio_access_key.clone().unwrap_or_default();
        let name = self.user.email.to_lowercase().clone();
        let parsed = name.split_once('@').unwrap().0.to_string().clone();
        PlatformSpawner::spawn(async move {
            let result = Self::perform_download(
                &parsed.clone(),
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

    // fn delete_selection(&self, path: String, filename: String) {
        // let tx = self.bytes_tx.clone();
        // let secret_key = self.user.minio_secret_key.clone().unwrap_or_default();
        // let access_key = self.user.minio_access_key.clone().unwrap_or_default();
        // PlatformSpawner::spawn(async move {
        //     let name = self.user.email;
        //     let region = "us-west";
        //     let bucket = Bucket::new(
        //         STORAGE_URL.to_string().parse::<Url>().unwrap(), 
        //         rusty_s3::UrlStyle::Path, name, region
        //     )
        //     .expect("Url has a valid scheme and host");
        //     let credentials = Credentials::new(access_key, secret_key);  
        //     let mut action = GetObject::new(&bucket, Some(&credentials), &path);
        //     action
        //         .query_mut()
        //         .insert("response-cache-control", "no-cache, no-store");
        //     let signed_url = action.sign(ONE_HOUR);
        //     let client = Client::new();
        // });
    // }

    async fn perform_upload(
        name: &String, 
        access_key: &String, 
        secret_key: &String, 
        path: &String,
        task: impl Future<Output = Option<Vec<FileHandle>>>
    ) -> Result<(), Error> {

        let name = name.clone();
        let region = "us-west";
        let client = Client::new();
        let credentials = Credentials::new(access_key, secret_key);
        let mut bytes: Bytes = Bytes::new();
        let files = task.await.unwrap();
        let mut file_name = String::new();

        let bucket = Bucket::new(
            STORAGE_URL.to_string().parse::<Url>()?, 
            rusty_s3::UrlStyle::Path, name, region
        )?;

        for file_handle in files {
            file_name = format!("{path}/{}", file_handle.file_name());
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
    
    // #[cfg(target_arch="wasm32")]
    async fn perform_download(
        name: &String, 
        access_key: &String, 
        secret_key: &String, 
        tx: Sender<(Vec<u8>, u64)>,
        path: &String,
        filename: &String,
        task: impl Future<Output = Option<FileHandle>>
    ) -> Result<(), Error> {
        let name = name.clone();
        let region = "us-west";
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
            let chunk = item?.clone();
            // _bytes = _bytes + chunk.clone();
            byte_vec.push(chunk.to_vec());
            let _ = tx.try_send((chunk.to_vec(), content_length));
            downloaded_bytes += chunk.len() as u64;
        }

        if downloaded_bytes == content_length {
            info!("Downloaded: {downloaded_bytes}");
            let x = byte_vec.concat();
            if let Some(ref file) = file {
                file.write(x.as_slice()).await?;
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