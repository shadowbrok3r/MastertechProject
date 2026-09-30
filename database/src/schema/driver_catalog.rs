//! Fleet driver catalog.
//!
//! The canonical driver and BIOS a board or GPU is supposed to be running,
//! ported from qc-app's local `swift_driver.sqlite` so every admin console
//! resolves the same targets. Lookup shape mirrors the SQLite one: SMBIOS
//! baseboard product -> [`CatalogBaseboard`] -> [`CatalogPackage`] (one driver
//! per part category) and [`CatalogBios`]; PCI device code ->
//! [`CatalogGpuDevice`] -> [`CatalogDriver`].
//!
//! [`build_driver_check`] compares a machine's installed inventory
//! ([`super::driver_intel::DriverRecord`], captured by the driverstore plugin)
//! against those targets.

use serde::{Deserialize, Serialize};

use crate::db;

use super::driver_intel::DriverRecord;
use super::{Datetime, RecordId, SurrealValue};

/// One installable driver file.
#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
pub struct CatalogDriver {
    pub id: RecordId,
    #[serde(default)]
    #[surreal(default)]
    pub file_name: String,
    #[serde(default)]
    pub url_download: Option<String>,
    #[serde(default)]
    pub argument_string: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub file_type: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub source_id: Option<i64>,
    pub imported_at: Datetime,
}

/// Latest BIOS file + release page for a board.
#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
pub struct CatalogBios {
    pub id: RecordId,
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    #[surreal(default)]
    pub url_webpage: String,
    #[serde(default)]
    pub url_download: Option<String>,
    #[serde(default)]
    pub source_id: Option<i64>,
    pub imported_at: Datetime,
}

/// The per-board driver set, one link per part category.
#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
pub struct CatalogPackage {
    pub id: RecordId,
    #[serde(default)]
    pub chipset: Option<RecordId>,
    #[serde(default)]
    pub me: Option<RecordId>,
    #[serde(default)]
    pub graphics: Option<RecordId>,
    #[serde(default)]
    pub audio: Option<RecordId>,
    #[serde(default)]
    pub lan: Option<RecordId>,
    #[serde(default)]
    pub bluetooth: Option<RecordId>,
    #[serde(default)]
    pub wifi: Option<RecordId>,
    #[serde(default)]
    pub raid: Option<RecordId>,
    #[serde(default)]
    pub control_center: Option<RecordId>,
    #[serde(default)]
    pub source_id: Option<i64>,
    pub imported_at: Datetime,
}

impl CatalogPackage {
    /// Every category link that is set, paired with its category name.
    fn links(&self) -> Vec<(&'static str, &RecordId)> {
        [
            ("chipset", self.chipset.as_ref()),
            ("me", self.me.as_ref()),
            ("graphics", self.graphics.as_ref()),
            ("audio", self.audio.as_ref()),
            ("lan", self.lan.as_ref()),
            ("bluetooth", self.bluetooth.as_ref()),
            ("wifi", self.wifi.as_ref()),
            ("raid", self.raid.as_ref()),
            ("control_center", self.control_center.as_ref()),
        ]
        .into_iter()
        .filter_map(|(name, link)| link.map(|l| (name, l)))
        .collect()
    }
}

/// PCI device code -> display driver.
#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
pub struct CatalogGpuDevice {
    pub id: RecordId,
    #[serde(default)]
    #[surreal(default)]
    pub code: String,
    #[serde(default)]
    #[surreal(default)]
    pub device_name: String,
    #[serde(default)]
    #[surreal(default)]
    pub vendor: String,
    #[serde(default)]
    pub driver: Option<RecordId>,
    #[serde(default)]
    pub source_id: Option<i64>,
    pub imported_at: Datetime,
}

/// Lookup root: SMBIOS baseboard product -> its package and BIOS.
#[derive(Debug, Clone, Serialize, Deserialize, SurrealValue)]
pub struct CatalogBaseboard {
    pub id: RecordId,
    #[serde(default)]
    #[surreal(default)]
    pub product: String,
    #[serde(default)]
    #[surreal(default)]
    pub manufacturer: String,
    #[serde(default)]
    pub package: Option<RecordId>,
    #[serde(default)]
    pub bios: Option<RecordId>,
    #[serde(default)]
    pub source_id: Option<i64>,
    pub imported_at: Datetime,
}

/// A catalog target driver flattened for display: install file + version.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct TargetDriver {
    pub file: String,
    pub version: Option<String>,
}

impl From<CatalogDriver> for TargetDriver {
    fn from(d: CatalogDriver) -> Self {
        Self { file: d.file_name, version: d.version }
    }
}

