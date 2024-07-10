use std::{iter, cell::RefCell, collections::{HashMap, HashSet}};
use crossbeam::channel::{Receiver, Sender};
use eframe::egui::{collapsing_header::CollapsingState, popup_below_widget, Align, Color32, Direction, Layout, PopupCloseBehavior::CloseOnClickOutside, ProgressBar, RichText, ScrollArea, Ui, Widget};
use futures::StreamExt;
use log::info;
use reqwest::{header::{CONTENT_TYPE, ETAG}, Client, Url};
use rusty_s3::{Bucket, Credentials, S3Action, actions::{CompleteMultipartUpload, CreateMultipartUpload, UploadPart, GetObject}};
use wasm_bindgen_futures::spawn_local;
use web_time::Duration;
use mime_guess::from_path;
use bytes::Bytes;

use crate::app_state::{ACCESS_KEY, SECRET_KEY};

const ONE_HOUR: Duration = Duration::from_secs(3600);

#[derive(Debug)]
pub struct FileSystem {
    root: Node,
    /// HashSet of selected files (hold CTRL key to select multiple)
    selected_items: RefCell<HashSet<String>>, 
    bytes_tx: Sender<(Vec<u8>, u64)>,
    pub bytes_rx: Receiver<(Vec<u8>, u64)>,
    progress: f64,
    total_size: f64,
    paths: Vec<String>,
    directory_paths: HashSet<String>
}

#[derive(Debug)]
pub enum Node {
    File(String),
    Folder(HashMap<String, Node>),
}

impl FileSystem {
    pub fn new() -> Self {
        let (bytes_tx, bytes_rx) = crossbeam::channel::unbounded();
        Self {
            bytes_tx,
            bytes_rx,
            root: Node::Folder(HashMap::new()),
            selected_items: RefCell::new(HashSet::new()),
            progress: 0.0,
            total_size: 0.0,
            paths: Vec::new(),
            directory_paths: HashSet::new()
        }
    }

    pub fn build_file_system(&mut self, paths: Vec<String>) {
        self.paths = paths.clone();
        for path in paths {
            let parts: Vec<&str> = path.split('/').collect();
            let mut current_path = String::new();
            let mut current = &mut self.root;

            for (i, part) in parts.iter().enumerate() {
                let part = part.to_string();
                if i == parts.len() - 1 { // It's a file
                    if let Some(folder) = current.as_folder_mut() {
                        folder.insert(part.clone(), Node::File(part.to_string()));
                    }
                } else { // It's a folder
                    current = current.as_folder_mut().unwrap().entry(part.clone())
                        .or_insert_with(|| Node::Folder(HashMap::new()));

                    if !current_path.is_empty() {
                        current_path.push('/');
                    }
                    current_path.push_str(&part);
                    self.directory_paths.insert(current_path.clone());

                }
            }
        }
         
    }

    pub fn display(&self, ui: &mut Ui) {
        let size = ui.available_size_before_wrap();
        ScrollArea::vertical().max_width(size.x)
            .max_height(size.y)
            .auto_shrink(false)
            .show(ui, |ui| 
        {
            ui.with_layout(Layout::from_main_dir_and_cross_align(Direction::TopDown, Align::Center), |ui| {
                self.display_path(ui, &self.root, "".to_string());
            });
        });
    }

