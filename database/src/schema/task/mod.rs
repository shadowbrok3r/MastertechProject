use crate::{schema::{Record, User, TASK_TABLE}, DATABASE};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use structdiff::{Difference, StructDiff};
use chrono::Utc;

use super::{random_record_id, ComputerData, CustomerData, Datetime, RecordId, SurrealValue, TaskNotePayload, TicketData, TicketPayload, USER_TABLE};

pub mod update;
pub mod sort;
pub mod filter;

pub use filter::*;
pub use sort::*;

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Difference, SurrealValue)]
pub struct TaskPayload {
    pub id: RecordId,
    pub task_name: String,
    pub service_ticket: Option<TicketPayload>,
    pub task_description: String,
    pub assignee: RecordId, // should i use a user id here or will email and name be enough for tracking?
    pub service_number: Option<String>,
    pub due_date: Datetime, // optional because if not provided, set due date to creation date
    pub priority: Priority,
    #[difference(collection_strategy = "ordered_array_like")]
    pub task_note: Vec<TaskNotePayload>,
    pub completed: bool,
    pub status: Status,
    pub created_at: Datetime
}

impl Default for TaskPayload {
    fn default() -> Self {
        Self {
            id: random_record_id(TASK_TABLE),
            task_name: String::new(),
            service_ticket: None,
            task_description: String::new(),
            assignee: random_record_id(USER_TABLE),
            service_number: None,
            due_date: Utc::now().into(),
            priority: Priority::Normal,
            task_note: Vec::new(),
            completed: false,
            status: Status::Todo,
            created_at: Utc::now().into()
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Difference, SurrealValue)]
pub struct LiveTaskPayload {
    pub id: RecordId,
    pub task_name: String,
    pub service_ticket: Option<RecordId>,
    pub task_description: String,
    pub assignee: RecordId, 
    pub service_number: Option<String>,
    pub due_date: Datetime,
    pub priority: Priority,
    pub completed: bool,
    pub status: Status,
    pub created_at: Datetime
}

impl Default for LiveTaskPayload {
    fn default() -> Self {
        Self {
            id: random_record_id(TASK_TABLE),
            task_name: String::new(),
            service_ticket: None,
            task_description: String::new(),
            assignee: random_record_id(USER_TABLE),
            service_number: None,
            due_date: Utc::now().into(),
            priority: Priority::Normal,
            completed: false,
            status: Status::Todo,
            created_at: Utc::now().into()
        }
    }
}

impl TaskPayload {
    /// Creates a JSON diff representation comparing this task to another.
    /// Returns a serde_json::Value with format: { "field_name": { "old": "old_value", "new": "new_value" }, ... }
    pub fn diff_to_json(&self, other: &Self) -> serde_json::Value {
        use serde_json::{json, Map, Value};
        use super::RecordIdExt;
        
        let mut diff_map = Map::new();
        
        if self.task_name != other.task_name {
            diff_map.insert("task_name".to_string(), json!({
                "old": self.task_name,
                "new": other.task_name
            }));
        }
        if self.task_description != other.task_description {
            diff_map.insert("task_description".to_string(), json!({
                "old": self.task_description,
                "new": other.task_description
            }));
        }
        if self.assignee != other.assignee {
            diff_map.insert("assignee".to_string(), json!({
                "old": self.assignee.key_string(),
                "new": other.assignee.key_string()
            }));
        }
        if self.service_number != other.service_number {
            diff_map.insert("service_number".to_string(), json!({
                "old": self.service_number.clone().unwrap_or_default(),
                "new": other.service_number.clone().unwrap_or_default()
            }));
        }
        if self.due_date != other.due_date {
            diff_map.insert("due_date".to_string(), json!({
                "old": self.due_date.to_string(),
                "new": other.due_date.to_string()
            }));
        }
        if self.priority != other.priority {
            diff_map.insert("priority".to_string(), json!({
                "old": self.priority.as_str(),
                "new": other.priority.as_str()
            }));
        }
        if self.status != other.status {
            diff_map.insert("status".to_string(), json!({
                "old": self.status.as_str(),
                "new": other.status.as_str()
            }));
        }
        if self.completed != other.completed {
            diff_map.insert("completed".to_string(), json!({
                "old": self.completed,
                "new": other.completed
            }));
        }
        
        Value::Object(diff_map)
    }
    
