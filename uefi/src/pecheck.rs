//! Structural PE/COFF validation for UEFI boot binaries.
//!
//! `bootdiag::check_file` only reports a file size, so a truncated or
//! zero-filled `bootmgfw.efi` reads as healthy. This parses the DOS stub, PE
//! signature, COFF header, optional header and section table out of a buffer
//! the caller already read from the ESP, and turns each break into a verdict.
//!
//! Structural only: no Authenticode or signature verification — Secure Boot
//! enforces that.
//!
//! [`validate`] touches no UEFI protocol and cannot block. [`read_file`] is the
//! only protocol user here; it mirrors `bootdiag::check_file` but returns the
//! bytes, and reports a directory as [`FileBytes::Directory`] instead of a
//! zero-length file, so callers never score `\EFI` as corruption.
//!
//! All reads are best-effort and never panic.

use uefi::proto::media::file::{
    Directory, File, FileAttribute, FileInfo, FileMode, FileType, RegularFile,
};

/// Largest boot binary that will be read; anything bigger is refused.
pub const MAX_FILE_BYTES: usize = 16 * 1024 * 1024;

const MAX_E_LFANEW: u32 = 0x1000;
const MAX_SIZE_OF_IMAGE: u32 = 256 * 1024 * 1024;
const MIN_OPTIONAL_HEADER: u16 = 70;
const MAX_SECTIONS: u16 = 96;
const SECTION_ENTRY_LEN: usize = 40;

const MACHINE_X64: u16 = 0x8664;
const MACHINE_AARCH64: u16 = 0xAA64;
const PE32_MAGIC: u16 = 0x010B;
const PE32PLUS_MAGIC: u16 = 0x020B;
const SUBSYSTEM_EFI_APPLICATION: u16 = 10;

/// Outcome of a structural parse.
pub enum PeVerdict {
    Valid { machine: u16, subsystem: u16, size_of_image: u32 },
    Truncated { expected: u64, actual: u64 },
    ZeroFilled,
    NotPe { reason: String },
    Suspect { reason: String },
}

/// Result of reading a path under an ESP root.
pub enum FileBytes {
    Missing,
    Directory,
    TooLarge { size: u64 },
    Unreadable,
    Read { size: u64, bytes: Vec<u8> },
}

fn u16_at(b: &[u8], off: usize) -> Option<u16> {
    let s = b.get(off..off.checked_add(2)?)?;
    Some(u16::from_le_bytes([*s.first()?, *s.get(1)?]))
}

fn u32_at(b: &[u8], off: usize) -> Option<u32> {
    let s = b.get(off..off.checked_add(4)?)?;
    let a: [u8; 4] = s.try_into().ok()?;
    Some(u32::from_le_bytes(a))
}

fn machine_name(m: u16) -> &'static str {
    match m {
        0x014C => "i386",
        0x01C0 | 0x01C2 | 0x01C4 => "ARM 32-bit",
        0x0200 | 0x0EBC => "IA64/EBC",
        MACHINE_X64 => "x64",
        MACHINE_AARCH64 => "ARM64",
        _ => "unknown",
    }
}

fn subsystem_name(s: u16) -> &'static str {
    match s {
        1 => "native",
        2 => "Windows GUI",
        3 => "Windows console",
        10 => "EFI application",
        11 => "EFI boot service driver",
        12 => "EFI runtime driver",
        13 => "EFI ROM",
        _ => "unknown",
    }
}

