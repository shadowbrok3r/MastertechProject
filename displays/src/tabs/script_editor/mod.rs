
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

    /// Spawns an async task that streams a script from the AI into the editor
    #[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
    pub fn generate_script_from_prompt(&mut self) {
        use crate::Spawner;
        let prompt = self.ai_prompt.clone();
        if prompt.trim().is_empty() {
            return;
        }

        let (tx, rx) = crossbeam::channel::unbounded();
        self.ai_result_rx = Some(rx);
        self.ai_generating = true;
        self.code.clear();

        crate::PlatformSpawner::spawn(async move {
            if let Err(e) = ai_generate_script_streaming(&prompt, tx.clone()).await {
                let _ = tx.send(AiGenResult::Error(e.to_string()));
            }
        });
    }

    #[cfg(target_arch = "wasm32")]
    pub fn generate_script_from_prompt(&mut self) {
        self.notification_text = "AI generation not available in browser".to_string();
        self.open_notification_modal = true;
    }
}

#[cfg(all(not(target_arch = "wasm32"), feature = "tokio"))]
async fn ai_generate_script_streaming(
    prompt: &str,
    tx: crossbeam::channel::Sender<AiGenResult>,
) -> anyhow::Result<()> {
    use crate::ai::oa_client::new_oa_client;
    use crate::openai::types::chat::{
        CreateChatCompletionRequestArgs, ChatCompletionRequestUserMessageArgs,
        ChatCompletionRequestSystemMessageArgs,
    };
    use crate::ai::gpts::MODEL;
    use futures::StreamExt;

    let client = new_oa_client()?;
    let system_msg = ChatCompletionRequestSystemMessageArgs::default()
        .content(
            "You are a PowerShell script generator for Windows IT technicians. \
             Output ONLY the raw script code, no markdown fences, no explanations. \
             Write clean, commented PowerShell."
        )
        .build()?;
    let user_msg = ChatCompletionRequestUserMessageArgs::default()
        .content(prompt)
        .build()?;

    let request = CreateChatCompletionRequestArgs::default()
        .model(MODEL)
        .messages(vec![system_msg.into(), user_msg.into()])
        .temperature(0.3f32)
        .build()?;

    let mut stream = client.chat().create_stream(request).await?;

    while let Some(result) = stream.next().await {
        match result {
            Ok(response) => {
                for choice in &response.choices {
                    if let Some(ref content) = choice.delta.content {
                        let _ = tx.send(AiGenResult::Chunk(content.clone()));
                    }
                }
            }
            Err(e) => {
                let _ = tx.send(AiGenResult::Error(e.to_string()));
                return Ok(());
            }
        }
    }

    let _ = tx.send(AiGenResult::Done);
    Ok(())
}