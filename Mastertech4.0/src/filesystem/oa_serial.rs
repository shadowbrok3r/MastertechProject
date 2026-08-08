use wmi::WMIConnection;
use serde::Deserialize;
// ----------------------------------------------------
// Get OA-style serial using WMI (PowerShell parity)
//   1) Win32_OperatingSystem.SerialNumber
//   2) Win32_BIOS.SerialNumber (fallback)
// ----------------------------------------------------
pub fn get_oa_style_serial() -> anyhow::Result<String, anyhow::Error> {
    if let Ok(wmi_con) = WMIConnection::new() {
        // 1) Win32_OperatingSystem
        #[derive(Deserialize, Debug)]
        #[allow(non_camel_case_types, non_snake_case)]
        struct Win32_OperatingSystem { SerialNumber: Option<String> }
        if let Ok(results) = wmi_con.query::<Win32_OperatingSystem>() {
            if let Some(item) = results.into_iter().next() {
                if let Some(sn) = item.SerialNumber {
                    let sn = sn.trim();
                    if !sn.is_empty() {
                        return Ok(sn.to_string());
                    }
                }
            }
        }

        // 2) Win32_BIOS
        #[derive(Deserialize, Debug)]
        #[allow(non_camel_case_types, non_snake_case)]
        struct Win32_BIOS { SerialNumber: Option<String> }
        if let Ok(results) = wmi_con.query::<Win32_BIOS>() {
            if let Some(item) = results.into_iter().next() {
                if let Some(sn) = item.SerialNumber {
                    let sn = sn.trim();
                    if !sn.is_empty() {
                        return Ok(sn.to_string());
                    }
                }
            }
        }
    }
    Err(anyhow::anyhow!("Unable to detect OA-style serial from WMI"))
}

// ----------------------------------------------------
// OA3/MSDM Windows key (SoftwareLicensingService.OA3xOriginalProductKey) —
// the same value the pre-boot UEFI app reads from the ACPI MSDM table, so
// both agents report one identity for the box.
// ----------------------------------------------------
pub fn get_oa3_msdm_key() -> anyhow::Result<String, anyhow::Error> {
    let wmi_con = WMIConnection::new()?;
    #[derive(Deserialize, Debug)]
    #[allow(non_camel_case_types, non_snake_case)]
    struct SoftwareLicensingService {
        OA3xOriginalProductKey: Option<String>,
    }
    for item in wmi_con.query::<SoftwareLicensingService>()? {
        if let Some(k) = item.OA3xOriginalProductKey {
            let k = k.trim();
            if !k.is_empty() {
                return Ok(k.to_string());
            }
        }
    }
    Err(anyhow::anyhow!("No OA3 key in SoftwareLicensingService (not an OA3.0 image)"))
}

// ----------------------------------------------------
// Convert OA3 → 13 digit (OS-style)
// Rules per request:
//  - Remove dashes
//  - Remove the last 5 characters (the final block)
//  - Remove the first two digits from the start
//  - Result must be exactly 13 digits (e.g., 00326-10000-00000-AA301 -> 3261000000000)
// Prefer exact-structure parse when possible; otherwise fall back to a generic transform.
// ----------------------------------------------------
pub fn to_oa3_13digit(input: &str) -> Result<String, anyhow::Error> {
    let s = input.trim();

    // Try exact structure first: split on '-' and use first 3 blocks
    if s.contains('-') {
        let parts: Vec<&str> = s.split('-').collect();
        if parts.len() >= 4 {
            let p0 = parts[0];
            let p1 = parts[1];
            let p2 = parts[2];
            if p0.len() >= 2 {
                let candidate: String = p0.chars().skip(2).chain(p1.chars()).chain(p2.chars()).collect();
                if candidate.len() == 13 && candidate.chars().all(|c| c.is_ascii_digit()) {
                    log::debug!("Parsed Windows S/N: {}", mask_serial(&candidate));
                    return Ok(candidate);
                }
            }
        }
    }

    // Fallback: remove dashes, drop last 5 chars, then drop first 2 chars
    let no_dashes: String = s.chars().filter(|c| *c != '-').collect();
    if no_dashes.len() < 7 {
        return Err(anyhow::anyhow!("Input too short after dash removal: '{}'", no_dashes));
    }
    let base = &no_dashes[..no_dashes.len() - 5];
    let candidate = base.chars().skip(2).collect::<String>();
    if candidate.len() != 13 {
        return Err(anyhow::anyhow!("Invalid 13-digit: got {} digits", candidate.len()));
    }
    if !candidate.chars().all(|c| c.is_ascii_digit()) {
        return Err(anyhow::anyhow!("Parsed serial contains non-digit characters"));
    }
    log::debug!("Parsed Windows S/N: {}", mask_serial(&candidate));
    Ok(candidate)
}

// Keeps only the trailing 5 characters of a serial.
pub(crate) fn mask_serial(serial: &str) -> String {
    let skip = serial.chars().count().saturating_sub(5);
    format!("***{}", serial.chars().skip(skip).collect::<String>())
}

#[cfg(test)]
mod tests {
    use super::to_oa3_13digit;
    #[test]
    fn converts_sample_os_serial() {
        let input = "00326-10000-00000-AA301";
        let out = to_oa3_13digit(input).unwrap();
        assert_eq!(out, "3261000000000");
    }
}