    /// Returns true if this task has any differences from the other task.
    pub fn has_changes_from(&self, other: &Self) -> bool {
        self.task_name != other.task_name
            || self.task_description != other.task_description
            || self.assignee != other.assignee
            || self.service_number != other.service_number
            || self.due_date != other.due_date
            || self.priority != other.priority
            || self.status != other.status
            || self.completed != other.completed
    }
}

impl LiveTaskPayload {
    /// Creates a JSON diff representation comparing this task to another.
    /// Returns a serde_json::Value with format: { "field_name": { "old": "old_value", "new": "new_value" }, ... }
    /// Uses structdiff's StructDiff trait under the hood.
    pub fn diff_to_json(&self, other: &Self) -> serde_json::Value {
        use serde_json::{json, Map, Value};
        use super::RecordIdExt;
        
        let mut diff_map = Map::new();
        
        // Compare each field and add to diff if different
        if self.task_name != other.task_name {
            diff_map.insert("task_name".to_string(), json!({
                "old": self.task_name,
                "new": other.task_name
            }));
        }
        if self.task_description != other.task_description {
            diff_map.insert("task_description".to_string(), json!({
                "old": self.task_description,
                "new": other.task_description
            }));
        }
        if self.assignee != other.assignee {
            diff_map.insert("assignee".to_string(), json!({
                "old": self.assignee.key_string(),
                "new": other.assignee.key_string()
            }));
        }
        if self.service_number != other.service_number {
            diff_map.insert("service_number".to_string(), json!({
                "old": self.service_number.clone().unwrap_or_default(),
                "new": other.service_number.clone().unwrap_or_default()
            }));
        }
        if self.service_ticket != other.service_ticket {
            diff_map.insert("service_ticket".to_string(), json!({
                "old": self.service_ticket.as_ref().map(|r| r.key_string()).unwrap_or_default(),
                "new": other.service_ticket.as_ref().map(|r| r.key_string()).unwrap_or_default()
            }));
        }
        if self.due_date != other.due_date {
            diff_map.insert("due_date".to_string(), json!({
                "old": self.due_date.to_string(),
                "new": other.due_date.to_string()
            }));
        }
        if self.priority != other.priority {
            diff_map.insert("priority".to_string(), json!({
                "old": self.priority.as_str(),
                "new": other.priority.as_str()
            }));
        }
        if self.status != other.status {
            diff_map.insert("status".to_string(), json!({
                "old": self.status.as_str(),
                "new": other.status.as_str()
            }));
        }
        if self.completed != other.completed {
            diff_map.insert("completed".to_string(), json!({
                "old": self.completed,
                "new": other.completed
            }));
        }
        
        Value::Object(diff_map)
    }
    
    /// Returns true if this task has any differences from the other task.
    /// Excludes created_at from comparison (creation time should never change).
    pub fn has_changes_from(&self, other: &Self) -> bool {
        self.task_name != other.task_name
            || self.task_description != other.task_description
            || self.assignee != other.assignee
            || self.service_number != other.service_number
            || self.service_ticket != other.service_ticket
            || self.due_date != other.due_date
            || self.priority != other.priority
            || self.status != other.status
            || self.completed != other.completed
    }

    pub async fn get_associated_computer(&self) -> anyhow::Result<ComputerData, anyhow::Error> {
        let computer: Option<ComputerData> = DATABASE
            .query("SELECT service_ticket FROM $id FETCH service_ticket.computer")
            .bind(("id", self.id.clone()))
            .await?
            .take(0)?;

        Ok(computer.unwrap_or_default())
    }

    pub async fn get_associated_service(&self) -> anyhow::Result<TicketData, anyhow::Error> {
        let ticket: Option<TicketData> = DATABASE
            .query("SELECT service_ticket FROM $id FETCH service_ticket")
            .bind(("id", self.id.clone()))
            .await?
            .take(0)?;

        Ok(ticket.unwrap_or_default())
    }

    pub async fn get_associated_customer(&self) -> anyhow::Result<CustomerData, anyhow::Error> {
        let customer: Option<CustomerData> = DATABASE
            .query("SELECT service_ticket FROM $id FETCH service_ticket.customer")
            .bind(("id", self.id.clone()))
            .await?
            .take(0)?;

        Ok(customer.unwrap_or_default())
    }

    pub async fn get_associated_notes(&self) -> anyhow::Result<Vec<TaskNotePayload>, anyhow::Error> {
        let notes: Vec<TaskNotePayload> = DATABASE
            .query("SELECT * FROM task_note WHERE task_id == $id")
            .bind(("id", self.id.clone()))
            .await?
            .take(0)?;

        Ok(notes)
    }

