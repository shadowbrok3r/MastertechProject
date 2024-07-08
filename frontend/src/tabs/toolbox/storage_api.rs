use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use egui::collapsing_header::CollapsingState;
use egui::{Layout, RichText, ScrollArea, Ui};
use log::info;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct FileSystem {
    root: Node,
    /// HashSet of selected files (hold CTRL key to select multiple)
    selected_items: RefCell<HashSet<String>>, 
}

#[derive(Debug, Serialize)]
pub enum Node {
    File(String),
    Folder(HashMap<String, Node>),
}

impl FileSystem {
    pub fn new() -> Self {
        Self {
            root: Node::Folder(HashMap::new()),
            selected_items: RefCell::new(HashSet::new()),
        }
    }

    pub fn build_file_system(&mut self, paths: Vec<String>) {
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
                self.display_path(ui, &self.root);
            });
        });
    }

    fn display_path(&self, ui: &mut Ui, node: &Node) {      
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

                for (label, node) in entries{
                    if let Node::Folder(_) = node {
                        let id = ui.make_persistent_id(format!("{label}+++{:?}", count + 1));
                        CollapsingState::load_with_default_open(ui.ctx(), id, false)
                        .show_header(ui, |ui| 
                        {
                            let is_selected = self.selected_items.borrow().contains(label);
                            let selectable_label = ui.selectable_label(is_selected, RichText::new(format!("🗀   {}", label)));
                        
                            if selectable_label.clicked() { // If the item was already selected, deselect it
                                if self.selected_items.borrow().contains(label) { 
                                    self.selected_items.borrow_mut().remove(label); 
                                } 
                            }
                        })
                        .body(|ui| self.display_path(ui, &node));

                    } else if let Node::File(label) = node{
                        let is_selected = self.selected_items.borrow().contains(label);
                        let selectable_label = ui.selectable_label(is_selected, RichText::new(format!("🗋   {}", label)));
                        if selectable_label.double_clicked() {

                        }
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