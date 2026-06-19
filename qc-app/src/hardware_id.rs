//! Reads the machine's system + motherboard serials (Win32_Bios /
//! Win32_BaseBoard via WMI) for auto-resolving the order from hardware.
//! Filters out the BIOS placeholder strings OEMs ship before SMBIOS is
//! written. Non-Windows builds return nothing.

/// BIOS placeholder serials that aren't real (case-insensitive match).
const PLACEHOLDERS: &[&str] = &[
    "to be filled by o.e.m.",
    "default string",
    "system serial number",
    "base board serial number",
    "none",
    "n/a",
    "not applicable",
    "not specified",
    "0",
    "00000000",
    "123456789",
    "...",
];

/// True when a serial is empty, too short, or a known BIOS placeholder.
pub fn is_placeholder_serial(serial: &str) -> bool {
    let s = serial.trim();
    if s.len() < 4 {
        return true;
    }
    let lower = s.to_lowercase();
    PLACEHOLDERS.iter().any(|p| lower == *p)
}

/// System serial first (reliable on laptops + post-branding desktops), then
/// the motherboard serial. Placeholders dropped; order preserved.
#[cfg(windows)]
pub fn read_machine_serials() -> Vec<String> {
    use serde::Deserialize;

    #[derive(Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct Bios {
        serial_number: Option<String>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct BaseBoard {
        serial_number: Option<String>,
    }

    let mut out = Vec::new();
    let Ok(wmi) = wmi::WMIConnection::with_namespace_path("ROOT\\CIMV2") else {
        return out;
    };
    if let Ok(rows) = wmi.query::<Bios>() {
        for r in rows {
            if let Some(s) = r.serial_number {
                if !is_placeholder_serial(&s) && !out.contains(&s) {
                    out.push(s.trim().to_string());
                }
            }
        }
    }
    if let Ok(rows) = wmi.query::<BaseBoard>() {
        for r in rows {
            if let Some(s) = r.serial_number {
                let s = s.trim().to_string();
                if !is_placeholder_serial(&s) && !out.contains(&s) {
                    out.push(s);
                }
            }
        }
    }
    out
}

/// Motherboard product string (Win32_BaseBoard.Product) for chipset lookup.
#[cfg(windows)]
pub fn read_baseboard_product() -> Option<String> {
    use serde::Deserialize;
    #[derive(Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct BaseBoard {
        product: Option<String>,
    }
    let wmi = wmi::WMIConnection::with_namespace_path("ROOT\\CIMV2").ok()?;
    let rows: Vec<BaseBoard> = wmi.query().ok()?;
    rows.into_iter().find_map(|r| r.product.filter(|p| !p.trim().is_empty()))
}

/// GPU PCI device codes (`DEV_xxxx`) from Win32_VideoController PNP ids.
#[cfg(windows)]
pub fn read_gpu_device_codes() -> Vec<String> {
    use serde::Deserialize;
    #[derive(Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct VideoController {
        #[serde(rename = "PNPDeviceID")]
        pnp_device_id: Option<String>,
    }
    let Ok(wmi) = wmi::WMIConnection::with_namespace_path("ROOT\\CIMV2") else {
        return Vec::new();
    };
    let rows: Vec<VideoController> = wmi.query().unwrap_or_default();
    rows.into_iter()
        .filter_map(|r| r.pnp_device_id)
        .filter_map(|id| {
            // PCI\VEN_10DE&DEV_2C02&... → "2C02"
            id.split("DEV_").nth(1).map(|rest| {
                rest.split(['&', '\\']).next().unwrap_or("").to_string()
            })
        })
        .filter(|c| !c.is_empty())
        .collect()
}

#[cfg(not(windows))]
pub fn read_machine_serials() -> Vec<String> {
    Vec::new()
}

#[cfg(not(windows))]
pub fn read_baseboard_product() -> Option<String> {
    None
}

#[cfg(not(windows))]
pub fn read_gpu_device_codes() -> Vec<String> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholders_rejected() {
        assert!(is_placeholder_serial("To Be Filled By O.E.M."));
        assert!(is_placeholder_serial("Default string"));
        assert!(is_placeholder_serial("0"));
        assert!(is_placeholder_serial("   "));
        assert!(is_placeholder_serial("abc")); // too short
    }

    #[test]
    fn real_serials_accepted() {
        assert!(!is_placeholder_serial("SEED-967041"));
        assert!(!is_placeholder_serial("MB-967041-234410"));
        assert!(!is_placeholder_serial("PF2Z9X7K"));
    }
}
