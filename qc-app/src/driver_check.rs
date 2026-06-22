//! Per-part driver comparison: the catalog's expected driver for each part
//! (chipset, GPU, audio, LAN, …) vs what's installed on the machine (Win32 PnP),
//! plus the set of expected drivers that are missing. Pure logic — testable and
//! platform-agnostic; WMI/SQLite gathering lives in `diagnostics`/`catalog_query`.

use serde::Serialize;

use crate::diagnostics::InstalledDriver;
use crate::provisioning::catalog_query::{PackageDrivers, TargetDriver};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DriverStatus {
    Installed,
    Outdated,
    Missing,
    NoTarget,
}

/// One row of the side-by-side driver comparison for an order's parts.
#[derive(Debug, Clone, Serialize)]
pub struct DriverCheckRow {
    pub category: String,
    /// Catalog target file (the canonical/"latest" driver to install), if mapped.
    pub target_file: Option<String>,
    pub target_version: Option<String>,
    pub installed_name: Option<String>,
    pub installed_version: Option<String>,
    pub installed_date: Option<String>,
    pub status: DriverStatus,
}

/// Win32 device class (`Win32_PnPSignedDriver.DeviceClass`) for a part category.
fn class_for(category: &str) -> Option<&'static str> {
    match category {
        "GPU" => Some("DISPLAY"),
        "Audio" => Some("MEDIA"),
        "LAN" | "WiFi" => Some("NET"),
        "Bluetooth" => Some("BLUETOOTH"),
        "RAID" => Some("HDC"),
        "Chipset" | "Management Engine" => Some("SYSTEM"),
        _ => None,
    }
}

fn is_wireless(name: &str) -> bool {
    let l = name.to_lowercase();
    l.contains("wi-fi") || l.contains("wifi") || l.contains("wireless") || l.contains("802.11")
}

/// Best installed-driver match for a category by device class + a name/provider hint.
fn match_installed<'a>(category: &str, installed: &'a [InstalledDriver]) -> Option<&'a InstalledDriver> {
    let class = class_for(category)?;
    let in_class = || installed.iter().filter(|d| d.class.eq_ignore_ascii_case(class));
    match category {
        "WiFi" => in_class().find(|d| is_wireless(&d.device_name)),
        "LAN" => in_class().find(|d| !is_wireless(&d.device_name)),
        "Management Engine" => {
            in_class().find(|d| d.device_name.to_lowercase().contains("management engine"))
        }
        // Chipset/RAID/etc.: prefer a real vendor driver over a Microsoft inbox one.
        _ => in_class()
            .find(|d| !d.provider.to_lowercase().contains("microsoft"))
            .or_else(|| in_class().next()),
    }
}

/// Build the comparison rows. A category is included only when the catalog
/// expects a driver for it OR the machine has a matching device.
pub fn build_driver_check(
    installed: &[InstalledDriver],
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
    for (cat, target) in categories {
        let found = match_installed(cat, installed);
        if target.is_none() && found.is_none() {
            continue;
        }
        let target_file = target.as_ref().map(|t| t.file.clone());
        let target_version = target.as_ref().and_then(|t| t.version.clone());
        let installed_version = found.map(|d| d.version.clone()).filter(|v| !v.is_empty());
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
            category: cat.to_string(),
            target_file,
            target_version,
            installed_name: found.map(|d| d.device_name.clone()),
            installed_version,
            installed_date: found.map(|d| d.date.clone()).filter(|v| !v.is_empty()),
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

    fn drv(class: &str, name: &str, ver: &str, provider: &str) -> InstalledDriver {
        InstalledDriver {
            class: class.into(),
            device_name: name.into(),
            version: ver.into(),
            date: "2026-01-01".into(),
            provider: provider.into(),
            manufacturer: provider.into(),
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
            drv("DISPLAY", "NVIDIA GeForce RTX 5090", "576.80", "NVIDIA"),
            drv("NET", "Intel Wi-Fi 6E AX211", "22.0", "Intel"),
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

        // No LAN target and no wired NIC installed → row omitted.
        assert!(rows.iter().all(|r| r.category != "LAN"));
    }

    #[test]
    fn flags_outdated_when_installed_older_than_target() {
        let installed = vec![drv("DISPLAY", "NVIDIA GeForce RTX 5090", "576.80", "NVIDIA")];
        let rows = build_driver_check(&installed, &PackageDrivers::default(), &[tgt_v("nvidia_dch.exe", "576.88")]);
        assert_eq!(rows.iter().find(|r| r.category == "GPU").unwrap().status, DriverStatus::Outdated);

        let newer = vec![drv("DISPLAY", "NVIDIA GeForce RTX 5090", "576.88", "NVIDIA")];
        let rows2 = build_driver_check(&newer, &PackageDrivers::default(), &[tgt_v("nvidia_dch.exe", "576.88")]);
        assert_eq!(rows2.iter().find(|r| r.category == "GPU").unwrap().status, DriverStatus::Installed);
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
