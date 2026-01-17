use std::fmt::Display;

use structdiff::{Difference, StructDiff};
use serde::{Deserialize, Serialize};
use surrealdb::types::Value;
use surrealdb_types::Datetime;

use crate::DATABASE;

use super::{random_record_id, RecordId, SurrealValue, NOTIFICATION_TABLE, USER_TABLE};


#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Difference, SurrealValue)]
pub struct Notification {
    pub id: RecordId,
    /// receiver of notification
    pub user: RecordId,
    /// description of notification
    pub notification_description: String,
    /// type of notification
    pub notification_type: String,
    /// Has the notification been read?
    pub status: String,
    /// Has the notification been read?
    pub created_at: Datetime,
    /// Time the notification was accessed
    pub accessed_at: Option<Datetime>,
}

impl Default for Notification {
    fn default() -> Self {
        Self {
            id: random_record_id(NOTIFICATION_TABLE),
            user: random_record_id(USER_TABLE),
            notification_description: Default::default(),
            notification_type: Default::default(),
            status: Default::default(),
            created_at: Datetime::now(),
            accessed_at: None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, SurrealValue)]
pub enum NotificationType {
    NewMessage,
    SpoStatusChange,
    NewTask,
    TaggedInComment,
    GroupTag,
    OverdueTask,
}

#[derive(Serialize, Deserialize, Clone, Debug, SurrealValue)]
pub enum NotificationStatus {
    Read,
    Unread,
}

#[derive(Serialize, Deserialize, Clone, Debug, SurrealValue)]
pub struct ModifyNotification {
    pub id: RecordId,
    /// either Read or Unread
    pub status: Option<NotificationStatus>,
    pub mark_all_read: Option<bool>,
    pub mark_all_unread: Option<bool>,
    pub archive: Option<bool>,
}

impl Notification {
    pub fn set_description(&mut self, description: impl Display) -> &mut Self{
        self.notification_description = description.to_string();
        self
    }

    pub fn set_type(&mut self, notif_type: impl Display) -> &mut Self{
        self.notification_type = notif_type.to_string();
        self
    }

    pub async fn create(&self) -> anyhow::Result<(), anyhow::Error> {
        let notif: Value = DATABASE
            .query("CREATE notification CONTENT $notif")
            .bind(("notif", self.clone()))
            .await?
            .take(0)?;

        log::info!("Created notification: {notif:?}");

        Ok(())
    }
}