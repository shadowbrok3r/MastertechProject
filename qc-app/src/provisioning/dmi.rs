//! SMBIOS/DMI field writing (ported from QCWizard `AMI` / `Insyde`). Pure
//! command builders (tested) + a Windows executor that shells the vendor tool
//! (AMIDEWIN for desktops, Insyde H2OSDE for Sager laptops). Executed only
//! behind an explicit confirm in the panel.

use database::orders::{BuildSpec, QcOrder};

use super::manifest::CompanyManifest;

/// Resolved DMI values for one order/machine.
#[derive(Debug, Clone, Default)]
pub struct DmiContext {
    pub manufacturer: Option<String>,
    pub asset_tag: String,
    pub system_serial: String,
    pub baseboard_serial: String,
    pub system_product: String,
    pub system_family: String,
    pub system_sku: String,
}

impl DmiContext {
    pub fn build(
        order: &QcOrder,
        spec: &BuildSpec,
        manifest: &CompanyManifest,
        board_serial: &str,
        system_serial: &str,
    ) -> Self {
        let config_id = order.config.as_ref().map(|c| c.id.clone()).unwrap_or_default();
        let config_name = order
            .config
            .as_ref()
            .map(|c| c.name.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| spec.model.clone());
        let asset_tag = if config_id.is_empty() {
            order.id.clone()
        } else {
            format!("{}-{}", order.id, config_id)
        };
        let baseboard_product = spec.motherboard.clone().unwrap_or_default();
        let system_product = manifest
            .dmi
            .product_template
            .replace("{config_name}", &config_name)
            .replace("{baseboard_product}", &baseboard_product)
            .trim()
            .to_string();
        Self {
            manufacturer: manifest.dmi.manufacturer.clone(),
            asset_tag,
            system_serial: system_serial.to_string(),
            baseboard_serial: board_serial.to_string(),
            system_product,
            system_family: config_name,
            system_sku: manifest.dmi.system_sku.clone(),
        }
    }
}

/// AMIDEWIN switch/value pairs (desktops). Empty values are dropped.
pub fn ami_commands(ctx: &DmiContext) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    if let Some(mfr) = ctx.manufacturer.as_ref() {
        out.push(("/SM", mfr.clone())); // system manufacturer
        out.push(("/CM", mfr.clone())); // chassis manufacturer
    }
    push_if(&mut out, "/BT", &ctx.asset_tag);
    push_if(&mut out, "/SS", &ctx.system_serial);
    push_if(&mut out, "/BS", &ctx.baseboard_serial);
    push_if(&mut out, "/SP", &ctx.system_product);
    push_if(&mut out, "/SF", &ctx.system_family);
    push_if(&mut out, "/SK", &ctx.system_sku);
    out
}

/// Insyde H2OSDE switch/value pairs (Sager laptops). Panel wires AMI desktop
/// only for now; the laptop path is the forward contract.
#[allow(dead_code)]
pub fn h2osde_commands(ctx: &DmiContext) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    if let Some(mfr) = ctx.manufacturer.as_ref() {
        out.push(("-cm", mfr.clone()));
    }
    push_if(&mut out, "-ca", &ctx.asset_tag);
    push_if(&mut out, "-ss", &ctx.system_serial);
    push_if(&mut out, "-bs", &ctx.baseboard_serial);
    push_if(&mut out, "-sp", &ctx.system_product);
    push_if(&mut out, "-sf", &ctx.system_family);
    push_if(&mut out, "-sk", &ctx.system_sku);
    out
}

