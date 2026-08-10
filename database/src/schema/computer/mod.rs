use crate::{db, schema::COMPUTER_TABLE};
use facet::Facet;
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
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq, SurrealValue, Facet)]
#[repr(u8)]
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
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, SurrealValue, Default, Facet)]
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
    /// Customer this computer belongs to. `None` for scratch / unlinked
    /// records — the schema was relaxed to `none | record<customer>` in
    /// migration 001 so writes with NONE no longer fail.
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
    /// Per-product security inventory (antivirus / antispyware / EDR).
    /// Replaces the historical `Vec<String>` shape — see
    /// [`InstalledSecurityProduct`] and `deserialize_security_products`
    /// for the backwards-compatibility story. Empty by default; will
    /// be filled in by slice 2 of this refactor (auto info-checks on
    /// connect).
    #[serde(default, deserialize_with = "deserialize_security_products")]
    #[surreal(default)]
    pub current_antivirus: Vec<InstalledSecurityProduct>,
    // Schema-optional (`none | string`); absent on rows created without specs.
    #[surreal(default)]
    pub motherboard_name: String,
    #[surreal(default)]
    pub motherboard_serial: String,
    #[surreal(default)]
    pub motherboard_asset_tag: String,
    #[surreal(default)]
    pub motherboard_vendor: String,
    #[surreal(default)]
    pub product_name: String,
    #[surreal(default)]
    pub product_sku: String,
    #[surreal(default)]
    pub product_serial: String,
    #[surreal(default)]
    pub product_vendor: String,
    /// OA3/MSDM Windows key — the cross-OS identity shared with the pre-boot
    /// UEFI app (SoftwareLicensingService.OA3xOriginalProductKey on Windows,
    /// ACPI MSDM in firmware).
    #[serde(default)]
    #[surreal(default)]
    pub oa3_key: Option<String>,
    pub installed_programs: Option<Value>
}