/// Parse `buf` as a UEFI boot image. `declared_size` is the real file size from
/// `FileInfo`, which may exceed `buf.len()` when the read was capped.
pub fn validate(buf: &[u8], declared_size: u64) -> PeVerdict {
    if buf.is_empty() {
        return PeVerdict::NotPe { reason: "file is empty — no bytes to inspect".into() };
    }
    if buf.iter().all(|&b| b == 0) {
        return PeVerdict::ZeroFilled;
    }
    if buf.get(0..2) != Some(&b"MZ"[..]) {
        let b0 = buf.first().copied().unwrap_or(0);
        let b1 = buf.get(1).copied().unwrap_or(0);
        return PeVerdict::NotPe {
            reason: format!("no 'MZ' signature — file starts with {b0:02X} {b1:02X}"),
        };
    }

    let Some(e_lfanew) = u32_at(buf, 0x3C) else {
        return PeVerdict::Truncated { expected: 0x40, actual: effective_size(buf, declared_size) };
    };
    if !(0x40..=MAX_E_LFANEW).contains(&e_lfanew) {
        return PeVerdict::Suspect {
            reason: format!("PE header offset 0x{e_lfanew:X} is out of range"),
        };
    }

    let actual = effective_size(buf, declared_size);
    let pe = e_lfanew as usize;
    match buf.get(pe..pe.saturating_add(4)) {
        None => {
            return PeVerdict::Truncated { expected: (pe as u64).saturating_add(4), actual };
        }
        Some(sig) if sig != b"PE\0\0" => {
            return PeVerdict::NotPe {
                reason: format!("no 'PE\\0\\0' signature at offset 0x{pe:X}"),
            };
        }
        Some(_) => {}
    }

    let coff = pe.saturating_add(4);
    let (Some(machine), Some(num_sections), Some(size_opt)) = (
        u16_at(buf, coff),
        u16_at(buf, coff.saturating_add(2)),
        u16_at(buf, coff.saturating_add(16)),
    ) else {
        return PeVerdict::Truncated { expected: (coff as u64).saturating_add(20), actual };
    };

    if machine != MACHINE_X64 && machine != MACHINE_AARCH64 {
        return PeVerdict::Suspect {
            reason: format!(
                "machine type 0x{machine:04X} ({}) is not x64 or ARM64",
                machine_name(machine)
            ),
        };
    }
    if num_sections == 0 {
        return PeVerdict::Suspect { reason: "COFF header declares zero sections".into() };
    }
    if num_sections > MAX_SECTIONS {
        return PeVerdict::Suspect {
            reason: format!("COFF header declares {num_sections} sections"),
        };
    }

    let opt = coff.saturating_add(20);
    let Some(magic) = u16_at(buf, opt) else {
        return PeVerdict::Truncated { expected: (opt as u64).saturating_add(2), actual };
    };
    if magic != PE32PLUS_MAGIC && magic != PE32_MAGIC {
        return PeVerdict::Suspect {
            reason: format!("optional header magic 0x{magic:04X} is neither PE32 nor PE32+"),
        };
    }
    if size_opt < MIN_OPTIONAL_HEADER {
        return PeVerdict::Suspect {
            reason: format!("optional header is only {size_opt} bytes"),
        };
    }

    // Offsets 16..70 of the optional header are identical for PE32 and PE32+.
    let (Some(entry), Some(size_of_image), Some(size_of_headers), Some(subsystem)) = (
        u32_at(buf, opt.saturating_add(16)),
        u32_at(buf, opt.saturating_add(56)),
        u32_at(buf, opt.saturating_add(60)),
        u16_at(buf, opt.saturating_add(68)),
    ) else {
        return PeVerdict::Truncated {
            expected: (opt as u64).saturating_add(MIN_OPTIONAL_HEADER as u64),
            actual,
        };
    };

    if subsystem != SUBSYSTEM_EFI_APPLICATION {
        return PeVerdict::Suspect {
            reason: format!(
                "subsystem is {subsystem} ({}), not 10 (EFI application)",
                subsystem_name(subsystem)
            ),
        };
    }
    if entry == 0 {
        return PeVerdict::Suspect { reason: "AddressOfEntryPoint is zero".into() };
    }
    if size_of_image == 0 || size_of_image > MAX_SIZE_OF_IMAGE || size_of_image < size_of_headers {
        return PeVerdict::Suspect {
            reason: format!(
                "SizeOfImage 0x{size_of_image:X} is implausible against SizeOfHeaders 0x{size_of_headers:X}"
            ),
        };
    }
    if actual < size_of_headers as u64 {
        return PeVerdict::Truncated { expected: size_of_headers as u64, actual };
    }

    // Highest section end in the file; the image must be at least this long.
    let sec_table = opt.saturating_add(size_opt as usize);
    let mut needed = size_of_headers as u64;
    for i in 0..num_sections as usize {
        let Some(off) = i.checked_mul(SECTION_ENTRY_LEN).and_then(|o| sec_table.checked_add(o))
        else {
            break;
        };
        let (Some(raw_size), Some(raw_ptr)) =
            (u32_at(buf, off.saturating_add(16)), u32_at(buf, off.saturating_add(20)))
        else {
            break;
        };
        if raw_ptr == 0 || raw_size == 0 {
            continue;
        }
        let end = (raw_ptr as u64).saturating_add(raw_size as u64);
        if end > needed {
            needed = end;
        }
    }
    if actual < needed {
        return PeVerdict::Truncated { expected: needed, actual };
    }

    PeVerdict::Valid { machine, subsystem, size_of_image }
}

