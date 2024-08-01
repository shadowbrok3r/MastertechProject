
use std::fmt::Display;

use serde::{Serialize, Deserialize};
use surrealdb::{opt::RecordId, sql::{Id, Thing}};

use super::{prestashop_schema::{Address, CustomerMessage, CustomerThread, Employee, Order}, SystemInformation};

pub const _NS: &str = "Mastertech";
pub const _DB: &str = "MastertechDB";
pub const _USER_SCOPE: &str = "user";

pub const TICKET_TABLE: &str = "service_order";
pub const CUSTOMER_TABLE: &str = "customer";
pub const COMPUTER_TABLE: &str = "computer";
pub const TASK_TABLE: &str = "task";
pub const CONNECTED_CLIENT_TABLE: &str = "connected_client";
pub const TASK_NOTE_TABLE: &str = "task_note";
pub const _SEB_TABLE: &str = "seb_data";
pub const _USER_TABLE: &str = "user";
pub const _NOTIFICATION_TABLE: &str = "notification";


#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Record {
    #[allow(dead_code)]
    pub id: Thing,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ClientId(pub RecordId);

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ComputerId(pub RecordId);

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CustomerId(pub RecordId);

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TicketId(pub RecordId);

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct UserId(pub RecordId);

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct TaskId(pub RecordId);

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TaskNoteId(pub RecordId);

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SebId(pub RecordId);

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct NotificationId(pub RecordId);