/// Catalog target driver per package category for a board.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PackageDrivers {
    pub chipset: Option<TargetDriver>,
    pub me: Option<TargetDriver>,
    pub graphics: Option<TargetDriver>,
    pub audio: Option<TargetDriver>,
    pub lan: Option<TargetDriver>,
    pub bluetooth: Option<TargetDriver>,
    pub wifi: Option<TargetDriver>,
    pub raid: Option<TargetDriver>,
    pub control_center: Option<TargetDriver>,
}

impl PackageDrivers {
    fn set(&mut self, category: &str, target: TargetDriver) {
        match category {
            "chipset" => self.chipset = Some(target),
            "me" => self.me = Some(target),
            "graphics" => self.graphics = Some(target),
            "audio" => self.audio = Some(target),
            "lan" => self.lan = Some(target),
            "bluetooth" => self.bluetooth = Some(target),
            "wifi" => self.wifi = Some(target),
            "raid" => self.raid = Some(target),
            "control_center" => self.control_center = Some(target),
            _ => {}
        }
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

// ============================================================
// Deterministic record ids — a re-import upserts in place
// ============================================================

/// Lowercases and replaces anything that would need quoting in a record key.
/// Empty when `raw` carries no alphanumerics, which callers must reject: a row
/// with a blank value in a UNIQUE index swallows every later blank write.
pub fn catalog_key(raw: &str) -> String {
    let cleaned: String = raw
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '_' { c } else { '-' })
        .collect();
    cleaned.trim_matches(|c: char| !c.is_ascii_alphanumeric()).to_string()
}

pub fn driver_record_id(file_name: &str) -> Option<RecordId> {
    let key = catalog_key(file_name);
    (!key.is_empty()).then(|| RecordId::new(super::CATALOG_DRIVER_TABLE, key))
}

pub fn baseboard_record_id(product: &str) -> Option<RecordId> {
    let key = catalog_key(product);
    (!key.is_empty()).then(|| RecordId::new(super::CATALOG_BASEBOARD_TABLE, key))
}

/// Canonical form of a PCI device code: trimmed and uppercased.
pub fn normalize_device_code(code: &str) -> String {
    code.trim().to_uppercase()
}

pub fn gpu_device_record_id(code: &str) -> Option<RecordId> {
    let key = catalog_key(code);
    (!key.is_empty()).then(|| RecordId::new(super::CATALOG_GPU_DEVICE_TABLE, key))
}

/// Packages and BIOS rows have no natural key, so they keep the source
/// catalog's row id.
pub fn package_record_id(source_id: i64) -> RecordId {
    RecordId::new(super::CATALOG_PACKAGE_TABLE, format!("pkg_{source_id}"))
}

pub fn bios_record_id(source_id: i64) -> RecordId {
    RecordId::new(super::CATALOG_BIOS_TABLE, format!("bios_{source_id}"))
}

impl CatalogBaseboard {
    /// Board row for an SMBIOS product string. Blank input never matches.
    pub async fn by_product(product: &str) -> anyhow::Result<Option<Self>> {
        let product = product.trim();
        if product.is_empty() {
            return Ok(None);
        }
        let rows: Vec<Self> = db()
            .query("SELECT * FROM catalog_baseboard WHERE product == $product LIMIT 1")
            .bind(("product", product.to_string()))
            .await?
            .take(0)?;
        Ok(rows.into_iter().next())
    }

