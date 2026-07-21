//! Fixed-offset parsing of the Windows kernel crash-dump header
//! (`DMP_HEADER64`, signature `PAGE` + `DU64`). All fields live in the first
//! 0x2000 bytes of every 64-bit kernel dump regardless of dump type, so
//! bugcheck triage needs only a small prefix read — no full-dump parser.
//!
//! Offsets cross-checked against Volatility's crash address space
//! (`_DMP_HEADER64`) and kdmp-parser's `Header64`.

use serde::{Deserialize, Serialize};

pub const DMP_HEADER64_SIZE: usize = 0x2000;

const OFF_DIRECTORY_TABLE_BASE: usize = 0x10;
const OFF_PS_LOADED_MODULE_LIST: usize = 0x20;
const OFF_MACHINE_IMAGE_TYPE: usize = 0x30;
const OFF_NUMBER_PROCESSORS: usize = 0x34;
const OFF_BUGCHECK_CODE: usize = 0x38;
const OFF_BUGCHECK_PARAMS: usize = 0x40;
const OFF_KD_DEBUGGER_DATA_BLOCK: usize = 0x80;
const OFF_CONTEXT_RECORD: usize = 0x348;
const OFF_EXCEPTION_RECORD: usize = 0xF00;
const OFF_DUMP_TYPE: usize = 0xF98;
const OFF_SYSTEM_TIME: usize = 0xFA8;
const OFF_COMMENT: usize = 0xFB0;
const COMMENT_LEN: usize = 128;
const OFF_SYSTEM_UP_TIME: usize = 0x1030;

/// `Rip` offset inside an AMD64 `CONTEXT` record.
const CONTEXT_OFF_RIP: usize = 0xF8;
/// `Rsp` offset inside an AMD64 `CONTEXT` record.
const CONTEXT_OFF_RSP: usize = 0x98;

/// AMD64 `CONTEXT` general-purpose + control register offsets (winnt.h layout),
/// in display order.
const AMD64_GP_REGISTERS: &[(&str, usize)] = &[
    ("rax", 0x78),
    ("rbx", 0x90),
    ("rcx", 0x80),
    ("rdx", 0x88),
    ("rsi", 0xA8),
    ("rdi", 0xB0),
    ("rbp", 0xA0),
    ("rsp", 0x98),
    ("r8", 0xB8),
    ("r9", 0xC0),
    ("r10", 0xC8),
    ("r11", 0xD0),
    ("r12", 0xD8),
    ("r13", 0xE0),
    ("r14", 0xE8),
    ("r15", 0xF0),
    ("rip", 0xF8),
];
const CONTEXT_OFF_EFLAGS: usize = 0x44;

/// Extract the AMD64 GP/control registers from a `CONTEXT` record at `blob`.
fn read_amd64_registers(blob: &[u8]) -> Vec<(String, u64)> {
    let mut regs: Vec<(String, u64)> = AMD64_GP_REGISTERS
        .iter()
        .filter_map(|(name, off)| u64_at(blob, *off).map(|v| (name.to_string(), v)))
        .collect();
    if let Some(ef) = u32_at(blob, CONTEXT_OFF_EFLAGS) {
        regs.push(("eflags".to_string(), ef as u64));
    }
    regs
}

/// Byte-signature classification of a dump file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DumpFormat {
    /// Breakpad/user-mode minidump (`MDMP`) — rust-minidump territory.
    UserMinidump,
    /// 64-bit kernel dump (`PAGE` + `DU64`) — full, BMP, kernel, live, or triage.
    Kernel64,
    /// 32-bit kernel dump (`PAGE` + `DUMP`) — legacy, unsupported here.
    Kernel32,
    Unknown,
}

impl DumpFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::UserMinidump => "user_minidump",
            Self::Kernel64 => "kernel64",
            Self::Kernel32 => "kernel32",
            Self::Unknown => "unknown",
        }
    }
}

/// Classify a dump by its first 8 bytes.
pub fn sniff_format(prefix: &[u8]) -> DumpFormat {
    if prefix.len() >= 4 && &prefix[..4] == b"MDMP" {
        return DumpFormat::UserMinidump;
    }
    if prefix.len() >= 8 && &prefix[..4] == b"PAGE" {
        return match &prefix[4..8] {
            b"DU64" => DumpFormat::Kernel64,
            b"DUMP" => DumpFormat::Kernel32,
            _ => DumpFormat::Unknown,
        };
    }
    DumpFormat::Unknown
}

