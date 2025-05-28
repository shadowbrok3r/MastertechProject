use chrono::Utc;
use surrealdb::{sql::Datetime, RecordId};
use super::ChatMessageType;
use crate::{schema::USER_MESSAGE_TABLE, DATABASE};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct UserMessage {
    pub id: RecordId,
    pub thread_id: RecordId,
    pub created_at: Datetime,
    pub user: RecordId,
    pub content: ChatMessageType
}


impl UserMessage {
    pub fn new(thread_id: RecordId, user: RecordId, content: ChatMessageType) -> Self {
        Self {
            id: RecordId::from((USER_MESSAGE_TABLE, surrealdb::RecordIdKey::from_inner(surrealdb::sql::Id::rand().into()))), 
            created_at: Utc::now().into(),
            thread_id,
            user,
            content,
        }
    }

    pub async fn create_message(self) -> anyhow::Result<Option<Self>, anyhow::Error> {
        log::info!("Creating message: {:?}", &self);
        let message_record: Option<Self> = DATABASE
            .create(USER_MESSAGE_TABLE)
            .content(self.clone())
            .await?;

        Ok(message_record)
    }

    pub async fn update_message(&mut self) -> anyhow::Result<Option<Self>, anyhow::Error> {
        let message_record: Option<Self> = DATABASE
            .update(self.id.clone())
            .content(self.clone())
            .await?;

        Ok(message_record)
    }

    pub async fn delete_message(&mut self) -> anyhow::Result<Option<Self>, anyhow::Error> {
        let message_record: Option<Self> = DATABASE
            .delete(self.id.clone())
            .await?;

        Ok(message_record)
    }

    pub async fn load_messages_from_thread(thread_id: RecordId) -> anyhow::Result<Vec<Self>, anyhow::Error> {
        let messages: Vec<Self> = DATABASE
            .query("SELECT * FROM user_message WHERE thread_id = $thread_id")
            .bind(("thread_id", thread_id.clone()))
            .await?
            .take(0)?;

        Ok(messages)
    }
}