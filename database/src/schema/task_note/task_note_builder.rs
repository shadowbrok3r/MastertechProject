use crate::{
    schema::{
        helper_traits::EmployeeHelper, prestashop_schema::{CustomerMessage, CustomerThread, Employee, Order, Prestashop}, Datetime, LiveTaskPayload, Notification, Record, RecordId, RecordIdExt, SurrealValue, User, TASK_NOTE_TABLE
    },
    DATABASE,
};
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use regex::Regex;
use anyhow::Result;


#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, SurrealValue)]
pub struct TaskNote {
    pub id: RecordId,
    pub task_id: RecordId,
    pub created_at: Datetime,
    pub note: String,
    pub username: String,
    pub id_customer_thread: Option<String>,
    pub id_customer_message: Option<String>,
    pub id_employee: String,
    pub user: RecordId,
    pub service_number: Option<String>,
    pub private: bool,
}

pub struct TaskNoteBuilder {
    task_id: RecordId,
    user: RecordId,
    id_employee: String,
    note: Option<String>,
    username: Option<String>,
    service_number: Option<String>,
    private: bool,
    id_customer_thread: Option<String>,
    created_at: Datetime,
}

impl TaskNoteBuilder {
    /// Initializes the builder with required fields.
    pub fn new(task_id: RecordId, user: RecordId, id_employee: String) -> Self {
        Self {
            task_id,
            user,
            id_employee,
            note: None,
            username: None,
            service_number: None,
            private: false,
            id_customer_thread: None,
            created_at: Utc::now().into(),
        }
    }

