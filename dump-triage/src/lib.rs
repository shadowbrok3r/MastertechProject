//! Windows kernel crash-dump (BSOD) triage.
//!
//! Parses the fixed-offset `DMP_HEADER64` present in every 64-bit kernel
//! dump (bugcheck code + parameters, crash-time RIP, dump type, system
//! time), the serialized driver list embedded in triage minidumps
//! (`C:\Windows\Minidump\*.dmp`, DumpType 4), and — behind the `kdmp`
//! feature — the `PsLoadedModuleList` of full/BMP/kernel/live dumps via
//! `kdmp-parser`. User-mode `MDMP` dumps are out of scope (rust-minidump's
//! job); [`sniff_format`] tells the two apart.
//!
//! All parsing is slice-based so the same code runs natively and inside a
//! WASI plugin that fetches byte ranges remotely.

pub mod bugcheck;
pub mod header;
pub mod triage;

#[cfg(feature = "kdmp")]
pub mod kdmp;

use serde::{Deserialize, Serialize};
use std::path::Path;

pub use header::{
    dump_type_name, parse_kernel_header, sniff_format, DumpFormat, KernelDumpHeader,
    DMP_HEADER64_SIZE,
};

/// One frame of a scanned stack walk. `trust` is `"context"` for the crash
/// RIP and `"scan"` for a value found on the stack that lands inside a module.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScannedFrame {
    /// Stack address the return value was found at (None for the RIP frame).
    pub stack_addr: Option<u64>,
    pub ret_addr: u64,
    pub module: String,
    pub offset: u64,
    pub trust: String,
}

/// A block of dump memory for a hex view.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HexRegion {
    pub base: u64,
    pub bytes: Vec<u8>,
}

/// Registers/stack/memory extracted beyond the header — feature-gated to the
/// dump types that carry it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KernelExtras {
    pub scanned_stack: Vec<ScannedFrame>,
    pub rip_region: Option<HexRegion>,
    pub rsp_region: Option<HexRegion>,
}

/// Scan a little-endian buffer for 8-byte values that fall inside a module's
/// address range — the WinDbg-style "scanned" stack fallback.
pub fn scan_stack_bytes(
    bytes: &[u8],
    base_va: Option<u64>,
    drivers: &[DriverEntry],
) -> Vec<ScannedFrame> {
    let mut out = Vec::new();
    let mut off = 0;
    while off + 8 <= bytes.len() {
        let val = u64::from_le_bytes(bytes[off..off + 8].try_into().unwrap());
        if val >= 0xFFFF_0000_0000_0000 {
            if let Some(d) = drivers
                .iter()
                .find(|d| val >= d.base && val < d.base.saturating_add(d.size))
            {
                out.push(ScannedFrame {
                    stack_addr: base_va.map(|b| b + off as u64),
                    ret_addr: val,
                    module: d.name.clone(),
                    offset: val - d.base,
                    trust: "scan".to_string(),
                });
            }
        }
        off += 8;
    }
    out
}

/// One loaded kernel module/driver from a dump.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriverEntry {
    /// Lowercased file name, e.g. `rtwlane.sys`.
    pub name: String,
    /// Full NT path as recorded in the dump.
    pub path: String,
    pub base: u64,
    pub size: u64,
    /// PE TimeDateStamp when available (triage driver list carries it).
    pub timestamp: Option<u32>,
}

/// Complete triage result for a kernel dump.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelDumpTriage {
    pub dump_type: u32,
    pub dump_type_name: String,
    /// Normalized like crash-intel expects: `0x133`.
    pub bugcheck_code: String,
    pub bugcheck_name: String,
    pub bugcheck_parameters: Vec<String>,
    /// Documented meaning of the parameters for the common codes.
    pub parameter_notes: Vec<String>,
    pub rip: Option<String>,
    pub rsp: Option<String>,
    pub exception_code: Option<String>,
    pub number_processors: u32,
    /// AMD64 GP/control registers from the crash CONTEXT, formatted as hex.
    pub registers: Vec<(String, String)>,
    /// Unix seconds of the crash wall-clock time.
    pub system_time_unix: Option<i64>,
    pub uptime_secs: Option<i64>,
    pub comment: Option<String>,
    pub drivers: Vec<DriverEntry>,
    /// Module whose address range contains the crash-time RIP.
    pub rip_module: Option<String>,
    /// True when RIP fell inside the kernel image itself (ntoskrnl/hal) —
    /// varied-process nt faults with no recurring third-party module is the
    /// classic bad-RAM pattern rather than a driver bug.
    pub rip_in_kernel_image: bool,
    /// Best single-module blame: the RIP module unless that is the kernel
    /// image, else the triage header's BrokenDriver entry when present.
    pub blamed_module: Option<String>,
    /// Scanned-stack backtrace (RIP context frame + stack scan hits).
    pub scanned_stack: Vec<ScannedFrame>,
    /// Raw bytes around RIP (full/BMP/live dumps only).
    pub rip_region: Option<HexRegion>,
    /// Raw bytes at RSP (full/BMP/live dumps only).
    pub rsp_region: Option<HexRegion>,
}

fn is_kernel_image(name: &str) -> bool {
    matches!(
        name,
        "ntoskrnl.exe" | "ntkrnlmp.exe" | "ntkrnlpa.exe" | "ntkrpamp.exe" | "hal.dll"
    )
}

