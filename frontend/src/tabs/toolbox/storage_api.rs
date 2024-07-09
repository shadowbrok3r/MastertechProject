use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use crossbeam::channel::{Receiver, Sender};
use egui::collapsing_header::CollapsingState;
use egui::{Color32, ProgressBar, Widget};
use egui::{Layout, RichText, ScrollArea, Ui, popup_below_widget, PopupCloseBehavior::CloseOnClickOutside};
use futures::StreamExt;
use log::info;
use reqwest::{header::CONTENT_TYPE, Client, Url};
use rusty_s3::actions::CreateMultipartUpload;
use rusty_s3::{actions::GetObject, Bucket, Credentials, S3Action};
use wasm_bindgen_futures::spawn_local;
use web_time::Duration;
use mime_guess::from_path;
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
    paths: Vec<String>
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
            paths: Vec::new()
        }
    }

    pub fn build_file_system(&mut self, paths: Vec<String>) {
        self.paths = paths.clone();
        for path in paths {
            let parts: Vec<&str> = path.split('/').collect();
            let mut current = &mut self.root;

            for (i, part) in parts.iter().enumerate() {
                let part = part.to_string();
                if i == parts.len() - 1 { // It's a file
                    if let Some(folder) = current.as_folder_mut() {
                        folder.insert(part.clone(), Node::File(part.to_string()));
                    }
                } else { // It's a folder
                    current = current.as_folder_mut().unwrap().entry(part)
                        .or_insert_with(|| Node::Folder(HashMap::new()));
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
            ui.with_layout(Layout::from_main_dir_and_cross_align(egui::Direction::TopDown, egui::Align::Center), |ui| {
                self.display_path(ui, &self.root, "".to_string());
            });
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
                    // let full_path = if current_path.is_empty() {
                    //     label.clone()
                    // } else {
                    //     format!("{}/{}", current_path, label)
                    // };

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
                            
                            let res = popup_below_widget(ui, format!("sub_menu-{:?}",label).into(), &selectable_label, CloseOnClickOutside, |ui| {
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
                        
                        let res = popup_below_widget(ui, format!("sub_menu-{:?}",label).into(), &selectable_label, CloseOnClickOutside, |ui| {
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

    fn path_lookup(&self, file_name: &str) -> Option<String> {
        self.paths.iter()
            .find(|path| path.ends_with(file_name))
            .cloned() // returns a clone of the matching path, if found
    }

    pub fn upload(&self, path: String) {
        let task = rfd::AsyncFileDialog::new().pick_files();
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
            
            let files = task.await.unwrap();
            for file_handle in files {
                let bytes: Vec<u8> = file_handle.read().await;
            }

            let mut action = CreateMultipartUpload::new(&bucket, Some(&credentials), &path);
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

            let file = task.await;
            while let Some(item) = byte_stream.next().await{
                let chunk = item.unwrap();
                tx.try_send((chunk.to_vec(), content_length));


                downloaded_bytes += chunk.len() as u64;
                if downloaded_bytes == content_length {
                    if let Some(ref file) = file {
                        file.write(&chunk.to_vec().as_slice()).await.unwrap();
                    }
                }
            }
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

            let file = task.await;
            while let Some(item) = byte_stream.next().await{
                let chunk = item.unwrap();
                tx.try_send((chunk.to_vec(), content_length));


                downloaded_bytes += chunk.len() as u64;
                if downloaded_bytes == content_length {
                    if let Some(ref file) = file {
                        file.write(&chunk.to_vec().as_slice()).await.unwrap();
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