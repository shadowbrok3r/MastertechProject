//! Boot Doctor: diagnose why Windows won't boot, from firmware.
//!
//! Pre-OS is the right place to triage a dead boot — Windows can't report why
//! it failed to load if it never loads. This walks the UEFI boot chain end to
//! end: the ESP and its Windows Boot Manager files, the `Boot####`/`BootOrder`
//! load options, the GPT partition layout, and cross-checks the storage-
//! controller mode (HII), Secure Boot state, disk visibility, and the firmware
//! error record — turning each break into a specific verdict.
//!
//! All reads are best-effort and never panic.

use uefi::boot::{self, OpenProtocolAttributes, OpenProtocolParams};
use uefi::proto::media::file::{File, FileAttribute, FileInfo, FileMode, FileType};
use uefi::proto::media::fs::SimpleFileSystem;
use uefi::proto::media::partition::{GptPartitionType, PartitionInfo};
use uefi::runtime::{self, VariableVendor};
use uefi::{CString16, Guid, cstr16, guid};

use crate::SysInfo;
use crate::logln;

/// Microsoft Reserved Partition.
const MSR_GUID: Guid = guid!("e3c9e316-0b5c-4db8-817d-f92df00215ae");
/// Windows/basic data partition (the OS lives here).
const WIN_DATA_GUID: Guid = guid!("ebd0a0a2-b9e5-4433-87c0-68b6b72699c7");
/// Windows Recovery Environment partition.
const WIN_RECOVERY_GUID: Guid = guid!("de94bba4-06d1-4d40-a16a-bfd50179d6ac");

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Ok,
    Warn,
    Fail,
}

impl Severity {
    /// Ascending order of concern; higher wins a roll-up.
    pub const fn rank(self) -> u8 {
        match self {
            Self::Ok => 0,
            Self::Warn => 1,
            Self::Fail => 2,
        }
    }

    pub const fn key(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Warn => "warn",
            Self::Fail => "fail",
        }
    }
}

/// Highest severity across the findings.
pub fn overall(d: &BootDiag) -> Severity {
    d.verdicts
        .iter()
        .map(|(s, _)| *s)
        .max_by_key(|s| s.rank())
        .unwrap_or(Severity::Ok)
}

pub struct BootEntry {
    pub num: u16,
    pub active: bool,
    pub description: String,
    pub is_windows: bool,
    pub in_boot_order: bool,
}

#[derive(Default)]
pub struct BootDiag {
    pub esp_found: bool,
    pub bootmgfw_present: bool,
    pub bootmgfw_size: u64,
    pub bootmgfw_pe: Option<crate::pecheck::PeVerdict>,
    pub fallback_present: bool,
    pub bcd_present: bool,
    pub bcd_size: u64,
    pub boot_entries: Vec<BootEntry>,
    pub windows_entry: Option<u16>,
    pub windows_in_boot_order: bool,
    pub windows_entry_active: bool,
    pub part_esp: usize,
    pub part_msr: usize,
    pub part_win_data: usize,
    pub part_recovery: usize,
    pub verdicts: Vec<(Severity, String)>,
}

fn read_ucs2(b: &[u8], off: usize) -> (String, usize) {
    let mut s = String::new();
    let mut p = off;
    while p + 2 <= b.len() {
        let cu = u16::from_le_bytes([b[p], b[p + 1]]);
        p += 2;
        if cu == 0 {
            break;
        }
        s.push(char::from_u32(cu as u32).unwrap_or('\u{FFFD}'));
    }
    (s, p)
}

fn ucs2_all(b: &[u8]) -> String {
    let mut s = String::new();
    let mut p = 0;
    while p + 2 <= b.len() {
        let cu = u16::from_le_bytes([b[p], b[p + 1]]);
        p += 2;
        if cu == 0 {
            continue;
        }
        s.push(char::from_u32(cu as u32).unwrap_or('\u{FFFD}'));
    }
    s
}

fn read_var(name: &str) -> Option<Vec<u8>> {
    let cn = CString16::try_from(name).ok()?;
    runtime::get_variable_boxed(&cn, &VariableVendor::GLOBAL_VARIABLE)
        .ok()
        .map(|(b, _)| b.into_vec())
}

