use eframe::egui::{collapsing_header::CollapsingState, popup_below_widget, Align, Color32, Direction, Id, Layout, PopupCloseBehavior::CloseOnClickOutside, ProgressBar, RichText, ScrollArea, Ui, Widget};
use rusty_s3::{Bucket, Credentials, S3Action, actions::{CompleteMultipartUpload, CreateMultipartUpload, UploadPart, GetObject}};
use std::{cell::RefCell, collections::{HashMap, HashSet}};
use reqwest::{header::{CONTENT_TYPE, ETAG}, Client, Url};
use crate::{channel_manager::ChannelManager, Spawner};
use crossbeam::channel::{Receiver, Sender};
use database::schema::{Node, User};
use futures::{StreamExt, Future};
use anyhow::{Result, Error};
use crate::PlatformSpawner;
use mime_guess::from_path;
use database::STORAGE_URL;
use surrealdb::sql::Uuid;
use rfd::FileHandle;
use regex::Regex;
use bytes::Bytes;
use std::iter;
use log::info;
#[cfg(feature="tokio")]
use std::path::PathBuf;
pub const ONE_HOUR: web_time::Duration = web_time::Duration::from_secs(3600);

#[derive(Debug, Clone)]
pub struct FileSystem {
    pub scroll_id: Id,
    pub root: Node,
    pub bytes_rx: Receiver<(Vec<u8>, u64)>,
    pub paths_channel: (Sender<Node>, Receiver<Node>),
    #[allow(dead_code)]
    bytes_tx: Sender<(Vec<u8>, u64)>,
    selected_items: RefCell<HashSet<String>>,
    directory_paths: HashSet<String>,
    pub paths: Vec<String>,
    total_size: f32,
    progress: f32,
    pub enter_directory: String,
    pub execute_file: String,
    pub open_folder: bool,
    pub secret_key: String,
    pub access_key: String,
    pub user: User
}

impl FileSystem {
    pub fn new() -> Self {
        let (bytes_tx, bytes_rx) = crossbeam::channel::unbounded();
        let paths_channel = <Node>::create_unbounded_channel();

        Self {
            scroll_id: Id::new(format!("virtual_fs_scrollarea-{}", Uuid::new_v4())),
            bytes_tx,
            bytes_rx,
            paths_channel,
            root: Node::Folder(String::new(), HashMap::new()),
            selected_items: RefCell::new(HashSet::new()),
            progress: 0.0,
            total_size: 0.0,
            paths: Vec::new(),
            directory_paths: HashSet::new(),
            enter_directory: String::new(),
            execute_file: String::new(),
            open_folder: false,
            secret_key: String::new(),
            access_key: String::new(),
            user: User::default()
        }
    }

    pub fn receive(&mut self) {
        if let Ok(node) = self.paths_channel.1.try_recv() {
            // if !received_paths.is_empty() && self.paths.is_empty() {
                log::info!("Files: {node:?}");
                self.root = node;
                // self.build_file_system(received_paths);
            // }
        }
    }
    
    pub fn set_user(&mut self, user: User) -> &mut Self {
        self.user = user;
        self
    }

