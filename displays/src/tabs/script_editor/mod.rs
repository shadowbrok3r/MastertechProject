
use crossbeam::channel::{Receiver, Sender};
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
    action_tx: Sender<ScriptEditorAction>,
    #[serde(skip)]
    action_rx: Receiver<ScriptEditorAction>,
    code: String,
    script_name: String,
    open_save_modal: bool,
    first_run: bool,
    #[serde(skip)]
    filesystem: FileSystem
}


impl ScriptEditor {
    pub fn new() -> Self {
        let (action_tx, action_rx) = crossbeam::channel::unbounded();

        Self { 
            action_tx, action_rx,
            code: Default::default(),
            script_name: Default::default(),
            open_save_modal: false,
            first_run: true,
            filesystem: FileSystem::new(),
         }
    }

    pub fn set_code(&mut self, code: String) -> &mut Self {
        self.code = code;
        self
    }

    pub fn open_save_dialog(&mut self) -> &mut Self {
        self.open_save_modal = true;
        self
    }

    pub fn save_file(&mut self) -> &mut Self {
        self.open_save_modal = true;
        self
    }

    pub fn set_working_folder(&mut self) -> &mut Self {
        self.filesystem.request_contents("");
        self
    }
}