#[derive(Serialize, Debug)]
pub struct RecordResult {
    pub result: bool,
    pub record: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RecordSuccess{
    pub success: bool
}

// A specific sentinel value for default initialization
const DEFAULT_USER_ID: RecordId = RecordId {
    tb: String::new(),
    id: Id::String(String::new()),
};

impl Default for UserId {
    fn default() -> Self {
        UserId(DEFAULT_USER_ID.clone())
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct TaskPayload{
    pub id: Option<TaskId>,
    pub task_name: String,
    pub service_ticket: Option<TicketPayload>,
    pub everest_initials: String,
    pub task_description: String, 
    pub assignee: UserId, // should i use a user id here or will email and name be enough for tracking?
    pub service_number: Option<String>,
    pub due_date: String, // optional because if not provided, set due date to creation date
    pub priority: Priority,
    pub task_note: Option<Vec<TaskNotePayload>>, // TaskNoteId
    pub completed: bool,
    pub status: Status,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct LiveTaskPayload{
    pub id: Option<TaskId>,
    pub task_name: String,
    pub service_ticket: Option<TicketId>,
    pub everest_initials: String,
    pub task_description: String, 
    pub assignee: Option<UserId>,
    pub service_number: Option<String>,
    pub due_date: String, 
    pub priority: Priority,
    pub task_note: Option<Vec<TaskNoteId>>,
    pub completed: bool,
    pub status: Status,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct TicketPayload{
    pub id: Option<TicketId>,
    pub created_at: Option<String>,
    pub customer: Option<CustomerData>,
    pub computer: Option<ComputerData>,
    pub service_task: Option<TaskId>,
    pub service_number: String,
    /// Person that checked computer in
    pub checkin_rep: String,
    /// This is main initials on ticket
    pub sales_rep: String,
    pub checkin_notes: String,
    pub tech: String,
    pub salesman: String,
    pub terms: String,
    pub ticket_total: String,
    pub doc_alias: String, // type of order (service,sales,transfer)
    pub current_antivirus: Option<Vec<String>>,
    pub hardware_test_results: HardwareTests,
}

#[derive(Serialize, Deserialize, Debug, Default, Clone)]
pub struct TicketData{
    pub id: Option<TicketId>,
    pub created_at: Option<String>,
    // pub due_date: Option<String>, // GET RID OF THIS, WHY IS IT HERE
    pub customer: Option<CustomerId>,
    pub computer: Option<ComputerId>,
    pub service_task: Option<TaskId>,
    pub service_number: String,
    /// Person that checked computer in
    pub checkin_rep: String,
    /// This is main initials on ticket
    pub sales_rep: String,
    pub checkin_notes: String,
    pub tech: String,
    pub salesman: String,
    pub terms: String,
    pub ticket_total: String,
    pub doc_alias: String, // type of order (service,sales,transfer)
    pub current_antivirus: Option<Vec<String>>,
    pub hardware_test_results: HardwareTests,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct CustomerData{
    pub id: Option<CustomerId>, 
    pub cust_code: String,
    pub part_order_links: Option<Vec<String>>,
    pub computers: Option<Vec<ComputerId>>,
    pub name: String,
    pub phone_number: String,
    pub phone_number_2: String,
    pub email: String,
    pub li_doc: String,
    pub li_amnt: String,
    pub num_inv: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
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

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct HardwareTests{
    pub hdd_test: String,
    pub ssd_test: String,
    pub ram_test: String
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct TaskNotePayload{
    pub id: Option<TaskNoteId>,
    pub task_id: Option<TaskId>,
    pub everest_initials: String,
    pub created_at: String,
    pub note: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct PrestashopPayload{
    pub customer: CustomerData,
    pub order: Order,
    pub sales_rep: Option<Employee>,
    pub split_rep: Option<Employee>,
    pub address: Address,
    pub customer_threads: Vec<CustomerThread>,
    pub customer_messages: Vec<CustomerMessage>
}

// I will probably end up merging ModifyTask and TaskPayload since they contain most of the exact same data
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ModifyTask{
    /// unique id for tasks
    pub task_id: TaskId,
    /// change priority
    pub priority: Option<Priority>, 
    /// change which status task is part of
    pub status: Option<Status>, 
    /// change completed / incomplete
    pub completed: Option<bool>, 
    /// update due_date 
    pub due_date: Option<String>, 
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

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub enum Status{
    #[default]
    Todo,
    InRepair,
    Complete
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub enum Priority{
    Express,
    Rfs,
    CustomerFire,
    Qc,
    #[default]
    Normal,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Category{
    StoreTasks,
    MyTasks,
    CompletedTasks,
}

// #[derive(Deserialize)]
// struct CommandRequest {
//     _client_id: String,
//     _command: String,
// }

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Copy, Default)]
pub enum Store{
    #[default]
    RIV,
    LTN,
    MUR,
    AF,
    WJ, 
    ORE,
    SAN,
    None
}

impl Store{
    pub fn as_str(&mut self) -> &str{
        match self{
            Store::RIV => "RIV",
            Store::LTN => "LTN",
            Store::MUR => "MUR",
            Store::AF => "AF",
            Store::WJ => "WJ",
            Store::ORE => "ORE",
            Store::SAN => "SAN",
            Store::None => "MUR"
        }
    }
    pub fn store_email(&self) -> &'static str {
        match *self {
            Store::RIV => "RIV",
            Store::MUR => "pclmur@pclaptops.com",
            Store::WJ => "pclwj@pclaptops.com",
            Store::LTN => "pclltn@pclaptops.com",
            Store::AF => "pclaf@pclaptops.com",
            Store::SAN => "pclsan@pclaptops.com",
            Store::ORE => "pclore@pclaptops.com",
            _ => "pclmur@pclaptops.com"
        }
    }
    pub const _VALUES: [Self; 7] = [Self::RIV, Self::LTN, Self::MUR, Self::AF, Self::WJ, Self::ORE, Self::SAN];
}


#[derive(Serialize, Debug, Clone, Deserialize, Default)]
pub struct ConnectedClient{ // <'a>
    pub id: Option<ClientId>,
    pub assigned_user: Option<UserId>,
    pub client_hash: String,
    pub connection_string: String,
    pub command_history: Vec<String>,
    pub connected: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
pub struct User {
    pub id: UserId,
    pub name: String,
    pub everest_initials: String,
    // #[serde(skip)]
    pub email: String,
    pub store: Store,
    pub notifications: Option<Vec<NotificationId>>,
    pub connected_clients: Option<Vec<ClientId>>
}

impl Priority{
    pub fn as_str(&mut self) -> &str{
        match self{
            Priority::Normal => "Normal",
            Priority::Rfs => "Rfs",
            Priority::Qc => "Qc",
            Priority::Express => "Express",
            Priority::CustomerFire => "CustomerFire",
        }
    }
    pub const VALUES: [Self; 5] = [Self::Normal, Self::Rfs, Self::Qc, Self::Express, Self::CustomerFire];
}

impl Status{
    pub fn as_str(&mut self) -> &str{
        match self{
            Status::Todo => "Todo",
            Status::InRepair => "In Repair",
            Status::Complete => "Complete",
        }
    }
    pub const VALUES: [Self; 3] = [Self::Todo, Self::InRepair, Self::Complete];
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


impl Display for SystemInformation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "==> cpu_percentage: {} \n==> comps: {:?} \n==> used_memory: {} \n==> total_memory: {} \n==> disks: {} \n==> name: {} \n==> kernel_version: {} \n==> os_version: {} \n==> hostname: {} \n==> number_of_cpus: {} \n==> network_interfaces: {:#?} \n", 
            self.cpu_percentage,
            self.component_temps,
            self.used_memory,
            self.total_memory,
            self.disks,
            self.name,
            self.kernel_version,
            self.os_version,
            self.hostname,
            self.number_of_cpus,
            self.network_interfaces,
        )
    }
}