    /// Sets the note content (required).
    pub fn note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }

    /// Sets the username. If not set, will be derived from id_employee.
    pub fn username(mut self, username: impl Into<String>) -> Self {
        self.username = Some(username.into());
        self
    }

    /// Sets the service number, indicating a Prestashop-linked note.
    pub fn service_number(mut self, service_number: impl Into<String>) -> Self {
        self.service_number = Some(service_number.into());
        self
    }

    /// Sets whether the note is private (database-only).
    pub fn private(mut self, private: bool) -> Self {
        self.private = private;
        self
    }

    /// Sets the customer thread ID for Prestashop notes.
    pub fn id_customer_thread(mut self, id_customer_thread: impl Into<String>) -> Self {
        self.id_customer_thread = Some(id_customer_thread.into());
        self
    }

    /// Derives username from id_employee if not set.
    async fn ensure_username(&mut self) -> Result<()> {
        if self.username.is_none() {
            let mut employee: Employee = Prestashop::default()
                .request_subresources_by_id_wasm("employees", "employee", &self.id_employee)
                .await?;
            let user = employee.find_user().await?.ok_or_else(|| {
                anyhow::anyhow!("No user found for employee ID {}", self.id_employee)
            })?;
            self.username = Some(user.get_username().to_string());
        }
        Ok(())
    }

    /// Ensures a customer thread exists for Prestashop notes.
    async fn ensure_customer_thread(&mut self) -> Result<()> {
        if self.private || self.service_number.is_none() {
            self.id_customer_thread = None;
            return Ok(());
        }

        if let Some(thread_id) = &self.id_customer_thread {
            if !thread_id.is_empty() {
                return Ok(());
            }
        }

        let service_number = self.service_number.as_ref().ok_or_else(|| {
            anyhow::anyhow!("Service number required for Prestashop note")
        })?;

        let api = Prestashop::default();
        let mut query: HashMap<&str, &str> = HashMap::new();
        query.insert("filter[id_order]", service_number);
        query.insert("output_format", "JSON");

        let threads: Vec<CustomerThread> = api
            .request_resources_wasm("customer_threads", query)
            .await?;

        let thread_id = if threads.is_empty() || threads.iter().any(|t| t.id.is_empty()) {
            let order: Order = api
                .request_subresources_by_id_wasm("orders", "order", service_number)
                .await?;
            let response = api
                .create_customer_thread(service_number, &order.id_customer)
                .await?;
            response.id
        } else {
            threads.first().cloned().unwrap_or_default().id
        };

        self.id_customer_thread = Some(thread_id);
        Ok(())
    }

    /// Checks for existing notes to prevent duplicates.
    async fn check_existing_note(&self, id_customer_message: &str) -> Result<Option<RecordId>> {
        let query_results: Vec<TaskNote> = DATABASE
            .query("SELECT * FROM task_note WHERE id_customer_message == $id_customer_message")
            .bind(("id_customer_message", id_customer_message.to_string()))
            .await?
            .take(0)?;

        if let Some(note) = query_results.first() {
            if note.task_id != self.task_id {
                let task: Option<LiveTaskPayload> = DATABASE
                    .query("SELECT * FROM task WHERE id == $id")
                    .bind(("id", note.task_id.clone()))
                    .await?
                    .take(0)?;

                if task.is_some() {
                    let query = if self.service_number.is_some() {
                        DATABASE.set("service_number", self.service_number.clone().unwrap_or_default()).await?;
                        "UPDATE task_note SET task_id = $new_id, service_number = $service_number WHERE id == $id"
                    } else {
                        "UPDATE task_note SET task_id = $new_id WHERE id == $id"
                    };

                    let updated_note: Option<TaskNote> = DATABASE
                        .query(query)
                        .bind(("id", note.id.clone()))
                        .bind(("new_id", self.task_id.clone()))
                        .await?
                        .take(0)?;

                    return Ok(updated_note.map(|n| n.id));
                }
            }
            Ok(Some(note.id.clone()))
        } else {
            Ok(None)
        }
    }

    /// Creates a notification for tagged users.
    async fn handle_tagged_users(&self, note: &TaskNote) -> Result<()> {
        let re = Regex::new(r"@\b[a-zA-Z]+(\.[a-zA-Z]+)?\b")?;
        let users: Vec<&str> = re.find_iter(&note.note).map(|m| m.as_str()).collect();

        for user_tag in users {
            let name = &user_tag[1..];
            let email = format!("{}@pclaptops.com", name);
            let mut employee = Employee::default();
            employee.email = email;
            if let Some(tagged_user) = employee.find_user().await? {
                let task_name: Option<String> = DATABASE
                    .query("SELECT VALUE task_name FROM task WHERE id == $task_id")
                    .bind(("task_id", note.task_id.clone()))
                    .await?
                    .take(0)?;

                let name = task_name.unwrap_or_else(|| note.task_id.key_string());
                let notification = Notification {
                    notification_description: format!(
                        "tagged {} in task {}",
                        tagged_user.get_username(),
                        name
                    ),
                    notification_type: String::from("Task Update"),
                    status: String::from("Unread"),
                    user: tagged_user.get_id(),
                    ..Default::default()
                };

                DATABASE
                    .query("CREATE notification CONTENT $notif")
                    .bind(("notif", notification))
                    .await?
                    .take::<Option<Record>>(0)?;

                DATABASE
                    .query("UPDATE task_note SET tagged_users += $user_id WHERE id == $id")
                    .bind(("user_id", tagged_user.get_id()))
                    .bind(("id", note.id.clone()))
                    .await?
                    .take::<Option<Record>>(0)?;
            }
        }
        Ok(())
    }

    /// Builds the TaskNote and persists it to the database and/or Prestashop.
    pub async fn build(mut self) -> Result<TaskNote> {
        // Validate required fields
        let note_content = self.note.clone().ok_or_else(|| anyhow::anyhow!("Note content is required"))?;
        self.ensure_username().await?;
        self.ensure_customer_thread().await?;

        let is_prestashop_note = self.service_number.is_some() && !self.private;

        let (id_customer_message, created_at, id) = if is_prestashop_note {
            let id_customer_thread = self.id_customer_thread.as_ref().ok_or_else(|| {
                anyhow::anyhow!("Customer thread ID required for Prestashop note")
            })?;

            let response = Prestashop::default()
                .create_customer_message(
                    &self.id_employee,
                    id_customer_thread,
                    &note_content,
                )
                .await?;

            if let Some(existing_id) = self.check_existing_note(&response.id).await? {
                return Err(anyhow::anyhow!("Note already exists with ID {:?}", existing_id));
            }

            (
                Some(response.id.clone()),
                parse_msg_date(&response.date_add).unwrap_or_else(|_| Utc::now().into()),
                RecordId::new(TASK_NOTE_TABLE, response.id),
            )
        } else {
            (
                None,
                self.created_at.clone(),
                RecordId::new(TASK_NOTE_TABLE, uuid::Uuid::new_v4().to_string()),
            )
        };

        let task_note = TaskNote {
            id,
            task_id: self.task_id.clone(),
            created_at,
            note: note_content,
            username: self.username.clone().unwrap_or(String::new()),
            id_customer_thread: self.id_customer_thread.clone(),
            id_customer_message,
            id_employee: self.id_employee.clone(),
            user: self.user.clone(),
            service_number: self.service_number.clone(),
            private: self.private,
        };

        // Persist to database
        DATABASE
            .query("CREATE task_note CONTENT $task_note")
            .bind(("task_note", task_note.clone()))
            .await?
            .take::<Option<Record>>(0)?;

        // Handle tagged users
        self.handle_tagged_users(&task_note).await?;

        Ok(task_note)
    }
}