    pub fn build_file_system(&mut self, paths: Vec<String>) -> &mut Self {
        // Precompile the regex outside the loop
        let file_pattern = Regex::new(r"\.[a-zA-Z]{1,4}$").unwrap();
        self.paths = paths.clone();
        for path in paths {
            let parts: Vec<&str> = if path.contains('\\') { path.split('\\').collect() } else { path.split('/').collect() };
            let mut current_path = String::new();
            let mut current = &mut self.root;

            for (_, part) in parts.iter().enumerate() {
                // let part = part.to_string();
                if Self::is_file(&part, &file_pattern) { // part.contains('.'){ // i == parts.len() - 1 { der.insert(part.to_string(), Node::File((path.clone(), part.to_string())));
                    if let Node::Folder(ref mut full_path, ref mut folder) = current {
                        let file_full_path = if full_path.contains('\\') { format!("{}\\{}", full_path, part) } else { format!("{}/{}", full_path, part) };
                        folder.insert(part.to_string(), Node::File((file_full_path, part.to_string().clone())));
                    }
                } else { // It's a folder
                    if !current_path.is_empty() {
                        current_path += if current_path.contains('\\') { "\\" } else { "/" };
                    }
                    current_path += part;
    
                    if let Node::Folder(_, ref mut folder) = current {
                        current = folder.entry(part.to_string()).or_insert_with(|| Node::Folder(current_path.clone(), HashMap::new()));
                    }
                }
            }
        }

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

    fn is_file(name: &str, file_pattern: &Regex) -> bool {
        file_pattern.is_match(name)
    }

    pub fn display(&mut self, ui: &mut Ui){
        let size = ui.available_size_before_wrap();
        ScrollArea::vertical()
            .id_salt(self.scroll_id)
            .max_width(size.x)
            .max_height(size.y)
            .auto_shrink(false)
            .show(ui, |ui| 
        {
            let x = self.root.clone();
            ui.with_layout(Layout::from_main_dir_and_cross_align(Direction::TopDown, Align::Center), |ui| {
                self.display_path(ui, &x, "".to_string());
            }).inner
        }).inner;
    }

    fn display_path(&mut self, ui: &mut Ui, node: &Node, current_path: String){
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

                        let collapsing_head = CollapsingState::load_with_default_open(
                            ui.ctx(), 
                            id, 
                            self.open_folder
                        );

                        let res = collapsing_head.show_header(ui, |ui| 
                        {
                            let is_selected = self.selected_items.borrow().contains(label);
                            let selectable_label = ui.selectable_label(is_selected, RichText::new(format!("🗀   {}", label)));

                            if selectable_label.clicked() { // If the item was already selected, deselect it
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
                            }

                            if selectable_label.double_clicked(){
                                self.enter_directory = full_path.clone();
                                self.open_folder = true;
                                info!("label double clicked: {label:?} // {:?}", self.directory_paths);
                                info!("self.find_directory_full_path(label): {:?}", self.find_directory_full_path(&full_path));
                                let path = self.path_lookup(&label.clone());
                                if let Some(path) = path { self.enter_directory = path.clone(); }
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
                                        if let Some(dir) = self.find_directory_full_path(&label){
                                            info!("Dir: {:?}", dir.clone());
                                            if cfg!(target_os="windows") || cfg!(target_os="linux"){
                                                #[cfg(target_os="windows")]
                                                self.upload_folder(dir);
                                            }
                                        }
                                    }
                                }).inner;
                            });
                        }).body(|ui| 
                            self.display_path(ui, &node, current_path.clone())
                        );

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
                                    if let Some(dir) = self.find_directory_full_path(&label){
                                        info!("Dir: {:?}", dir.clone());
                                        self.upload(dir);
                                    }
                                }

                                ui.add_space(5.0);

                                if ui.button("Upload Folder").clicked(){
                                    if let Some(dir) = self.find_directory_full_path(&label){
                                        info!("Dir: {:?}", dir.clone());
                                        if cfg!(target_os="windows") || cfg!(target_os="linux"){
                                            #[cfg(target_os="windows")]
                                            self.upload_folder(dir);
                                        }
                                    }
                                }
                            }).inner;
                        });

                    } else if let Node::File((full_path, label)) = node{

                        // let id = ui.make_persistent_id(format!("sub_menu-{:?}", full_path));
                        let file_selected = self.selected_items.borrow().contains(full_path);
                        let selectable_label = ui.selectable_label(file_selected, RichText::new(format!("🗋   {}", label)));

                        if selectable_label.clicked() {
                            if self.selected_items.borrow().contains(full_path) {
                                // If the item was already selected, deselect it
                                self.selected_items.borrow_mut().remove(full_path);
                            } 

                            if modifiers.ctrl { 
                                self.selected_items.borrow_mut().insert(full_path.clone());
                            } else { // If the control key is not down, clear previous selection and select the current item
                                self.selected_items.borrow_mut().clear();
                                self.selected_items.borrow_mut().insert(full_path.clone());
                            }
                        }

                        if selectable_label.secondary_clicked(){
                            ui.memory_mut(|mem| mem.open_popup(
                                ui.make_persistent_id(format!("sub_menu-{:?}", full_path))
                            ));
                        }

                        if selectable_label.double_clicked(){
                            self.execute_file = full_path.clone();
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

    fn path_lookup(&self, file_name: &str) -> Option<String> {
        // info!("Self.paths: {:?}", self.paths);
        self.paths.iter()
            .find(|path| path.ends_with(format!("/{file_name}").as_str()) || path.ends_with(format!("\\{file_name}").as_str()))
            .cloned() // returns a clone of the matching path, if found
    }
    
    pub fn find_directory_full_path(&self, label: &str) -> Option<String> {
        info!("self.direc_paths: {:?} // {:?}", self.directory_paths, &format!("\\\\{label}"));
        self.directory_paths.iter().find(|path| path.ends_with(&format!("\\\\{label}"))).cloned()
    }

    pub fn upload(&self, path: String) {
        let task = rfd::AsyncFileDialog::new().pick_files();
        let secret_key = self.secret_key.clone();
        let access_key = self.access_key.clone();
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
        let _secret_key = self.secret_key.clone();
        let _access_key = self.access_key.clone();
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
        let secret_key = self.secret_key.clone();
        let access_key = self.access_key.clone();
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
        // let secret_key = self.secret_key.clone();
        // let access_key = self.access_key.clone();
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


