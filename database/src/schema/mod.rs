use reqwest::{header::{ACCEPT, CONTENT_TYPE}, Client};
use crate::{DATABASE, SCAFFOLD_PASS, SCAFFOLD_USER, schema::prestashop::Order};
use helper_traits::GetAssociatedDataFromId;
use structdiff::{Difference, StructDiff};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use async_trait::async_trait;
use anyhow::Error;

// Re-export types from surrealdb for use throughout the schema
pub use surrealdb::types::{RecordId, Datetime, Bytes, Value as SurrealDBValue, SurrealValue};

pub mod helper_traits;
pub mod deserializer;
pub mod utilities;
pub mod get_data;
pub mod buckets;
pub mod task;
pub mod task_note;
pub mod task_note_read;
pub mod user;
pub mod ticket;
pub mod prestashop;
pub mod notification;
pub mod odoo;
pub mod computer;
pub mod customer;
pub mod client;
pub mod sales_tracker;
pub mod everest;
pub mod duplicate_check;
pub mod file_storage;
pub mod plugin_registry;
pub mod diagnostic;

pub use task::*;
pub use task_note::*;
pub use task_note_read::*;
pub use user::*;
pub use ticket::*;
pub use notification::*;
pub use computer::*;
pub use customer::*;
pub use client::*;
pub use sales_tracker::*;
pub use everest::*;
pub use utilities::TaskCreationResult;
pub use duplicate_check::*;
pub use file_storage::*;
pub use plugin_registry::*;
pub use diagnostic::*;

pub const NS: &str = "Mastertech";
pub const DB: &str = "MastertechDB";
pub const USER_SCOPE: &str = "user";
pub const TICKET_TABLE: &str = "service_order";
pub const CUSTOMER_TABLE: &str = "customer";
pub const COMPUTER_TABLE: &str = "computer";
pub const TASK_TABLE: &str = "task";
pub const TASK_HISTORY_TABLE: &str = "task_history";
pub const TASK_NOTE_TABLE: &str = "task_note";
pub const TASK_NOTE_READ_TABLE: &str = "task_note_read";
pub const SEB_TABLE: &str = "seb_data";
pub const USER_TABLE: &str = "user";
pub const NOTIFICATION_TABLE: &str = "notification";
pub const CONNECTED_CLIENT_TABLE: &str = "connected_client";
pub const CHAT_THREAD_TABLE: &str = "chat_thread";
pub const USER_MESSAGE_TABLE: &str = "user_message";
pub const QC_TABLE: &str = "qc";
pub const SALES_NOTE_TABLE: &str = "sales_note";
pub const PLUGIN_REGISTRY_TABLE: &str = "plugin_registry";
pub const DIAGNOSTIC_SESSION_TABLE: &str = "diagnostic_session";
pub const DIAGNOSTIC_ENTRY_TABLE: &str = "diagnostic_entry";

pub use prestashop as prestashop_schema;

// Re-export RecordIdKey for use in other modules
pub use surrealdb::types::RecordIdKey;

// Helper function to generate random record IDs
pub fn random_record_id(table: &str) -> RecordId {
    RecordId::new(table, uuid::Uuid::new_v4().to_string())
}

/// Helper function to convert RecordIdKey to String
/// SurrealDB 3.0 RecordIdKey doesn't implement Display directly
pub fn record_id_key_to_string(key: &RecordIdKey) -> String {
    match key {
        RecordIdKey::String(s) => s.clone(),
        RecordIdKey::Number(n) => n.to_string(),
        RecordIdKey::Uuid(u) => u.to_string(),
        RecordIdKey::Array(a) => serde_json::to_string(a).unwrap_or_default(),
        RecordIdKey::Object(o) => serde_json::to_string(o).unwrap_or_default(),
        RecordIdKey::Range(r) => serde_json::to_string(r).unwrap_or_default(),
    }
}

/// Helper trait extension for RecordId to easily get key as string
pub trait RecordIdExt {
    fn key_string(&self) -> String;
}

impl RecordIdExt for RecordId {
    fn key_string(&self) -> String {
        record_id_key_to_string(&self.key)
    }
}

#[async_trait(?Send)]
impl<D: SurrealValue> GetAssociatedDataFromId<D> for RecordId {
    async fn get_associated_data<T>(&mut self) -> Result<D, Error>
    where
        D: for<'de> Deserialize<'de> + SurrealValue,
    {
        let id = self.clone();

        let data: D = DATABASE.select(id).await?.unwrap();
        Ok(data)
    }
}

#[derive(Debug, Deserialize, Serialize, Clone, SurrealValue)]
pub struct Record {
    #[allow(dead_code)]
    pub id: RecordId,
}

#[derive(Serialize, Debug, SurrealValue)]
pub struct RecordResult {
    pub result: bool,
    pub record: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, SurrealValue)]
pub struct RecordSuccess {
    pub success: bool,
}

#[derive(Debug, Clone, PartialEq, Difference, SurrealValue)]
pub struct Qc {
    pub task: RecordId,
    pub order: Order,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Difference, SurrealValue)]
pub struct Job {
    computer: RecordId
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, SurrealValue)]
pub struct ChatThreads {
    pub id: RecordId,
    pub files: Option<Vec<String>>,
    pub messages: Vec<HashMap<String, String>>,
    pub user: RecordId,
    pub images: Option<Vec<bytes::Bytes>>
}

#[derive(Serialize, Deserialize, Debug, Default, Clone, PartialEq, SurrealValue)]
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
            "user_email": SCAFFOLD_USER,
            "user_password": SCAFFOLD_PASS,
            "application": "carbonite",
            "action": "search",
            "search": &customer_email
        });

        let response = client
            .post(crate::SCAFFOLD_URL)
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

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq, SurrealValue)]
pub struct HardwareTests {
    pub hdd_test: String,
    pub ssd_test: String,
    pub ram_test: String,
}

#[derive(Debug, Deserialize, Serialize, Default, Clone, SurrealValue)]
pub struct GetKeysResponse {
    pub webroot_key: String,
    pub superanti_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
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

#[derive(Debug, Clone, Serialize, SurrealValue)]
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

#[derive(PartialEq, Default, Debug, Serialize, Clone, SurrealValue)]
pub enum SpoStatus {
    #[default]
    AwaitingQuote,
    QuoteFullfilled,
    OrderPendingDM,
}

#[derive(PartialEq, Default, Debug, Serialize, Clone, SurrealValue)]
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