    /// Board products in the catalog, for pickers and coverage checks.
    pub async fn products(limit: u32) -> anyhow::Result<Vec<String>> {
        let rows: Vec<String> = db()
            .query("SELECT VALUE product FROM catalog_baseboard ORDER BY product LIMIT $limit")
            .bind(("limit", limit as i64))
            .await?
            .take(0)?;
        Ok(rows)
    }
}

/// Every catalog target driver mapped to a board's package, by category.
/// `None` when the board is not in the catalog at all.
pub async fn package_drivers_for_baseboard(product: &str) -> anyhow::Result<Option<PackageDrivers>> {
    let Some(board) = CatalogBaseboard::by_product(product).await? else {
        return Ok(None);
    };
    let Some(package_id) = board.package else {
        return Ok(Some(PackageDrivers::default()));
    };
    let Some(package): Option<CatalogPackage> = db().select(package_id).await? else {
        return Ok(Some(PackageDrivers::default()));
    };

    let links = package.links();
    if links.is_empty() {
        return Ok(Some(PackageDrivers::default()));
    }
    let ids: Vec<RecordId> = links.iter().map(|(_, id)| (*id).clone()).collect();
    let drivers: Vec<CatalogDriver> = db()
        .query("SELECT * FROM catalog_driver WHERE id IN $ids")
        .bind(("ids", ids))
        .await?
        .take(0)?;

    let mut out = PackageDrivers::default();
    for (category, link) in links {
        if let Some(driver) = drivers.iter().find(|d| &d.id == link) {
            out.set(category, driver.clone().into());
        }
    }
    Ok(Some(out))
}

/// Display driver for a PCI device code. Codes are hex, so both sides are
/// normalized to uppercase — a case difference between the catalog and the
/// machine's reading would otherwise just miss.
pub async fn gpu_driver_for_device(device_code: &str) -> anyhow::Result<Option<TargetDriver>> {
    let code = normalize_device_code(device_code);
    if code.is_empty() {
        return Ok(None);
    }
    let rows: Vec<CatalogGpuDevice> = db()
        .query("SELECT * FROM catalog_gpu_device WHERE code == $code LIMIT 1")
        .bind(("code", code))
        .await?
        .take(0)?;
    let Some(driver_id) = rows.into_iter().next().and_then(|d| d.driver) else {
        return Ok(None);
    };
    let driver: Option<CatalogDriver> = db().select(driver_id).await?;
    Ok(driver.map(Into::into))
}

/// Catalog BIOS entry for a board.
pub async fn bios_for_baseboard(product: &str) -> anyhow::Result<Option<CatalogBios>> {
    let Some(board) = CatalogBaseboard::by_product(product).await? else {
        return Ok(None);
    };
    let Some(bios_id) = board.bios else {
        return Ok(None);
    };
    Ok(db().select(bios_id).await?)
}

// ============================================================
// Installed-vs-catalog comparison
// ============================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriverStatus {
    Installed,
    Outdated,
    Missing,
    NoTarget,
}

/// One row of the side-by-side driver comparison for a machine's parts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriverCheckRow {
    pub category: String,
    /// Catalog target file (the canonical driver to install), if mapped.
    pub target_file: Option<String>,
    pub target_version: Option<String>,
    pub installed_name: Option<String>,
    pub installed_version: Option<String>,
    pub installed_date: Option<String>,
    pub status: DriverStatus,
}

/// Setup class (`pnputil` "Class Name", `Win32_PnPSignedDriver.DeviceClass`)
/// for a part category.
fn class_for(category: &str) -> Option<&'static str> {
    match category {
        "GPU" => Some("Display"),
        "Audio" => Some("Media"),
        "LAN" | "WiFi" => Some("Net"),
        "Bluetooth" => Some("Bluetooth"),
        "RAID" => Some("HDC"),
        "Chipset" | "Management Engine" => Some("System"),
        _ => None,
    }
}

/// Device-name keywords for the categories a class alone cannot separate,
/// used when a snapshot carries no class names (the WMI payload path).
fn name_hints(category: &str) -> &'static [&'static str] {
    match category {
        "GPU" => &["geforce", "radeon", "nvidia", "intel(r) arc", "graphics"],
        "Audio" => &["audio", "realtek", "sound"],
        "LAN" => &["ethernet", "gbe", "gigabit"],
        "WiFi" => &["wi-fi", "wifi", "wireless", "802.11"],
        "Bluetooth" => &["bluetooth"],
        "RAID" => &["raid", "sata", "ahci"],
        "Chipset" => &["chipset"],
        "Management Engine" => &["management engine"],
        _ => &[],
    }
}

fn is_wireless(name: &str) -> bool {
    let l = name.to_lowercase();
    l.contains("wi-fi") || l.contains("wifi") || l.contains("wireless") || l.contains("802.11")
}

/// Text a record can be matched on by name.
fn label(d: &DriverRecord) -> String {
    match d.device_name.as_deref() {
        Some(n) if !n.trim().is_empty() => n.to_string(),
        _ => d.original_name.clone(),
    }
}

/// Best installed-driver match for a category by setup class + a name hint.
/// Falls back to name-only matching for inventories with no class names.
fn match_installed<'a>(category: &str, installed: &'a [DriverRecord]) -> Option<&'a DriverRecord> {
    let Some(class) = class_for(category) else {
        return None;
    };
    let in_class: Vec<&DriverRecord> = installed
        .iter()
        .filter(|d| d.class_name.eq_ignore_ascii_case(class))
        .collect();

    if in_class.is_empty() {
        // No class data (or no device in that class): fall back to the name.
        let hints = name_hints(category);
        return installed.iter().find(|d| {
            let l = label(d).to_lowercase();
            hints.iter().any(|h| l.contains(h))
        });
    }

    let pick = |f: &dyn Fn(&&DriverRecord) -> bool| in_class.iter().copied().find(|d| f(d));
    match category {
        "WiFi" => pick(&|d| is_wireless(&label(d))),
        "LAN" => pick(&|d| !is_wireless(&label(d))),
        "Management Engine" => pick(&|d| label(d).to_lowercase().contains("management engine")),
        // Chipset/RAID/etc.: prefer a real vendor driver over a Microsoft inbox one.
        _ => pick(&|d| !d.provider.to_lowercase().contains("microsoft"))
            .or_else(|| in_class.first().copied()),
    }
}

