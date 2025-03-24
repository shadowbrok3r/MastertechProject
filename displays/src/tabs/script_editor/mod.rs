
use crossbeam::channel::{Receiver, Sender};
use itertools::Itertools;
use crate::virtual_filesystem::FileSystem;
use serde::Serialize;

pub mod ui;
pub mod action;

#[derive(Serialize, Clone)]
pub enum ScriptEditorAction {

}


#[derive(Serialize, Clone)]
pub struct ScriptEditor {
    #[serde(skip)]
    _action_tx: Sender<ScriptEditorAction>,
    #[serde(skip)]
    _action_rx: Receiver<ScriptEditorAction>,
    code: String,
    script_name: String,
    open_notification_modal: bool,
    open_file_browser: bool,
    first_run: bool,
    #[serde(skip)]
    filesystem: FileSystem,
    notification_text: String
}


impl ScriptEditor {
    pub fn new() -> Self {
        let (_action_tx, _action_rx) = crossbeam::channel::unbounded();

        Self { 
            _action_tx, _action_rx,
            code: Default::default(),
            script_name: Default::default(),
            open_notification_modal: false,
            open_file_browser: true,
            first_run: true,
            filesystem: FileSystem::new(),
            notification_text: String::new()
         }
    }

    pub fn set_code(&mut self, code: String) -> &mut Self {
        self.code = code;
        self
    }

    pub fn open_save_dialog(&mut self) -> &mut Self {
        self.open_notification_modal = true;
        self
    }

    pub fn save_file(&mut self) -> &mut Self {
        if self.script_name.len() > 0 {
            self.filesystem.upload_script(
                self.script_name.clone(), 
                self.code.clone()
            );
        }
        self
    }

    pub fn set_working_folder(&mut self) -> &mut Self {
        let item = &mut None;
        {
            let selected = self.filesystem.selected_items.try_borrow();
            if let Ok(items) = selected.as_deref() {
                let item_vec = items.iter().cloned().collect_vec();
                if item_vec.len() == 1 {
                    *item = Some(item_vec[0].clone());
                }
                log::info!("Opened folder: {:?}\n{:?}", self.filesystem.current_prefix, items);
            }
        }

        if let Some(item) = item {
            self.filesystem.navigate_to(item.to_string());
        }
        // self.filesystem.request_contents("");
        self
    }
}