/// Open a file relative to an ESP root and return its size if present.
fn check_file(root: &mut uefi::proto::media::file::Directory, path: &uefi::CStr16) -> Option<u64> {
    let handle = root.open(path, FileMode::Read, FileAttribute::empty()).ok()?;
    match handle.into_type().ok()? {
        FileType::Regular(mut f) => f.get_boxed_info::<FileInfo>().ok().map(|i| i.file_size()),
        FileType::Dir(_) => Some(0),
    }
}

/// Find an ESP (a volume with an `\EFI` directory) and probe the Windows boot
/// files on it. Everything runs while the protocol is held open.
fn probe_esp(diag: &mut BootDiag) {
    let Ok(handles) = boot::find_handles::<SimpleFileSystem>() else {
        return;
    };
    for h in handles {
        let mut sfs = match unsafe {
            boot::open_protocol::<SimpleFileSystem>(
                OpenProtocolParams {
                    handle: h,
                    agent: boot::image_handle(),
                    controller: None,
                },
                OpenProtocolAttributes::GetProtocol,
            )
        } {
            Ok(s) => s,
            Err(_) => continue,
        };
        let mut root = match sfs.open_volume() {
            Ok(r) => r,
            Err(_) => continue,
        };
        // An ESP has a top-level \EFI directory.
        if check_file(&mut root, cstr16!("EFI")).is_none() {
            continue;
        }
        diag.esp_found = true;
        match crate::pecheck::read_file(&mut root, cstr16!("EFI\\Microsoft\\Boot\\bootmgfw.efi")) {
            crate::pecheck::FileBytes::Read { size, bytes } => {
                diag.bootmgfw_present = true;
                diag.bootmgfw_size = size;
                diag.bootmgfw_pe = Some(crate::pecheck::validate(&bytes, size));
            }
            crate::pecheck::FileBytes::TooLarge { size } => {
                diag.bootmgfw_present = true;
                diag.bootmgfw_size = size;
            }
            _ => {}
        }
        if let Some(sz) = check_file(&mut root, cstr16!("EFI\\Microsoft\\Boot\\BCD")) {
            diag.bcd_present = true;
            diag.bcd_size = sz;
        }
        if check_file(&mut root, cstr16!("EFI\\Boot\\bootx64.efi")).is_some() {
            diag.fallback_present = true;
        }
        // First ESP with Windows Boot Manager wins; otherwise keep looking.
        if diag.bootmgfw_present {
            break;
        }
    }
}

/// Walk a device-path byte blob for a Media/File-Path node naming bootmgfw.
fn device_path_has_bootmgfw(dp: &[u8]) -> bool {
    let mut p = 0;
    while p + 4 <= dp.len() {
        let t = dp[p];
        let st = dp[p + 1];
        let len = u16::from_le_bytes([dp[p + 2], dp[p + 3]]) as usize;
        if len < 4 || p + len > dp.len() {
            break;
        }
        if t == 0x04 && st == 0x04 {
            let s = ucs2_all(&dp[p + 4..p + len]).to_ascii_lowercase();
            if s.contains("bootmgfw") {
                return true;
            }
        }
        if t == 0x7F {
            break;
        }
        p += len;
    }
    false
}

/// Parse an EFI_LOAD_OPTION (`Boot####` variable body).
fn parse_load_option(num: u16, bytes: &[u8], in_order: bool) -> Option<BootEntry> {
    if bytes.len() < 6 {
        return None;
    }
    let attributes = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    let fpl_len = u16::from_le_bytes(bytes[4..6].try_into().unwrap()) as usize;
    let (desc, dp_start) = read_ucs2(bytes, 6);
    let dp = bytes.get(dp_start..dp_start + fpl_len).unwrap_or(&[]);
    let is_windows =
        desc.eq_ignore_ascii_case("Windows Boot Manager") || device_path_has_bootmgfw(dp);
    Some(BootEntry {
        num,
        active: attributes & 0x1 != 0,
        description: desc,
        is_windows,
        in_boot_order: in_order,
    })
}

