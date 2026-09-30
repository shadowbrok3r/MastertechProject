//! Firmware, licensing and SMBIOS identity for the admin console's QC page.
//!
//! Ported from qc-app's `diagnostics::{SystemIdentity, FirmwareSecurity}` and
//! `hardware_id`. Answers [`displays::Cmd::GatherQcFirmware`]. A reading that
//! fails stays `None` rather than reporting a wrong value.

use database::orders::is_placeholder_serial;
use displays::QcFirmware;

#[cfg(target_os = "windows")]
pub fn gather() -> QcFirmware {
    use serde::Deserialize;

    #[derive(Deserialize)]
    #[serde(rename = "Win32_Bios", rename_all = "PascalCase")]
    struct Bios {
        #[serde(rename = "SMBIOSBIOSVersion")]
        smbios_bios_version: Option<String>,
        manufacturer: Option<String>,
        release_date: Option<String>,
        serial_number: Option<String>,
    }
    #[derive(Deserialize)]
    #[serde(rename = "Win32_BaseBoard", rename_all = "PascalCase")]
    struct BaseBoard {
        product: Option<String>,
        serial_number: Option<String>,
    }
    #[derive(Deserialize)]
    #[serde(rename = "Win32_VideoController", rename_all = "PascalCase")]
    struct VideoController {
        #[serde(rename = "PNPDeviceID")]
        pnp_device_id: Option<String>,
    }
    #[derive(Deserialize)]
    #[serde(rename = "SoftwareLicensingService", rename_all = "PascalCase")]
    struct LicensingService {
        #[serde(rename = "OA3xOriginalProductKey")]
        oa3x_original_product_key: Option<String>,
    }

    let (boot_mode, secure_boot_enabled) = read_secure_boot();
    let mut out = QcFirmware { boot_mode, secure_boot_enabled, ..Default::default() };

    let Ok(wmi) = wmi::WMIConnection::with_namespace_path("ROOT\\CIMV2") else {
        log::warn!("qc_firmware: ROOT\\CIMV2 unavailable; returning firmware defaults");
        return out;
    };

    if let Ok(rows) = wmi.query::<Bios>() {
        if let Some(b) = rows.into_iter().next() {
            out.bios_version = non_empty(b.smbios_bios_version);
            out.bios_vendor = non_empty(b.manufacturer);
            out.bios_date = non_empty(b.release_date).as_deref().map(fmt_cim_date);
            out.system_serial = non_empty(b.serial_number).filter(|s| !is_placeholder_serial(s));
        }
    }
    if let Ok(rows) = wmi.query::<BaseBoard>() {
        if let Some(bb) = rows.into_iter().next() {
            out.baseboard_product = non_empty(bb.product);
            out.board_serial = non_empty(bb.serial_number).filter(|s| !is_placeholder_serial(s));
        }
    }
    if let Ok(rows) = wmi.query::<VideoController>() {
        out.gpu_device_codes = rows
            .into_iter()
            .filter_map(|r| r.pnp_device_id)
            // PCI\VEN_10DE&DEV_2C02&… → "2C02"
            .filter_map(|id| {
                id.split("DEV_")
                    .nth(1)
                    .map(|rest| rest.split(['&', '\\']).next().unwrap_or("").to_string())
            })
            .filter(|c| !c.is_empty())
            .collect();
    }
    if let Ok(rows) = wmi.query::<LicensingService>() {
        out.oa3_key_present = rows
            .into_iter()
            .any(|r| r.oa3x_original_product_key.is_some_and(|k| !k.trim().is_empty()));
    }

    read_tpm(&mut out);
    out.windows_activated = read_windows_activated(&wmi);
    out
}

#[cfg(target_os = "windows")]
fn non_empty(v: Option<String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// CIM datetime ("YYYYMMDDhhmmss.……") → "YYYY-MM-DD".
#[cfg(target_os = "windows")]
fn fmt_cim_date(d: &str) -> String {
    if d.len() >= 8 && d[0..8].chars().all(|c| c.is_ascii_digit()) {
        format!("{}-{}-{}", &d[0..4], &d[4..6], &d[6..8])
    } else {
        d.to_string()
    }
}

/// "UEFI" + Secure Boot flag when the SecureBoot state key exists; else Legacy/Unknown.
#[cfg(target_os = "windows")]
fn read_secure_boot() -> (String, Option<bool>) {
    let out = std::process::Command::new("reg")
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

#[cfg(target_os = "windows")]
fn read_tpm(out: &mut QcFirmware) {
    use serde::Deserialize;
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
        out.tpm_present = true;
        out.tpm_enabled = t.is_enabled;
        out.tpm_manufacturer = non_empty(t.manufacturer);
        out.tpm_spec_version = non_empty(t.spec_version);
    }
}

/// True when any Windows licensing product reports LicenseStatus 1 (Licensed).
#[cfg(target_os = "windows")]
fn read_windows_activated(wmi: &wmi::WMIConnection) -> Option<bool> {
    use serde::Deserialize;
    #[derive(Deserialize)]
    struct Product {
        #[serde(rename = "Name")]
        name: Option<String>,
        #[serde(rename = "LicenseStatus")]
        license_status: Option<u32>,
        #[serde(rename = "PartialProductKey")]
        partial_product_key: Option<String>,
    }
    let rows: Vec<Product> = wmi
        .raw_query("SELECT Name, LicenseStatus, PartialProductKey FROM SoftwareLicensingProduct")
        .ok()?;
    let mut seen = false;
    for p in rows {
        let is_windows = p.name.as_deref().is_some_and(|n| n.contains("Windows"));
        let has_key = p.partial_product_key.as_ref().is_some_and(|k| !k.is_empty());
        if is_windows && has_key {
            seen = true;
            if p.license_status == Some(1) {
                return Some(true);
            }
        }
    }
    seen.then_some(false)
}

#[cfg(not(target_os = "windows"))]
pub fn gather() -> QcFirmware {
    QcFirmware { boot_mode: "Unknown".into(), ..Default::default() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "windows")]
    #[test]
    fn cim_date_trims_to_ymd() {
        assert_eq!(fmt_cim_date("20250714000000.000000+000"), "2025-07-14");
        assert_eq!(fmt_cim_date("not a date"), "not a date");
    }
}