fn effective_size(buf: &[u8], declared_size: u64) -> u64 {
    declared_size.max(buf.len() as u64)
}

pub fn verdict_label(v: &PeVerdict) -> &'static str {
    match v {
        PeVerdict::Valid { .. } => "valid",
        PeVerdict::Truncated { .. } => "truncated",
        PeVerdict::ZeroFilled => "zero_filled",
        PeVerdict::NotPe { .. } => "not_pe",
        PeVerdict::Suspect { .. } => "suspect",
    }
}

/// Bench-facing description of a verdict.
pub fn verdict_detail(v: &PeVerdict) -> String {
    match v {
        PeVerdict::Valid { machine, subsystem, size_of_image } => format!(
            "Structurally valid EFI application ({}, subsystem {subsystem}, image 0x{size_of_image:X}).",
            machine_name(*machine)
        ),
        PeVerdict::Truncated { expected, actual } => format!(
            "Boot binary is truncated — the PE headers describe at least {expected} bytes but the file is {actual}. Restore it with bcdboot."
        ),
        PeVerdict::ZeroFilled => {
            "Boot binary is entirely zero bytes — an interrupted write or failing flash left a placeholder. Restore it with bcdboot.".into()
        }
        PeVerdict::NotPe { reason } => {
            format!("Boot binary is not a PE image: {reason}. Restore it with bcdboot.")
        }
        PeVerdict::Suspect { reason } => {
            format!("Boot binary parses but is unsound: {reason}.")
        }
    }
}

pub fn is_valid(v: &PeVerdict) -> bool {
    matches!(v, PeVerdict::Valid { .. })
}

/// `pe_check` object for the fingerprint upload.
pub fn verdict_json(v: &PeVerdict) -> serde_json::Value {
    let mut obj = serde_json::json!({
        "verdict": verdict_label(v),
        "detail": verdict_detail(v),
    });
    if let Some(map) = obj.as_object_mut() {
        match v {
            PeVerdict::Valid { machine, subsystem, size_of_image } => {
                map.insert("machine".into(), serde_json::json!(machine));
                map.insert("subsystem".into(), serde_json::json!(subsystem));
                map.insert("size_of_image".into(), serde_json::json!(size_of_image));
            }
            PeVerdict::Truncated { expected, actual } => {
                map.insert("expected_bytes".into(), serde_json::json!(expected));
                map.insert("actual_bytes".into(), serde_json::json!(actual));
            }
            _ => {}
        }
    }
    obj
}

/// Read a file relative to an ESP root, capped at [`MAX_FILE_BYTES`].
/// Directories return [`FileBytes::Directory`], never an empty read.
pub fn read_file(root: &mut Directory, path: &uefi::CStr16) -> FileBytes {
    let Ok(handle) = root.open(path, FileMode::Read, FileAttribute::empty()) else {
        return FileBytes::Missing;
    };
    let mut file: RegularFile = match handle.into_type() {
        Ok(FileType::Regular(f)) => f,
        Ok(FileType::Dir(_)) => return FileBytes::Directory,
        Err(_) => return FileBytes::Unreadable,
    };
    let Ok(info) = file.get_boxed_info::<FileInfo>() else {
        return FileBytes::Unreadable;
    };
    let size = info.file_size();
    if size > MAX_FILE_BYTES as u64 {
        return FileBytes::TooLarge { size };
    }
    let cap = size.min(MAX_FILE_BYTES as u64) as usize;
    let mut bytes: Vec<u8> = Vec::new();
    if bytes.try_reserve_exact(cap).is_err() {
        return FileBytes::Unreadable;
    }
    bytes.resize(cap, 0);
    if file.set_position(0).is_err() {
        return FileBytes::Unreadable;
    }
    let mut filled = 0usize;
    while filled < cap {
        let Some(slice) = bytes.get_mut(filled..cap) else {
            break;
        };
        match file.read(slice) {
            Ok(0) => break,
            Ok(n) => filled = filled.saturating_add(n).min(cap),
            Err(_) => break,
        }
    }
    bytes.truncate(filled);
    FileBytes::Read { size, bytes }
}
