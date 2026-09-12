//! Laptop battery wear reading for the spec gather and the TUR sheet.

use database::schema::BatteryHealth;

#[cfg(target_os = "windows")]
use super::system_info::CREATE_NO_WINDOW;

/// Design capacity, full-charge capacity and cycle count live in the
/// `root/WMI` battery classes; name, chemistry and charge come from
/// `Win32_Battery`. Written without inline-if expressions so Windows
/// PowerShell 5.1 parses it.
#[cfg(target_os = "windows")]
const BATTERY_PROBE: &str = r#"
$static = Get-CimInstance -Namespace root/WMI -ClassName BatteryStaticData -ErrorAction SilentlyContinue | Select-Object -First 1
$full   = Get-CimInstance -Namespace root/WMI -ClassName BatteryFullChargedCapacity -ErrorAction SilentlyContinue | Select-Object -First 1
$cycle  = Get-CimInstance -Namespace root/WMI -ClassName BatteryCycleCount -ErrorAction SilentlyContinue | Select-Object -First 1
$w32    = Get-CimInstance Win32_Battery -ErrorAction SilentlyContinue | Select-Object -First 1
if (-not $static -and -not $w32) { exit 0 }
$name = ''
$chem = 0
$charge = 0
if ($w32) {
    $name = [string]$w32.Name
    $chem = [int]$w32.Chemistry
    if ($w32.EstimatedChargeRemaining) { $charge = [int]$w32.EstimatedChargeRemaining }
}
$design = 0
if ($static) { $design = [uint64]$static.DesignedCapacity }
$fullCap = 0
if ($full) { $fullCap = [uint64]$full.FullChargedCapacity }
$cycles = 0
if ($cycle) { $cycles = [uint64]$cycle.CycleCount }
[pscustomobject]@{ name = $name; chemistry = $chem; design = $design; full = $fullCap; cycles = $cycles; charge = $charge } | ConvertTo-Json -Compress
"#;

#[cfg(target_os = "windows")]
#[derive(serde::Deserialize)]
struct ProbeOutput {
    #[serde(default)]
    name: String,
    #[serde(default)]
    chemistry: u8,
    #[serde(default)]
    design: u64,
    #[serde(default)]
    full: u64,
    #[serde(default)]
    cycles: u64,
    #[serde(default)]
    charge: u8,
}

/// `Win32_Battery.Chemistry` codes.
#[cfg(target_os = "windows")]
fn chemistry_name(code: u8) -> &'static str {
    match code {
        3 => "Lead Acid",
        4 => "Nickel Cadmium",
        5 => "Nickel Metal Hydride",
        6 => "Lithium-ion",
        7 => "Zinc Air",
        8 => "Lithium Polymer",
        _ => "",
    }
}

/// Reads the battery report, or `None` on a machine without a battery.
#[cfg(target_os = "windows")]
pub async fn read_battery_health() -> Option<BatteryHealth> {
    let output = tokio::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", BATTERY_PROBE])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .await
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let probe: ProbeOutput = serde_json::from_str(json_object(&stdout)?).ok()?;

    // A desktop with a UPS attached can answer Win32_Battery with no capacity
    // figures; without capacities there is no wear to report.
    if probe.design == 0 && probe.full == 0 {
        return None;
    }

    Some(BatteryHealth {
        name: probe.name.trim().to_string(),
        chemistry: chemistry_name(probe.chemistry).to_string(),
        design_capacity_mwh: probe.design,
        full_charge_capacity_mwh: probe.full,
        cycle_count: (probe.cycles > 0).then_some(probe.cycles),
        charge_percent: (probe.charge > 0).then_some(probe.charge),
    })
}

#[cfg(not(target_os = "windows"))]
pub async fn read_battery_health() -> Option<BatteryHealth> {
    None
}

/// Slices the JSON object out of PowerShell output that may carry stray
/// progress or warning lines around it.
#[cfg(target_os = "windows")]
fn json_object(raw: &str) -> Option<&str> {
    let start = raw.find('{')?;
    let end = raw.rfind('}')?;
    (end > start).then(|| &raw[start..=end])
}