fn push_if(out: &mut Vec<(&'static str, String)>, switch: &'static str, value: &str) {
    if !value.trim().is_empty() {
        out.push((switch, value.to_string()));
    }
}

/// AMIDEWIN switch/value pairs that wipe the written SMBIOS fields (empty values).
pub fn ami_clear_commands() -> Vec<(&'static str, String)> {
    ["/SS", "/BS", "/BT", "/SP", "/SF"]
        .into_iter()
        .map(|s| (s, String::new()))
        .collect()
}

/// SMBIOS fields read natively (no AMIDEWIN) from the firmware table.
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct DmiReadResult {
    pub system_manufacturer: String,
    pub system_product: String,
    pub system_serial: String,
    pub baseboard_manufacturer: String,
    pub baseboard_product: String,
    pub baseboard_serial: String,
    pub bios_vendor: String,
    pub bios_version: String,
}

impl DmiReadResult {
    /// Multi-line summary of the populated fields.
    pub fn summary(&self) -> String {
        let rows = [
            ("System mfr", &self.system_manufacturer),
            ("System product", &self.system_product),
            ("System serial", &self.system_serial),
            ("Board mfr", &self.baseboard_manufacturer),
            ("Board product", &self.baseboard_product),
            ("Board serial", &self.baseboard_serial),
            ("BIOS vendor", &self.bios_vendor),
            ("BIOS version", &self.bios_version),
        ];
        rows.iter()
            .filter(|(_, v)| !v.trim().is_empty())
            .map(|(k, v)| format!("{k}: {v}"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Read SMBIOS type 0/1/2 fields via `GetSystemFirmwareTable('RSMB')`.
#[cfg(windows)]
pub fn read_smbios() -> anyhow::Result<DmiReadResult> {
    use windows::Win32::System::SystemInformation::{FIRMWARE_TABLE_PROVIDER, GetSystemFirmwareTable};
    const RSMB: FIRMWARE_TABLE_PROVIDER = FIRMWARE_TABLE_PROVIDER(u32::from_be_bytes(*b"RSMB"));
    let size = unsafe { GetSystemFirmwareTable(RSMB, 0, None) };
    if size == 0 {
        return Err(anyhow::anyhow!("GetSystemFirmwareTable size probe returned 0"));
    }
    let mut buf = vec![0u8; size as usize];
    let written = unsafe { GetSystemFirmwareTable(RSMB, 0, Some(&mut buf)) };
    if written == 0 || written as usize > buf.len() {
        return Err(anyhow::anyhow!("GetSystemFirmwareTable read failed (got {written}, cap {})", buf.len()));
    }
    buf.truncate(written as usize);
    Ok(parse_smbios(&buf))
}

#[cfg(not(windows))]
pub fn read_smbios() -> anyhow::Result<DmiReadResult> {
    Err(anyhow::anyhow!("SMBIOS read is Windows-only"))
}

/// 1-based SMBIOS string lookup; index 0 is "no string".
fn smbios_string(strings: &[String], idx: u8) -> String {
    if idx == 0 {
        return String::new();
    }
    strings.get((idx - 1) as usize).cloned().unwrap_or_default()
}

/// Parse a `RawSMBIOSData` blob into type 0/1/2 fields.
fn parse_smbios(raw: &[u8]) -> DmiReadResult {
    let mut out = DmiReadResult::default();
    if raw.len() < 8 {
        return out;
    }
    let length = u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]) as usize;
    let data_end = (8 + length).min(raw.len());
    let data = &raw[8..data_end];

    let mut i = 0usize;
    while i + 4 <= data.len() {
        let typ = data[i];
        let len = data[i + 1] as usize;
        if len < 4 {
            break;
        }
        let formatted_end = i + len;
        if formatted_end > data.len() {
            break;
        }
        let mut s = formatted_end;
        while s + 1 < data.len() && !(data[s] == 0 && data[s + 1] == 0) {
            s += 1;
        }
        let strings_end = if s + 1 < data.len() { s } else { data.len() };
        let strings: Vec<String> = data[formatted_end..strings_end]
            .split(|b| *b == 0)
            .filter(|seg| !seg.is_empty())
            .map(|seg| String::from_utf8_lossy(seg).trim().to_string())
            .collect();
        let field = |off: usize| -> String {
            if off < len {
                smbios_string(&strings, data[i + off])
            } else {
                String::new()
            }
        };
        match typ {
            0 => {
                out.bios_vendor = field(0x04);
                out.bios_version = field(0x05);
            }
            1 => {
                out.system_manufacturer = field(0x04);
                out.system_product = field(0x05);
                out.system_serial = field(0x07);
            }
            2 => {
                out.baseboard_manufacturer = field(0x04);
                out.baseboard_product = field(0x05);
                out.baseboard_serial = field(0x07);
            }
            127 => break,
            _ => {}
        }
        i = s + 2;
    }
    out
}

/// Human-readable preview of the exact commands that would run.
pub fn preview(tool_exe: &str, cmds: &[(&'static str, String)]) -> String {
    cmds.iter()
        .map(|(sw, val)| format!("\"{tool_exe}\" {sw} \"{val}\""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Threadripper boards reject SMBIOS writes from these tools — QCWizard skips them.
pub fn is_threadripper(spec: &BuildSpec) -> bool {
    spec.cpu.to_lowercase().contains("threadripper")
}

/// Run the AMIDEWIN commands (one switch per invocation, matching QCWizard).
/// Returns combined stdout/stderr. Windows-only; behind a panel confirm.
#[cfg(windows)]
pub fn run(tool_exe: &std::path::Path, cmds: &[(&'static str, String)]) -> anyhow::Result<String> {
    use std::process::Command;
    let mut log = String::new();
    for (switch, value) in cmds {
        let output = Command::new(tool_exe)
            .arg(switch)
            .arg(value)
            .output()
            .map_err(|e| anyhow::anyhow!("spawn {tool_exe:?} {switch}: {e}"))?;
        log.push_str(&format!(
            "{switch} \"{value}\" → {}\n{}{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        ));
    }
    Ok(log)
}

#[cfg(not(windows))]
pub fn run(_tool_exe: &std::path::Path, _cmds: &[(&'static str, String)]) -> anyhow::Result<String> {
    Err(anyhow::anyhow!("DMI writes are Windows-only"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use database::orders::{OrderConfigInfo, QcOrder};

    fn ctx() -> DmiContext {
        let order = QcOrder {
            id: "212345".into(),
            config: Some(OrderConfigInfo { id: "9".into(), name: "X-6 Performance".into(), ..Default::default() }),
            ..Default::default()
        };
        let spec = BuildSpec {
            cpu: "INTEL CORE I7 13700KF".into(),
            motherboard: Some("MSI MEG X670E".into()),
            ..Default::default()
        };
        let manifest = CompanyManifest::default_for(super::super::company::Company::Pcl);
        DmiContext::build(&order, &spec, &manifest, "BOARDSN1", "SYSSN1")
    }

    #[test]
    fn ami_commands_map_fields() {
        let c = ctx();
        let cmds = ami_commands(&c);
        assert!(cmds.contains(&("/SM", "PCL".to_string())));
        assert!(cmds.contains(&("/CM", "PCL".to_string())));
        assert!(cmds.contains(&("/BT", "212345-9".to_string())));
        assert!(cmds.contains(&("/BS", "BOARDSN1".to_string())));
        assert!(cmds.contains(&("/SK", "0001".to_string())));
        assert!(cmds.iter().any(|(s, v)| *s == "/SP" && v == "X-6 Performance MSI MEG X670E"));
        assert!(cmds.iter().any(|(s, v)| *s == "/SF" && v == "X-6 Performance"));
    }

    #[test]
    fn no_manufacturer_skips_mfr_switches() {
        let mut c = ctx();
        c.manufacturer = None;
        let cmds = ami_commands(&c);
        assert!(!cmds.iter().any(|(s, _)| *s == "/SM" || *s == "/CM"));
        assert!(cmds.iter().any(|(s, _)| *s == "/BT"));
    }

    #[test]
    fn threadripper_detected() {
        let spec = BuildSpec { cpu: "AMD Ryzen Threadripper 7980X".into(), ..Default::default() };
        assert!(is_threadripper(&spec));
        let spec = BuildSpec { cpu: "INTEL CORE I7 13700KF".into(), ..Default::default() };
        assert!(!is_threadripper(&spec));
    }

    #[test]
    fn preview_quotes_values() {
        let cmds = vec![("/SM", "PCL".to_string())];
        assert_eq!(preview("AMIDEWIN527", &cmds), "\"AMIDEWIN527\" /SM \"PCL\"");
    }

    #[test]
    fn clear_commands_wipe_fields() {
        let cmds = ami_clear_commands();
        assert!(cmds.iter().all(|(_, v)| v.is_empty()));
        assert!(cmds.iter().any(|(s, _)| *s == "/SS"));
    }

    #[test]
    fn parse_smbios_reads_type1() {
        let mut table: Vec<u8> = Vec::new();
        table.extend_from_slice(&[1, 8, 0x01, 0x00, 1, 2, 3, 4]);
        table.extend_from_slice(b"ACME\0Box\01.0\0SER123\0\0");
        table.extend_from_slice(&[127, 4, 0x02, 0x00, 0, 0]);

        let mut raw: Vec<u8> = vec![0, 2, 8, 0];
        raw.extend_from_slice(&(table.len() as u32).to_le_bytes());
        raw.extend_from_slice(&table);

        let r = parse_smbios(&raw);
        assert_eq!(r.system_manufacturer, "ACME");
        assert_eq!(r.system_product, "Box");
        assert_eq!(r.system_serial, "SER123");
    }
}
