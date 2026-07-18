//! Triage-dump (DumpType 4) driver-list extraction.
//!
//! Layout per Microsoft's `ntiodump.h` (`TRIAGE_DUMP64`, `DUMP_DRIVER_ENTRY64`,
//! `DUMP_STRING`), cross-checked against Volatility/Rekall vtypes and the
//! dumplib 010 template. Key facts:
//! - `TRIAGE_DUMP64` starts at file offset 0x2000 (right after `DMP_HEADER64`).
//! - Every `*Offset` field is an ABSOLUTE file offset.
//! - `DUMP_DRIVER_ENTRY64` stride is version-dependent (0x98 pre-Win8, 0xA8
//!   Win8+) but `DriverNameOffset`(+0x00), `DllBase`(+0x38) and
//!   `SizeOfImage`(+0x48) are version-stable.
//! - `DUMP_STRING` is `u32 Length` (BYTES, excl. NUL) then UTF-16LE chars.

use crate::DriverEntry;

const TRIAGE_HDR_OFF: usize = 0x2000;

const T_VALID_OFFSET: usize = 0x08;
const T_CALL_STACK_OFFSET: usize = 0x28;
const T_SIZE_OF_CALL_STACK: usize = 0x2C;
const T_DRIVER_LIST_OFFSET: usize = 0x30;
const T_DRIVER_COUNT: usize = 0x34;
const T_STRING_POOL_OFFSET: usize = 0x38;
const T_STRING_POOL_SIZE: usize = 0x3C;
const T_BROKEN_DRIVER_OFFSET: usize = 0x40;
const T_TRIAGE_OPTIONS: usize = 0x44;
const T_TOP_OF_STACK: usize = 0x48;

/// `TriageOptions` bit set when the driver list was truncated.
const TRIAGE_OPTION_OVERFLOWED: u32 = 0x0100;

const DE_DLLBASE: usize = 0x38;
const DE_SIZEOFIMAGE: usize = 0x48;
/// Known `DUMP_DRIVER_ENTRY64` strides: pre-Win8 and Win8+.
const KNOWN_STRIDES: [usize; 2] = [0x98, 0xA8];

/// Sanity cap on a single driver-name `DUMP_STRING` (bytes).
const MAX_NAME_BYTES: u32 = 1024;

pub struct TriageDrivers {
    pub drivers: Vec<DriverEntry>,
    pub broken_driver: Option<String>,
    /// True when `TRIAGE_OPTION_OVERFLOWED` was set — the list is best-effort.
    pub truncated: bool,
    /// Crashing thread's saved kernel stack: `(top_of_stack_va, bytes)`.
    /// Present when the triage dump embedded a call stack.
    pub call_stack: Option<(u64, Vec<u8>)>,
}

fn u32_at(buf: &[u8], off: usize) -> Option<u32> {
    buf.get(off..off + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()))
}

fn u64_at(buf: &[u8], off: usize) -> Option<u64> {
    buf.get(off..off + 8).map(|b| u64::from_le_bytes(b.try_into().unwrap()))
}

