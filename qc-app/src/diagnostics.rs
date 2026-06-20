//! Read-only Windows diagnostics surfaced over MCP: system identity, firmware
//! security posture, and per-disk storage health. WMI/registry-backed; non-Windows
//! builds return empty/default. WMI reads use explicit `FROM <Class>` queries.

use serde::Serialize;

/// Stable identity + firmware/licensing identifiers for this machine.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SystemIdentity {
    pub machine_id: String,
    pub system_serial: Option<String>,
    pub board_serial: Option<String>,
    pub baseboard_product: Option<String>,
    pub bios_version: Option<String>,
    pub oa3_product_key: Option<String>,
    pub gpu_device_codes: Vec<String>,
}

/// Identity assembled from the WMI/SMBIOS readers in `hardware_id`.
pub fn system_identity() -> SystemIdentity {
    let serials = crate::hardware_id::read_machine_serials();
    SystemIdentity {
        machine_id: crate::reporting::machine_id(),
        system_serial: serials.first().cloned(),
        board_serial: serials.get(1).cloned(),
        baseboard_product: crate::hardware_id::read_baseboard_product(),
        bios_version: crate::hardware_id::read_bios_version(),
        oa3_product_key: crate::hardware_id::read_oa3_product_key(),
        gpu_device_codes: crate::hardware_id::read_gpu_device_codes(),
    }
}

/// Firmware + licensing posture relevant to QC sign-off.
#[derive(Debug, Clone, Default, Serialize)]
pub struct FirmwareSecurity {
    pub boot_mode: String,
    pub secure_boot_enabled: Option<bool>,
    pub tpm_present: bool,
    pub tpm_enabled: Option<bool>,
    pub tpm_spec_version: Option<String>,
    pub tpm_manufacturer: Option<String>,
    pub windows_activated: Option<bool>,
}

/// One physical disk's health from the Storage Management WMI provider.
#[derive(Debug, Clone, Default, Serialize)]
pub struct StorageDisk {
    pub friendly_name: Option<String>,
    pub serial_number: Option<String>,
    pub size_bytes: Option<u64>,
    pub media_type: String,
    pub bus_type: String,
    pub health: String,
}

#[cfg(windows)]
mod imp {
    use super::{FirmwareSecurity, StorageDisk};
    use serde::Deserialize;

    pub fn firmware_security() -> FirmwareSecurity {
        let (boot_mode, secure_boot_enabled) = read_secure_boot();
        let mut fs = FirmwareSecurity { boot_mode, secure_boot_enabled, ..Default::default() };
        read_tpm(&mut fs);
        fs.windows_activated = read_windows_activated();
        fs
    }

    /// "UEFI" + Secure Boot flag when the SecureBoot state key exists; else Legacy/Unknown.
    fn read_secure_boot() -> (String, Option<bool>) {
        use std::process::Command;
        let out = Command::new("reg")
            .args([
                "query",
                r"HKLM\SYSTEM\CurrentControlSet\Control\SecureBoot\State",
                "/v",
                "UEFISecureBootEnabled",
            ])
            .output();
        match out {
            Ok(o) if o.status.success() => {
                let text = String::from_utf8_lossy(&o.stdout);
                let enabled = match text.split_whitespace().last().unwrap_or("") {
                    "0x1" => Some(true),
                    "0x0" => Some(false),
                    _ => None,
                };
                ("UEFI".to_string(), enabled)
            }
            _ => ("Legacy BIOS or unknown".to_string(), None),
        }
    }

    fn read_tpm(fs: &mut FirmwareSecurity) {
        #[derive(Deserialize)]
        struct Tpm {
            #[serde(rename = "IsEnabled_InitialValue")]
            is_enabled: Option<bool>,
            #[serde(rename = "ManufacturerIdTxt")]
            manufacturer: Option<String>,
            #[serde(rename = "SpecVersion")]
            spec_version: Option<String>,
        }
        let Ok(wmi) = wmi::WMIConnection::with_namespace_path("ROOT\\CIMV2\\Security\\MicrosoftTpm")
        else {
            return;
        };
        let rows: Vec<Tpm> = wmi
            .raw_query("SELECT IsEnabled_InitialValue, ManufacturerIdTxt, SpecVersion FROM Win32_Tpm")
            .unwrap_or_default();
        if let Some(t) = rows.into_iter().next() {
            fs.tpm_present = true;
            fs.tpm_enabled = t.is_enabled;
            fs.tpm_manufacturer = t.manufacturer.map(|s| s.trim().to_string());
            fs.tpm_spec_version = t.spec_version.map(|s| s.trim().to_string());
        }
    }