fn parse_boot_entries(diag: &mut BootDiag) {
    let order: Vec<u16> = read_var("BootOrder")
        .map(|b| b.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect())
        .unwrap_or_default();
    for &num in &order {
        let name = format!("Boot{num:04X}");
        let Some(bytes) = read_var(&name) else { continue };
        if let Some(e) = parse_load_option(num, &bytes, true) {
            if e.is_windows {
                diag.windows_entry = Some(num);
                diag.windows_in_boot_order = true;
                diag.windows_entry_active = e.active;
            }
            diag.boot_entries.push(e);
        }
    }
    // A Windows entry can exist outside BootOrder; note it as present-but-unordered.
    if diag.windows_entry.is_none() {
        for num in 0u16..=0x20 {
            let name = format!("Boot{num:04X}");
            let Some(bytes) = read_var(&name) else { continue };
            if let Some(e) = parse_load_option(num, &bytes, false) {
                if e.is_windows {
                    diag.windows_entry = Some(num);
                    diag.windows_entry_active = e.active;
                    diag.boot_entries.push(e);
                    break;
                }
            }
        }
    }
}

fn tally_partitions(diag: &mut BootDiag) {
    let Ok(handles) = boot::find_handles::<PartitionInfo>() else {
        return;
    };
    for h in handles {
        let pi = match unsafe {
            boot::open_protocol::<PartitionInfo>(
                OpenProtocolParams {
                    handle: h,
                    agent: boot::image_handle(),
                    controller: None,
                },
                OpenProtocolAttributes::GetProtocol,
            )
        } {
            Ok(p) => p,
            Err(_) => continue,
        };
        if let Some(gpt) = pi.gpt_partition_entry() {
            let g = gpt.partition_type_guid.0;
            if g == GptPartitionType::EFI_SYSTEM_PARTITION.0 {
                diag.part_esp += 1;
            } else if g == MSR_GUID {
                diag.part_msr += 1;
            } else if g == WIN_DATA_GUID {
                diag.part_win_data += 1;
            } else if g == WIN_RECOVERY_GUID {
                diag.part_recovery += 1;
            }
        }
    }
}

/// Storage-controller mode from the HII audit, if resolved (e.g. "AHCI"/"RAID").
fn storage_mode(info: &SysInfo) -> Option<String> {
    info.hii
        .settings
        .iter()
        .find(|s| s.category == Some("Storage"))
        .and_then(|s| s.current.clone())
}

/// Short identity for a drive in a verdict line.
fn drive_label(d: &crate::smart::SataDrive) -> String {
    match (d.model.is_empty(), d.port) {
        (true, 0xFFFF) => "an ATA device".to_string(),
        (true, p) => format!("port {p}"),
        (false, p) => format!("{} (port {p})", d.model),
    }
}

