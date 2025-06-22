use crate::{schema::COMPUTER_TABLE, DATABASE};
use surrealdb::RecordId;
use serde_json::Value;

pub mod system_information;
pub mod seb;

pub use system_information::*;
pub use seb::*;

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
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
    pub installed_programs: Option<Value>
}

impl Default for ComputerData {
    fn default() -> Self {
        Self {
            id: RecordId::from((COMPUTER_TABLE, surrealdb::RecordIdKey::from_inner(surrealdb::sql::Id::rand().into()))),
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

    pub async fn get_computers(start: i32) -> anyhow::Result<Vec<Self>, anyhow::Error> {
        let computers: Vec<Self> = DATABASE
            .query("SELECT * FROM computer START $start LIMIT 200")
            .bind(("start", start))
            .await?
            .take(0)?;

        Ok(computers)
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
pub struct DriveData {
    pub drive_letter: String,
    pub drive_type: String,
    pub total_size: String,
    pub space_left: String,
}