    pub async fn get_tasks(start: i32) -> anyhow::Result<Vec<Self>, anyhow::Error> {
        let tasks: Vec<Self> = DATABASE
            .query("SELECT * FROM task ORDER BY due_date DESC START $start LIMIT 200")
            .bind(("start", start))
            .await?
            .take(0)?;

        Ok(tasks)
    }

    /// Fetch all tasks linked to a specific customer (via service_ticket.customer)
    pub async fn get_tasks_by_customer_id(customer_id: &RecordId) -> anyhow::Result<Vec<Self>, anyhow::Error> {
        let tasks: Vec<Self> = DATABASE
            .query("SELECT * FROM task WHERE service_ticket.customer = $cust_id ORDER BY due_date DESC LIMIT 50")
            .bind(("cust_id", customer_id.clone()))
            .await?
            .take(0)?;
        Ok(tasks)
    }

    /// Fetch all tasks linked to a specific computer (via service_ticket.computer)
    pub async fn get_tasks_by_computer_id(computer_id: &RecordId) -> anyhow::Result<Vec<Self>, anyhow::Error> {
        let tasks: Vec<Self> = DATABASE
            .query("SELECT * FROM task WHERE service_ticket.computer = $comp_id ORDER BY due_date DESC LIMIT 50")
            .bind(("comp_id", computer_id.clone()))
            .await?
            .take(0)?;
        Ok(tasks)
    }

    pub async fn create_task_payload(
        mut task_data: Self,
        ticket_data: TicketData,
        customer_data: CustomerData,
        computer_data: ComputerData,
        // mut task_data: LiveTaskPayload,
        mut task_notes: Vec<TaskNotePayload>,
        send_specs: bool,
    ) -> anyhow::Result<(), anyhow::Error> {
        // let mut task_data = self;
        log::info!("schema/utilities.rs -> Send_Payload");
        let queried_salesman = match User::query_user_from_email(ticket_data.salesman.clone()).await {
            Ok(user) => user,
            Err(e) => {
                log::warn!("schema/task -> salesman '{}' has no user record ({e:?}); assigning to current user", ticket_data.salesman);
                User::get_current_user_from_auth()
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("No authenticated user to assign task to"))?
            }
        };
        
        
        // let task_id = task_data.id.clone();
        let ticket_id = ticket_data.id.clone();
        let customer_id = customer_data.id.clone();
        let computer_id = computer_data.id.clone();
        let service_number = ticket_data.service_number.clone();
        task_data.task_name = format!(
            "{} - {}",
            &customer_data.name,
            service_number.clone()
        );
        task_data.service_ticket = Some(ticket_id.clone());
        task_data.service_number = Some(service_number.clone());
        task_data.priority = Priority::Normal;
        task_data.assignee = queried_salesman.get_id();
    
        // if ticket_data.computer.is_none() {
        //     ticket_data.computer = Some(computer_data.id.clone());
        // }
    
        log::info!("schema/utilities.rs -> cust_record: {customer_data:?}");
        let update_customer: Result<Option<Record>, surrealdb::Error> = DATABASE
            .upsert(customer_id)
            .content(customer_data.clone())
            .await;
        
        match update_customer {
            Ok(record) => log::info!("Updated Customer {record:?}"),
            Err(e) => {
                log::warn!("Error updating Customer {e:?}");
                // if i have a customer from everest, i will need to delete
                // and recreate the record.. 
            }
        }
    
        // panic!("");
        if send_specs {
            let create_computer_record: Option<Record> = DATABASE
                .upsert(computer_id)
                .content(computer_data)
                .await?;
            log::info!("schema/utilities.rs -> create_computer_record: {create_computer_record:?}");
        }
    
        log::info!("schema/utilities.rs -> ticket record: {ticket_data:?}");
        let service_ticket_record: Option<Record> = DATABASE
            .upsert(ticket_id)
            .content(ticket_data)
            .await?;
        log::info!("schema/utilities.rs -> service_ticket_record: {service_ticket_record:?}");
    
        log::info!("schema/utilities.rs -> Task Data: {:?}", &task_data);
    
        
        let check_task_record: Vec<LiveTaskPayload> = DATABASE
            .query("SELECT * FROM task WHERE service_number == $service_number")
            .bind(("service_number", service_number.clone()))
            .await?
            .take(0)?;
    
        log::info!("schema/utilities.rs -> check_task_record: {check_task_record:?}");
    
