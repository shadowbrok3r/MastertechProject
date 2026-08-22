//! Module list, stack scan, and memory peek for full/BMP/kernel/live dumps via
//! `kdmp-parser`. Triage dumps (DumpType 4) are rejected by kdmp-parser and
//! handled by the fixed-offset parser in [`crate::triage`] instead.

use crate::{scan_stack_bytes, DriverEntry, HexRegion, KernelExtras};
use kdmp_parser::gxa::{Gva, Gxa};
use kdmp_parser::parse::KernelDumpParser;
use kdmp_parser::virt::Reader;
use std::path::Path;

/// Stack bytes to scan for return-address candidates.
const STACK_SCAN_BYTES: usize = 0x2000;
/// Bytes captured around RIP / at RSP for the hex view.
const RIP_WINDOW: usize = 0x80;
const RSP_WINDOW: usize = 0x100;

/// Basename of an NT path like `\SystemRoot\system32\drivers\foo.sys`.
fn nt_basename(path: &str) -> String {
    path.rsplit(['\\', '/']).next().unwrap_or(path).to_ascii_lowercase()
}

/// Read up to `len` bytes at `va`, tolerating pages missing from the dump.
fn read_region(reader: &Reader, va: u64, len: usize) -> Option<HexRegion> {
    let mut buf = vec![0u8; len];
    match reader.read(Gva::new(va), &mut buf) {
        Ok(n) if n > 0 => {
            buf.truncate(n);
            Some(HexRegion { base: va, bytes: buf })
        }
        _ => None,
    }
}

/// Module list for a non-triage kernel dump. Standalone entry used when only
/// the driver list is needed.
pub fn kernel_modules(path: &Path) -> Result<Vec<DriverEntry>, String> {
    let parser = KernelDumpParser::new(path)
        .map_err(|e| format!("kdmp-parser failed to open dump: {e}"))?;
    Ok(collect_modules(&parser))
}

fn collect_modules(parser: &KernelDumpParser) -> Vec<DriverEntry> {
    let mut out: Vec<DriverEntry> = parser
        .kernel_modules()
        .map(|(range, name)| DriverEntry {
            name: nt_basename(name),
            path: name.to_string(),
            base: range.start.u64(),
            size: range.end.u64().saturating_sub(range.start.u64()),
            timestamp: None,
        })
        .collect();
    out.sort_by(|a, b| a.base.cmp(&b.base));
    out
}

/// Full crash detail: modules + a scanned stack (from RSP memory) + hex windows
/// around RIP and at RSP.
pub fn crash_detail(
    path: &Path,
    rip: Option<u64>,
    rsp: Option<u64>,
) -> Result<(Vec<DriverEntry>, KernelExtras), String> {
    let parser = KernelDumpParser::new(path)
        .map_err(|e| format!("kdmp-parser failed to open dump: {e}"))?;
    let modules = collect_modules(&parser);
    let reader = Reader::new(&parser);

    let mut scanned_stack = Vec::new();
    if let Some(rsp) = rsp {
        let mut buf = vec![0u8; STACK_SCAN_BYTES];
        if let Ok(n) = reader.read(Gva::new(rsp), &mut buf) {
            buf.truncate(n);
            scanned_stack = scan_stack_bytes(&buf, Some(rsp), &modules);
        }
    }

    let rip_region = rip.and_then(|a| read_region(&reader, a.saturating_sub(0x20), RIP_WINDOW));
    let rsp_region = rsp.and_then(|a| read_region(&reader, a, RSP_WINDOW));

    Ok((
        modules,
        KernelExtras { scanned_stack, rip_region, rsp_region, warnings: Vec::new() },
    ))
}