/// SATA SMART findings, capped so the verdict pane stays readable.
fn smart_findings(diag: &mut BootDiag, info: &SysInfo) {
    const MAX_DRIVES: usize = 2;
    const MAX_ATTRS: usize = 3;
    let failing: Vec<&crate::smart::SataDrive> =
        info.sata.iter().filter(|d| d.is_failing()).collect();
    for d in failing.iter().take(MAX_DRIVES) {
        let attrs: Vec<String> = d
            .alarm_attrs()
            .take(MAX_ATTRS)
            .map(|a| format!("{} {}<={}", a.name, a.value, a.threshold.unwrap_or(0)))
            .collect();
        let detail = if attrs.is_empty() {
            "the drive reports SMART threshold exceeded".to_string()
        } else {
            attrs.join(", ")
        };
        diag.verdicts.push((
            Severity::Fail,
            format!(
                "SMART failure on {}: {detail}. Image the drive and replace it — full attribute list on the Storage page.",
                drive_label(d)
            ),
        ));
    }
    let hidden = failing.len().saturating_sub(MAX_DRIVES);
    if hidden > 0 {
        diag.verdicts.push((
            Severity::Fail,
            format!("{hidden} further drive(s) report failing SMART attributes — see the Storage page."),
        ));
    }
    // Absence of SMART is reported as unverified, never as health.
    let no_handle = info
        .sata
        .iter()
        .any(|d| d.status == crate::smart::SmartStatus::NoHandle);
    let fixed_ata = info
        .disks
        .iter()
        .any(|d| !d.removable && matches!(d.bus, "SATA" | "ATAPI" | "SCSI/SAS"));
    if no_handle && fixed_ata {
        diag.verdicts.push((
            Severity::Warn,
            "No ATA pass-thru handle while non-NVMe disks are present — SMART unavailable (RAID/RST mode or a USB bridge). Drive health was NOT verified.".into(),
        ));
    }
    let mut unread = info
        .sata
        .iter()
        .filter(|d| d.status != crate::smart::SmartStatus::NoHandle)
        .filter_map(|d| d.unverified_reason().map(|r| (d, r)));
    if let Some((d, reason)) = unread.next() {
        let extra = unread.count();
        let more = if extra > 0 {
            format!(" (+{extra} more)")
        } else {
            String::new()
        };
        diag.verdicts.push((
            Severity::Warn,
            format!(
                "SMART unavailable on {}: {reason}{more}. Drive health was NOT verified.",
                drive_label(d)
            ),
        ));
    }
}

/// Volume findings in volsig's wording, BitLocker first, capped for the pane.
fn volume_findings(diag: &mut BootDiag, info: &SysInfo) {
    const MAX_VOLUMES: usize = 3;
    let mut ordered = info.volumes.clone();
    ordered.sort_by_key(|v| u8::from(v.kind != crate::volsig::VolKind::BitLocker));
    let found = crate::volsig::verdicts(&ordered);
    let hidden = found.len().saturating_sub(MAX_VOLUMES);
    for f in found.into_iter().take(MAX_VOLUMES) {
        diag.verdicts.push(f);
    }
    if hidden > 0 {
        diag.verdicts.push((
            Severity::Warn,
            format!("{hidden} further volume finding(s) in the uploaded report."),
        ));
    }
}

fn synthesize(diag: &mut BootDiag, info: &SysInfo) {
    let disks_visible = !info.nvme.is_empty() || !info.disks.is_empty();

    if !disks_visible {
        diag.verdicts.push((
            Severity::Fail,
            "No boot media detected by firmware — dead/unseated drive, or the SATA controller is disabled in BIOS.".into(),
        ));
    }
    if !diag.esp_found {
        diag.verdicts.push((
            Severity::Fail,
            "No readable EFI System Partition — GPT/ESP damage, a wiped disk, or the OS drive isn't visible. Windows cannot boot.".into(),
        ));
    } else if !diag.bootmgfw_present {
        diag.verdicts.push((
            Severity::Fail,
            "ESP present but \\EFI\\Microsoft\\Boot\\bootmgfw.efi is missing — Windows Boot Manager gone. Rebuild with bcdboot.".into(),
        ));
    }
    if diag.esp_found && !diag.bcd_present {
        diag.verdicts.push((
            Severity::Warn,
            "BCD store missing/unreadable on the ESP — boot configuration lost (bcdboot / bootrec /rebuildbcd).".into(),
        ));
    }
    match diag.windows_entry {
        None => diag.verdicts.push((
            Severity::Warn,
            "No Boot#### entry points to Windows Boot Manager — firmware has no Windows boot option (recreate with bcdboot, or add in firmware setup).".into(),
        )),
        Some(_) if !diag.windows_in_boot_order => diag.verdicts.push((
            Severity::Warn,
            "A Windows boot entry exists but is not in BootOrder — firmware won't try it. Fix boot priority.".into(),
        )),
        Some(_) if !diag.windows_entry_active => diag.verdicts.push((
            Severity::Warn,
            "The Windows boot entry is marked inactive.".into(),
        )),
        _ => {}
    }
    if diag.part_win_data == 0 && diag.part_esp > 0 {
        diag.verdicts.push((
            Severity::Warn,
            "No Windows data partition in the GPT — the OS partition is missing, deleted, or converted (RAW/NTFS damage).".into(),
        ));
    }
    if let Some(mode) = storage_mode(info) {
        let m = mode.to_ascii_lowercase();
        if m.contains("raid") || m.contains("vmd") {
            diag.verdicts.push((
                Severity::Warn,
                format!("Storage controller mode = {mode}: if Windows was installed under AHCI this causes INACCESSIBLE_BOOT_DEVICE (0x7B). Switch back to AHCI or load the RST/VMD driver."),
            ));
        }
    }
    if info.secure_boot == Some(true) && !diag.bootmgfw_present && diag.fallback_present {
        diag.verdicts.push((
            Severity::Warn,
            "Secure Boot is on and only a fallback bootloader was found — an unsigned/non-Microsoft loader will be rejected.".into(),
        ));
    }
    if info.bert.present && info.bert.error_present {
        diag.verdicts.push((
            Severity::Warn,
            format!("Firmware recorded a fatal hardware error last boot (BERT: {}).", info.bert.severity),
        ));
    }
    if let Some(pe) = &diag.bootmgfw_pe {
        if !crate::pecheck::is_valid(pe) {
            diag.verdicts
                .push((Severity::Fail, crate::pecheck::verdict_detail(pe)));
        }
    }
    smart_findings(diag, info);
    volume_findings(diag, info);
    if diag.verdicts.is_empty() {
        diag.verdicts.push((
            Severity::Ok,
            "Boot chain looks intact (ESP + bootmgfw + BCD + ordered Windows entry). If Windows still fails, suspect driver/registry/filesystem corruption — run WinRE Startup Repair or sfc/DISM.".into(),
        ));
    }
}