        if !check_task_record.is_empty() {
            for task in check_task_record.iter() {
                if task.id == task_data.id {
                    let upsert_task_record: Option<Record> = DATABASE
                        .update(task.id.clone())
                        .content(LiveTaskPayload {
                            id: task.id.clone(),
                            ..task_data.clone()
                        }).await?;
    
                    for note in task_notes.iter_mut() {
                        if note.task_id == Some(task_data.id.clone()) && note.task_id != Some(task.id.clone()) {
                            note.task_id = Some(task.id.clone());
                        }
                    }
                    log::info!("schema/utilities.rs -> upsert_task_record: {upsert_task_record:?}");
                }
    
            } 
        } else {
            let create_task_record: Option<Record> = DATABASE
                .create(TASK_TABLE)
                .content(task_data).await?;
            log::info!("schema/utilities.rs -> create_task_record: {create_task_record:?}");
        }
    
        for mut note in task_notes {
            let res = note.handle_note_creation().await;
            log::info!("schema/utilities.rs -> Task Note Creation from Mastertech: {res:?}");
        }
    
        Ok(())
    }
}

impl From<LiveTaskPayload> for TaskPayload {
    fn from(live_task: LiveTaskPayload) -> Self {
        Self {
            id: live_task.id,
            task_name: live_task.task_name,
            task_description: live_task.task_description,
            assignee: live_task.assignee,
            service_number: live_task.service_number,
            due_date: live_task.due_date,
            priority: live_task.priority,
            completed: live_task.completed,
            status: live_task.status,
            ..Default::default()
        }
    }
}

impl From<TaskPayload> for LiveTaskPayload {
    fn from(task: TaskPayload) -> Self {
        Self {
            id: task.id,
            task_name: task.task_name,
            service_ticket: Some(task.service_ticket.unwrap_or_default().id),
            task_description: task.task_description,
            assignee: task.assignee,
            service_number: task.service_number,
            due_date: task.due_date,
            priority: task.priority,
            completed: task.completed,
            status: task.status,
            created_at: task.created_at
        }
    }
}

#[derive(Clone, Debug, PartialEq, Default, Eq, Hash, SurrealValue)]
#[surreal(untagged)]
pub enum Status {
    #[default]
    #[surreal(value = "Todo")]
    Todo,
    #[surreal(value = "In Repair")]
    InRepair,
    #[surreal(value = "Complete")]
    Complete,
    #[surreal(value = "Sales")]
    Sales,
    #[surreal(value = "QC")]
    Qc,
    CustomStatus(String),
}


impl Status {
    pub const VALUES: [Self; 6] = [Self::Todo, Self::InRepair, Self::Complete, Self::Sales, Self::Qc, Status::CustomStatus(String::new())];
    pub fn as_str(&self) -> &str {
        match self {
            Status::Todo => "Todo",
            Status::InRepair => "In Repair",
            Status::Complete => "Complete",
            Status::Sales => "Sales",
            Status::Qc => "QC",
            Status::CustomStatus(status) => &status
        }
    }
pub fn from_str(status: &str) -> Self {
        let normalized = status.trim().to_lowercase();
        match normalized.as_str() {
            "todo" => Status::Todo,
            "in repair" | "inrepair" => Status::InRepair,
            "complete" => Status::Complete,
            "sales" => Status::Sales,
            "qc" => Status::Qc,
            _ => Status::CustomStatus(status.to_string())
        }
    }
}

// Custom serialization
impl Serialize for Status {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

// Custom deserialization
impl<'de> Deserialize<'de> for Status {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        Ok(Status::from_str(&s))
    }
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, PartialEq, Default, SurrealValue)]
#[surreal(untagged)]
pub enum Priority {
    Express,
    Rfs,
    Fire,
    Qc,
    #[default]
    Normal,
}

impl Priority {
    pub fn as_str(&self) -> &str {
        match self {
            Priority::Normal => "Normal",
            Priority::Rfs => "Rfs",
            Priority::Qc => "Qc",
            Priority::Express => "Express",
            Priority::Fire => "Fire",
        }
    }
    pub const VALUES: [Self; 5] = [Self::Normal, Self::Rfs, Self::Qc, Self::Express, Self::Fire];
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Copy, Default, Eq, PartialOrd, Ord, SurrealValue)]
#[surreal(untagged)]
pub enum Store {
    #[default]
    RIV,
    LTN,
    MUR,
    ORE,
    SAN,
}



impl Store {
    pub fn as_str(&self) -> &str {
        match self {
            Store::RIV => "RIV",
            Store::LTN => "LTN",
            Store::MUR => "MUR",
            Store::ORE => "ORE",
            Store::SAN => "SAN",
        }
    }
    