impl TaskNote {
    /// Updates the note in the database and/or Prestashop.
    pub async fn update(&mut self) -> Result<()> {
        if let (Some(id_customer_message), Some(id_customer_thread)) = (
            &self.id_customer_message,
            &self.id_customer_thread,
        ) {
            Prestashop::default()
                .modify_customer_message(
                    id_customer_message,
                    &self.id_employee,
                    id_customer_thread,
                    &self.note,
                )
                .await?;
        }

        let _: Option<TaskNote> = DATABASE
            .upsert(self.id.clone())
            .content(self.clone())
            .await?;

        Ok(())
    }

    /// Deletes the note from the database and/or Prestashop.
    pub async fn delete(self) -> Result<()> {
        if let (Some(id_customer_message), Some(id_customer_thread)) = (
            &self.id_customer_message,
            &self.id_customer_thread,
        ) {
            if !id_customer_message.is_empty() && !id_customer_thread.is_empty() {
                Prestashop::default()
                    .delete_resource_wasm("customer_messages", id_customer_message)
                    .await?;
            }
        }

        let _: Option<TaskNote> = DATABASE
            .delete((TASK_NOTE_TABLE, self.id.key_string()))
            .await?;

        Ok(())
    }

    /// Retrieves notes from Prestashop for a service number.
    pub async fn get_prestashop_notes(service_number: &str, task_id: Option<RecordId>) -> Result<Vec<Self>> {
        if service_number.is_empty() {
            return Err(anyhow::anyhow!("Service number is empty"));
        }

        let api = Prestashop::default();
        let mut query: HashMap<&str, &str> = HashMap::new();
        query.insert("filter[id_order]", service_number);
        query.insert("output_format", "JSON");

        let threads: Vec<CustomerThread> = api
            .request_resources_wasm("customer_threads", query)
            .await?;

        let mut notes = vec![];
        for thread in threads {
            for msg in thread.associations.customer_messages {
                let customer_message: CustomerMessage = api
                    .request_subresources_by_id_wasm("customer_messages", "customer_message", &msg.id)
                    .await?;

                if !customer_message.id_employee.is_empty() && customer_message.id_employee != "0" {
                    let employee: Employee = api
                        .request_subresources_by_id_wasm("employees", "employee", &customer_message.id_employee)
                        .await?;

                    let user = User::query_user_from_email(employee.email.clone()).await?;
                    let task_note = TaskNote {
                        id: RecordId::new(TASK_NOTE_TABLE, customer_message.id.clone()),
                        task_id: task_id.clone().ok_or_else(|| {
                            anyhow::anyhow!("Task ID required for Prestashop note")
                        })?,
                        created_at: parse_msg_date(&customer_message.date_add)
                            .unwrap_or_else(|_| Utc::now().into()),
                        note: customer_message.message,
                        username: user.get_username().to_string(),
                        id_customer_thread: Some(thread.id.clone()),
                        id_customer_message: Some(customer_message.id.clone()),
                        id_employee: customer_message.id_employee,
                        user: user.get_id(),
                        service_number: Some(service_number.to_string()),
                        private: false,
                    };

                    if task_id.is_some() {
                        DATABASE
                            .query("CREATE task_note CONTENT $task_note")
                            .bind(("task_note", task_note.clone()))
                            .await?
                            .take::<Option<Record>>(0)?;
                    }
                    notes.push(task_note);
                }
            }
        }
        Ok(notes)
    }

