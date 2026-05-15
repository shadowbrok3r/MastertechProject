use crate::{DATABASE, schema::{COMPUTER_TABLE, CUSTOMER_TABLE}};
use structdiff::{Difference, StructDiff};
use serde_json::Value;

use super::{random_record_id, RecordId, SurrealValue};

pub mod system_information;
pub mod seb;

pub use system_information::*;
pub use seb::*;

/// Where the inventory record came from — different sources have
/// different reliability and freshness characteristics. The admin
/// surfaces this in the row so it's visible whether "Webroot is
/// installed" is a Windows Security Center fact (authoritative) or
/// just a heuristic from finding the install directory under
/// `Program Files`.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq, SurrealValue)]
#[serde(rename_all = "snake_case")]
pub enum SecurityProductSource {
    /// WMI `SecurityCenter2` — the authoritative Windows-managed list
    /// of registered AV/AS products, including `is_enabled` and
    /// `is_up_to_date` flags. Available on Win 7+.
    SecurityCenter,
    /// Windows registry `HKLM\…\Uninstall` walk — covers any product
    /// with an MSI/InnoSetup/NSIS installer footprint, AV or not. Used
    /// as a fallback to fill in `version` when SecurityCenter doesn't
    /// publish it.
    Registry,
    /// Best-effort directory scan under `Program Files` /
    /// `Program Files (x86)`. Last-resort identifier when neither
    /// SecurityCenter nor the registry has the product — common for
    /// portable or "scanner-only" tools like SuperAntiSpyware Portable.
    Heuristic,
}

impl Default for SecurityProductSource {
    fn default() -> Self {
        Self::Heuristic
    }
}

/// One installed security product (antivirus, antispyware, EDR agent,
/// etc.) discovered on a connected client. Replaces the old
/// `Vec<String>` shape that only stored a display name — keeping the
/// per-product `active` / `definitions_updated_at` / `update_available`
/// fields means the admin can see at a glance which clients have a
/// dormant or outdated AV without opening each one.
///
/// **Backwards compatibility.** Existing SurrealDB rows have either
/// `current_antivirus: []`, `current_antivirus: null`, or
/// `current_antivirus: ["Webroot", …]` (legacy). The custom
/// deserializer below (`deserialize_security_products`) handles all
/// three shapes, mapping legacy strings to
/// `InstalledSecurityProduct { name, ..Default::default() }`.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, SurrealValue, Default)]
pub struct InstalledSecurityProduct {
    /// Human display name — what the user sees in the row. Always
    /// populated; everything else is best-effort.
    pub name: String,
    /// Publisher (e.g. "Webroot Inc.", "Microsoft Corporation").
    /// Pulled from the registry's `Publisher` value when available.
    #[serde(default)]
    pub vendor: Option<String>,
    /// Installed version string as the publisher reports it. Free-form
    /// because vendors don't agree on a versioning scheme — comparison
    /// against "update_available" should be done by the gathering code,
    /// not by the admin UI.
    #[serde(default)]
    pub version: Option<String>,
    /// `Some(true)` = the product is currently running / monitoring;
    /// `Some(false)` = installed but disabled (often a sign of
    /// tampering on infected machines); `None` = we couldn't determine.
    #[serde(default)]
    pub active: Option<bool>,
    /// When the product's virus / threat definitions were last updated,
    /// per the product itself. Important for "looks installed but is
    /// actually six months stale" situations.
    #[serde(default)]
    pub definitions_updated_at: Option<chrono::DateTime<chrono::Utc>>,
    /// `Some(true)` if the gathering code has determined a newer
    /// product version is available; `Some(false)` if the install is
    /// current; `None` if we don't know how to check this product.
    /// The admin can sort by this to surface clients that need
    /// updates.
    #[serde(default)]
    pub update_available: Option<bool>,
    /// Where this record came from — see [`SecurityProductSource`].
    #[serde(default)]
    pub source: SecurityProductSource,
}

/// Tolerant deserializer that accepts:
///   - `null` → empty vec
///   - missing field → empty vec (via `#[serde(default)]` on the field)
///   - `[]` → empty vec
///   - `["string", …]` (legacy schema before this struct existed) →
///     each string maps to `InstalledSecurityProduct { name, .. }`
///   - `[{ "name": "…", "version": "…" }, …]` (new schema) → straight
///     deserialization
///
/// Without this, the field-type change would break every existing
/// `computer` row whose JSON still contains the old string array.
fn deserialize_security_products<'de, D>(
    deserializer: D,
) -> Result<Vec<InstalledSecurityProduct>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::Deserialize;

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Compat {
        Modern(InstalledSecurityProduct),
        Legacy(String),
    }

    let raw: Option<Vec<Compat>> = Option::deserialize(deserializer)?;
    Ok(raw
        .unwrap_or_default()
        .into_iter()
        .map(|c| match c {
            Compat::Modern(p) => p,
            Compat::Legacy(name) => InstalledSecurityProduct {
                name,
                ..Default::default()
            },
        })
        .collect())
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Difference, SurrealValue)]
pub struct ComputerData {
    pub id: RecordId,
    pub customer: RecordId,
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
    /// Per-product security inventory (antivirus / antispyware / EDR).
    /// Replaces the historical `Vec<String>` shape — see
    /// [`InstalledSecurityProduct`] and `deserialize_security_products`
    /// for the backwards-compatibility story. Empty by default; will
    /// be filled in by slice 2 of this refactor (auto info-checks on
    /// connect).
    #[serde(default, deserialize_with = "deserialize_security_products")]
    pub current_antivirus: Vec<InstalledSecurityProduct>,
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
            customer: random_record_id(CUSTOMER_TABLE),
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


