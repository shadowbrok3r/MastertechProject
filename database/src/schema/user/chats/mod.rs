pub mod messages;
pub mod threads;
pub use messages::*;
use rfd::FileHandle;
pub use threads::*;

use crate::schema::{RecordId, SurrealValue};
use surrealdb_types::{Kind, Value, Object, Array};

pub type ImageType = (String, bytes::Bytes);

// Note: ChatAction doesn't derive SurrealValue because FileHandle doesn't implement it
// We skip the UploadedFiles variant in serde, but the derive macro still requires all types to impl SurrealValue
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ChatAction {
    SelectThread(RecordId),
    NewThread(RecordId),
    CreateGroupThread(Vec<RecordId>), // New action for group chats
    SubmitMessage(ChatMessageType),
    UpdateMessage(RecordId),
    DeleteMessage(RecordId),
    RemoveUser(RecordId),
    UpdateChat(RecordId),
    ArchiveChat(RecordId),
    RemoveChat(RecordId),
    AddUser(RecordId),
    OpenImage(String),
    Edit(RecordId),
    CancelEdit(RecordId),
    SaveNote(UserMessage),
    #[serde(skip)]
    UploadedFiles(Vec<FileHandle>)
}

// Manual implementation of SurrealValue for ChatAction
impl SurrealValue for ChatAction {
    fn kind_of() -> Kind {
        Kind::Any
    }
    
    fn into_value(self) -> Value {
        // Serialize to JSON and then convert to Value
        match serde_json::to_value(&self) {
            Ok(json) => json_to_surreal_value(json),
            Err(_) => Value::None,
        }
    }
    
    fn from_value(value: Value) -> surrealdb_types::anyhow::Result<Self> {
        // Convert Value to JSON and then deserialize
        let json = surreal_value_to_json(value);
        serde_json::from_value(json).map_err(|e| surrealdb_types::anyhow::anyhow!(e))
    }
}

// Helper to convert serde_json::Value to surrealdb_types::Value
fn json_to_surreal_value(json: serde_json::Value) -> Value {
    match json {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(b) => Value::Bool(b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Number(i.into())
            } else if let Some(f) = n.as_f64() {
                Value::Number(f.into())
            } else {
                Value::None
            }
        },
        serde_json::Value::String(s) => Value::String(s),
        serde_json::Value::Array(arr) => {
            let mut surreal_arr = Array::new();
            for item in arr {
                surreal_arr.push(json_to_surreal_value(item));
            }
            Value::Array(surreal_arr)
        },
        serde_json::Value::Object(obj) => {
            let mut surreal_obj = Object::new();
            for (k, v) in obj {
                surreal_obj.insert(k, json_to_surreal_value(v));
            }
            Value::Object(surreal_obj)
        },
    }
}

// Helper to convert surrealdb_types::Value to serde_json::Value
fn surreal_value_to_json(value: Value) -> serde_json::Value {
    match value {
        Value::None | Value::Null => serde_json::Value::Null,
        Value::Bool(b) => serde_json::Value::Bool(b),
        Value::Number(n) => serde_json::json!(n),
        Value::String(s) => serde_json::Value::String(s),
        Value::Array(arr) => {
            let json_arr: Vec<serde_json::Value> = arr.into_iter().map(surreal_value_to_json).collect();
            serde_json::Value::Array(json_arr)
        },
        Value::Object(obj) => {
            let mut json_obj = serde_json::Map::new();
            for (k, v) in obj.into_iter() {
                json_obj.insert(k, surreal_value_to_json(v));
            }
            serde_json::Value::Object(json_obj)
        },
        _ => serde_json::Value::Null,
    }
}



#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, SurrealValue)]
pub enum ChatMessageType {
    Text(String),
    Image(ImageType),
}

impl Default for ChatMessageType {
    fn default() -> Self {
        Self::Text(String::new())
    }
}