    /// Retrieves database notes for a service number.
    pub async fn get_db_notes_by_service(service_number: &str) -> Result<Vec<Self>> {
        if service_number.is_empty() {
            return Err(anyhow::anyhow!("Service number is empty"));
        }

        let query_results: Vec<Self> = DATABASE
            .query("SELECT * FROM task_note WHERE task_id.service_number == $service_number ")
            .bind(("service_number", service_number.to_string()))
            .await?
            .take(0)?;

        if query_results.is_empty() {
            Ok(DATABASE
                .query("SELECT * FROM task_note WHERE service_number == $service_number ")
                .bind(("service_number", service_number.to_string()))
                .await?
                .take::<Vec<TaskNote>>(0)?)
        } else {
            Ok(query_results)
        }
    }

    /// Retrieves database notes for a task ID.
    pub async fn get_db_notes_by_task_id(task_id: RecordId) -> Result<Vec<Self>> {
        Ok(DATABASE
            .query("SELECT * FROM task_note WHERE task_id == $task_id ")
            .bind(("task_id", task_id))
            .await?
            .take::<Vec<TaskNote>>(0)?)
    }
}

pub fn parse_msg_date(date_str: &str) -> Result<Datetime, chrono::ParseError> {
    let naive_dt = chrono::NaiveDateTime::parse_from_str(date_str, "%Y-%m-%d %H:%M:%S")?;
    let dt_utc = DateTime::<Utc>::from_naive_utc_and_offset(naive_dt, Utc);
    Ok(dt_utc.into())
}

