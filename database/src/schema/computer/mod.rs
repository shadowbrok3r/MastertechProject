use crate::{schema::COMPUTER_TABLE, DATABASE};
use structdiff::{Difference, StructDiff};
use serde_json::Value;

use super::{random_record_id, RecordId, SurrealValue};

pub mod system_information;
pub mod seb;

pub use system_information::*;
pub use seb::*;

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Difference, SurrealValue)]
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
    pub current_antivirus: Vec<String>,
    pub motherboard_name: String,
    pub motherboard_serial: String,
    pub motherboard_asset_tag: String,
    pub motherboard_vendor: String,
    pub product_name: String,
    pub product_sku: String,
    pub product_serial: String,
    pub product_vendor: String,
    pub installed_programs: Option<Value>
}

impl Default for ComputerData {
    fn default() -> Self {
        Self {
            id: random_record_id(COMPUTER_TABLE),
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
            motherboard_name: Default::default(),
            motherboard_serial: Default::default(),
            motherboard_asset_tag: Default::default(),
            motherboard_vendor: Default::default(),
            product_name: Default::default(),
            product_sku: Default::default(),
            product_serial: Default::default(),
            product_vendor: Default::default(),
            installed_programs: Default::default(),
            current_antivirus: Default::default(),
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

    pub async fn get_associated_computer(id: RecordId) -> anyhow::Result<Self, anyhow::Error> {
        let computer: Option<Self> = DATABASE
            .query("SELECT VALUE service_ticket.computer.* FROM task WHERE id == $id")
            .bind(("id", id))
            .await?
            .take(0)?;
        Ok(computer.unwrap_or_default())
    }

    pub async fn get_computers_by_customer_id(customer_id: String) -> anyhow::Result<Vec<Self>, anyhow::Error> {
        let computers: Vec<Self> = DATABASE
            .query("SELECT * FROM computer WHERE customer.cust_code == $customer_id")
            .bind(("customer_id", customer_id))
            .await?
            .take(0)?;

        Ok(computers)
    }

    pub async fn get_computers(start: i32) -> anyhow::Result<Vec<Self>, anyhow::Error> {
        let computers: Vec<Self> = DATABASE
            .query("SELECT * FROM computer START $start LIMIT 200")
            .bind(("start", start))
            .await?
            .take(0)?;

        Ok(computers)
    }

    pub async fn create_computer(&self) -> anyhow::Result<Option<Self>, anyhow::Error> {
        let computer: Option<Self> = DATABASE
            .create(self.id.clone())
            .content(self.clone())
            .await?;

        Ok(computer)
    }

    pub async fn update_computer(&self) -> anyhow::Result<Option<Self>, anyhow::Error> {
        let computer: Option<Self> = DATABASE
            .upsert(self.id.clone())
            .content(self.clone())
            .await?;

        Ok(computer)
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, SurrealValue)]
pub struct DriveData {
    pub drive_letter: String,
    pub drive_type: String,
    pub total_size: String,
    pub space_left: String,
}


