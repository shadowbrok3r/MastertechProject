use reqwest::{header::{ACCEPT, CONTENT_TYPE}, Client};
use helper_traits::GetAssociatedDataFromId;
use structdiff::{Difference, StructDiff};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use async_trait::async_trait;
use surrealdb::RecordId;
use serde_json::Value;
use crate::DATABASE;
use anyhow::Error;

pub mod helper_traits;
pub mod deserializer;
pub mod utilities;
pub mod get_data;
pub mod buckets;
pub mod task;
pub mod task_note;
pub mod user;
pub mod ticket;
pub mod prestashop;

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
pub const CHAT_THREAD_TABLE: &str = "chat_thread";
pub const USER_MESSAGE_TABLE: &str = "user_message";


pub use task::*;
pub use task_note::*;
pub use user::*;
pub use ticket::*;
pub use prestashop as prestashop_schema;

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
    pub computers: Vec<RecordId>
}

impl Default for CustomerData {
    fn default() -> Self {
        Self {
            id: RecordId::from((CUSTOMER_TABLE, surrealdb::RecordIdKey::from_inner(surrealdb::sql::Id::rand()))),
            cust_code: Default::default(),
            part_order_links: Default::default(),
            name: Default::default(),
            phone_number: Default::default(),
            phone_number_2: Default::default(),
            email: Default::default(),
            li_doc: Default::default(),
            li_amnt: Default::default(),
            num_inv: Default::default(),
            computers: Default::default(),
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
    pub installed_programs: Option<Value>
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Difference)]
pub struct Job {
    computer: RecordId
}

impl Default for ComputerData {
    fn default() -> Self {
        Self {
            id: RecordId::from((COMPUTER_TABLE, surrealdb::RecordIdKey::from_inner(surrealdb::sql::Id::rand()))),
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
        // let mut params: HashMap<&str, &str> = HashMap::new();
        let json = serde_json::json!({
            "user_email": "logan.lees@pclaptops.com",
            "user_password": "Poolparty1",
            "application": "carbonite",
            "action": "search",
            "search": &customer_email
        });

        let response = client
            .post("https://scaffold.pclaptops.com/api/index")
            .header(CONTENT_TYPE, "application/json") // application/x-www-form-urlencoded
            .header(ACCEPT, "application/json")
            .json(&json)
            // .form(&params)
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
            id: RecordId::from((CONNECTED_CLIENT_TABLE, surrealdb::RecordIdKey::from_inner(surrealdb::sql::Id::rand()))),
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
            id: RecordId::from((NOTIFICATION_TABLE, surrealdb::RecordIdKey::from_inner(surrealdb::sql::Id::rand()))),
            user: RecordId::from((USER_TABLE, surrealdb::RecordIdKey::from_inner(surrealdb::sql::Id::rand()))),
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
    Sales,
    Qc,
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
        match status {
            "Todo" => Status::Todo,
            "In Repair" => Status::InRepair,
            "Complete" => Status::Complete,
            "Sales" => Status::Sales,
            "QC" => Status::Qc,
            _ => Status::CustomStatus(status.to_string())
        }
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

    pub fn from_presta_store_id(store_id: &str) -> Self {
        match store_id {
            "7" => Self::RIV,
            "8" => Self::LTN,
            "10" => Self::MUR,
            "11" => Self::WJ,
            "12" => Self::SAN,
            "13" => Self::AF,
            "14" => Self::ORE,
            _ => Self::RIV,
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
