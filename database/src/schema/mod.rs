use anyhow::Error;
use async_trait::async_trait;
use helper_traits::GetAssociatedDataFromId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use structdiff::{Difference, StructDiff};
use surrealdb::{
    opt::RecordId,
    sql::{Id, Thing},
};

use crate::DATABASE;

pub mod buckets;
pub mod deserializer;
pub mod helper_traits;
pub mod prestashop_schema;
pub mod utilities;

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

#[async_trait(?Send)]
impl<D> GetAssociatedDataFromId<D> for Thing {
    async fn get_associated_data<Thing>(&mut self) -> Result<D, Error>
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
    pub id: Thing,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ClientId(pub RecordId);

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ComputerId(pub RecordId);

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct CustomerId(pub RecordId);

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct TicketId(pub RecordId);

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct UserId(pub RecordId);

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct TaskId(pub RecordId);

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct TaskNoteId(pub RecordId);

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct SebId(pub RecordId);

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct NotificationId(pub RecordId);

#[derive(Serialize, Debug)]
pub struct RecordResult {
    pub result: bool,
    pub record: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RecordSuccess {
    pub success: bool,
}

impl Default for UserId {
    fn default() -> Self {
        UserId(Thing::from((String::new(), Id::String(String::new()))).clone())
    }
}

impl Default for TaskId {
    fn default() -> Self {
        TaskId(Thing::from((String::new(), Id::String(String::new()))).clone())
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Difference)]
pub struct TaskPayload {
    pub id: Option<TaskId>,
    pub task_name: String,
    pub service_ticket: Option<TicketPayload>,
    pub everest_initials: String,
    pub task_description: String,
    pub assignee: UserId, // should i use a user id here or will email and name be enough for tracking?
    pub service_number: Option<String>,
    pub due_date: String, // optional because if not provided, set due date to creation date
    pub priority: Priority,
    #[difference(collection_strategy = "ordered_array_like")]
    pub task_note: Vec<TaskNotePayload>,
    pub completed: bool,
    pub status: Status,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Difference)]
pub struct LiveTaskPayload {
    pub id: Option<TaskId>,
    pub task_name: String,
    pub service_ticket: Option<TicketId>,
    // #[serde(skip)]
    pub everest_initials: String,
    pub task_description: String,
    pub assignee: UserId, // should i use a user id here or will email and name be enough for tracking?
    pub service_number: Option<String>,
    pub due_date: String, // optional because if not provided, set due date to creation date
    pub priority: Priority,
    #[difference(collection_strategy = "ordered_array_like")]
    pub task_note: Vec<TaskNoteId>,
    pub completed: bool,
    pub status: Status,
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

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Difference)]
pub struct TicketPayload {
    pub id: Option<TicketId>,
    pub created_at: Option<String>,
    pub customer: Option<CustomerData>,
    pub computer: Option<ComputerData>,
    pub service_ticket: Option<TaskId>,
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

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq, Difference)]
pub struct TicketData {
    // Live Ticket Payload
    pub id: Option<TicketId>,
    pub created_at: Option<String>,
    pub customer: Option<CustomerId>,
    pub computer: Option<ComputerId>,
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
            customer: ticket.customer.unwrap_or_default().id,
            computer: ticket.computer.unwrap_or_default().id,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Difference)]
pub struct CustomerData {
    pub id: Option<CustomerId>,
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

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
pub struct ComputerData {
    pub id: Option<ComputerId>,
    pub customer: Option<CustomerId>,
    pub seb_info: Option<LocalSebData>,
    pub hostname: String,
    pub operating_system: String,
    pub cpu: String,
    pub gpu: String,
    pub ram: String,
    pub drives: Vec<DriveData>,
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

#[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq)]
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

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, Difference)]
pub struct TaskNotePayload {
    pub id: Option<TaskNoteId>,
    pub task_id: Option<TaskId>,
    pub everest_initials: String,
    pub created_at: String,
    pub note: String,
    // pub id_customer_thread: Option<String>,
    // pub id_employee: i32
}

#[derive(Serialize, Debug, Clone, Deserialize, Default, PartialEq, Difference)]
pub struct ConnectedClient {
    pub id: Option<ClientId>,
    pub assigned_user: Option<UserId>,
    pub client_hash: String,
    pub connection_string: String,
    pub command_history: Option<Vec<String>>,
    pub connected: bool,
    pub friendly_name: Option<String>,
    pub customer: Option<CustomerId>,
    pub last_update: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Notification {
    /// receiver of notification
    pub user: UserId,
    /// description of notification
    pub notification_description: String,
    /// type of notification
    pub notification_type: String,
    /// Has the notification been read?
    pub status: String,
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
    pub id: NotificationId,
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

#[derive(Clone, Serialize, Deserialize, Debug, Difference)]
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

    pub network_interfaces: HashMap<String, String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Cmd {
    LiveData,
    Command,
    Tuneup,
    Cps,
    Qc,
    SfcScan,
    DismScan,
    ChkDsk,
    Mbr2Gpt,
    TaskManager,
    ReadDir(String),
    UninstallProgram(String),
    PullKeys(String),
    PullTicket(String),
    DirContents(Node),
    UpDirectory(String),
    ChangeDirectory(String),
    Execute(String),
    InteractiveInput(String),
    CopyTools(String),
    QuitInteractive,
    ReadEvents,
    Quit,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Node {
    File((String, String)),
    Folder(String, HashMap<String, Node>),
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

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default, Eq, PartialOrd, Ord)]
pub struct User {
    pub id: UserId,
    pub name: String,
    pub everest_initials: String,
    pub email: String,
    pub store: Store,
    // pub notifications: Option<Vec<NotificationId>>,
    pub minio_access_key: Option<String>,
    pub minio_secret_key: Option<String>,
    pub user_settings: Option<UserSettings>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default, Eq, PartialOrd, Ord)]
pub struct UserSettings {
    pub color_scheme: ColorSchemes, // ui.color_edit_button_srgba(color)
    pub startup_tabs: String,
    pub my_column_layout: String,
    pub opened_tabs: String,
    pub filters: String,
    pub saved_queries: String,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default, Eq, PartialOrd, Ord)]
pub struct ColorSchemes {
    pub visuals: String,
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

impl Status {
    pub fn as_str(&self) -> &str {
        match self {
            Status::Todo => "Todo",
            Status::InRepair => "In Repair",
            Status::Complete => "Complete",
        }
    }
    pub const VALUES: [Self; 3] = [Self::Todo, Self::InRepair, Self::Complete];
}