impl Default for ComputerData {
    fn default() -> Self {
        Self {
            id: random_record_id(COMPUTER_TABLE),
            customer: None,
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
            oa3_key: Default::default(),
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
        let computer: Option<Self> = db()
            .query("SELECT VALUE service_ticket.computer.* FROM task WHERE id == $id")
            .bind(("id", id))
            .await?
            .take(0)?;
        Ok(computer.unwrap_or_default())
    }

    pub async fn get_computers_by_customer_id(customer_id: String) -> anyhow::Result<Vec<Self>, anyhow::Error> {
        let computers: Vec<Self> = db()
            .query("SELECT * FROM computer WHERE customer.cust_code == $customer_id")
            .bind(("customer_id", customer_id))
            .await?
            .take(0)?;

        Ok(computers)
    }

    pub async fn get_computers(start: i32) -> anyhow::Result<Vec<Self>, anyhow::Error> {
        let computers: Vec<Self> = db()
            .query("SELECT * FROM computer START $start LIMIT 200")
            .bind(("start", start))
            .await?
            .take(0)?;

        Ok(computers)
    }

    pub async fn create_computer(&self) -> anyhow::Result<Option<Self>, anyhow::Error> {
        let computer: Option<Self> = db()
            .create(self.id.clone())
            .content(self.clone())
            .await?;

        Ok(computer)
    }

    pub async fn update_computer(&self) -> anyhow::Result<Option<Self>, anyhow::Error> {
        let computer: Option<Self> = db()
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

/// The `computer` spec fields a live [`SystemInformation`] payload can supply.
/// An empty string or an empty `drives` means the feed carried nothing for
/// that field, and writers must leave the stored value alone.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ComputerSpecs {
    pub operating_system: String,
    pub cpu: String,
    pub gpu: String,
    pub ram: String,
    pub drives: Vec<DriveData>,
    pub motherboard_name: String,
    pub motherboard_serial: String,
    pub motherboard_asset_tag: String,
    pub motherboard_vendor: String,
    pub product_name: String,
    pub product_sku: String,
    pub product_serial: String,
    pub product_vendor: String,
}

impl ComputerSpecs {
    /// True when every field is blank, i.e. there is nothing to write.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// The string-valued spec columns carrying a value, as `(column, value)`.
    /// `drives` is excluded — it is the one field that is not a string.
    pub fn populated_strings(&self) -> Vec<(&'static str, &str)> {
        [
            ("operating_system", &self.operating_system),
            ("cpu", &self.cpu),
            ("gpu", &self.gpu),
            ("ram", &self.ram),
            ("motherboard_name", &self.motherboard_name),
            ("motherboard_serial", &self.motherboard_serial),
            ("motherboard_asset_tag", &self.motherboard_asset_tag),
            ("motherboard_vendor", &self.motherboard_vendor),
            ("product_name", &self.product_name),
            ("product_sku", &self.product_sku),
            ("product_serial", &self.product_serial),
            ("product_vendor", &self.product_vendor),
        ]
        .into_iter()
        .filter(|(_, value)| !value.is_empty())
        .map(|(name, value)| (name, value.as_str()))
        .collect()
    }

    /// Names of the fields carrying a value, for reporting what a write covered.
    pub fn populated_fields(&self) -> Vec<&'static str> {
        let mut fields: Vec<&'static str> =
            self.populated_strings().into_iter().map(|(n, _)| n).collect();
        if !self.drives.is_empty() {
            fields.push("drives");
        }
        fields
    }
}

/// Group digits into thousands, matching the `num_format` output the client's
/// own spec-gather writes into [`DriveData`] and `ComputerData::ram`.
fn with_thousands_separators(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

impl From<&SystemInformation> for ComputerSpecs {
    fn from(info: &SystemInformation) -> Self {
        // `name` is the OS family ("Windows") and `os_version` the release
        // ("11"); the client's own gather stores `long_os_version()`.
        let operating_system = format!("{} {}", info.name.trim(), info.os_version.trim())
            .trim()
            .to_string();

        let gpu = info
            .gpu_info
            .card
            .iter()
            .map(|c| {
                let name = c.name.trim();
                let brand = c.brand.trim();
                if !brand.is_empty() && !name.starts_with(brand) {
                    format!("{brand} {name}")
                } else {
                    name.to_string()
                }
            })
            .filter(|name| !name.is_empty())
            .collect::<Vec<_>>()
            .join(", ");

        // `total_memory` arrives in MiB; the client rounds up to whole GiB.
        let ram = if info.total_memory > 0.0 {
            let gib = (info.total_memory as f64 / 1024.0).floor() as u64 + 1;
            format!("{} Gb", with_thousands_separators(gib))
        } else {
            String::new()
        };

        let drives = info
            .disks
            .iter()
            .filter(|d| d.total_space > 0)
            .map(|d| DriveData {
                drive_letter: d.mount_point.clone(),
                // The live feed carries no `DiskKind`; the resource monitor's
                // machine panel labels drives by file system for the same reason.
                drive_type: d.file_system.clone(),
                total_size: with_thousands_separators(d.total_space / (1024 * 1024 * 1024)),
                space_left: with_thousands_separators(d.available_space / (1024 * 1024 * 1024)),
            })
            .collect();

        Self {
            operating_system,
            cpu: info.cpu.trim().to_string(),
            gpu,
            ram,
            drives,
            motherboard_name: info.motherboard_name.trim().to_string(),
            motherboard_serial: info.motherboard_serial.trim().to_string(),
            motherboard_asset_tag: info.motherboard_asset_tag.trim().to_string(),
            motherboard_vendor: info.motherboard_vendor.trim().to_string(),
            product_name: info.product_name.trim().to_string(),
            product_sku: info.product_sku.trim().to_string(),
            product_serial: info.product_serial.trim().to_string(),
            product_vendor: info.product_vendor.trim().to_string(),
        }
    }
}

#[cfg(test)]
mod spec_tests {
    use super::*;

    #[test]
    fn blank_sysinfo_yields_nothing_to_write() {
        let specs = ComputerSpecs::from(&SystemInformation::default());
        assert!(specs.is_empty());
        assert!(specs.populated_fields().is_empty());
    }

    #[test]
    fn sysinfo_maps_onto_computer_spec_fields() {
        let info = SystemInformation {
            cpu: "  AMD Ryzen 7 8845HS  ".to_string(),
            name: "Windows".to_string(),
            os_version: "11".to_string(),
            total_memory: 32_768.0,
            motherboard_name: "GX5HRXG".to_string(),
            product_vendor: "SchenkerTechnologiesGmbH".to_string(),
            gpu_info: Gpu {
                card: vec![
                    GraphicsCard {
                        name: "GeForce RTX 4060 Laptop GPU".to_string(),
                        brand: "NVIDIA".to_string(),
                        ..Default::default()
                    },
                    GraphicsCard {
                        name: "AMD Radeon 780M".to_string(),
                        brand: "AMD".to_string(),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
            disks: vec![
                Disk {
                    mount_point: "C:\\".to_string(),
                    file_system: "NTFS".to_string(),
                    total_space: 2_000_000_000_000,
                    available_space: 500_000_000_000,
                    ..Default::default()
                },
                // Zero-capacity entries are the live feed's virtual mounts.
                Disk::default(),
            ],
            ..Default::default()
        };

        let specs = ComputerSpecs::from(&info);
        assert_eq!(specs.cpu, "AMD Ryzen 7 8845HS");
        assert_eq!(specs.operating_system, "Windows 11");
        assert_eq!(specs.ram, "33 Gb");
        assert_eq!(
            specs.gpu,
            "NVIDIA GeForce RTX 4060 Laptop GPU, AMD Radeon 780M"
        );
        assert_eq!(specs.motherboard_name, "GX5HRXG");
        assert_eq!(specs.drives.len(), 1);
        assert_eq!(specs.drives[0].drive_letter, "C:\\");
        assert_eq!(specs.drives[0].drive_type, "NTFS");
        assert_eq!(specs.drives[0].total_size, "1,862");
        assert_eq!(specs.drives[0].space_left, "465");
        assert!(!specs.is_empty());
    }

    #[test]
    fn populated_fields_lists_only_what_was_read() {
        let info = SystemInformation {
            cpu: "Intel Core i7-9750H".to_string(),
            ..Default::default()
        };
        assert_eq!(ComputerSpecs::from(&info).populated_fields(), vec!["cpu"]);
    }
}

#[cfg(test)]
mod deser_tests {
    use super::*;
    use crate::schema::{COMPUTER_TABLE, CUSTOMER_TABLE};
    use surrealdb::types::Value;

    /// The shape a spec-less `computer` row actually stores: only the fields
    /// carrying a schema DEFAULT, plus id/customer. Everything else is absent.
    fn spec_less_row() -> Value {
        let mut v = ComputerData {
            id: RecordId::new(COMPUTER_TABLE, "DESKTOP-TU0PGC9:2ad433d07"),
            customer: Some(RecordId::new(CUSTOMER_TABLE, "2")),
            hostname: "DESKTOP-TU0PGC9".to_string(),
            ..Default::default()
        }
        .into_value();
        let keep = [
            "id",
            "customer",
            "hostname",
            "operating_system",
            "cpu",
            "gpu",
            "ram",
            "drives",
        ];
        match &mut v {
            Value::Object(obj) => obj.retain(|k, _| keep.contains(&k.as_str())),
            other => panic!("ComputerData should serialize to an object, got {other:?}"),
        }
        v
    }

    #[test]
    fn spec_less_row_deserializes() {
        let parsed = ComputerData::from_value(spec_less_row())
            .expect("a computer row created without specs must deserialize");
        assert_eq!(parsed.hostname, "DESKTOP-TU0PGC9");
        assert_eq!(parsed.cpu, "");
        assert!(parsed.drives.is_empty());
        assert!(parsed.current_antivirus.is_empty());
        assert_eq!(parsed.motherboard_name, "");
        assert_eq!(parsed.product_serial, "");
        assert!(parsed.customer.is_some());
    }
}