/// `DumpType` values written by the kernel.
pub fn dump_type_name(dump_type: u32) -> &'static str {
    match dump_type {
        1 => "full",
        4 => "triage_minidump",
        5 => "bitmap_kernel",
        6 => "live_kernel",
        8 => "kernel_memory",
        9 => "kernel_and_user_memory",
        0xA => "complete_memory",
        _ => "unknown",
    }
}

/// Parsed `DMP_HEADER64` fields relevant to bugcheck triage.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "facet", derive(facet::Facet))]
pub struct KernelDumpHeader {
    pub bugcheck_code: u32,
    pub bugcheck_parameters: [u64; 4],
    pub dump_type: u32,
    /// x86_64 = 0x8664, arm64 = 0xAA64, x86 = 0x14C.
    pub machine_image_type: u32,
    pub number_processors: u32,
    pub directory_table_base: u64,
    pub ps_loaded_module_list: u64,
    pub kd_debugger_data_block: u64,
    /// Instruction pointer from the embedded crash-time CONTEXT (AMD64 only).
    pub rip: Option<u64>,
    pub rsp: Option<u64>,
    /// AMD64 GP/control registers from the crash CONTEXT (empty on non-AMD64).
    pub registers: Vec<(String, u64)>,
    pub exception_code: u32,
    pub exception_address: u64,
    /// Crash wall-clock time as unix seconds (from the FILETIME field).
    pub system_time_unix: Option<i64>,
    /// Uptime at crash, in seconds.
    pub uptime_secs: Option<i64>,
    pub comment: Option<String>,
}

fn u32_at(buf: &[u8], off: usize) -> Option<u32> {
    buf.get(off..off + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()))
}

fn u64_at(buf: &[u8], off: usize) -> Option<u64> {
    buf.get(off..off + 8).map(|b| u64::from_le_bytes(b.try_into().unwrap()))
}

/// FILETIME (100ns ticks since 1601-01-01) → unix seconds.
fn filetime_to_unix(ft: u64) -> Option<i64> {
    const EPOCH_DELTA_100NS: u64 = 116_444_736_000_000_000;
    if ft == 0 || ft == u64::MAX || ft < EPOCH_DELTA_100NS {
        return None;
    }
    Some(((ft - EPOCH_DELTA_100NS) / 10_000_000) as i64)
}

