use crate::{schema::{random_record_id, Datetime, RecordId, SurrealValue, User, CHAT_THREAD_TABLE}, db};
use chrono::Utc;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, SurrealValue)]
pub struct ChatThread {
    /// ID
    pub id: RecordId,
    /// All users in the thread
    pub thread_users: Vec<RecordId>,
    /// Owner of the thread
    pub user_created: RecordId,
    pub created_at: Datetime
}

impl Default for ChatThread {
    fn default() -> Self {
        Self {
             id: random_record_id(CHAT_THREAD_TABLE), 
             thread_users: Default::default(),
             user_created: User::default().get_id(),
             created_at: Utc::now().into()
        }
    }
}

impl ChatThread {
    pub fn new(user_created: User) -> Self {
        Self {
             id: random_record_id(CHAT_THREAD_TABLE), 
             thread_users: vec![user_created.id.clone()],
             user_created: user_created.get_id(),
             created_at: Utc::now().into()
        }
    }

    pub fn insert_user_to_thread(self, user_id: RecordId) -> Self {
        let _ = self.thread_users.iter()
            .find(|t| **t == user_id)
            .get_or_insert(&user_id);
        self
    }

    pub fn remove_user_from_thread(&mut self, user: User) {
        self.thread_users.retain(|u| *u != user.get_id());
    }

    pub fn get_thread_id(&self) -> RecordId {
        self.id.clone()
    }

    pub fn get_thread_created_at(&self) -> Datetime {
        self.created_at.clone()
    }

    pub async fn get_thread_users(&self) -> anyhow::Result<Vec<User>, anyhow::Error> {
        let usrs = &mut vec![];
        for user in &self.thread_users {
            let usr_record: Option<User> = db()
                .query("SELECT * FROM user WHERE id == $id")
                .bind(("id", user.clone()))
                .await?
                .take(0)?;

            if let Some(user) = usr_record {
                usrs.push(user.clone());
            }
        }

        Ok(usrs.clone())
    }

    pub async fn get_thread_owner(&self) -> anyhow::Result<Option<User>, anyhow::Error> {
        let usr_record: Option<User> = db()
            .query("SELECT * FROM user WHERE id == $id")
            .bind(("id", self.user_created.clone()))
            .await?
            .take(0)?;

        Ok(usr_record)
    }

    pub async fn get_thread_from_id(id: RecordId) -> anyhow::Result<Option<Self>, anyhow::Error> {
        let thread_record: Option<Self> = db()
            .query("SELECT * FROM chat_thread WHERE id == $id")
            .bind(("id", id.clone()))
            .await?
            .take(0)?;

        Ok(thread_record)
    }

    pub async fn submit_user_to_thread(&mut self, user: User) -> anyhow::Result<Option<Self>, anyhow::Error> {
        let thread_record: Option<Self> = db()
            .query("UPDATE $id SET thread_users += $user")
            .bind(("id", self.id.clone()))
            .bind(("user", user.clone()))
            .await?
            .take(0)?;

        Ok(thread_record)
    }

    pub async fn create_thread(self) -> anyhow::Result<Option<Self>, anyhow::Error> {
        log::info!("Creating thread: {:?}", &self);
        let thread_record: Option<Self> = db()
            .create(CHAT_THREAD_TABLE)
            .content(self.clone())
            .await?;

        Ok(thread_record)
    }

    pub async fn update_thread(&mut self) -> anyhow::Result<Option<Self>, anyhow::Error> {
        let message_record: Option<Self> = db()
            .update(self.id.clone())
            .content(self.clone())
            .await?;

        Ok(message_record)
    }

    pub async fn delete_thread(&mut self) -> anyhow::Result<Option<Self>, anyhow::Error> {
        let message_record: Option<Self> = db()
            .delete(self.id.clone())
            .await?;

        Ok(message_record)
    }

    pub async fn load_threads(user_id: RecordId) -> anyhow::Result<Vec<Self>, anyhow::Error> {
        let query = "SELECT * FROM chat_thread WHERE thread_users CONTAINS $user_id";
        let threads: Vec<Self> = db()
            .query(query)
            .bind(("user_id", user_id))
            .await?
            .take(0)?;

        Ok(threads)
    }

    pub fn new_group(user_created: User, users: Vec<RecordId>) -> Self {
        let mut thread = Self {
            id: random_record_id(CHAT_THREAD_TABLE),
            thread_users: vec![user_created.get_id()],
            user_created: user_created.get_id(),
            created_at: Utc::now().into(),
        };
        for user_id in users {
            thread = thread.insert_user_to_thread(user_id);
        }
        thread
    }

    /// Finds an existing thread for the given users or creates a new one.
    /// For one-on-one chats, users should contain exactly two RecordIds.
    pub async fn find_or_create_thread(
        user_created: User,
        users: Vec<RecordId>,
    ) -> anyhow::Result<Self> {
        // Ensure users includes the creator and is exactly two users for one-on-one
        let mut thread_users = users;
        let user_id = user_created.get_id();
        if !thread_users.contains(&user_id) {
            thread_users.push(user_id.clone());
        }
        if thread_users.len() != 2 {
            return Err(anyhow::anyhow!("One-on-one threads must have exactly two users"));
        }

        // Sort users to ensure consistent querying (e.g., [A, B] == [B, A])
        thread_users.sort();

        // Query for an existing thread with exactly these users
        let existing_thread: Option<Self> = db()
            .query("SELECT * FROM chat_thread WHERE thread_users = $users")
            .bind(("users", thread_users.clone()))
            .await?
            .take(0)?;

        if let Some(thread) = existing_thread {
            Ok(thread)
        } else {
            // Create a new thread
            let new_thread = Self {
                id: random_record_id(CHAT_THREAD_TABLE),
                thread_users,
                user_created: user_id,
                created_at: Utc::now().into(),
            };

            log::info!("New Thread: {new_thread:?}");

            let created_thread = db()
                .create(new_thread.clone().id)
                .content(new_thread.clone())
                .await?;

            // log::warn!("CreatedThread: {created_thread:?}");
            
            created_thread.ok_or_else(|| anyhow::anyhow!("Failed to create thread"))
        }
    }
}