/// Build the comparison rows. A category is included only when the catalog
/// expects a driver for it OR the machine has a matching device.
pub fn build_driver_check(
    installed: &[DriverRecord],
    package: &PackageDrivers,
    gpu_targets: &[TargetDriver],
) -> Vec<DriverCheckRow> {
    let gpu_target = gpu_targets.first().cloned().or_else(|| package.graphics.clone());
    let categories = [
        ("Chipset", package.chipset.clone()),
        ("Management Engine", package.me.clone()),
        ("GPU", gpu_target),
        ("Audio", package.audio.clone()),
        ("LAN", package.lan.clone()),
        ("WiFi", package.wifi.clone()),
        ("Bluetooth", package.bluetooth.clone()),
        ("RAID", package.raid.clone()),
    ];

    let mut rows = Vec::new();
    for (category, target) in categories {
        let found = match_installed(category, installed);
        if target.is_none() && found.is_none() {
            continue;
        }
        let target_file = target.as_ref().map(|t| t.file.clone());
        let target_version = target.as_ref().and_then(|t| t.version.clone());
        let installed_version = found
            .map(|d| d.driver_version.clone())
            .filter(|v| !v.is_empty());
        let status = if found.is_some() {
            match (installed_version.as_deref(), target_version.as_deref()) {
                (Some(iv), Some(tv)) if version_lt(iv, tv) => DriverStatus::Outdated,
                _ => DriverStatus::Installed,
            }
        } else if target_file.is_some() {
            DriverStatus::Missing
        } else {
            DriverStatus::NoTarget
        };
        rows.push(DriverCheckRow {
            category: category.to_string(),
            target_file,
            target_version,
            installed_name: found.map(label),
            installed_version,
            installed_date: found.map(|d| d.driver_date.clone()).filter(|v| !v.is_empty()),
            status,
        });
    }

    // Control Center is an app, not a PnP driver: catalog target only.
    if let Some(cc) = &package.control_center {
        rows.push(DriverCheckRow {
            category: "Control Center".to_string(),
            target_file: Some(cc.file.clone()),
            target_version: cc.version.clone(),
            installed_name: None,
            installed_version: None,
            installed_date: None,
            status: DriverStatus::NoTarget,
        });
    }
    rows
}

/// Dotted-numeric version compare (`31.0.15.5176` vs `31.0.101.5186`); non-digit
/// separators are ignored and missing trailing components count as 0.
fn version_cmp(a: &str, b: &str) -> core::cmp::Ordering {
    let parse = |s: &str| -> Vec<u64> {
        s.split(|c: char| !c.is_ascii_digit())
            .filter(|p| !p.is_empty())
            .filter_map(|p| p.parse::<u64>().ok())
            .collect()
    };
    let (av, bv) = (parse(a), parse(b));
    for i in 0..av.len().max(bv.len()) {
        let x = av.get(i).copied().unwrap_or(0);
        let y = bv.get(i).copied().unwrap_or(0);
        match x.cmp(&y) {
            core::cmp::Ordering::Equal => continue,
            other => return other,
        }
    }
    core::cmp::Ordering::Equal
}