/* 
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        schema::{
            helper_traits::EmployeeHelper,
            prestashop_schema::{CustomerMessage, CustomerThread, Employee, Order, PrestashopResponse},
        },
        DATABASE,
    };
    use async_trait::async_trait;
    use mockall::{mock, predicate::*};
    use surrealdb::RecordId;
    use std::collections::HashMap;
    use Uuid;

    // Mock Prestashop API
    mock! {
        PrestashopApi {}
        #[async_trait]
        impl Prestashop for PrestashopApi {
            async fn request_subresources_by_id_wasm(&self, resource: &str, subresource: &str, id: &str) -> Result<Employee>;
            async fn request_resources_wasm(&self, resource: &str, query: HashMap<&str, &str>) -> Result<Vec<CustomerThread>>;
            async fn create_customer_thread(&self, service_number: &str, id_customer: &str) -> Result<PrestashopResponse>;
            async fn create_customer_message(&self, id_employee: &str, id_customer_thread: &str, message: &str) -> Result<PrestashopResponse>;
            async fn modify_customer_message(&self, id_customer_message: &str, id_employee: &str, id_customer_thread: &str, message: &str) -> Result<PrestashopResponse>;
            async fn delete_resource_wasm(&self, resource: &str, id: &str) -> Result<()>;
        }
    }

    // Mock EmployeeHelper for find_user
    mock! {
        EmployeeHelperMock {}
        #[async_trait]
        impl EmployeeHelper for EmployeeHelperMock {
            async fn find_user(&self) -> Result<Option<User>>;
        }
    }

    // Setup common test data
    fn setup_test_data() -> (RecordId, RecordId, String) {
        let task_id = RecordId::new("task", Uuid::new_v4().to_string());
        let user_id = RecordId::new("user", Uuid::new_v4().to_string());
        let id_employee = "123".to_string();
        (task_id, user_id, id_employee)
    }

    #[tokio::test]
    async fn test_missing_note_fails() {
        let (task_id, user_id, id_employee) = setup_test_data();
        let builder = TaskNoteBuilder::new(task_id, user_id, id_employee);
        let result = builder.build().await;
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Note content is required"
        );
    }

    #[tokio::test]
    async fn test_missing_username_derives_from_employee() {
        let (task_id, user_id, id_employee) = setup_test_data();
        let mut mock_presta = MockPrestashopApi::new();
        let mut mock_employee = MockEmployeeHelperMock::new();
        let mut user = User::default()
            .set_email("test.user@pclaptops.com");

        mock_presta
            .expect_request_subresources_by_id_wasm()
            .with(eq("employees"), eq("employee"), eq(id_employee.as_str()))
            .times(1)
            .returning(|_, _, _| {
                Ok(Employee {
                    email: "test@pclaptops.com".to_string(),
                    firstname: "Test".to_string(),
                    ..Default::default()
                })
            });

        mock_employee
            .expect_find_user()
            .times(1)
            .returning(|| {
                Ok(Some(user))
            });

        let builder = TaskNoteBuilder::new(task_id, user_id, id_employee)
            .note("Test note");

        // Mock DATABASE to avoid actual DB calls
        DATABASE
            .expect_query()
            .times(1)
            .returning(|_| Ok(vec![Record { id: RecordId::new("task_note", Uuid::new_v4().to_string()) }]));
        
        let result = builder.build().await;
        assert!(result.is_ok());
        let note = result.unwrap();
        assert_eq!(note.username, "test.user");
    }

    #[tokio::test]
    async fn test_prestashop_note_with_service_number() {
        let (task_id, user_id, id_employee) = setup_test_data();
        let mut mock_presta = MockPrestashopApi::new();
        let service_number = "12345";
        let thread_id = "67890";
        let message_id = "54321";

        mock_presta
            .expect_request_resources_wasm()
            .with(eq("customer_threads"), always())
            .times(1)
            .returning(|_, _| Ok(vec![CustomerThread { id: thread_id.to_string(), ..Default::default() }]));

        mock_presta
            .expect_create_customer_message()
            .with(eq(id_employee.as_str()), eq(thread_id), eq("Test note"))
            .times(1)
            .returning(|_, _, _| {
                Ok(PrestashopResponse {
                    id: message_id.to_string(),
                    date_add: "2025-05-19 07:00:00".to_string(),
                    ..Default::default()
                })
            });

        DATABASE
            .expect_query()
            .with(always())
            .times(2) // check_existing_note and create
            .returning(|_| Ok(vec![])); // No existing note

        let builder = TaskNoteBuilder::new(task_id.clone(), user_id, id_employee)
            .note("Test note")
            .username("test.user")
            .service_number(service_number)
            .id_customer_thread(thread_id);

        let result.ConcurrentTaskNotes = builder.build().await;
        assert!(result.is_ok());
        let note = result.unwrap();
        assert_eq!(note.id_customer_thread, Some(thread_id.to_string()));
        assert_eq!(note.id_customer_message, Some(message_id.to_string()));
        assert_eq!(note.service_number, Some(service_number.to_string()));
        assert_eq!(note.task_id, task_id);
        assert!(!note.private);
    }

    #[tokio::test]
    async fn test_private_note_no_prestashop() {
        let (task_id, user_id, id_employee) = setup_test_data();

        DATABASE
            .expect_query()
            .times(1)
            .returning(|_| Ok(vec![Record { id: RecordId::new("task_note", Uuid::new_v4().to_string()) }]));

        let builder = TaskNoteBuilder::new(task_id.clone(), user_id, id_employee)
            .note("Private note")
            .username("test.user")
            .private(true);

        let result = builder.build().await;
        assert!(result.is_ok());
        let note = result.unwrap();
        assert_eq!(note.id_customer_thread, None);
        assert_eq!(note.id_customer_message, None);
        assert_eq!(note.service_number, None);
        assert_eq!(note.task_id, task_id);
        assert!(note.private);
    }

    #[tokio::test]
    async fn test_duplicate_prestashop_note_fails() {
        let (task_id, user_id, id_employee) = setup_test_data();
        let mut mock_presta = MockPrestashopApi::new();
        let service_number = "12345";
        let thread_id = "67890";
        let message_id = "54321";

        mock_presta
            .expect_request_resources_wasm()
            .with(eq("customer_threads"), always())
            .times(1)
            .returning(|_, _| Ok(vec![CustomerThread { id: thread_id.to_string(), ..Default::default() }]));

        mock_presta
            .expect_create_customer_message()
            .with(eq(id_employee.as_str()), eq(thread_id), eq("Test note"))
            .times(1)
            .returning(|_, _, _| {
                Ok(PrestashopResponse {
                    id: message_id.to_string(),
                    date_add: "2025-05-19 07:00:00".to_string(),
                    ..Default::default()
                })
            });

        DATABASE
            .expect_query()
            .with(always())
            .times(1)
            .returning(|_| {
                Ok(vec![TaskNote {
                    id: RecordId::new("task_note", message_id),
                    task_id: RecordId::new("task", Uuid::new_v4().to_string()),
                    ..Default::default()
                }])
            });

        let builder = TaskNoteBuilder::new(task_id, user_id, id_employee)
            .note("Test note")
            .username("test.user")
            .service_number(service_number)
            .id_customer_thread(thread_id);

        let result = builder.build().await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Note already exists"));
    }

    #[tokio::test]
    async fn test_tagged_user_notification() {
        let (task_id, user_id, id_employee) = setup_test_data();
        let mut mock_employee = MockEmployeeHelperMock::new();
        let mut user = User::default()
            .set_email("test.user@pclaptops.com");

        mock_employee
            .expect_find_user()
            .times(1)
            .returning(|| {
                Ok(Some(user))
            });

        DATABASE
            .expect_query()
            .with(always())
            .times(3) // create note, task_name, create notification
            .returning(|_| Ok(vec![Record { id: RecordId::new("task_note", Uuid::new_v4().to_string()) }]));

        let builder = TaskNoteBuilder::new(task_id.clone(), user_id, id_employee)
            .note("Note with @tagged.user")
            .username("test.user")
            .private(true);

        let result = builder.build().await;
        assert!(result.is_ok());
        let note = result.unwrap();
        assert_eq!(note.note, "Note with @tagged.user");
    }

    #[tokio::test]
    async fn test_update_prestashop_note() {
        let (task_id, user_id, id_employee) = setup_test_data();
        let mut mock_presta = MockPrestashopApi::new();
        let message_id = "54321";
        let thread_id = "67890";

        mock_presta
            .expect_modify_customer_message()
            .with(eq(message_id), eq(id_employee.as_str()), eq(thread_id), eq("Updated note"))
            .times(1)
            .returning(|_, _, _, _| Ok(PrestashopResponse { id: message_id.to_string(), ..Default::default() }));

        DATABASE
            .expect_upsert()
            .times(1)
            .returning(|_| Ok(Some(TaskNote { id: RecordId::new("task_note", message_id), ..Default::default() })));

        let mut note = TaskNote {
            id: RecordId::new("task_note", message_id),
            task_id,
            created_at: Utc::now().into(),
            note: "Updated note".to_string(),
            username: "test.user".to_string(),
            id_customer_thread: Some(thread_id.to_string()),
            id_customer_message: Some(message_id.to_string()),
            id_employee,
            user: user_id,
            service_number: Some("12345".to_string()),
            private: false,
        };

        let result = note.update().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_delete_prestashop_note() {
        let (task_id, user_id, id_employee) = setup_test_data();
        let mut mock_presta = MockPrestashopApi::new();
        let message_id = "54321";
        let thread_id = "67890";

        mock_presta
            .expect_delete_resource_wasm()
            .with(eq("customer_messages"), eq(message_id))
            .times(1)
            .returning(|_, _| Ok(()));

        DATABASE
            .expect_delete()
            .times(1)
            .returning(|_| Ok(Some(TaskNote { id: RecordId::new("task_note", message_id), ..Default::default() })));

        let note = TaskNote {
            id: RecordId::new("task_note", message_id),
            task_id,
            created_at: Utc::now().into(),
            note: "Test note".to_string(),
            username: "test.user".to_string(),
            id_customer_thread: Some(thread_id.to_string()),
            id_customer_message: Some(message_id.to_string()),
            id_employee,
            user: user_id,
            service_number: Some("12345".to_string()),
            private: false,
        };

        let result = note.delete().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_get_prestashop_notes_empty_service_number() {
        let result = TaskNote::get_prestashop_notes("", None).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "Service number is empty");
    }

    #[tokio::test]
    async fn test_get_db_notes_by_service_empty() {
        let result = TaskNote::get_db_notes_by_service("").await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "Service number is empty");
    }
} 
*/