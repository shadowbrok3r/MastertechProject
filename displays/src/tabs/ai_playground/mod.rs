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

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct ChatMessage {
    pub id: String,
    pub thread_id: String,
    pub ts: i32,
    pub from: SentFrom,
    pub content: ChatMessageType,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub enum SentFrom {
    #[default]
    Me,
    Gpt,
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
