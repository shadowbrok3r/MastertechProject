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

/// One installed device driver from `Win32_PnPSignedDriver`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct InstalledDriver {
    pub class: String,
    pub device_name: String,
    pub version: String,
    pub date: String,
    pub provider: String,
    pub manufacturer: String,
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
    use super::{FirmwareSecurity, InstalledDriver, StorageDisk};
    use serde::Deserialize;
    use windows::Win32::Devices::DeviceAndDriverInstallation::{HDEVINFO, SP_DEVINFO_DATA};
    use windows::Win32::Foundation::DEVPROPKEY;

    /// Installed drivers, preferring SetupAPI (Device Manager's source); falls back
    /// to WMI `Win32_PnPSignedDriver` if SetupAPI yields nothing.
    pub fn installed_drivers() -> Vec<InstalledDriver> {
        let native = installed_drivers_native();
        if native.is_empty() {
            installed_drivers_wmi()
        } else {
            native
        }
    }

    /// Enumerate present devices via SetupAPI and read driver version/provider/date.
    fn installed_drivers_native() -> Vec<InstalledDriver> {
        use windows::Win32::Devices::DeviceAndDriverInstallation::{
            DIGCF_ALLCLASSES, DIGCF_PRESENT, SETUP_DI_GET_CLASS_DEVS_FLAGS, SetupDiDestroyDeviceInfoList,
            SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
        };
        use windows::Win32::Devices::Properties::{
            DEVPKEY_Device_Class, DEVPKEY_Device_DeviceDesc, DEVPKEY_Device_DriverDate,
            DEVPKEY_Device_DriverProvider, DEVPKEY_Device_DriverVersion, DEVPKEY_Device_Manufacturer,
            DEVPKEY_NAME,
        };
        use windows::core::PCWSTR;

        let flags = SETUP_DI_GET_CLASS_DEVS_FLAGS(DIGCF_PRESENT.0 | DIGCF_ALLCLASSES.0);
        let set = match unsafe { SetupDiGetClassDevsW(None, PCWSTR::null(), None, flags) } {
            Ok(h) => h,
            Err(_) => return Vec::new(),
        };

        let mut out = Vec::new();
        let mut index = 0u32;
        loop {
            let mut data = SP_DEVINFO_DATA {
                cbSize: core::mem::size_of::<SP_DEVINFO_DATA>() as u32,
                ..Default::default()
            };
            if unsafe { SetupDiEnumDeviceInfo(set, index, &mut data) }.is_err() {
                break;
            }
            index += 1;

            let class = dev_prop_string(set, &data, &DEVPKEY_Device_Class).unwrap_or_default();
            if !relevant_class(&class) {
                continue;
            }
            let name = dev_prop_string(set, &data, &DEVPKEY_NAME)
                .or_else(|| dev_prop_string(set, &data, &DEVPKEY_Device_DeviceDesc))
                .unwrap_or_default();
            if name.trim().is_empty() {
                continue;
            }
            out.push(InstalledDriver {
                class,
                device_name: name,
                version: dev_prop_string(set, &data, &DEVPKEY_Device_DriverVersion).unwrap_or_default(),
                date: dev_prop_filetime(set, &data, &DEVPKEY_Device_DriverDate),
                provider: dev_prop_string(set, &data, &DEVPKEY_Device_DriverProvider).unwrap_or_default(),
                manufacturer: dev_prop_string(set, &data, &DEVPKEY_Device_Manufacturer).unwrap_or_default(),
            });
        }
        let _ = unsafe { SetupDiDestroyDeviceInfoList(set) };
        out
    }

    fn relevant_class(class: &str) -> bool {
        matches!(
            class.to_ascii_lowercase().as_str(),
            "display" | "net" | "media" | "bluetooth" | "system" | "hdc" | "scsiadapter"
        )
    }

    /// Read a string device property (size probe, then fetch).
    fn dev_prop_string(set: HDEVINFO, data: &SP_DEVINFO_DATA, key: &DEVPROPKEY) -> Option<String> {
        use windows::Win32::Devices::DeviceAndDriverInstallation::SetupDiGetDevicePropertyW;
        use windows::Win32::Devices::Properties::{DEVPROP_TYPE_STRING, DEVPROPTYPE};
        let mut ptype = DEVPROPTYPE(0);
        let mut required: u32 = 0;
        let _ = unsafe {
            SetupDiGetDevicePropertyW(set, data, key, &mut ptype, None, Some(&mut required as *mut u32), 0)
        };
        if required == 0 || ptype != DEVPROP_TYPE_STRING {
            return None;
        }
        let mut buf = vec![0u8; required as usize];
        unsafe {
            SetupDiGetDevicePropertyW(set, data, key, &mut ptype, Some(buf.as_mut_slice()), None, 0).ok()?
        };
        let u16s: Vec<u16> = buf.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        let s = String::from_utf16_lossy(&u16s);
        let s = s.trim_end_matches('\0').trim().to_string();
        (!s.is_empty()).then_some(s)
    }

    /// Read a FILETIME device property as a `YYYY-MM-DD` string.
    fn dev_prop_filetime(set: HDEVINFO, data: &SP_DEVINFO_DATA, key: &DEVPROPKEY) -> String {
        use windows::Win32::Devices::DeviceAndDriverInstallation::SetupDiGetDevicePropertyW;
        use windows::Win32::Devices::Properties::{DEVPROP_TYPE_FILETIME, DEVPROPTYPE};
        let mut ptype = DEVPROPTYPE(0);
        let mut buf = [0u8; 8];
        if unsafe {
            SetupDiGetDevicePropertyW(set, data, key, &mut ptype, Some(buf.as_mut_slice()), None, 0)
        }
        .is_err()
            || ptype != DEVPROP_TYPE_FILETIME
        {
            return String::new();
        }
        let low = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
        let high = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
        filetime_to_date(low, high)
    }

    /// 100ns-since-1601 FILETIME → `YYYY-MM-DD` (Howard Hinnant civil-from-days).
    fn filetime_to_date(low: u32, high: u32) -> String {
        let ft = (((high as u64) << 32) | low as u64) as i64;
        if ft <= 0 {
            return String::new();
        }
        let unix = (ft - 116_444_736_000_000_000) / 10_000_000;
        if unix < 0 {
            return String::new();
        }
        let z = unix.div_euclid(86_400) + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = if m <= 2 { y + 1 } else { y };
        format!("{y:04}-{m:02}-{d:02}")
    }

    /// Installed drivers in the QC-relevant device classes (Win32_PnPSignedDriver).
    fn installed_drivers_wmi() -> Vec<InstalledDriver> {
        #[derive(Deserialize)]
        struct Row {
            #[serde(rename = "DeviceClass")]
            device_class: Option<String>,
            #[serde(rename = "DeviceName")]
            device_name: Option<String>,
            #[serde(rename = "DriverVersion")]
            driver_version: Option<String>,
            #[serde(rename = "DriverDate")]
            driver_date: Option<String>,
            #[serde(rename = "DriverProviderName")]
            driver_provider_name: Option<String>,
            #[serde(rename = "Manufacturer")]
            manufacturer: Option<String>,
        }
        let Ok(wmi) = wmi::WMIConnection::with_namespace_path("ROOT\\CIMV2") else {
            return Vec::new();
        };
        let rows: Vec<Row> = wmi
            .raw_query(
                "SELECT DeviceClass, DeviceName, DriverVersion, DriverDate, DriverProviderName, Manufacturer \
                 FROM Win32_PnPSignedDriver WHERE DeviceClass='DISPLAY' OR DeviceClass='NET' \
                 OR DeviceClass='MEDIA' OR DeviceClass='BLUETOOTH' OR DeviceClass='SYSTEM' OR DeviceClass='HDC'",
            )
            .unwrap_or_default();
        rows.into_iter()
            .filter_map(|r| {
                let name = r.device_name.unwrap_or_default();
                if name.trim().is_empty() {
                    return None;
                }
                Some(InstalledDriver {
                    class: r.device_class.unwrap_or_default(),
                    device_name: name,
                    version: r.driver_version.unwrap_or_default(),
                    date: fmt_cim_date(r.driver_date.as_deref()),
                    provider: r.driver_provider_name.unwrap_or_default(),
                    manufacturer: r.manufacturer.unwrap_or_default(),
                })
            })
            .collect()
    }

    /// CIM datetime ("YYYYMMDDhhmmss.……") → "YYYY-MM-DD".
    fn fmt_cim_date(d: Option<&str>) -> String {
        match d {
            Some(s) if s.len() >= 8 => format!("{}-{}-{}", &s[0..4], &s[4..6], &s[6..8]),
            _ => String::new(),
        }
    }

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
pub use imp::{firmware_security, installed_drivers, storage_health};

#[cfg(not(windows))]
pub fn firmware_security() -> FirmwareSecurity {
    FirmwareSecurity { boot_mode: "Unknown".into(), ..Default::default() }
}

#[cfg(not(windows))]
pub fn storage_health() -> Vec<StorageDisk> {
    Vec::new()
}

#[cfg(not(windows))]
pub fn installed_drivers() -> Vec<InstalledDriver> {
    Vec::new()
}
