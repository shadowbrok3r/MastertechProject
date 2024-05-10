// use std::fmt::Display;
use serde::{Serialize, Deserialize};
use surrealdb::{sql::Thing, opt::RecordId};

pub const NS: &str = "Mastertech";
pub const DB: &str = "MastertechDB";
pub const USER_SCOPE: &str = "user";

pub const TICKET_TABLE: &str = "service_order";
pub const CUSTOMER_TABLE: &str = "customer";
pub const COMPUTER_TABLE: &str = "computer";
pub const TASK_TABLE: &str = "task";
pub const TASK_NOTE_TABLE: &str = "task_note";
pub const SEB_TABLE: &str = "seb_data";
pub const USER_TABLE: &str = "user";
pub const NOTIFICATION_TABLE: &str = "notification";


#[derive(Debug, Deserialize, Serialize)]
pub struct Record {
    #[allow(dead_code)]
    pub id: Thing,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ComputerId(pub RecordId);

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CustomerId(pub RecordId);

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TicketId(pub RecordId);

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct UserId(pub RecordId);

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TaskId(pub RecordId);

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TaskNoteId(pub RecordId);

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SebId(pub RecordId);

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NotificationId(pub RecordId);

#[derive(Serialize, Debug)]
pub struct RecordResult {
    pub result: bool,
    pub record: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct GetTicketResult{
    pub data: TicketResponse
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TaskPayload{
    pub id: Option<TaskId>,
    pub task_name: String,
    pub service_ticket: Option<TicketId>,
    // pub assignee_name: Option<String>,
    pub assignee_email: Option<String>,
    pub assignee_initials: Option<String>,
    pub task_description: Option<String>, 
    pub assignee: Option<UserId>, // should i use a user id here or will email and name be enough for tracking?
    pub service_number: Option<i32>,
    pub due_date: String, // optional because if not provided, set due date to creation date
    pub priority: Option<i32>,
    pub task_note: Option<Vec<TaskNoteId>>,
    pub completed: bool,
    pub status: Status,
    pub dep: Option<String>
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TicketPayload{
    pub ticket_data: TicketData,
    pub customer_data: CustomerData,
    pub computer_data: ComputerData
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TicketResponse{
    pub ticket_response: TicketPayload,
    pub customer_data: CustomerData,
    pub computer_data: ComputerData
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TicketData{
    pub created_at: Option<String>,
    pub id: Option<TicketId>,
    pub due_date: String,
    pub customer: Option<CustomerId>,
    pub computer: Option<ComputerId>,
    pub service_task: Option<TaskId>,
    pub service_number: i32,
    /// Person that checked computer in
    pub checkin_rep: String,
    /// This is main initials on ticket
    pub sales_rep: String,
    pub checkin_notes: String,
    pub recommendations: String,
    pub tech: String,
    pub salesman: String,
    pub dep: String, // Store
    pub terms: String,
    pub ticket_total: String,
    pub doc_alias: String, // type of order (service,sales,transfer)
    pub current_antivirus: Option<Vec<String>>,
    pub hardware_test_results: HardwareTests,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CustomerData{
    pub id: Option<CustomerId>, 
    pub cust_code: i32,
    pub computers: Option<Vec<ComputerId>>,
    pub services: Option<Vec<TicketId>>,
    pub name: String,
    pub phone_number: String,
    pub phone_number_2: String, // Option<String>
    pub email: String,
    pub li_doc: i32,
    pub li_amnt: String,
    pub num_inv: i32,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct ComputerData{
    pub id: Option<ComputerId>,
    pub customer: Option<CustomerId>,
    // pub seb_id: Option<SebId>,
    pub seb_info: Option<LocalSebData>,
    pub hostname: String,
    pub operating_system: String,
    pub cpu: String,
    pub gpu: String,
    pub ram: String,
    pub drives: Vec<DriveData>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
#[allow(non_snake_case)]
#[serde(rename_all(serialize = "PascalCase", deserialize = "snake_case"))]
#[serde(rename = "xml")]
pub struct LocalSebData {
    // pub id: Option<SebId>,
    pub InstalledDeviceId: String,
    pub InstallInstanceId: String,
    pub HasIssues: String,
    pub InstallationStage: String,
    pub ReasonCode: String,
    pub ActivationCode: String,
    pub InstallVersion: String,
    pub MachineName: String,
    pub ExtendedSeb: Option<ExtendedSeb>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct ExtendedSeb {
    pub email: String,
    pub phone: String,
    pub userid: String,
    pub device_name: String,
    pub device_id: String,
    pub state: String,
    pub usage_gb: String,
    pub date_device_created: String,
    pub activated: String,
    pub activation_code: String,
    pub last_complete_backup: String,
    pub last_client_status_update: String,
    pub id_recurly_account: String,
    pub date_last_scan: String,
    pub date_email_sent: String,
    pub date_canceled_account: String,
    pub date_deleted_account: String,
    pub current_period_ends_at: String,
    pub date_modified: String,
    pub date_created: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DriveData{
    pub drive_letter: String,
    pub drive_type: String,
    pub total_size: String,
    pub space_left: String,
}

impl ComputerData{
    pub fn new() -> Self{
        ComputerData{
            drives: Vec::new(),
            ..Default::default()
        }
    }

    pub fn add_disk(&mut self, disk: DriveData){
        self.drives.push(disk);
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct HardwareTests{
    pub hdd_test: String,
    pub ssd_test: String,
    pub ram_test: String
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TaskNotePayload{
    pub task_id: Option<TaskId>,
    pub everest_initials: String,
    // pub service_number: Option<i32>,
    pub created_at: String,
    pub note: String,
}

// I will probably end up merging ModifyTask and TaskPayload since they contain most of the exact same data
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ModifyTask{
    /// unique id for tasks
    pub task_id: TaskId,
    /// ability to change store
    // pub dep: Option<String>, 
    /// change priority
    pub priority: Option<i32>, 
    /// change which status task is part of
    pub status: Option<Status>, 
    /// change completed / incomplete
    pub completed: Option<bool>, 
    /// update due_date 
    pub due_date: Option<String>, 
    /// update assignee 
    pub assignee_initials: Option<String>, 
    /// update task name 
    pub task_name: Option<String>, 
    /// modify description of task
    pub task_description: Option<String>, 
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Notification{
    /// receiver of notification
    pub user: UserId,
    /// description of notification
    pub notification_description: String, 
    /// type of notification
    pub notification_type: NotificationType,
    /// Has the notification been read?
    pub status: NotificationStatus, 
    pub user_initials: String
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum NotificationType {
    NewMessage,
    SpoStatusChange,
    NewTask,
    TaggedInComment,
    GroupTag,
    OverdueTask
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum NotificationStatus{
    Read,
    Unread
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ModifyNotification{
    pub id: NotificationId,
    pub everest_initials: Option<String>,
    /// either Read or Unread
    pub status: Option<NotificationStatus>,
    pub mark_all_read: Option<bool>,
    pub mark_all_unread: Option<bool>,
    pub archive: Option<bool>
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Status{
    Todo,
    InRepair,
    Complete
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Category{
    StoreTasks,
    MyTasks,
    CompletedTasks,
}

#[derive(Deserialize)]
struct CommandRequest {
    _client_id: String,
    _command: String,
}

#[derive(Serialize)]
struct CommandResponse {
    status: String,
}


// impl Display for Status{
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         write!(f, "{}", self)
//     }
// }

// impl Display for Category{
//     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
//         write!(f, "{}", self)
//     }
// }