    pub fn store_email(&self) -> &'static str {
        match *self {
            Store::RIV => "pclriv@pclaptops.com",
            Store::MUR => "pclmur@pclaptops.com",
            Store::LTN => "pclltn@pclaptops.com",
            Store::SAN => "pclsan@pclaptops.com",
            Store::ORE => "pclore@pclaptops.com",
        }
    }

    pub fn from_presta_store_id(store_id: &str) -> Self {
        match store_id {
            "7" => Self::RIV,
            "8" => Self::LTN,
            "10" => Self::MUR,
            "12" => Self::SAN,
            "14" => Self::ORE,
            _ => Self::RIV,
        }
    }

    pub fn into_store_id(&self) -> i32 {
        match self {
            Self::RIV => 7,
            Self::LTN => 8,
            Self::MUR => 10,
            Self::SAN => 12,
            Self::ORE => 14,
        }
    }

    pub fn from_odoo_store_id(store_id: &str) -> Self {
        match store_id {
            "76" => Self::RIV,
            "73" => Self::LTN,
            "74" => Self::MUR,
            "75" => Self::ORE,
            "77" => Self::SAN,
            _ => Self::RIV,
        }
    }

    pub fn into_odoo_store_id(&self) -> i32 {
        match self {
            Self::RIV => 76,
            Self::LTN => 73,
            Self::MUR => 74,
            Self::ORE => 75,
            Self::SAN => 77,
        }
    }

    /// Resolve a store id that may be in either the PrestaShop (7, 8, 10, 12, 14)
    /// or Odoo (73-77) numbering scheme. Useful when a UI control reuses one
    /// `store_selection` field across views that bind to different schemes —
    /// callers can normalize via `Store::from_any_store_id(...).into_store_id()`
    /// or `.into_odoo_store_id()` before issuing a backend request.
    pub fn from_any_store_id(store_id: &str) -> Self {
        match store_id {
            // PrestaShop
            "7" => Self::RIV,
            "8" => Self::LTN,
            "10" => Self::MUR,
            "12" => Self::SAN,
            "14" => Self::ORE,
            // Odoo
            "73" => Self::LTN,
            "74" => Self::MUR,
            "75" => Self::ORE,
            "76" => Self::RIV,
            "77" => Self::SAN,
            _ => Self::RIV,
        }
    }

    pub const VALUES: [Self; 5] = [
        Self::RIV,
        Self::LTN,
        Self::MUR,
        Self::ORE,
        Self::SAN,
    ];
}

// ============================================================================
// Task History - tracks changes made to tasks
// ============================================================================

use super::TASK_HISTORY_TABLE;

/// Represents a historical record of changes made to a task
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, SurrealValue)]
pub struct TaskHistory {
    pub id: RecordId,
    /// The task that was modified
    pub task_id: RecordId,
    /// The user who made the change
    pub user: RecordId,
    /// The username of the user (for display purposes)
    pub username: String,
    /// JSON object containing the diff of changed fields
    /// Format: { "field_name": { "old": "old_value", "new": "new_value" }, ... }
    pub diff: serde_json::Value,
    /// When the change was made
    pub created_at: Datetime,
}

impl Default for TaskHistory {
    fn default() -> Self {
        Self {
            id: random_record_id(TASK_HISTORY_TABLE),
            task_id: random_record_id(TASK_TABLE),
            user: random_record_id(USER_TABLE),
            username: String::new(),
            diff: serde_json::Value::Null,
            created_at: Utc::now().into(),
        }
    }
}

impl TaskHistory {
    /// Create a new TaskHistory record from diff data
    pub fn new(
        task_id: RecordId,
        user_id: RecordId,
        username: String,
        diff: serde_json::Value,
    ) -> Self {
        Self {
            id: random_record_id(TASK_HISTORY_TABLE),
            task_id,
            user: user_id,
            username,
            diff,
            created_at: Utc::now().into(),
        }
    }

    /// Save this history record to the database
    pub async fn save(&self) -> anyhow::Result<Option<Record>, anyhow::Error> {
        let record: Option<Record> = DATABASE
            .create(TASK_HISTORY_TABLE)
            .content(self.clone())
            .await?;
        log::info!("Created task history record: {:?}", record);
        Ok(record)
    }

    /// Get all history records for a specific task
    pub async fn get_history_for_task(task_id: RecordId) -> anyhow::Result<Vec<Self>, anyhow::Error> {
        let history: Vec<Self> = DATABASE
            .query("SELECT * FROM task_history WHERE task_id == $task_id ORDER BY created_at DESC")
            .bind(("task_id", task_id))
            .await?
            .take(0)?;
        Ok(history)
    }
}