/// Run the full boot-chain diagnosis. Call after HII/Secure-Boot/disk collection.
pub fn collect(info: &SysInfo) -> BootDiag {
    let mut diag = BootDiag::default();
    probe_esp(&mut diag);
    parse_boot_entries(&mut diag);
    tally_partitions(&mut diag);
    synthesize(&mut diag, info);
    logln(format!(
        "bootdiag: esp={} bootmgfw={} bcd={} win_entry={:?} parts(esp/msr/data/rec)={}/{}/{}/{} verdicts={}",
        diag.esp_found,
        diag.bootmgfw_present,
        diag.bcd_present,
        diag.windows_entry,
        diag.part_esp,
        diag.part_msr,
        diag.part_win_data,
        diag.part_recovery,
        diag.verdicts.len()
    ));
    diag
}

/// `boot_diagnostics` object for the fingerprint upload.
pub fn diag_json(d: &BootDiag) -> serde_json::Value {
    let entries: Vec<serde_json::Value> = d
        .boot_entries
        .iter()
        .map(|e| {
            serde_json::json!({
                "num": format!("Boot{:04X}", e.num),
                "description": e.description,
                "active": e.active,
                "is_windows": e.is_windows,
                "in_boot_order": e.in_boot_order,
            })
        })
        .collect();
    let verdicts: Vec<serde_json::Value> = d
        .verdicts
        .iter()
        .map(|(sev, msg)| {
            serde_json::json!({
                "severity": sev.key(),
                "rank": sev.rank(),
                "message": msg,
            })
        })
        .collect();
    let roll = overall(d);
    serde_json::json!({
        "esp_found": d.esp_found,
        "bootmgfw_present": d.bootmgfw_present,
        "bootmgfw_size": d.bootmgfw_size,
        "bootmgfw_pe": d.bootmgfw_pe.as_ref().map(crate::pecheck::verdict_json),
        "overall": roll.key(),
        "overall_rank": roll.rank(),
        "fallback_present": d.fallback_present,
        "bcd_present": d.bcd_present,
        "windows_entry": d.windows_entry.map(|n| format!("Boot{n:04X}")),
        "windows_in_boot_order": d.windows_in_boot_order,
        "partitions": {
            "esp": d.part_esp,
            "msr": d.part_msr,
            "windows_data": d.part_win_data,
            "recovery": d.part_recovery,
        },
        "boot_entries": entries,
        "verdicts": verdicts,
    })
}
