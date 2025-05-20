pub mod messages;
pub mod threads;
pub use messages::*;
pub use threads::*;

pub type ImageType = (String, bytes::Bytes);

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum ChatAction {
    SelectThread(surrealdb::RecordId),
    NewThread(surrealdb::RecordId),
    CreateGroupThread(Vec<surrealdb::RecordId>), // New action for group chats
    SubmitMessage(ChatMessageType),
    UpdateMessage(surrealdb::RecordId),
    DeleteMessage(surrealdb::RecordId),
    AddUser(surrealdb::RecordId),
    RemoveUser(surrealdb::RecordId),
    UpdateChat(surrealdb::RecordId),
    ArchiveChat(surrealdb::RecordId),
    RemoveChat(surrealdb::RecordId),
    OpenModal((bool, String)),
    Edit(surrealdb::RecordId),
    CancelEdit(surrealdb::RecordId),
    SaveNote(UserMessage),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub enum ChatMessageType {
    Text(String),
    Image(ImageType),
}

impl Default for ChatMessageType {
    fn default() -> Self {
        Self::Text(String::new())
    }
}
