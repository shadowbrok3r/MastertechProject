use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::iter::FromIterator;
use std::path::PathBuf;
use std::thread::current;

use egui::collapsing_header::CollapsingState;
use egui::{Color32, RichText, TextFormat, Ui, WidgetText};
use log::info;

#[derive(Debug)]
pub enum Node {
    File(String),
    Folder(HashMap<String, Node>),
}

#[derive(Debug)]
pub struct FileSystem {
    root: Node,
    /// HashSet of selected files (hold CTRL key to select multiple)
    selected_items: RefCell<HashSet<String>>, 
    /// HashMap of subcontents of a given dir
    dir_contents: RefCell<HashMap<String, Vec<String>>>, 
}

impl FileSystem {
    pub fn new() -> Self {
        Self {
            root: Node::Folder(HashMap::new()),
            selected_items: RefCell::new(HashSet::new()),
            dir_contents: RefCell::new(HashMap::new()),
        }
    }

    pub fn build_file_system(&mut self, paths: Vec<String>) {
        for path in paths {
            let parts: Vec<&str> = path.split('/').collect();
            let mut current_node = &mut self.root;

            for (i, part) in parts.iter().enumerate() {
                // if part.contains('.'){
                //     info!("FILE: {part}");
                // }
                if i == parts.len() - 1 {
                    // Last part, treat it as a file
                    match current_node {
                        Node::Folder(children) => {
                            // info!("children: {children:?}");
                            children.insert(part.to_string(), Node::File(part.to_string()));
                        }
                        _ => panic!("Unexpected node type"),
                    }
                } else {
                    // Intermediate part, treat it as a folder
                    match current_node {
                        Node::Folder(children) => {
                            if !children.contains_key(*part) {
                                // info!("!children.contains_key(*part) ");
                                children.insert(part.to_string(), Node::Folder(HashMap::new()));
                            }
                            current_node = children.get_mut(*part).unwrap();
                            // info!("Current Node: {current_node:?}");
                        }
                        _ => panic!("Unexpected node type"),
                    }
                }
            }
        }
    }

    pub fn display(&self, ui: &mut Ui) {
        self.display_path(ui, &self.root);
    }

    fn display_path(
        &self,
        ui: &mut Ui,
        node: &Node
    ){      
        ui.horizontal_top(|ui| 
        {
            info!("self.root: {:?}", self.root);

            
            match node {
                Node::File(label) => {
                    let is_selected = self.selected_items.borrow().contains(label);
                    let modifiers = ui.input(|i| i.modifiers); // Get the current modifiers
                    let selectable_label = ui.selectable_label(is_selected, RichText::new(format!("🗋   {}", label)));
                    if selectable_label.clicked() {

                    }
                },
                Node::Folder(children) => {
                    ui.vertical_centered_justified(|ui| {
                        for (label, node) in children.iter(){
                            let id = ui.make_persistent_id(label);
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
        
                            }).body(|ui| 
                            {
                                self.display_path(
                                    ui,
                                    &node,
                                );
                            });
                        }
                    });
                }
            }
        });
    }
}