    fn display_path(&self, ui: &mut Ui, node: &Node, current_path: String) {      
        let count = 0;
        ui.vertical(|ui| 
        {
            if let Node::Folder(children) = node {
                // Collect entries into a vector for sorting
                let mut entries: Vec<(&String, &Node)> = children.iter().collect();
                entries.sort_by(|a, b| {
                    let a_is_dir = matches!(a.1, Node::Folder(_));
                    let b_is_dir = matches!(b.1, Node::Folder(_));
                    match a_is_dir == b_is_dir {
                        true => a.0.cmp(b.0), // Sort alphabetically if both are files or both are directories
                        false => b_is_dir.cmp(&a_is_dir), // Directories first
                    }
                });

                for (label, node) in entries {

                    let is_selected = self.selected_items.borrow().contains(label);
                    let modifiers = ui.input(|i| i.modifiers); // Get the current modifiers

                    if let Node::Folder(_) = node {
                        let id = ui.make_persistent_id(format!("{label}+++{:?}", count + 1));
                        CollapsingState::load_with_default_open(ui.ctx(), id, false)
                        .show_header(ui, |ui| 
                        {
                            
                            let selectable_label = ui.selectable_label(is_selected, RichText::new(format!("🗀   {}", label)));
                            if selectable_label.clicked() { // If the item was already selected, deselect it
                                if modifiers.ctrl { self.selected_items.borrow_mut().insert(label.clone());} 
                                if self.selected_items.borrow().contains(label) {
                                    // If the item was already selected, deselect it
                                    self.selected_items.borrow_mut().remove(label);
                                } 
                                else { // If the control key is not down, clear previous selection and select the current item
                                    self.selected_items.borrow_mut().clear();
                                    self.selected_items.borrow_mut().insert(label.clone());
                                }
                            }
                            
                            if selectable_label.secondary_clicked(){
                                ui.memory_mut(|mem| mem.open_popup(format!("sub_menu-{:?}",label).into()));
                            }
                            
                            let _res = popup_below_widget(ui, format!("sub_menu-{:?}",label).into(), &selectable_label, CloseOnClickOutside, |ui| {
                                ui.vertical_centered_justified(|ui| {
                                    ui.set_width(200.0);
                                    if ui.button("Download").clicked(){
                                        let path = self.path_lookup(&label.clone());
                                        if let Some(path) = path {
                                            info!("Path: {:?}", path.clone());
                                            self.download_selection(path, label.clone());
                                        }
                                    }
                                    if ui.button("Upload").clicked(){
                                        if let Some(dir) = self.find_directory_full_path(&label){
                                            info!("Dir: {:?}", dir.clone());
                                            self.upload(dir);
                                        }
                                    }
                                }).inner
                            });

                        })
                        .body(|ui| self.display_path(ui, &node, current_path.clone()));

                    } else if let Node::File(label) = node{
                        let selectable_label = ui.selectable_label(is_selected, RichText::new(format!("🗋   {}", label)));
                        if selectable_label.clicked() {
                            if modifiers.ctrl { self.selected_items.borrow_mut().insert(label.clone());} 
                            if self.selected_items.borrow().contains(label) {
                                // If the item was already selected, deselect it
                                self.selected_items.borrow_mut().remove(label);
                            } 
                            else { // If the control key is not down, clear previous selection and select the current item
                                self.selected_items.borrow_mut().clear();
                                self.selected_items.borrow_mut().insert(label.clone());
                            }
                        }
                        if selectable_label.double_clicked() {

                        }
                        if selectable_label.secondary_clicked(){
                            ui.memory_mut(|mem| mem.open_popup(format!("sub_menu-{:?}",label).into()));
                        }
                        
                        let _res = popup_below_widget(ui, format!("sub_menu-{:?}",label).into(), &selectable_label, CloseOnClickOutside, |ui| {
                            ui.vertical_centered_justified(|ui| {
                                ui.set_width(200.0);
                                if ui.button("Download").clicked(){
                                    let path = self.path_lookup(&label.clone());
                                    if let Some(path) = path {
                                        info!("Path: {:?}", path.clone());
                                        self.download_selection(path, label.clone());
                                    }
                                }
                            }).inner
                        });
                    }
                }
            }
        });
    }

    pub fn show_progress(&mut self, ui: &mut Ui) {
        while let Ok(x) = self.bytes_rx.try_recv() {
            self.total_size = x.1 as f64;
            for y in x.0 {
                self.progress += y as f64;
            }
        }
        ProgressBar::new(self.progress as f32/ self.total_size as f32).show_percentage().fill(Color32::from_rgb(200, 50, 200)).ui(ui);
    }

    fn path_lookup(&self, file_name: &str) -> Option<String> {
        self.paths.iter()
            .find(|path| path.ends_with(file_name))
            .cloned() // returns a clone of the matching path, if found
    }
    
    fn find_directory_full_path(&self, label: &str) -> Option<String> {
        self.directory_paths.iter().find(|path| path.ends_with(label)).cloned()
    }

