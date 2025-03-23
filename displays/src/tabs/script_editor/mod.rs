use crate::virtual_filesystem::FileSystem;
use serde::Serialize;
use log::info;

pub mod ui;

#[derive(Serialize)]
pub struct ScriptEditor {
    code: String,
    script_name: String,
    open_save_modal: bool,
    #[serde(skip)]
    filesystem: FileSystem,
    first_run: bool
}


impl ScriptEditor {
    pub fn new() -> Self {
        Self { 
            code: Default::default(),
            script_name: Default::default(),
            open_save_modal: false,
            filesystem: FileSystem::new(),
            first_run: true,
         }
    }

    pub fn set_filesystem(&mut self, filesystem: FileSystem) -> &mut Self {
        // filesystem.set_user(user);
        info!("{:?}", filesystem.request_contents(""));
        // filesystem.navigate_to("Scripts".to_string());
        info!("ROOT FOR SCRIPT EDITOR: {:?}", self.filesystem.root);
        self.filesystem = filesystem;
        self
    }
}