fn version_lt(a: &str, b: &str) -> bool {
    version_cmp(a, b) == core::cmp::Ordering::Less
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drv(class: &str, name: &str, ver: &str, provider: &str) -> DriverRecord {
        DriverRecord {
            class_name: class.into(),
            device_name: Some(name.into()),
            original_name: "oem0.inf".into(),
            driver_version: ver.into(),
            driver_date: "2026-01-01".into(),
            provider: provider.into(),
            ..Default::default()
        }
    }

    fn tgt(file: &str) -> TargetDriver {
        TargetDriver { file: file.into(), version: None }
    }
    fn tgt_v(file: &str, v: &str) -> TargetDriver {
        TargetDriver { file: file.into(), version: Some(v.into()) }
    }

    #[test]
    fn matches_gpu_and_flags_missing_chipset() {
        let installed = vec![
            drv("Display", "NVIDIA GeForce RTX 5090", "576.80", "NVIDIA"),
            drv("Net", "Intel Wi-Fi 6E AX211", "22.0", "Intel"),
        ];
        let package = PackageDrivers {
            chipset: Some(tgt("amd_chipset.exe")),
            audio: Some(tgt("realtek.exe")),
            wifi: Some(tgt("intel_wifi.exe")),
            ..Default::default()
        };
        let rows = build_driver_check(&installed, &package, &[tgt("nvidia_dch.exe")]);

        let gpu = rows.iter().find(|r| r.category == "GPU").unwrap();
        assert_eq!(gpu.status, DriverStatus::Installed);
        assert_eq!(gpu.installed_version.as_deref(), Some("576.80"));
        assert_eq!(gpu.target_file.as_deref(), Some("nvidia_dch.exe"));

        let wifi = rows.iter().find(|r| r.category == "WiFi").unwrap();
        assert_eq!(wifi.status, DriverStatus::Installed);

        let chipset = rows.iter().find(|r| r.category == "Chipset").unwrap();
        assert_eq!(chipset.status, DriverStatus::Missing);
        let audio = rows.iter().find(|r| r.category == "Audio").unwrap();
        assert_eq!(audio.status, DriverStatus::Missing);

        // No LAN target and no wired NIC installed -> row omitted.
        assert!(rows.iter().all(|r| r.category != "LAN"));
    }

    #[test]
    fn flags_outdated_when_installed_older_than_target() {
        let installed = vec![drv("Display", "NVIDIA GeForce RTX 5090", "576.80", "NVIDIA")];
        let rows =
            build_driver_check(&installed, &PackageDrivers::default(), &[tgt_v("nvidia_dch.exe", "576.88")]);
        assert_eq!(rows.iter().find(|r| r.category == "GPU").unwrap().status, DriverStatus::Outdated);

        let newer = vec![drv("Display", "NVIDIA GeForce RTX 5090", "576.88", "NVIDIA")];
        let rows2 =
            build_driver_check(&newer, &PackageDrivers::default(), &[tgt_v("nvidia_dch.exe", "576.88")]);
        assert_eq!(rows2.iter().find(|r| r.category == "GPU").unwrap().status, DriverStatus::Installed);
    }

    /// A WMI-sourced inventory carries no class names; matching falls back to
    /// the device name so the rows are still populated.
    #[test]
    fn matches_by_name_when_class_is_absent() {
        let installed = vec![
            drv("", "NVIDIA GeForce RTX 5090", "576.80", "NVIDIA"),
            drv("", "Realtek High Definition Audio", "6.0.9", "Realtek"),
        ];
        let package = PackageDrivers {
            audio: Some(tgt("realtek.exe")),
            ..Default::default()
        };
        let rows = build_driver_check(&installed, &package, &[tgt("nvidia_dch.exe")]);
        assert_eq!(rows.iter().find(|r| r.category == "GPU").unwrap().status, DriverStatus::Installed);
        assert_eq!(rows.iter().find(|r| r.category == "Audio").unwrap().status, DriverStatus::Installed);
    }

    #[test]
    fn prefers_vendor_driver_over_microsoft_inbox() {
        let installed = vec![
            drv("System", "Microsoft Basic Chipset", "10.0", "Microsoft"),
            drv("System", "AMD Chipset Device", "6.11.22", "AMD"),
        ];
        let package = PackageDrivers { chipset: Some(tgt("amd_chipset.exe")), ..Default::default() };
        let rows = build_driver_check(&installed, &package, &[]);
        let chipset = rows.iter().find(|r| r.category == "Chipset").unwrap();
        assert_eq!(chipset.installed_name.as_deref(), Some("AMD Chipset Device"));
    }

    #[test]
    fn version_cmp_numeric_components() {
        use core::cmp::Ordering;
        assert_eq!(version_cmp("31.0.15.5176", "31.0.101.5186"), Ordering::Less);
        assert_eq!(version_cmp("576.88", "576.80"), Ordering::Greater);
        assert_eq!(version_cmp("1.2.3", "1.2.3"), Ordering::Equal);
    }

    #[test]
    fn missing_count() {
        let package = PackageDrivers {
            chipset: Some(tgt("c.exe")),
            audio: Some(tgt("a.exe")),
            ..Default::default()
        };
        let rows = build_driver_check(&[], &package, &[]);
        let missing = rows.iter().filter(|r| r.status == DriverStatus::Missing).count();
        assert_eq!(missing, 2);
    }
}
