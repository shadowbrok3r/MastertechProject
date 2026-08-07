use bytes::Bytes;
use serde::{Deserialize, Serialize};

pub mod enhanced;

pub type ImageType = (String, Bytes);

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChatThread {
    pub id: String,
    pub messages: Vec<ChatMessage>,
    pub images: Vec<ImageType>,
    pub input: String,
}

/// Unix seconds for a chat message.
pub fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    pub id: String,
    pub thread_id: String,
    /// Unix seconds. i64 so it survives 2038.
    pub ts: i64,
    pub from: SentFrom,
    pub content: ChatMessageType,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum SentFrom {
    #[default]
    Me,
    /// Any assistant engine. `Gpt` is the pre-Claude name kept for stored rows.
    #[serde(alias = "Gpt")]
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ChatMessageType {
    Text(String),
    /// Streamed model reasoning ("thinking") tokens, rendered in a collapsible block.
    Reasoning(String),
    FileId(String),
    Code(String),
    Image(ImageType),
    Error(String),
    Done,
}

impl Default for ChatMessageType {
    fn default() -> Self {
        ChatMessageType::Text(String::new())
    }
}