    pub fn upload(&self, path: String) {

        let task = rfd::AsyncFileDialog::new().pick_files();
        let _tx = self.bytes_tx.clone();
        // self.total_size = bytes.len() as f64;
        spawn_local(async move {
            let name = "logan";
            let region = "us-west";
            let client = Client::new();
            let credentials = Credentials::new(ACCESS_KEY, SECRET_KEY);
            let mut bytes: Bytes = Bytes::new();
            let files = task.await.unwrap();
            let mut file_name = String::new();

            let bucket = Bucket::new(
                "https://storage-api.master-tech.app".to_string().parse::<Url>().unwrap(), 
                rusty_s3::UrlStyle::Path, name, region
            )
            .expect("Url has a valid scheme and host");

            for file_handle in files {
                file_name = format!("{path}/{}", file_handle.file_name());
                bytes = Bytes::copy_from_slice(file_handle.read().await.as_slice());
            }

            
            // self.progress = 
            let action = CreateMultipartUpload::new(&bucket, Some(&credentials), &file_name);
            let url = action.sign(ONE_HOUR);
            let resp = client.post(url).send().await.unwrap().error_for_status().unwrap();
            let body = resp.text().await.unwrap();
            let multipart = CreateMultipartUpload::parse_response(&body).unwrap();
        
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
                .await.unwrap()
                .error_for_status().unwrap();

            let etag = resp
                .headers()
                .get(ETAG)
                .expect("every UploadPart request returns an Etag");
        
            info!("etag: {}", etag.to_str().unwrap());
        
            let action = CompleteMultipartUpload::new(
                &bucket,
                Some(&credentials),
                &file_name,
                multipart.upload_id(),
                iter::once(etag.to_str().unwrap()),
            );
            let url = action.sign(ONE_HOUR);
        
            let resp = client
                .post(url)
                .body(action.body())
                .send()
                .await.unwrap()
                .error_for_status().unwrap();

            let body = resp.text().await.unwrap();

            info!("it worked! {body}");
        });
    }

    fn download_selection(&self, path: String, filename: String) {
        let task = rfd::AsyncFileDialog::new().set_file_name(filename.clone()).save_file();
        let tx = self.bytes_tx.clone();
        spawn_local(async move {
            let name = "logan";
            let region = "us-west";
            let bucket = Bucket::new(
                "https://storage-api.master-tech.app".to_string().parse::<Url>().unwrap(), 
                rusty_s3::UrlStyle::Path, name, region
            )
            .expect("Url has a valid scheme and host");

            let credentials = Credentials::new(ACCESS_KEY, SECRET_KEY);
            
            let mut action = GetObject::new(&bucket, Some(&credentials), &path);
            action
                .query_mut()
                .insert("response-cache-control", "no-cache, no-store");

            let signed_url = action.sign(ONE_HOUR);

            let client = Client::new();
            let mime = from_path(filename).first_or_octet_stream();
            let resp = client.get(signed_url).header(CONTENT_TYPE, mime.essence_str()).send().await.unwrap();
            let content_length = resp.content_length().unwrap();
            let mut downloaded_bytes: u64 = 0;
            // let bytes = resp.await.unwrap();
            let mut byte_stream = resp.bytes_stream();
            info!("Content length: {content_length}");
            let file = task.await;
            let mut bytes = Bytes::new();
            while let Some(item) = byte_stream.next().await{
                let chunk = item.unwrap().clone();
                bytes = chunk.clone();
                let _ = tx.try_send((chunk.to_vec(), content_length));
                downloaded_bytes += chunk.len() as u64;
                if downloaded_bytes == content_length {
                    info!("Downloaded: {downloaded_bytes}");
                    if let Some(ref file) = file {
                        file.write(&bytes.to_vec().as_slice()).await.unwrap();
                    }
                }
            }

        });
    }
}


impl Node {
    fn as_folder_mut(&mut self) -> Option<&mut HashMap<String, Node>> {
        if let Node::Folder(ref mut map) = self {
            Some(map)
        } else {
            None
        }
    }
}