    /// True when any Windows licensing product reports LicenseStatus 1 (Licensed).
    fn read_windows_activated() -> Option<bool> {
        #[derive(Deserialize)]
        struct Product {
            #[serde(rename = "Name")]
            name: Option<String>,
            #[serde(rename = "LicenseStatus")]
            license_status: Option<u32>,
            #[serde(rename = "PartialProductKey")]
            partial_product_key: Option<String>,
        }
        let wmi = wmi::WMIConnection::with_namespace_path("ROOT\\CIMV2").ok()?;
        let rows: Vec<Product> = wmi
            .raw_query("SELECT Name, LicenseStatus, PartialProductKey FROM SoftwareLicensingProduct")
            .ok()?;
        let mut seen = false;
        for p in rows {
            let is_windows = p.name.as_deref().map(|n| n.contains("Windows")).unwrap_or(false);
            let has_key = p.partial_product_key.as_ref().map(|k| !k.is_empty()).unwrap_or(false);
            if is_windows && has_key {
                seen = true;
                if p.license_status == Some(1) {
                    return Some(true);
                }
            }
        }
        seen.then_some(false)
    }

    pub fn storage_health() -> Vec<StorageDisk> {
        #[derive(Deserialize)]
        struct PhysicalDisk {
            #[serde(rename = "FriendlyName")]
            friendly_name: Option<String>,
            #[serde(rename = "SerialNumber")]
            serial_number: Option<String>,
            #[serde(rename = "Size")]
            size: Option<u64>,
            #[serde(rename = "MediaType")]
            media_type: Option<u16>,
            #[serde(rename = "BusType")]
            bus_type: Option<u16>,
            #[serde(rename = "HealthStatus")]
            health_status: Option<u16>,
        }
        let Ok(wmi) = wmi::WMIConnection::with_namespace_path("ROOT\\Microsoft\\Windows\\Storage")
        else {
            return Vec::new();
        };
        let rows: Vec<PhysicalDisk> = wmi
            .raw_query("SELECT FriendlyName, SerialNumber, Size, MediaType, BusType, HealthStatus FROM MSFT_PhysicalDisk")
            .unwrap_or_default();
        rows.into_iter()
            .map(|d| StorageDisk {
                friendly_name: d.friendly_name,
                serial_number: d.serial_number.map(|s| s.trim().to_string()),
                size_bytes: d.size,
                media_type: media_type_label(d.media_type),
                bus_type: bus_type_label(d.bus_type),
                health: health_label(d.health_status),
            })
            .collect()
    }

    fn media_type_label(v: Option<u16>) -> String {
        match v {
            Some(3) => "HDD",
            Some(4) => "SSD",
            Some(5) => "SCM",
            Some(0) => "Unspecified",
            _ => "Unknown",
        }
        .to_string()
    }

    fn bus_type_label(v: Option<u16>) -> String {
        match v {
            Some(1) => "SCSI",
            Some(3) => "ATA",
            Some(4) => "1394",
            Some(7) => "USB",
            Some(8) => "RAID",
            Some(9) => "iSCSI",
            Some(10) => "SAS",
            Some(11) => "SATA",
            Some(17) => "NVMe",
            _ => "Unknown",
        }
        .to_string()
    }

    fn health_label(v: Option<u16>) -> String {
        match v {
            Some(0) => "Healthy",
            Some(1) => "Warning",
            Some(2) => "Unhealthy",
            _ => "Unknown",
        }
        .to_string()
    }
}

#[cfg(windows)]
pub use imp::{firmware_security, storage_health};

#[cfg(not(windows))]
pub fn firmware_security() -> FirmwareSecurity {
    FirmwareSecurity { boot_mode: "Unknown".into(), ..Default::default() }
}

#[cfg(not(windows))]
pub fn storage_health() -> Vec<StorageDisk> {
    Vec::new()
}
