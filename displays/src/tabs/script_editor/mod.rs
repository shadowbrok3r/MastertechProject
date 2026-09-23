
use crossbeam::channel::{Receiver, Sender};
use itertools::Itertools;
use crate::virtual_filesystem::FileSystem;
use serde::Serialize;

pub mod ui;
pub mod action;

#[derive(Serialize, Clone)]
pub enum ScriptEditorAction {}

#[derive(Serialize, Clone)]
pub struct ScriptEditor {
    #[serde(skip)]
    _action_tx: Sender<ScriptEditorAction>,
    #[serde(skip)]
    _action_rx: Receiver<ScriptEditorAction>,
    pub code: String,
    pub script_name: String,
    open_notification_modal: bool,
    open_file_browser: bool,
    first_run: bool,
    #[serde(skip)]
    filesystem: FileSystem,
    notification_text: String,

    /// AI generation popup state
    pub show_ai_popup: bool,
    pub ai_prompt: String,
    pub ai_generating: bool,
    #[serde(skip)]
    ai_result_rx: Option<Receiver<AiGenResult>>,
}

#[derive(Clone)]
pub enum AiGenResult {
    Chunk(String),
    Done,
    Error(String),
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
            notification_text: String::new(),
            show_ai_popup: false,
            ai_prompt: String::new(),
            ai_generating: false,
            ai_result_rx: None,
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
        if !self.script_name.is_empty() {
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
        self
    }

    /// Drains all available streaming chunks from the AI channel into the editor
    pub fn poll_ai_result(&mut self) {
        if let Some(rx) = &self.ai_result_rx {
            while let Ok(result) = rx.try_recv() {
                match result {
                    AiGenResult::Chunk(text) => {
                        self.code.push_str(&text);
                    }
                    AiGenResult::Done => {
                        let trimmed = self.code.trim().to_string();
                        let trimmed = trimmed.strip_prefix("```powershell")
                            .or_else(|| trimmed.strip_prefix("```ps1"))
                            .or_else(|| trimmed.strip_prefix("```"))
                            .unwrap_or(&trimmed);
                        let trimmed = trimmed.strip_suffix("```").unwrap_or(trimmed);
                        self.code = trimmed.trim().to_string();
                        self.ai_generating = false;
                        self.ai_result_rx = None;
                        return;
                    }
                    AiGenResult::Error(e) => {
                        self.notification_text = format!("AI error: {e}");
                        self.open_notification_modal = true;
                        self.ai_generating = false;
                        self.ai_result_rx = None;
                        return;
                    }
                }
            }
        }
    }

    /// Asks the technician's agent session for a script and loads its code block into the editor.
    #[cfg(any(target_arch = "wasm32", feature = "tokio"))]
    pub fn generate_script_from_prompt(&mut self) {
        use crate::Spawner;
        let prompt = self.ai_prompt.trim().to_string();
        if prompt.is_empty() {
            return;
        }
        let Some(email) = crate::get_current_user_from_auth().map(|u| u.get_email().to_string()) else {
            self.notification_text = "Sign in to ask the agent for a script.".to_string();
            self.open_notification_modal = true;
            return;
        };

        let (tx, rx) = crossbeam::channel::unbounded();
        self.ai_result_rx = Some(rx);
        self.ai_generating = true;
        self.code.clear();

        crate::PlatformSpawner::spawn(async move {
            let session = database::schema::general_connection(&email);
            let request = format!("{SCRIPT_BRIEF}\n\n{prompt}");
            match database::agent_chat::ask(&session, Some(&email), &request, SCRIPT_TIMEOUT).await {
                Ok(reply) => {
                    let _ = tx.send(AiGenResult::Chunk(database::agent_chat::code_block(&reply)));
                    let _ = tx.send(AiGenResult::Done);
                }
                Err(e) => {
                    let _ = tx.send(AiGenResult::Error(e.to_string()));
                }
            }
        });
    }
}

/// Instruction sent ahead of the technician's description.
#[cfg(any(target_arch = "wasm32", feature = "tokio"))]
const SCRIPT_BRIEF: &str = "Write a PowerShell script for a Windows bench technician. Reply with only the script, \
    clean and commented, in one ```powershell block. Write it; do not run anything.";

#[cfg(any(target_arch = "wasm32", feature = "tokio"))]
const SCRIPT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);