/// Assemble a [`KernelDumpTriage`] from parsed header + driver list + extras.
pub fn build_triage(
    header: &KernelDumpHeader,
    drivers: Vec<DriverEntry>,
    broken_driver: Option<String>,
    extras: KernelExtras,
) -> KernelDumpTriage {
    let rip_module = header.rip.and_then(|rip| {
        drivers
            .iter()
            .find(|d| rip >= d.base && rip < d.base.saturating_add(d.size))
            .map(|d| d.name.clone())
    });
    let rip_in_kernel_image = rip_module.as_deref().is_some_and(is_kernel_image);
    let blamed_module = match &rip_module {
        Some(m) if !is_kernel_image(m) => Some(m.clone()),
        _ => broken_driver.or_else(|| rip_module.clone()),
    };

    // Lead the backtrace with the crash RIP as a "context" frame.
    let mut scanned_stack = extras.scanned_stack;
    if let (Some(rip), Some(module)) = (header.rip, &rip_module) {
        let base = drivers.iter().find(|d| d.name == *module).map(|d| d.base).unwrap_or(0);
        scanned_stack.insert(
            0,
            ScannedFrame {
                stack_addr: None,
                ret_addr: rip,
                module: module.clone(),
                offset: rip.saturating_sub(base),
                trust: "context".to_string(),
            },
        );
    }

    KernelDumpTriage {
        dump_type: header.dump_type,
        dump_type_name: dump_type_name(header.dump_type).to_string(),
        bugcheck_code: format!("{:#x}", header.bugcheck_code),
        bugcheck_name: bugcheck::bugcheck_name(header.bugcheck_code).to_string(),
        bugcheck_parameters: header
            .bugcheck_parameters
            .iter()
            .map(|p| format!("{p:#x}"))
            .collect(),
        parameter_notes: bugcheck::parameter_notes(
            header.bugcheck_code,
            &header.bugcheck_parameters,
        ),
        rip: header.rip.map(|r| format!("{r:#x}")),
        rsp: header.rsp.map(|r| format!("{r:#x}")),
        exception_code: (header.exception_code != 0)
            .then(|| format!("{:#x}", header.exception_code)),
        number_processors: header.number_processors,
        registers: header
            .registers
            .iter()
            .map(|(n, v)| (n.clone(), format!("{v:#018x}")))
            .collect(),
        system_time_unix: header.system_time_unix,
        uptime_secs: header.uptime_secs,
        comment: header.comment.clone(),
        drivers,
        rip_module,
        rip_in_kernel_image,
        blamed_module,
        scanned_stack,
        rip_region: extras.rip_region,
        rsp_region: extras.rsp_region,
    }
}

/// Triage a kernel dump from an in-memory prefix (or whole file). Driver
/// lists are extracted for triage dumps when the slice covers them; other
/// dump types get header-only results from this entry point.
pub fn analyze_prefix(data: &[u8]) -> Result<KernelDumpTriage, String> {
    let header = parse_kernel_header(data)?;
    let (drivers, broken, extras) = if header.dump_type == 4 {
        // Header-only result beats a hard failure on a corrupt driver list.
        match triage::parse_triage_drivers(data) {
            Ok(t) => {
                let scanned = t
                    .call_stack
                    .as_ref()
                    .map(|(top, bytes)| scan_stack_bytes(bytes, Some(*top), &t.drivers))
                    .unwrap_or_default();
                (t.drivers, t.broken_driver, KernelExtras { scanned_stack: scanned, ..Default::default() })
            }
            Err(_) => (Vec::new(), None, KernelExtras::default()),
        }
    } else {
        (Vec::new(), None, KernelExtras::default())
    };
    Ok(build_triage(&header, drivers, broken, extras))
}

/// Triage a kernel dump file on disk. Triage dumps are read whole (they are
/// 1–2 MB); larger dump types read only the header, then walk
/// `PsLoadedModuleList` via kdmp-parser when the `kdmp` feature is on.
pub fn analyze_file(path: &Path) -> Result<KernelDumpTriage, String> {
    use std::io::Read;

    let mut file = std::fs::File::open(path).map_err(|e| format!("open {path:?}: {e}"))?;
    let mut prefix = vec![0u8; DMP_HEADER64_SIZE];
    file.read_exact(&mut prefix)
        .map_err(|e| format!("read header of {path:?}: {e}"))?;
    let header = parse_kernel_header(&prefix)?;

    if header.dump_type == 4 {
        let mut rest = Vec::new();
        file.read_to_end(&mut rest)
            .map_err(|e| format!("read {path:?}: {e}"))?;
        prefix.extend_from_slice(&rest);
        return analyze_prefix(&prefix);
    }

    // Full/BMP/kernel/live/complete: walk PsLoadedModuleList and read the
    // crash stack + memory around RIP/RSP via kdmp-parser's page-table walker.
    #[cfg(feature = "kdmp")]
    let (drivers, extras) =
        kdmp::crash_detail(path, header.rip, header.rsp).unwrap_or_default();
    #[cfg(not(feature = "kdmp"))]
    let (drivers, extras) = (Vec::new(), KernelExtras::default());

    Ok(build_triage(&header, drivers, None, extras))
}