/// Parse the fixed-offset header fields from a 64-bit kernel dump prefix.
/// `prefix` must contain at least the first [`DMP_HEADER64_SIZE`] bytes.
pub fn parse_kernel_header(prefix: &[u8]) -> Result<KernelDumpHeader, String> {
    match sniff_format(prefix) {
        DumpFormat::Kernel64 => {}
        DumpFormat::Kernel32 => {
            return Err("32-bit kernel dump (PAGEDUMP) is not supported".into())
        }
        other => return Err(format!("not a 64-bit kernel dump (format: {})", other.as_str())),
    }
    if prefix.len() < DMP_HEADER64_SIZE {
        return Err(format!(
            "need at least {DMP_HEADER64_SIZE:#x} header bytes, got {:#x}",
            prefix.len()
        ));
    }

    let mut params = [0u64; 4];
    for (i, p) in params.iter_mut().enumerate() {
        *p = u64_at(prefix, OFF_BUGCHECK_PARAMS + i * 8).unwrap_or(0);
    }

    let machine_image_type = u32_at(prefix, OFF_MACHINE_IMAGE_TYPE).unwrap_or(0);
    // CONTEXT layout is architecture-specific; only decode registers for AMD64.
    let (rip, rsp, registers) = if machine_image_type == 0x8664 {
        let blob = prefix.get(OFF_CONTEXT_RECORD..).unwrap_or(&[]);
        (
            u64_at(prefix, OFF_CONTEXT_RECORD + CONTEXT_OFF_RIP).filter(|v| *v != 0),
            u64_at(prefix, OFF_CONTEXT_RECORD + CONTEXT_OFF_RSP).filter(|v| *v != 0),
            read_amd64_registers(blob),
        )
    } else {
        (None, None, Vec::new())
    };

    let comment = prefix
        .get(OFF_COMMENT..OFF_COMMENT + COMMENT_LEN)
        .map(|b| {
            let end = b.iter().position(|c| *c == 0).unwrap_or(b.len());
            String::from_utf8_lossy(&b[..end]).trim().to_string()
        })
        .filter(|s| !s.is_empty());

    Ok(KernelDumpHeader {
        bugcheck_code: u32_at(prefix, OFF_BUGCHECK_CODE).unwrap_or(0),
        bugcheck_parameters: params,
        dump_type: u32_at(prefix, OFF_DUMP_TYPE).unwrap_or(0),
        machine_image_type,
        number_processors: u32_at(prefix, OFF_NUMBER_PROCESSORS).unwrap_or(0),
        directory_table_base: u64_at(prefix, OFF_DIRECTORY_TABLE_BASE).unwrap_or(0),
        ps_loaded_module_list: u64_at(prefix, OFF_PS_LOADED_MODULE_LIST).unwrap_or(0),
        kd_debugger_data_block: u64_at(prefix, OFF_KD_DEBUGGER_DATA_BLOCK).unwrap_or(0),
        rip,
        rsp,
        registers,
        exception_code: u32_at(prefix, OFF_EXCEPTION_RECORD).unwrap_or(0),
        exception_address: u64_at(prefix, OFF_EXCEPTION_RECORD + 0x10).unwrap_or(0),
        system_time_unix: u64_at(prefix, OFF_SYSTEM_TIME).and_then(filetime_to_unix),
        uptime_secs: u64_at(prefix, OFF_SYSTEM_UP_TIME)
            .filter(|v| *v != 0 && *v != u64::MAX)
            .map(|v| (v / 10_000_000) as i64),
        comment,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a synthetic PAGEDU64 header with known field values.
    pub(crate) fn synthetic_header() -> Vec<u8> {
        let mut buf = vec![0u8; DMP_HEADER64_SIZE];
        buf[..4].copy_from_slice(b"PAGE");
        buf[4..8].copy_from_slice(b"DU64");
        buf[OFF_MACHINE_IMAGE_TYPE..OFF_MACHINE_IMAGE_TYPE + 4]
            .copy_from_slice(&0x8664u32.to_le_bytes());
        buf[OFF_NUMBER_PROCESSORS..OFF_NUMBER_PROCESSORS + 4]
            .copy_from_slice(&16u32.to_le_bytes());
        buf[OFF_BUGCHECK_CODE..OFF_BUGCHECK_CODE + 4].copy_from_slice(&0x133u32.to_le_bytes());
        buf[OFF_BUGCHECK_PARAMS..OFF_BUGCHECK_PARAMS + 8].copy_from_slice(&1u64.to_le_bytes());
        buf[OFF_BUGCHECK_PARAMS + 8..OFF_BUGCHECK_PARAMS + 16]
            .copy_from_slice(&0x1E00u64.to_le_bytes());
        buf[OFF_CONTEXT_RECORD + CONTEXT_OFF_RIP..OFF_CONTEXT_RECORD + CONTEXT_OFF_RIP + 8]
            .copy_from_slice(&0xFFFFF803_1234_5678u64.to_le_bytes());
        buf[OFF_DUMP_TYPE..OFF_DUMP_TYPE + 4].copy_from_slice(&4u32.to_le_bytes());
        // 2026-01-01T00:00:00Z as FILETIME.
        let ft: u64 = 116_444_736_000_000_000 + 1_767_225_600 * 10_000_000;
        buf[OFF_SYSTEM_TIME..OFF_SYSTEM_TIME + 8].copy_from_slice(&ft.to_le_bytes());
        buf
    }

    #[test]
    fn sniffs_formats() {
        assert_eq!(sniff_format(b"MDMP\x93\xa7\x00\x00"), DumpFormat::UserMinidump);
        assert_eq!(sniff_format(b"PAGEDU64"), DumpFormat::Kernel64);
        assert_eq!(sniff_format(b"PAGEDUMP"), DumpFormat::Kernel32);
        assert_eq!(sniff_format(b"junkjunk"), DumpFormat::Unknown);
    }

    #[test]
    fn parses_synthetic_header() {
        let h = parse_kernel_header(&synthetic_header()).unwrap();
        assert_eq!(h.bugcheck_code, 0x133);
        assert_eq!(h.bugcheck_parameters[0], 1);
        assert_eq!(h.bugcheck_parameters[1], 0x1E00);
        assert_eq!(h.dump_type, 4);
        assert_eq!(h.machine_image_type, 0x8664);
        assert_eq!(h.number_processors, 16);
        assert_eq!(h.rip, Some(0xFFFFF803_1234_5678));
        assert_eq!(h.system_time_unix, Some(1_767_225_600));
    }

    #[test]
    fn rejects_short_and_wrong_format() {
        assert!(parse_kernel_header(b"PAGEDU64").is_err());
        assert!(parse_kernel_header(&vec![0u8; DMP_HEADER64_SIZE]).is_err());
    }
}