/// Read a `DUMP_STRING` at an absolute file offset: `u32` byte length then
/// UTF-16LE characters.
fn dump_string_at(data: &[u8], off: usize) -> Option<String> {
    let len_bytes = u32_at(data, off)?;
    if len_bytes == 0 || len_bytes > MAX_NAME_BYTES || len_bytes % 2 != 0 {
        return None;
    }
    let chars = data.get(off + 4..off + 4 + len_bytes as usize)?;
    let units: Vec<u16> = chars
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let s = String::from_utf16_lossy(&units);
    let s = s.trim_end_matches('\0').trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// Basename of an NT path, lowercased.
fn nt_basename(path: &str) -> String {
    path.rsplit(['\\', '/']).next().unwrap_or(path).to_ascii_lowercase()
}

/// Fraction of driver entries whose name offset lands inside the string pool —
/// used to pick the right entry stride when it can't be derived exactly.
fn stride_score(data: &[u8], list: usize, count: usize, stride: usize, pool: (usize, usize)) -> usize {
    (0..count)
        .filter(|i| {
            u32_at(data, list + i * stride)
                .map(|name_off| {
                    let name_off = name_off as usize;
                    name_off >= pool.0 && name_off < pool.0 + pool.1
                })
                .unwrap_or(false)
        })
        .count()
}

/// Parse the serialized driver list of a triage dump. `data` must contain the
/// file from offset 0 through the end of the string pool (triage dumps are
/// only 1–2 MB, so callers usually pass the whole file).
pub fn parse_triage_drivers(data: &[u8]) -> Result<TriageDrivers, String> {
    let t = |field: usize| TRIAGE_HDR_OFF + field;

    // TRIAGE_DUMP_VALID magic ('DGRT') at ValidOffset, byte order defensive.
    let valid_offset = u32_at(data, t(T_VALID_OFFSET)).ok_or("triage header out of range")? as usize;
    match data.get(valid_offset..valid_offset + 4) {
        Some(m) if m == b"TRGD" || m == b"DGRT" => {}
        Some(_) => return Err("TRIAGE_DUMP_VALID magic mismatch (corrupt triage dump?)".into()),
        None => return Err("ValidOffset out of range".into()),
    }

    let list = u32_at(data, t(T_DRIVER_LIST_OFFSET)).unwrap_or(0) as usize;
    let count = u32_at(data, t(T_DRIVER_COUNT)).unwrap_or(0) as usize;
    let pool_off = u32_at(data, t(T_STRING_POOL_OFFSET)).unwrap_or(0) as usize;
    let pool_size = u32_at(data, t(T_STRING_POOL_SIZE)).unwrap_or(0) as usize;
    let broken_off = u32_at(data, t(T_BROKEN_DRIVER_OFFSET)).unwrap_or(0) as usize;
    let options = u32_at(data, t(T_TRIAGE_OPTIONS)).unwrap_or(0);
    let truncated = options & TRIAGE_OPTION_OVERFLOWED != 0;

    if list < TRIAGE_HDR_OFF || count == 0 || count > 4096 {
        return Err(format!("implausible driver list (offset {list:#x}, count {count})"));
    }
    if pool_off < TRIAGE_HDR_OFF || pool_size == 0 || pool_off + pool_size > data.len() {
        return Err(format!(
            "string pool out of range (offset {pool_off:#x}, size {pool_size:#x}, have {:#x})",
            data.len()
        ));
    }

    // Entry stride varies by Windows version; derive it from the gap between
    // list and pool, falling back to whichever known stride resolves the most
    // name offsets into the pool.
    let derived = (pool_off > list).then(|| (pool_off - list) / count);
    let mut candidates: Vec<usize> = Vec::new();
    if let Some(d) = derived {
        if KNOWN_STRIDES.contains(&d) {
            candidates.push(d);
        }
    }
    for s in KNOWN_STRIDES {
        if !candidates.contains(&s) {
            candidates.push(s);
        }
    }
    let stride = candidates
        .into_iter()
        .max_by_key(|s| stride_score(data, list, count, *s, (pool_off, pool_size)))
        .unwrap_or(0xA8);

    let mut drivers = Vec::with_capacity(count);
    for i in 0..count {
        let entry = list + i * stride;
        let Some(name_off) = u32_at(data, entry) else { break };
        let Some(path) = dump_string_at(data, name_off as usize) else { continue };
        let base = u64_at(data, entry + DE_DLLBASE).unwrap_or(0);
        let size = u32_at(data, entry + DE_SIZEOFIMAGE).unwrap_or(0) as u64;
        drivers.push(DriverEntry {
            name: nt_basename(&path),
            path,
            base,
            size,
            timestamp: None,
        });
    }
    if drivers.is_empty() {
        return Err("driver list parsed to zero entries".into());
    }
    drivers.sort_by(|a, b| a.base.cmp(&b.base));

    // BrokenDriverOffset points at a DUMP_DRIVER_ENTRY64 for the blamed driver.
    let broken_driver = (broken_off >= TRIAGE_HDR_OFF)
        .then(|| u32_at(data, broken_off))
        .flatten()
        .and_then(|name_off| dump_string_at(data, name_off as usize))
        .map(|p| nt_basename(&p));

    // Crashing thread's saved kernel stack, for a scanned-frame backtrace.
    let call_stack = {
        let cs_off = u32_at(data, t(T_CALL_STACK_OFFSET)).unwrap_or(0) as usize;
        let cs_size = u32_at(data, t(T_SIZE_OF_CALL_STACK)).unwrap_or(0) as usize;
        let top = u64_at(data, t(T_TOP_OF_STACK)).unwrap_or(0);
        if cs_off >= TRIAGE_HDR_OFF && cs_size >= 8 && cs_off + cs_size <= data.len() {
            Some((top, data[cs_off..cs_off + cs_size].to_vec()))
        } else {
            None
        }
    };

    Ok(TriageDrivers { drivers, broken_driver, truncated, call_stack })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::header::DMP_HEADER64_SIZE;

    const STRIDE: usize = 0xA8;

    fn put_u32(buf: &mut [u8], off: usize, v: u32) {
        buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn put_u64(buf: &mut [u8], off: usize, v: u64) {
        buf[off..off + 8].copy_from_slice(&v.to_le_bytes());
    }

    /// Synthesize a minimal triage dump: header, two-driver list, string pool.
    fn synthetic_triage() -> Vec<u8> {
        let mut buf = vec![0u8; 0x4000];
        buf[..4].copy_from_slice(b"PAGE");
        buf[4..8].copy_from_slice(b"DU64");
        put_u32(&mut buf, 0x30, 0x8664); // MachineImageType
        put_u32(&mut buf, 0x38, 0xD1); // BugCheckCode
        put_u64(&mut buf, 0x40, 0xFFFF_8000_0000_1000); // P1
        // RIP inside rtwlane.sys's range.
        put_u64(&mut buf, 0x348 + 0xF8, 0xFFFFF803_2000_0100);
        put_u32(&mut buf, 0xF98, 4); // DumpType = triage

        let valid_off = 0x2080usize;
        let list = 0x2100usize;
        let count = 2usize;
        let pool = list + count * STRIDE;

        put_u32(&mut buf, 0x2000 + T_VALID_OFFSET, valid_off as u32);
        buf[valid_off..valid_off + 4].copy_from_slice(b"TRGD");
        put_u32(&mut buf, 0x2000 + T_DRIVER_LIST_OFFSET, list as u32);
        put_u32(&mut buf, 0x2000 + T_DRIVER_COUNT, count as u32);
        put_u32(&mut buf, 0x2000 + T_STRING_POOL_OFFSET, pool as u32);
        put_u32(&mut buf, 0x2000 + T_STRING_POOL_SIZE, 0x200);

        let mut pool_cursor = pool;
        let mut write_name = |buf: &mut [u8], s: &str| -> u32 {
            let start = pool_cursor;
            let units: Vec<u16> = s.encode_utf16().collect();
            put_u32(buf, pool_cursor, (units.len() * 2) as u32);
            pool_cursor += 4;
            for u in units {
                buf[pool_cursor..pool_cursor + 2].copy_from_slice(&u.to_le_bytes());
                pool_cursor += 2;
            }
            pool_cursor += 2; // NUL terminator
            start as u32
        };

        let name0 = write_name(&mut buf, r"\SystemRoot\system32\ntoskrnl.exe");
        let name1 = write_name(&mut buf, r"\SystemRoot\system32\drivers\rtwlane.sys");

        put_u32(&mut buf, list, name0);
        put_u64(&mut buf, list + DE_DLLBASE, 0xFFFFF803_1000_0000);
        put_u32(&mut buf, list + DE_SIZEOFIMAGE, 0x0100_0000);

        put_u32(&mut buf, list + STRIDE, name1);
        put_u64(&mut buf, list + STRIDE + DE_DLLBASE, 0xFFFFF803_2000_0000);
        put_u32(&mut buf, list + STRIDE + DE_SIZEOFIMAGE, 0x0010_0000);

        assert!(pool_cursor - pool <= 0x200);
        assert!(buf.len() >= DMP_HEADER64_SIZE);
        buf
    }

    #[test]
    fn parses_synthetic_triage_drivers() {
        let buf = synthetic_triage();
        let t = parse_triage_drivers(&buf).unwrap();
        assert_eq!(t.drivers.len(), 2);
        assert_eq!(t.drivers[0].name, "ntoskrnl.exe");
        assert_eq!(t.drivers[1].name, "rtwlane.sys");
        assert_eq!(t.drivers[1].base, 0xFFFFF803_2000_0000);
        assert!(!t.truncated);
    }

    #[test]
    fn end_to_end_blame_via_analyze_prefix() {
        let buf = synthetic_triage();
        let triage = crate::analyze_prefix(&buf).unwrap();
        assert_eq!(triage.bugcheck_code, "0xd1");
        assert_eq!(triage.bugcheck_name, "DRIVER_IRQL_NOT_LESS_OR_EQUAL");
        assert_eq!(triage.rip_module.as_deref(), Some("rtwlane.sys"));
        assert_eq!(triage.blamed_module.as_deref(), Some("rtwlane.sys"));
        assert!(!triage.rip_in_kernel_image);
    }

    #[test]
    fn rejects_corrupt_magic() {
        let mut buf = synthetic_triage();
        let valid_off = 0x2080usize;
        buf[valid_off..valid_off + 4].copy_from_slice(b"XXXX");
        assert!(parse_triage_drivers(&buf).is_err());
    }
}
