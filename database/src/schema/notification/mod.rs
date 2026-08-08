use std::fmt::Display;

use structdiff::{Difference, StructDiff};
use serde::{Deserialize, Serialize};
use surrealdb::types::Value;
use surrealdb_types::Datetime;

use crate::db;

use super::{random_record_id, RecordId, RecordIdExt, SurrealValue, NOTIFICATION_TABLE, USER_TABLE};


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
        let notif: Value = db()
            .query("CREATE notification CONTENT $notif")
            .bind(("notif", self.clone()))
            .await?
            .take(0)?;

        log::info!(
            "Created notification '{}' for {}",
            self.notification_type,
            self.user.key_string()
        );
        log::debug!("Created notification: {notif:?}");

        Ok(())
    }

    /// Upserts this session's live-query canary (record id fixed per
    /// user+session so concurrent tabs never clobber each other's nonce)
    /// stamped with `nonce`. The `live_query_check` type tells the client to
    /// confirm live-stream liveness without surfacing a toast.
    pub async fn send_live_query_canary(
        user: RecordId,
        session: &str,
        nonce: String,
    ) -> anyhow::Result<()> {
        let id = RecordId::new(
            NOTIFICATION_TABLE,
            format!("canary_{}_{}", user.key_string(), session),
        );
        db()
            .query("UPSERT $id SET user = $user, notification_type = 'live_query_check', notification_description = $nonce, status = 'Read', created_at = time::now()")
            .bind(("id", id))
            .bind(("user", user))
            .bind(("nonce", nonce))
            .await?;
        Ok(())
    }

    /// Deletes this user's canary records older than a day (dead sessions).
    pub async fn purge_stale_canaries(user: RecordId) -> anyhow::Result<()> {
        db()
            .query("DELETE notification WHERE notification_type = 'live_query_check' AND user = $user AND created_at < time::now() - 1d")
            .bind(("user", user))
            .await?;
        Ok(())
    }
}