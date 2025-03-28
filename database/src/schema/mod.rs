use helper_traits::GetAssociatedDataFromId;
use structdiff::{Difference, StructDiff};
// use deserializer::deserialize_to_string;
use surrealdb::{sql::Uuid, RecordId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use async_trait::async_trait;
use serde_json::Value;
use reqwest::Client;
use crate::DATABASE;
use anyhow::Error;

pub mod prestashop_schema;
pub mod helper_traits;
pub mod deserializer;
pub mod utilities;
pub mod get_data;
pub mod buckets;

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
pub const CONNECTED_CLIENT_TABLE: &str = "connected_client";
pub const CHAT_THREADS_TABLE: &str = "threads";

#[async_trait(?Send)]
impl<D> GetAssociatedDataFromId<D> for RecordId {
    async fn get_associated_data<RecordId>(&mut self) -> Result<D, Error>
    where
        D: for<'de> Deserialize<'de>,
    {
        let id = self.clone();

        let data: D = DATABASE.select(id).await?.unwrap();
        Ok(data)
    }
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Record {
    #[allow(dead_code)]
    pub id: RecordId,
}

#[derive(Serialize, Debug)]
pub struct RecordResult {
    pub result: bool,
    pub record: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RecordSuccess {
    pub success: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Difference)]
pub struct TaskPayload {
    pub id: RecordId,
    pub task_name: String,
    pub service_ticket: Option<TicketPayload>,
    pub everest_initials: String,
    pub task_description: String,
    pub assignee: RecordId, // should i use a user id here or will email and name be enough for tracking?
    pub service_number: Option<String>,
    pub due_date: String, // optional because if not provided, set due date to creation date
    pub priority: Priority,
    #[difference(collection_strategy = "ordered_array_like")]
    pub task_note: Vec<TaskNotePayload>,
    pub completed: bool,
    pub status: Status,
}

impl Default for TaskPayload {
    fn default() -> Self {
        Self {
            id: RecordId::from((TASK_TABLE, Uuid::new_v4().to_raw().split_terminator('-').collect::<Vec<&str>>().concat())),
            task_name: String::new(),
            service_ticket: None,
            everest_initials: String::new(),
            task_description: String::new(),
            assignee: RecordId::from((USER_TABLE, Uuid::new_v4().to_raw().split_terminator('-').collect::<Vec<&str>>().concat())),
            service_number: None,
            due_date: String::new(),
            priority: Priority::Normal,
            task_note: Vec::new(),
            completed: false,
            status: Status::Todo,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Difference)]
pub struct LiveTaskPayload {
    pub id: RecordId,
    pub task_name: String,
    pub service_ticket: Option<RecordId>,
    pub everest_initials: String,
    pub task_description: String,
    pub assignee: RecordId, // should i use a user id here or will email and name be enough for tracking?
    pub service_number: Option<String>,
    pub due_date: String, // optional because if not provided, set due date to creation date
    pub priority: Priority,
    pub completed: bool,
    pub status: Status,
}

impl Default for LiveTaskPayload {
    fn default() -> Self {
        Self {
            id: RecordId::from((TASK_TABLE, Uuid::new_v4().to_raw().split_terminator('-').collect::<Vec<&str>>().concat())),
            task_name: String::new(),
            service_ticket: None,
            everest_initials: String::new(),
            task_description: String::new(),
            assignee: RecordId::from((USER_TABLE, Uuid::new_v4().to_raw().split_terminator('-').collect::<Vec<&str>>().concat())),
            service_number: None,
            due_date: String::new(),
            priority: Priority::Normal,
            completed: false,
            status: Status::Todo,
        }
    }
}

impl From<LiveTaskPayload> for TaskPayload {
    fn from(live_task: LiveTaskPayload) -> Self {
        Self {
            id: live_task.id,
            task_name: live_task.task_name,
            everest_initials: live_task.everest_initials,
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

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Difference)]
pub struct TicketPayload {
    pub id: RecordId,
    pub created_at: Option<String>,
    pub customer: Option<CustomerData>,
    pub computer: Option<ComputerData>,
    pub service_ticket: Option<RecordId>,
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
    pub jobs: Option<Vec<Job>>
}

impl Default for TicketPayload {
    fn default() -> Self {
        Self {
            id: RecordId::from((TICKET_TABLE, Uuid::new_v4().to_raw().split_terminator('-').collect::<Vec<&str>>().concat())),
            created_at: Default::default(),
            customer: Default::default(),
            computer: Default::default(),
            service_number: Default::default(),
            checkin_rep: Default::default(),
            sales_rep: Default::default(),
            checkin_notes: Default::default(),
            tech: Default::default(),
            salesman: Default::default(),
            terms: Default::default(),
            ticket_total: Default::default(),
            doc_alias: Default::default(),
            current_antivirus: Default::default(),
            hardware_test_results: Default::default(),
            service_ticket: Default::default(),
            jobs: Default::default()
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Difference)]
pub struct TicketData {
    // Live Ticket Payload
    pub id: RecordId,
    pub created_at: Option<String>,
    pub customer: Option<RecordId>,
    pub computer: Option<RecordId>,
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
    pub jobs: Option<Vec<Job>>
}

impl Default for TicketData {
    fn default() -> Self {
        Self {
            id: RecordId::from((TICKET_TABLE, Uuid::new_v4().to_raw().split_terminator('-').collect::<Vec<&str>>().concat())),
            created_at: Default::default(),
            customer: Default::default(),
            computer: Default::default(),
            service_number: Default::default(),
            checkin_rep: Default::default(),
            sales_rep: Default::default(),
            checkin_notes: Default::default(),
            tech: Default::default(),
            salesman: Default::default(),
            terms: Default::default(),
            ticket_total: Default::default(),
            doc_alias: Default::default(),
            current_antivirus: Default::default(),
            hardware_test_results: Default::default(),
            jobs: Default::default(),
        }
    }
}

impl From<TicketData> for TicketPayload {
    fn from(ticket: TicketData) -> Self {
        Self {
            id: ticket.id,
            created_at: ticket.created_at,
            service_number: ticket.service_number,
            checkin_rep: ticket.checkin_rep,
            sales_rep: ticket.sales_rep,
            checkin_notes: ticket.checkin_notes,
            tech: ticket.tech,
            salesman: ticket.salesman,
            terms: ticket.terms,
            ticket_total: ticket.ticket_total,
            doc_alias: ticket.doc_alias,
            current_antivirus: ticket.current_antivirus,
            hardware_test_results: ticket.hardware_test_results,
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
            everest_initials: task.everest_initials,
            task_description: task.task_description,
            assignee: task.assignee,
            service_number: task.service_number,
            due_date: task.due_date,
            priority: task.priority,
            completed: task.completed,
            status: task.status,
        }
    }
}

impl From<TicketPayload> for TicketData {
    fn from(ticket: TicketPayload) -> Self {
        Self {
            id: ticket.id,
            created_at: ticket.created_at,
            service_number: ticket.service_number,
            checkin_rep: ticket.checkin_rep,
            sales_rep: ticket.sales_rep,
            checkin_notes: ticket.checkin_notes,
            tech: ticket.tech,
            salesman: ticket.salesman,
            terms: ticket.terms,
            ticket_total: ticket.ticket_total,
            doc_alias: ticket.doc_alias,
            current_antivirus: ticket.current_antivirus,
            hardware_test_results: ticket.hardware_test_results,
            customer: Some(ticket.customer.unwrap_or_default().id),
            computer: Some(ticket.computer.unwrap_or_default().id),
            jobs: ticket.jobs,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Difference)]
pub struct CustomerData {
    pub id: RecordId,
    pub cust_code: String,
    pub part_order_links: Option<Vec<String>>,
    pub name: String,
    pub phone_number: String,
    pub phone_number_2: String, // Option<String>
    pub email: String,
    pub li_doc: String,
    pub li_amnt: String,
    pub num_inv: String,
}

impl Default for CustomerData {
    fn default() -> Self {
        Self {
            id: RecordId::from((CUSTOMER_TABLE, Uuid::new_v4().to_raw().split_terminator('-').collect::<Vec<&str>>().concat())),
            cust_code: Default::default(),
            part_order_links: Default::default(),
            name: Default::default(),
            phone_number: Default::default(),
            phone_number_2: Default::default(),
            email: Default::default(),
            li_doc: Default::default(),
            li_amnt: Default::default(),
            num_inv: Default::default(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ComputerData {
    pub id: RecordId,
    pub customer: Option<RecordId>,
    pub seb_info: Option<LocalSebData>,
    pub hostname: String,
    pub operating_system: String,
    pub cpu: String,
    pub gpu: String,
    pub ram: String,
    pub drives: Vec<DriveData>,
    pub device_name: Option<String>,
    pub device_mfg: Option<String>,
    pub device_model: Option<String>,
    pub device_serial: Option<String>,
    pub windows_active: Option<bool>,
    pub installed_programs: Option<Vec<Value>>
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Difference)]
pub struct Job {
    computer: RecordId
}

impl Default for ComputerData {
    fn default() -> Self {
        Self {
            id: RecordId::from((COMPUTER_TABLE, Uuid::new_v4().to_raw().split_terminator('-').collect::<Vec<&str>>().concat())),
            customer: Default::default(),
            seb_info: Default::default(),
            hostname: Default::default(),
            operating_system: Default::default(),
            cpu: Default::default(),
            gpu: Default::default(),
            ram: Default::default(),
            drives: Default::default(),
            device_name: Default::default(),
            device_mfg: Default::default(),
            device_model: Default::default(),
            device_serial: Default::default(),
            installed_programs: Default::default(),
            windows_active: Default::default(),
        }
    }
}

impl ComputerData {
    pub fn new() -> Self {
        ComputerData {
            drives: Vec::new(),
            ..Default::default()
        }
    }
    pub fn add_disk(&mut self, disk: DriveData) {
        self.drives.push(disk);
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ChatThreads {
    pub id: RecordId,
    pub files: Option<Vec<String>>,
    pub messages: Vec<HashMap<String, String>>,
    pub user: RecordId,
    pub images: Option<Vec<bytes::Bytes>>
}


#[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq)]
#[allow(non_snake_case)]
#[serde(rename_all(serialize = "PascalCase", deserialize = "snake_case"))]
#[serde(rename = "xml")]
pub struct LocalSebData {
    // pub id: RecordId,
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

#[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq)]
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

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq)]
pub struct CarboniteResponse {
    pub id_carbonite: String,
    pub id_customer: String,
    pub record: String,
    pub email: String,
    pub phone: String,
    pub userid: String,
    pub first_name: String,
    pub last_name: String,
    pub company: String,
    pub companyid: String,
    pub partner: String,
    pub partnerid: String,
    pub device_name: String,
    pub device_id: String,
    pub user_group: String,
    pub state: String,
    pub policy_set: String,
    pub usage_gb: String,
    pub quota_gb: String,
    pub date_device_created: String,
    pub activated: String,
    pub activation_code: String,
    pub client_version: String,
    pub operating_system: String,
    pub os_edition: String,
    pub service_pack: String,
    pub os_bit_size: String,
    pub cache_used_mb: String,
    pub cache_available_mb: String,
    pub last_complete_backup: String,
    pub last_client_status_update: String,
    pub physical_memory_installed_mb: String,
    pub id_recurly_account: String,
    pub scanned: String,
    pub delete_scanned: String,
    pub date_last_scan: String,
    pub date_email_sent: String,
    pub date_canceled_account: String,
    pub date_deleted_account: String,
    pub current_period_ends_at: String,
    pub id_user_modified: String,
    pub id_user_owner: String,
    pub date_modified: String,
    pub date_created: String,
}

impl CarboniteResponse {
    pub async fn from_customer_email(&self, customer_email: String, client: Client) -> anyhow::Result<Vec<Self>, anyhow::Error> {
        let mut params: HashMap<&str, &str> = HashMap::new();
        params.insert("user_email", "logan.lees@pclaptops.com");
        params.insert("user_password", "Poolparty1");
        params.insert("application", "carbonite");
        params.insert("action", "search");
        params.insert("search", &customer_email);

        let response = client
            .post("https://scaffold.pclaptops.com/api/index")
            .header(reqwest::header::CONTENT_TYPE, "application/json") // application/x-www-form-urlencoded
            .form(&params)
            .send()
            .await?;

        let response_json: Vec<Self> = response.json().await?;
        log::info!("response_json: {:?}", response_json);
        Ok(response_json)
    }

    fn latest_timestamp(&self) -> Option<chrono::NaiveDateTime> {
        let format = "%Y-%m-%d %H:%M:%S";

        let last_scan = chrono::NaiveDateTime::parse_from_str(&self.date_last_scan, format).ok();
        let date_modified = chrono::NaiveDateTime::parse_from_str(&self.date_modified, format).ok();

        match (last_scan, date_modified) {
            (Some(scan), Some(modified)) => Some(scan.max(modified)),
            (Some(scan), None) => Some(scan),
            (None, Some(modified)) => Some(modified),
            _ => None,
        }
    }
}

pub fn find_latest_carbonite_entry(entries: &[CarboniteResponse]) -> Option<&CarboniteResponse> {
    entries
        .iter()
        .filter_map(|entry| entry.latest_timestamp().map(|ts| (entry, ts)))
        .max_by_key(|&(_, ts)| ts)
        .map(|(entry, _)| entry)
}


#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DriveData {
    pub drive_letter: String,
    pub drive_type: String,
    pub total_size: String,
    pub space_left: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct HardwareTests {
    pub hdd_test: String,
    pub ssd_test: String,
    pub ram_test: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Difference)]
pub struct TaskNotePayload {
    pub id: RecordId,
    pub task_id: Option<RecordId>,
    pub everest_initials: String,
    pub created_at: String,
    pub note: String,
    pub username: String,
    pub id_customer_thread: Option<String>,
    pub id_customer_message: Option<String>,
    pub id_employee: Option<String>,
    pub user: Option<RecordId>,
    // #[serde(deserialize_with = "deserialize_to_string")]
    pub service_number: Option<String>
}

impl Default for TaskNotePayload {
    fn default() -> Self {
        Self {
            id: RecordId::from((TASK_NOTE_TABLE, Uuid::new_v4().to_raw().split_terminator('-').collect::<Vec<&str>>().concat())),
            task_id: Default::default(),
            everest_initials: Default::default(),
            created_at: Default::default(),
            note: Default::default(),
            username: Default::default(),
            id_customer_thread: Default::default(),
            id_customer_message: Default::default(),
            id_employee: Default::default(),
            user: Default::default(),
            service_number: Default::default()
        }
    }
}

#[derive(Serialize, Debug, Clone, Deserialize, PartialEq, Difference)]
pub struct ConnectedClient {
    pub id: RecordId,
    pub assigned_user: Option<RecordId>,
    pub client_hash: String,
    pub connection_string: String,
    pub command_history: Option<Vec<String>>,
    pub connected: bool,
    pub friendly_name: Option<String>,
    pub customer: Option<RecordId>,
    pub last_update: Option<String>,
    pub created_at: Option<String>,
    pub computer:  Option<RecordId>
}

impl Default for ConnectedClient {
    fn default() -> Self {
        Self {
            id: RecordId::from((CONNECTED_CLIENT_TABLE, Uuid::new_v4().to_raw().split_terminator('-').collect::<Vec<&str>>().concat())),
            assigned_user: Default::default(),
            client_hash: Default::default(),
            connection_string: Default::default(),
            command_history: Default::default(),
            connected: Default::default(),
            friendly_name: Default::default(),
            customer: Default::default(),
            last_update: Default::default(),
            created_at: Default::default(),
            computer: Default::default(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Difference)]
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
}
impl Default for Notification {
    fn default() -> Self {
        Self {
            id: RecordId::from((NOTIFICATION_TABLE, Uuid::new_v4().to_raw().split_terminator('-').collect::<Vec<&str>>().concat())),
            user: RecordId::from((USER_TABLE, Uuid::new_v4().to_raw().split_terminator('-').collect::<Vec<&str>>().concat())),
            notification_description: Default::default(),
            notification_type: Default::default(),
            status: Default::default()
        }
    }
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum NotificationType {
    NewMessage,
    SpoStatusChange,
    NewTask,
    TaggedInComment,
    GroupTag,
    OverdueTask,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum NotificationStatus {
    Read,
    Unread,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ModifyNotification {
    pub id: RecordId,
    pub everest_initials: Option<String>,
    /// either Read or Unread
    pub status: Option<NotificationStatus>,
    pub mark_all_read: Option<bool>,
    pub mark_all_unread: Option<bool>,
    pub archive: Option<bool>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub enum Status {
    #[default]
    Todo,
    InRepair,
    Complete,
    CustomStatus(String),
}
// Trait that all statuses (including user-defined ones) can implement
// trait TaskStatuses {
//     fn get_user_statuses(&self) -> Vec<Status>;
//     fn add_new_user_status(&self) -> Vec<Status>;
// }
// impl TaskStatuses for Status {
//     fn get_user_statuses(&self) -> Vec<Status> {
//         match self {
//             Status::Todo => todo!(),
//             Status::InRepair => todo!(),
//             Status::Complete => todo!(),
//             Status::CustomStatus(_) => todo!(),
//         }
//     }

//     fn add_new_user_status(&self) -> Vec<Status> {
//         todo!()
//     }
// }
// // Implement the TaskStatus trait for your predefined statuses
// impl TaskStatuses for User {
//     fn get_user_statuses(&self) -> &str {
//         match self {
//             Status::Todo => "Todo",
//             Status::InRepair => "In Repair",
//             Status::Complete => "Complete",
//             Status::CustomStatuses(user_statuses) => {

//             }
//         }
//     }
    
//     fn add_new_user_status(&self) -> Vec<Status> {
        
//         self.user_statuses.push(value);
//     }
// }

impl Status {
    pub const VALUES: [Self; 3] = [Self::Todo, Self::InRepair, Self::Complete];
    pub fn as_str(&self) -> &str {
        match self {
            Status::Todo => "Todo",
            Status::InRepair => "In Repair",
            Status::Complete => "Complete",
            Status::CustomStatus(status) => &status
        }
    }
}

impl User {
    pub fn add_custom_status(&mut self, _new_status: &str) {
        // if let Status::CustomStatus(ref mut user_statuses) = self {
        //     user_statuses.push(new_status.to_string());
        // }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub enum Priority {
    Express,
    Rfs,
    Fire,
    Qc,
    #[default]
    Normal,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Category {
    StoreTasks,
    MyTasks,
    CompletedTasks,
}

#[derive(Deserialize)]
struct CommandRequest {
    _client_id: String,
    _command: String,
}

#[derive(Debug, Deserialize, Serialize, Default, Clone)]
pub struct GetKeysResponse {
    pub webroot_key: String,
    pub superanti_key: String,
}

#[derive(Clone, Serialize, Deserialize, Debug, Default )]
pub struct SystemInformation {
    /// Live CPU usage as a percentaget
    pub cpu_percentage: f32,
    /// Live CPU clock speed
    pub cpu_clock: f32,
    /// Live system temps
    pub component_temps: HashMap<String, f32>,
    /// Live RAM usage in Mb
    pub used_memory: f32,
    /// Total RAM
    pub total_memory: f32,
    /// Disk usage
    pub disks: String,
    /// Name of machine
    pub name: String,
    /// Kernel version
    pub kernel_version: String,
    /// OS version
    pub os_version: String,
    /// Hostname based on DNS
    pub hostname: String,
    /// Number of Physical CPU's
    pub number_of_cpus: String,
    /// list of network interfaces and 
    pub network_interfaces: Vec<NetworkInterface>,
    /// List of active processes on host
    pub processes: Vec<Process>,
    pub gpu_info: Gpu
}

#[derive(Clone, Serialize, Deserialize, Debug, Default)]
pub struct Gpu {
    pub usage: Vec<GraphicsUsage>,
    pub card: Vec<GraphicsCard>
}

/// Graphic card usage by process
#[derive(Clone, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct GraphicsProcessUtilization {
    /// Process identificator
    pub pid: u32,
    /// Gpu identificator
    pub gpu: u32,
    /// Memory usage
    pub memory: u32,
    /// Gpu encoder utilization as percentage
    pub encoder: u32,
    /// Gpu decoder utilization as percentage
    pub decoder: u32    
}

/// Graphic card usage summary
#[derive(Clone, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct GraphicsUsage {
    /// Graphic card id
    pub id: String,
    /// Memory utilization as percentage
    pub memory_usage: u32,
    /// Memroy usage as bytes
    pub memory_used: u64,
    /// Gpu encoder utilization as percentage
    pub encoder: u32,
    /// Gpu decoder utilization as percentage
    pub decoder: u32,
    /// Gpu utilization as percentage
    pub gpu: u32,
    /// Gpu temperature
    pub temperature: u32,
    /// Processes using this GPU
    pub processes: Vec<GraphicsProcessUtilization>
}

/// Information about a graphic card
#[derive(Clone, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct GraphicsCard {
    /// Device id
    pub id: String,
    /// Device id
    pub name: String,
    /// Device brand
    pub brand: String,
    /// Total memory
    pub memory: u64,
    /// Device temperature
    pub temperature: u32,
    pub nvidia_info: NvidiaInfo
}


/// Nvidia drivers configuration
#[derive(Clone, Serialize, Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct NvidiaInfo {
     /// Nvidia drivers
     pub driver_version: String,
     /// NVML version
     pub nvml_version: String,
     /// Cuda version
     pub cuda_version: i32,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Default)]
pub struct Process {
    /// Process ID
    pub id: u32,
    pub name: String,
    pub cmd: String,
    pub user_id: Option<String>,
    pub memory: f32,
    pub cpu_usage: f32,
    pub process_disk_usage: ProcessDiskUsage
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Default)]
pub struct ProcessDiskUsage {
    pub read_bytes: f32,
    pub total_read_bytes: f32,
    pub total_written_bytes: f32,
    pub written_bytes: f32,
}

#[derive(Clone, Serialize, Deserialize, Debug, PartialEq, Default)]
pub struct NetworkInterface {
    /// Process ID
    pub interface_name: String,
    pub total_received: f32,
    pub total_transmitted: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Node {
    Folder(String, HashMap<String, Node>),
    File((String, String)),
}

impl Default for Node {
    fn default() -> Self {
        let node = Node::Folder(String::new(), HashMap::new());
        node
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Copy, Default, Eq, PartialOrd, Ord)]
pub enum Store {
    #[default]
    RIV,
    LTN,
    MUR,
    AF,
    WJ,
    ORE,
    SAN,
}

#[derive(Debug, Clone, Serialize)]
pub struct SpecialPartOrder {
    customer_name: String,              //  "kathleen Hoffmon",
    customer_phone_number: String,      //  "801-888-8888",
    notes: String,                      //  "These are some notes",
    system_order_number: String,        //  "123456",
    id_location: String,                //  "Riverdale",
    request_type: String,               //  "Any",
    shipping_method: String,            //  "2 - 2-3 Day Express",
    part_manufacturer: Manufacturer,    //  "PC Laptops",
    manufacturer_model_number: String,  //  "12345Test",
    manufacturer_serial_number: String, //  "123456789",
    manufacturer_part_number: String,   //  "324657687",
    part_color: String,                 //  "N/A",
    part_description: String,           //  "Test",
    part_lcd_toggle: bool,              //  "0"
    spo_status: SpoStatus,
}

#[derive(PartialEq, Default, Debug, Serialize, Clone)]
pub enum SpoStatus {
    #[default]
    AwaitingQuote,
    QuoteFullfilled,
    OrderPendingDM,
}

#[derive(PartialEq, Default, Debug, Serialize, Clone)]
pub enum Manufacturer {
    #[default]
    Pclaptops,
    Other,
}

impl Manufacturer {
    pub fn as_str(&mut self) -> &str {
        match self {
            Manufacturer::Pclaptops => "PC Laptops",
            Manufacturer::Other => "Other",
        }
    }
}

impl SpoStatus {
    pub fn as_str(&mut self) -> &str {
        match self {
            SpoStatus::AwaitingQuote => "Awaiting Quote",
            SpoStatus::OrderPendingDM => "Pending DM",
            SpoStatus::QuoteFullfilled => "Quote Fullfilled",
        }
    }
}

impl Default for SpecialPartOrder {
    fn default() -> Self {
        Self {
            customer_name: String::new(),
            customer_phone_number: String::new(),
            notes: String::new(),
            system_order_number: String::new(),
            id_location: "0".to_string(),
            request_type: String::new(),
            shipping_method: "2 - 2-3 Day Express".to_string(),
            part_manufacturer: Manufacturer::Pclaptops,
            manufacturer_model_number: String::new(),
            manufacturer_serial_number: String::new(),
            manufacturer_part_number: String::new(),
            part_color: "N/A".to_string(),
            part_description: String::new(),
            part_lcd_toggle: false,
            spo_status: SpoStatus::AwaitingQuote,
        }
    }
}

impl Store {
    pub fn as_str(&self) -> &str {
        match self {
            Store::RIV => "RIV",
            Store::LTN => "LTN",
            Store::MUR => "MUR",
            Store::AF => "AF",
            Store::WJ => "WJ",
            Store::ORE => "ORE",
            Store::SAN => "SAN",
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
        }
    }
    pub const VALUES: [Self; 7] = [
        Self::RIV,
        Self::LTN,
        Self::MUR,
        Self::AF,
        Self::WJ,
        Self::ORE,
        Self::SAN,
    ];
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct User {
    pub id: RecordId,
    pub name: String,
    pub everest_initials: String,
    pub email: String,
    pub store: Store,
    // pub notifications: Option<Vec<NotificationId>>,
    pub minio_access_key: Option<String>,
    pub minio_secret_key: Option<String>,
    pub user_settings: UserSettings,
    pub id_prestashop: Option<u64>,
    pub id_store: Option<String>,
    pub chat_threads: Option<Vec<ChatThreads>>,
    pub user_statuses: Option<Vec<Status>>
}

impl Default for User {
    fn default() -> Self {
        Self {
            id: RecordId::from((USER_TABLE, Uuid::new_v4().to_raw().split_terminator('-').collect::<Vec<&str>>().concat())),
            name: String::new(),
            everest_initials: String::new(),
            email: String::new(),
            store: Store::default(),
            minio_access_key: None,
            minio_secret_key: None,
            user_settings: UserSettings::default(),
            id_store: None,
            id_prestashop: None,
            chat_threads: None,
            user_statuses: None,
        }
    }
}

impl Eq for User {}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default, Eq)]
pub struct UserSettings {
    pub color_scheme: Value,
    pub ui_layout: UiLayout
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default, Eq)]
pub struct UiLayout {
    pub mtechserver: Value,
    pub mastertech: Value,
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
