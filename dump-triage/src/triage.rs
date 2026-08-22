//! Triage-dump (DumpType 4) driver-list extraction.
//!
//! Layout per Microsoft's `ntiodump.h` (`TRIAGE_DUMP64`, `DUMP_DRIVER_ENTRY64`,
//! `DUMP_STRING`), cross-checked against Volatility/Rekall vtypes and verified
//! byte-for-byte against Win11 build 26100 triage dumps. Key facts:
//! - `TRIAGE_DUMP64` starts at file offset 0x2000 (right after `DMP_HEADER64`).
//! - Every `*Offset` field is an ABSOLUTE file offset.
//! - `DUMP_DRIVER_ENTRY64` embeds a `KLDR_DATA_TABLE_ENTRY` at +0x08, putting
//!   `DllBase` at +0x38, `SizeOfImage` at +0x48 and `BaseDllName` at +0x60.
//!   The stride is version-dependent — 144 on Win11 24H2+, 0x98/0xA8 on older
//!   builds — so it is derived from the list/pool gap, never assumed.
//! - `DUMP_STRING` is `u32 Length` (UTF-16 CODE UNITS, excl. NUL) then those
//!   units little-endian. Volatility's `_DUMP_STRING` and Rekall's
//!   `length=lambda x: x.Length*2` agree; reading `Length` as a byte count
//!   halves every name. Records are padded to an 8-byte boundary.
//! - `BaseDllName.Buffer` is a kernel VA that does not map into the file, so
//!   names come from the pool: via `DriverNameOffset` (+0x00) when that field
//!   is usable, else by pool order, which runs parallel to the driver list.
//!
//! Every name is resolved through the string pool and validated as a module
//! name, and every entry's `DllBase`/`SizeOfImage` must look like a loaded
//! image, so an uninitialized `DUMP_DRIVER_ENTRY64` cannot contribute a
//! plausible-looking driver. A parse that survives all that is still checked
//! against the header before it is allowed to name a culprit: entry 0 must be
//! the kernel image and its address range must contain the header's
//! `KdDebuggerDataBlock` and `PsLoadedModuleList`.

use crate::{is_plausible_module_name, DriverEntry};

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
/// `BaseDllName.Length`, in bytes — the cross-check on a resolved name.
const DE_BASE_NAME_LEN: usize = 0x60;
/// Strides to try when the list/pool gap does not yield a usable one:
/// pre-Win8 and Win8+.
const FALLBACK_STRIDES: [usize; 2] = [0x98, 0xA8];
/// A stride must at least span the embedded `KLDR_DATA_TABLE_ENTRY`.
const MIN_STRIDE: usize = DE_BASE_NAME_LEN + 0x10;
const MAX_STRIDE: usize = 0x400;

/// `DUMP_STRING` records are padded up to this boundary.
const POOL_RECORD_ALIGN: usize = 8;

/// Sanity cap on a single driver-name `DUMP_STRING`, in UTF-16 code units.
const MAX_NAME_CHARS: u32 = 512;

/// Lowest canonical kernel-mode virtual address.
const KERNEL_VA_MIN: u64 = 0xFFFF_8000_0000_0000;
const PAGE: u64 = 4096;
const MIN_IMAGE_SIZE: u64 = 4 * 1024;
const MAX_IMAGE_SIZE: u64 = 512 * 1024 * 1024;

/// Percentage of entries that must resolve for the parse to be trusted.
const MIN_RESOLVE_PCT: usize = 50;
/// Relaxed floor when `TRIAGE_OPTION_OVERFLOWED` marks the list partial.
const MIN_RESOLVE_PCT_TRUNCATED: usize = 20;
/// Absolute floor, clamped to the entry count.
const MIN_RESOLVED: usize = 2;

#[derive(Debug)]
pub struct TriageDrivers {
    pub drivers: Vec<DriverEntry>,
    pub broken_driver: Option<String>,
    /// True when `TRIAGE_OPTION_OVERFLOWED` was set — the list is best-effort.
    pub truncated: bool,
    /// Crashing thread's saved kernel stack: `(top_of_stack_va, bytes)`.
    /// Present when the triage dump embedded a call stack.
    pub call_stack: Option<(u64, Vec<u8>)>,
}

/// Absolute file range of the driver-name string pool.
#[derive(Clone, Copy)]
struct StringPool {
    off: usize,
    end: usize,
}

impl StringPool {
    fn new(off: usize, size: usize) -> Self {
        Self { off, end: off.saturating_add(size) }
    }

    /// True when `[start, start + len)` lies wholly inside the pool.
    fn holds(&self, start: usize, len: usize) -> bool {
        match start.checked_add(len) {
            Some(stop) => start >= self.off && stop <= self.end,
            None => false,
        }
    }
}

fn u16_at(buf: &[u8], off: usize) -> Option<u16> {
    buf.get(off..off + 2).map(|b| u16::from_le_bytes(b.try_into().unwrap()))
}

fn u32_at(buf: &[u8], off: usize) -> Option<u32> {
    buf.get(off..off + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()))
}

fn u64_at(buf: &[u8], off: usize) -> Option<u64> {
    buf.get(off..off + 8).map(|b| u64::from_le_bytes(b.try_into().unwrap()))
}

/// Decode `len_bytes` of UTF-16LE at `off + 4`, trimmed.
fn decode_utf16_at(data: &[u8], off: usize, len_bytes: usize) -> Option<String> {
    let chars = data.get(off + 4..off + 4 + len_bytes)?;
    let units: Vec<u16> = chars
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    let s = String::from_utf16_lossy(&units);
    let s = s.trim_end_matches('\0').trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// A `DUMP_STRING` decoded out of the pool.
struct PoolString {
    text: String,
    /// Bytes the record occupies, padded — how far to step for the next one.
    record_len: usize,
}

/// Read a `DUMP_STRING` that must live inside `pool` and name a module.
/// `Length` is a code-unit count; the byte-count reading is tried as a
/// fallback for any producer that wrote one.
fn dump_string_at(data: &[u8], off: usize, pool: &StringPool) -> Option<PoolString> {
    if off % 2 != 0 || !pool.holds(off, 4) {
        return None;
    }
    let units = u32_at(data, off)?;
    if units == 0 || units > MAX_NAME_CHARS {
        return None;
    }
    [units as usize * 2, units as usize]
        .into_iter()
        .filter(|len| *len % 2 == 0 && pool.holds(off, 4 + *len))
        .find_map(|len| {
            let text = decode_utf16_at(data, off, len)?;
            is_plausible_module_name(&text).then(|| PoolString {
                text,
                record_len: (4 + len + 2).next_multiple_of(POOL_RECORD_ALIGN),
            })
        })
}

/// Name of each entry taken from its `DriverNameOffset` (+0x00), the absolute
/// pool offset the kernel writes per entry.
fn names_by_offset(
    data: &[u8],
    list: usize,
    count: usize,
    stride: usize,
    pool: &StringPool,
) -> Vec<Option<String>> {
    (0..count)
        .map(|i| {
            let name_off = u32_at(data, list + i * stride)? as usize;
            dump_string_at(data, name_off, pool).map(|s| s.text)
        })
        .collect()
}

/// Name of each entry taken from pool order: the pool holds one record per
/// driver in driver-list order, so record `i` belongs to entry `i`. Used when
/// `DriverNameOffset` is unusable, and only once `BaseDllName.Length`
/// corroborates the alignment.
fn names_by_pool_order(data: &[u8], count: usize, pool: &StringPool) -> Vec<Option<String>> {
    let mut out = Vec::with_capacity(count);
    let mut cur = pool.off;
    while out.len() < count {
        match dump_string_at(data, cur, pool) {
            Some(s) => {
                cur += s.record_len;
                out.push(Some(s.text));
            }
            None => break,
        }
    }
    out.resize(count, None);
    out
}

/// How many resolved names agree with the entry's own `BaseDllName.Length`,
/// as `(checked, agreed)`. Entries with no length field recorded are not
/// counted either way.
fn corroborate_names(
    data: &[u8],
    list: usize,
    count: usize,
    stride: usize,
    names: &[Option<String>],
) -> (usize, usize) {
    let mut checked = 0;
    let mut agreed = 0;
    for i in 0..count {
        let Some(name) = names.get(i).and_then(Option::as_ref) else { continue };
        let Some(len) = u16_at(data, list + i * stride + DE_BASE_NAME_LEN) else { continue };
        if len == 0 {
            continue;
        }
        checked += 1;
        // BaseDllName counts the basename only.
        let base_units: usize = name
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or(name)
            .encode_utf16()
            .count();
        if len as usize == base_units * 2 {
            agreed += 1;
        }
    }
    (checked, agreed)
}

/// Basename of an NT path, lowercased.
fn nt_basename(path: &str) -> String {
    path.rsplit(['\\', '/']).next().unwrap_or(path).to_ascii_lowercase()
}

/// True when `base`/`size` look like a loaded kernel image rather than
/// uninitialized `DUMP_DRIVER_ENTRY64` bytes.
fn plausible_image(base: u64, size: u64) -> bool {
    base >= KERNEL_VA_MIN
        && base % PAGE == 0
        && (MIN_IMAGE_SIZE..=MAX_IMAGE_SIZE).contains(&size)
        && size % PAGE == 0
}

/// Entries at `stride` paired with `names`, keeping the driver-list index so
/// the header cross-check can find entry 0 after unresolvable rows drop out.
/// Used both to score a candidate stride and to emit.
fn resolve_entries(
    data: &[u8],
    list: usize,
    count: usize,
    stride: usize,
    names: &[Option<String>],
) -> Vec<(usize, DriverEntry)> {
    let mut out = Vec::new();
    for i in 0..count {
        let entry = list + i * stride;
        let Some(path) = names.get(i).and_then(Option::as_ref) else { continue };
        let base = u64_at(data, entry + DE_DLLBASE).unwrap_or(0);
        let size = u32_at(data, entry + DE_SIZEOFIMAGE).unwrap_or(0) as u64;
        if !plausible_image(base, size) {
            continue;
        }
        out.push((
            i,
            DriverEntry {
                name: nt_basename(path),
                path: path.clone(),
                base,
                size,
                timestamp: None,
            },
        ));
    }
    out
}

/// Resolve the driver list at one candidate stride, preferring the per-entry
/// `DriverNameOffset` and falling back to pool order only when the entries'
/// own `BaseDllName.Length` fields corroborate that mapping.
fn resolve_at_stride(
    data: &[u8],
    list: usize,
    count: usize,
    stride: usize,
    pool: &StringPool,
) -> Vec<(usize, DriverEntry)> {
    let by_offset = names_by_offset(data, list, count, stride, pool);
    let resolved = resolve_entries(data, list, count, stride, &by_offset);
    if resolved.len() == count {
        return resolved;
    }

    let by_order = names_by_pool_order(data, count, pool);
    let (checked, agreed) = corroborate_names(data, list, count, stride, &by_order);
    if checked == 0 || agreed != checked {
        return resolved;
    }
    let positional = resolve_entries(data, list, count, stride, &by_order);
    if positional.len() > resolved.len() {
        positional
    } else {
        resolved
    }
}

/// Reject a parse whose entry 0 is not the kernel image covering the header's
/// `KdDebuggerDataBlock` / `PsLoadedModuleList`. Both anchors sit inside
/// ntoskrnl, so a stride or field offset that is out of phase fails here
/// instead of naming an innocent driver.
fn validate_against_header(data: &[u8], entries: &[(usize, DriverEntry)]) -> Result<(), String> {
    let Some((idx, nt)) = entries.first() else {
        return Err("driver list resolved no entries".into());
    };
    if *idx != 0 {
        return Err(format!(
            "driver-list entry 0 did not resolve (first resolved index {idx}) — field offsets are wrong"
        ));
    }
    if !crate::is_kernel_image(&nt.name) {
        return Err(format!(
            "driver-list entry 0 is {:?}, expected the kernel image — field offsets are wrong",
            nt.name
        ));
    }
    let span = nt.base..nt.base.saturating_add(nt.size);
    for (label, va) in [
        ("PsLoadedModuleList", u64_at(data, 0x20).unwrap_or(0)),
        ("KdDebuggerDataBlock", u64_at(data, 0x80).unwrap_or(0)),
    ] {
        // Zero means the producer recorded no anchor.
        if va != 0 && !span.contains(&va) {
            return Err(format!(
                "{label} {va:#x} lies outside {} {:#x}..{:#x} — field offsets are wrong",
                nt.name, span.start, span.end
            ));
        }
    }
    Ok(())
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
    let pool = StringPool::new(pool_off, pool_size);

    // Measured from the list/pool gap, not matched against a table of strides.
    let derived = (pool_off > list)
        .then(|| (pool_off - list) / count)
        .filter(|d| (MIN_STRIDE..=MAX_STRIDE).contains(d));
    let mut candidates: Vec<usize> = derived.into_iter().collect();
    for s in FALLBACK_STRIDES {
        if !candidates.contains(&s) {
            candidates.push(s);
        }
    }

    let mut entries: Vec<(usize, DriverEntry)> = Vec::new();
    for stride in candidates {
        let resolved = resolve_at_stride(data, list, count, stride, &pool);
        if resolved.len() > entries.len() {
            entries = resolved;
        }
        if entries.len() == count {
            break;
        }
    }

    let floor_pct = if truncated { MIN_RESOLVE_PCT_TRUNCATED } else { MIN_RESOLVE_PCT };
    let floor_count = MIN_RESOLVED.min(count);
    if entries.len() < floor_count || entries.len() * 100 < count * floor_pct {
        return Err(format!(
            "driver list did not resolve ({} of {count} entries, need {floor_pct}% and {floor_count})",
            entries.len()
        ));
    }
    validate_against_header(data, &entries)?;

    let mut drivers: Vec<DriverEntry> = entries.into_iter().map(|(_, d)| d).collect();
    drivers.sort_by(|a, b| a.base.cmp(&b.base));

    // BrokenDriverOffset points at a DUMP_DRIVER_ENTRY64 for the blamed driver.
    let broken_driver = (broken_off >= TRIAGE_HDR_OFF)
        .then(|| u32_at(data, broken_off))
        .flatten()
        .and_then(|name_off| dump_string_at(data, name_off as usize, &pool))
        .map(|p| nt_basename(&p.text));

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
    const NT_PATH: &str = r"\SystemRoot\system32\ntoskrnl.exe";
    const RTW_PATH: &str = r"\SystemRoot\system32\drivers\rtwlane.sys";
    const NT_BASE: u64 = 0xFFFFF803_1000_0000;
    const NT_SIZE: u64 = 0x0100_0000;
    const RTW_BASE: u64 = 0xFFFFF803_2000_0000;
    const RTW_SIZE: u64 = 0x0010_0000;

    /// Where a built fixture put its driver list and string pool.
    struct Layout {
        list: usize,
        pool: usize,
        pool_size: usize,
        stride: usize,
    }

    fn put_u32(buf: &mut [u8], off: usize, v: u32) {
        buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn put_u64(buf: &mut [u8], off: usize, v: u64) {
        buf[off..off + 8].copy_from_slice(&v.to_le_bytes());
    }

    /// Write a `DUMP_STRING` at `*cursor`, returning its start offset.
    /// `len_in_bytes` selects the legacy byte-count prefix.
    fn write_dump_string(buf: &mut [u8], cursor: &mut usize, s: &str, len_in_bytes: bool) -> u32 {
        let start = *cursor;
        let units: Vec<u16> = s.encode_utf16().collect();
        let prefix = if len_in_bytes { units.len() * 2 } else { units.len() };
        put_u32(buf, *cursor, prefix as u32);
        *cursor += 4;
        for u in units {
            buf[*cursor..*cursor + 2].copy_from_slice(&u.to_le_bytes());
            *cursor += 2;
        }
        *cursor += 2; // NUL terminator
        start as u32
    }

    /// Synthesize a triage dump from `(path, base, size)` entries.
    fn synthetic_triage_from(
        entries: &[(&str, u64, u64)],
        stride: usize,
        len_in_bytes: bool,
    ) -> (Vec<u8>, Layout) {
        let mut buf = vec![0u8; 0x8000];
        buf[..4].copy_from_slice(b"PAGE");
        buf[4..8].copy_from_slice(b"DU64");
        put_u32(&mut buf, 0x30, 0x8664); // MachineImageType
        put_u32(&mut buf, 0x38, 0xD1); // BugCheckCode
        put_u64(&mut buf, 0x40, 0xFFFF_8000_0000_1000); // P1
        // RIP inside rtwlane.sys's range.
        put_u64(&mut buf, 0x348 + 0xF8, RTW_BASE + 0x100);
        put_u32(&mut buf, 0xF98, 4); // DumpType = triage

        let valid_off = 0x2080usize;
        let list = 0x2100usize;
        let count = entries.len();
        let pool = list + count * stride;
        let pool_size = 0x800usize;

        put_u32(&mut buf, 0x2000 + T_VALID_OFFSET, valid_off as u32);
        buf[valid_off..valid_off + 4].copy_from_slice(b"TRGD");
        put_u32(&mut buf, 0x2000 + T_DRIVER_LIST_OFFSET, list as u32);
        put_u32(&mut buf, 0x2000 + T_DRIVER_COUNT, count as u32);
        put_u32(&mut buf, 0x2000 + T_STRING_POOL_OFFSET, pool as u32);
        put_u32(&mut buf, 0x2000 + T_STRING_POOL_SIZE, pool_size as u32);

        let mut cursor = pool;
        for (i, (path, base, size)) in entries.iter().enumerate() {
            let name_off = write_dump_string(&mut buf, &mut cursor, path, len_in_bytes);
            let entry = list + i * stride;
            put_u32(&mut buf, entry, name_off);
            put_u64(&mut buf, entry + DE_DLLBASE, *base);
            put_u32(&mut buf, entry + DE_SIZEOFIMAGE, *size as u32);
        }

        assert!(cursor - pool <= pool_size);
        assert!(buf.len() >= DMP_HEADER64_SIZE);
        (buf, Layout { list, pool, pool_size, stride })
    }

    fn default_fixture() -> (Vec<u8>, Layout) {
        synthetic_triage_from(
            &[(NT_PATH, NT_BASE, NT_SIZE), (RTW_PATH, RTW_BASE, RTW_SIZE)],
            STRIDE,
            false,
        )
    }

    #[test]
    fn parses_synthetic_triage_drivers() {
        let (buf, _) = default_fixture();
        let t = parse_triage_drivers(&buf).unwrap();
        assert_eq!(t.drivers.len(), 2);
        assert_eq!(t.drivers[0].name, "ntoskrnl.exe");
        assert_eq!(t.drivers[1].name, "rtwlane.sys");
        assert_eq!(t.drivers[1].base, RTW_BASE);
        assert!(!t.truncated);
    }

    #[test]
    fn end_to_end_blame_via_analyze_prefix() {
        let (buf, _) = default_fixture();
        let triage = crate::analyze_prefix(&buf).unwrap();
        assert_eq!(triage.bugcheck_code, "0xd1");
        assert_eq!(triage.bugcheck_name, "DRIVER_IRQL_NOT_LESS_OR_EQUAL");
        assert_eq!(triage.rip_module.as_deref(), Some("rtwlane.sys"));
        assert_eq!(triage.blamed_module.as_deref(), Some("rtwlane.sys"));
        assert!(!triage.rip_in_kernel_image);
    }

    #[test]
    fn rejects_corrupt_magic() {
        let (mut buf, _) = default_fixture();
        let valid_off = 0x2080usize;
        buf[valid_off..valid_off + 4].copy_from_slice(b"XXXX");
        assert!(parse_triage_drivers(&buf).is_err());
    }

    /// The Bug A regression: a code-unit prefix must yield the WHOLE name,
    /// not the first half of it.
    #[test]
    fn names_are_not_halved() {
        let (buf, _) = default_fixture();
        let t = parse_triage_drivers(&buf).unwrap();
        assert_eq!(t.drivers[0].path, NT_PATH);
        assert_eq!(t.drivers[1].path, RTW_PATH);
        assert!(!t.drivers.iter().any(|d| d.name == "ntoskr" || d.name == "rtwlan"));
    }

    /// A legacy byte-count prefix still resolves through the fallback.
    #[test]
    fn byte_count_prefix_still_resolves() {
        let (buf, _) = synthetic_triage_from(
            &[(NT_PATH, NT_BASE, NT_SIZE), (RTW_PATH, RTW_BASE, RTW_SIZE)],
            STRIDE,
            true,
        );
        let t = parse_triage_drivers(&buf).unwrap();
        assert_eq!(t.drivers.len(), 2);
        assert_eq!(t.drivers[0].path, NT_PATH);
    }

    /// A name offset outside the pool is rejected even though those bytes
    /// decode to a perfectly good module name.
    #[test]
    fn name_offset_outside_the_pool_is_rejected() {
        let (mut buf, lay) = default_fixture();
        // Plant a valid DUMP_STRING past the pool end, then point at it.
        let mut outside = lay.pool + lay.pool_size + 0x100;
        assert!(outside >= lay.pool + lay.pool_size);
        let planted = write_dump_string(&mut buf, &mut outside, RTW_PATH, false);
        put_u32(&mut buf, lay.list, planted);
        let t = parse_triage_drivers(&buf);
        assert!(t.is_err(), "expected Err, got {:?}", t.map(|x| x.drivers.len()));
    }

    /// An odd name offset cannot come from a real DUMP_STRING.
    #[test]
    fn off_parity_name_offset_is_rejected() {
        let (mut buf, lay) = default_fixture();
        let good = u32_at(&buf, lay.list).unwrap();
        put_u32(&mut buf, lay.list, good + 1);
        assert!(parse_triage_drivers(&buf).is_err());
    }

    /// An all-junk list whose offsets all land inside the pool — the exact
    /// case the old in-pool-only score rated perfect — must now fail.
    #[test]
    fn all_junk_entries_inside_the_pool_error() {
        let entries: Vec<(&str, u64, u64)> = (0..6).map(|_| (NT_PATH, NT_BASE, NT_SIZE)).collect();
        let (mut buf, lay) = synthetic_triage_from(&entries, STRIDE, false);
        // Repoint every entry at pool bytes that are not a DUMP_STRING, and
        // wipe the image fields the way an uninitialized entry looks.
        for i in 0..entries.len() {
            let entry = lay.list + i * lay.stride;
            put_u32(&mut buf, entry, (lay.pool + lay.pool_size - 6) as u32);
            put_u64(&mut buf, entry + DE_DLLBASE, 0xFFFF_D09A_0000_0123);
            put_u32(&mut buf, entry + DE_SIZEOFIMAGE, 652_238_848u32);
        }
        assert!(parse_triage_drivers(&buf).is_err());
    }

    /// Junk entries are dropped while good ones survive.
    #[test]
    fn mixed_list_emits_only_resolvable_entries() {
        let entries = [
            (NT_PATH, NT_BASE, NT_SIZE),
            (RTW_PATH, RTW_BASE, RTW_SIZE),
            (r"\SystemRoot\system32\drivers\Ntfs.sys", 0xFFFFF803_3000_0000, 0x0020_0000),
            (r"\SystemRoot\system32\drivers\tm.sys", 0xFFFFF803_4000_0000, 0x0004_0000),
            (r"\SystemRoot\system32\drivers\junk1.sys", 0xFFFFF803_5000_0000, 0x0004_0000),
            (r"\SystemRoot\system32\drivers\junk2.sys", 0xFFFFF803_6000_0000, 0x0004_0000),
        ];
        let (mut buf, lay) = synthetic_triage_from(&entries, STRIDE, false);
        // Break the last two: uninitialized-looking image bounds.
        for i in 4..6 {
            let entry = lay.list + i * lay.stride;
            put_u64(&mut buf, entry + DE_DLLBASE, 0x0000_0000_0000_0001);
            put_u32(&mut buf, entry + DE_SIZEOFIMAGE, 0x0000_0007);
        }
        let t = parse_triage_drivers(&buf).unwrap();
        assert_eq!(t.drivers.len(), 4);
        assert!(!t.drivers.iter().any(|d| d.name.starts_with("junk")));
    }

    /// The 622 MB bogus size is dropped; a real 103 MB module is kept.
    #[test]
    fn image_size_bounds_drop_junk_but_keep_large_real_modules() {
        let big: u64 = 103 * 1024 * 1024;
        let entries = [
            (NT_PATH, NT_BASE, NT_SIZE),
            (r"\SystemRoot\system32\drivers\nvlddmkm.sys", RTW_BASE, big),
            (r"\SystemRoot\system32\drivers\bogus.sys", 0xFFFFF803_7000_0000, 652_238_848),
        ];
        let (buf, _) = synthetic_triage_from(&entries, STRIDE, false);
        let t = parse_triage_drivers(&buf).unwrap();
        let names: Vec<&str> = t.drivers.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"nvlddmkm.sys"), "103 MB module was dropped: {names:?}");
        assert!(!names.contains(&"bogus.sys"), "622 MB junk was kept: {names:?}");
    }

    /// OVERFLOWED keeps a resolved prefix that the strict floor would reject.
    #[test]
    fn overflowed_relaxes_the_resolve_floor() {
        let mut entries: Vec<(&str, u64, u64)> = vec![
            (NT_PATH, NT_BASE, NT_SIZE),
            (RTW_PATH, RTW_BASE, RTW_SIZE),
        ];
        for _ in 0..6 {
            entries.push((r"\SystemRoot\system32\drivers\gone.sys", 0x1, 0x7));
        }
        let (buf, _) = synthetic_triage_from(&entries, STRIDE, false);
        assert!(parse_triage_drivers(&buf).is_err(), "2 of 8 must fail the strict floor");

        let mut with_flag = buf.clone();
        put_u32(&mut with_flag, 0x2000 + T_TRIAGE_OPTIONS, TRIAGE_OPTION_OVERFLOWED);
        let t = parse_triage_drivers(&with_flag).unwrap();
        assert_eq!(t.drivers.len(), 2);
        assert!(t.truncated);
    }

    /// BrokenDriverOffset resolves through the pool-bounded reader, and an
    /// out-of-pool target yields None rather than junk.
    #[test]
    fn broken_driver_respects_the_pool() {
        let (mut buf, lay) = default_fixture();
        let rtw_name_off = u32_at(&buf, lay.list + lay.stride).unwrap();
        let broken_entry = 0x2C00usize;
        put_u32(&mut buf, broken_entry, rtw_name_off);
        put_u32(&mut buf, 0x2000 + T_BROKEN_DRIVER_OFFSET, broken_entry as u32);
        let t = parse_triage_drivers(&buf).unwrap();
        assert_eq!(t.broken_driver.as_deref(), Some("rtwlane.sys"));

        put_u32(&mut buf, broken_entry, 0x3000);
        let t = parse_triage_drivers(&buf).unwrap();
        assert_eq!(t.broken_driver, None);
    }

    /// Win11 24H2+ (build 26100/26200) triage layout, reproduced from real
    /// `C:\Windows\Minidump` dumps: 144-byte entries embedding a
    /// `KLDR_DATA_TABLE_ENTRY` at +0x08, absolute offsets, and a string pool of
    /// base-name `DUMP_STRING`s padded to 8 bytes.
    mod build_26100 {
        use super::*;

        pub const STRIDE_26100: usize = 144;
        pub const NT_BASE_26100: u64 = 0xFFFF_F807_9540_0000;
        pub const NT_SIZE_26100: u64 = 0x0145_0000;
        /// Both anchors sit inside ntoskrnl, as they do in a real dump.
        pub const PS_LOADED_MODULE_LIST: u64 = NT_BASE_26100 + 0x00EF_51D0;
        pub const KD_DEBUGGER_DATA_BLOCK: u64 = NT_BASE_26100 + 0x00E0_1040;
        pub const CULPRIT: &str = "rcbottom.sys";
        pub const CULPRIT_BASE: u64 = 0xFFFF_F807_A100_0000;
        pub const CULPRIT_SIZE: u64 = 0x0001_2000;

        pub fn modules() -> Vec<(&'static str, u64, u64)> {
            vec![
                ("ntoskrnl.exe", NT_BASE_26100, NT_SIZE_26100),
                ("hal.dll", 0xFFFF_F807_96C0_0000, 0x6000),
                ("kdcom.dll", 0xFFFF_F807_26E0_0000, 0xB000),
                ("mcupdate.dll", 0xFFFF_F807_9700_0000, 0x9_0000),
                ("Ntfs.sys", 0xFFFF_F807_9800_0000, 0x20_0000),
                (CULPRIT, CULPRIT_BASE, CULPRIT_SIZE),
            ]
        }

        /// Write a pool record: u32 code-unit count, the units, a NUL, padded
        /// up to the 8-byte boundary the kernel aligns records to.
        fn write_pool_record(buf: &mut [u8], cursor: &mut usize, s: &str) -> u32 {
            let start = *cursor;
            let units: Vec<u16> = s.encode_utf16().collect();
            put_u32(buf, *cursor, units.len() as u32);
            *cursor += 4;
            for u in &units {
                buf[*cursor..*cursor + 2].copy_from_slice(&u.to_le_bytes());
                *cursor += 2;
            }
            *cursor += 2;
            *cursor = start + (*cursor - start).next_multiple_of(8);
            start as u32
        }

        /// Build a build-26100-shaped triage dump. `rip` lands wherever the
        /// caller wants blame to fall.
        pub fn fixture(rip: u64) -> Vec<u8> {
            let mods = modules();
            let mut buf = vec![0u8; 0x1_0000];
            buf[..4].copy_from_slice(b"PAGE");
            buf[4..8].copy_from_slice(b"DU64");
            put_u64(&mut buf, 0x20, PS_LOADED_MODULE_LIST);
            put_u32(&mut buf, 0x30, 0x8664);
            put_u32(&mut buf, 0x38, 0xD1);
            put_u64(&mut buf, 0x80, KD_DEBUGGER_DATA_BLOCK);
            put_u64(&mut buf, 0x348 + 0xF8, rip);
            put_u32(&mut buf, 0xF98, 4);

            let list = 0x2100usize;
            let count = mods.len();
            let pool = list + count * STRIDE_26100;
            let pool_size = 0x200usize;

            let valid_off = 0x20F0usize;
            put_u32(&mut buf, 0x2000 + T_VALID_OFFSET, valid_off as u32);
            buf[valid_off..valid_off + 4].copy_from_slice(b"TRGD");
            put_u32(&mut buf, 0x2000 + T_DRIVER_LIST_OFFSET, list as u32);
            put_u32(&mut buf, 0x2000 + T_DRIVER_COUNT, count as u32);
            put_u32(&mut buf, 0x2000 + T_STRING_POOL_OFFSET, pool as u32);
            put_u32(&mut buf, 0x2000 + T_STRING_POOL_SIZE, pool_size as u32);

            let mut cursor = pool;
            for (i, (name, base, size)) in mods.iter().enumerate() {
                let name_off = write_pool_record(&mut buf, &mut cursor, name);
                let entry = list + i * STRIDE_26100;
                put_u32(&mut buf, entry, name_off);
                put_u64(&mut buf, entry + DE_DLLBASE, *base);
                put_u64(&mut buf, entry + 0x40, *base); // EntryPoint
                put_u32(&mut buf, entry + DE_SIZEOFIMAGE, *size as u32);
                // BaseDllName UNICODE_STRING: byte lengths, then a kernel-VA Buffer.
                let bytes = (name.encode_utf16().count() * 2) as u16;
                buf[entry + DE_BASE_NAME_LEN..entry + DE_BASE_NAME_LEN + 2]
                    .copy_from_slice(&bytes.to_le_bytes());
                buf[entry + DE_BASE_NAME_LEN + 2..entry + DE_BASE_NAME_LEN + 4]
                    .copy_from_slice(&bytes.to_le_bytes());
                put_u64(&mut buf, entry + 0x68, 0xFFFF_9883_1069_FD80);
            }
            assert!(cursor - pool <= pool_size, "pool overflowed the declared size");
            buf
        }
    }
    use build_26100::*;

    /// The shipped regression: a 144-byte stride was not in the stride table,
    /// so every field landed out of phase and the driver list came back empty.
    #[test]
    fn parses_build_26100_triage_layout() {
        let buf = fixture(CULPRIT_BASE + 0x1234);
        let t = parse_triage_drivers(&buf).unwrap();
        assert_eq!(t.drivers.len(), modules().len(), "every entry must resolve");
        let names: Vec<&str> = t.drivers.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"ntoskrnl.exe"), "{names:?}");
        assert!(names.contains(&CULPRIT), "{names:?}");
        let nt = t.drivers.iter().find(|d| d.name == "ntoskrnl.exe").unwrap();
        assert_eq!(nt.base, NT_BASE_26100);
        assert_eq!(nt.size, NT_SIZE_26100);
    }

    /// The stride is measured from the list/pool gap, so a value absent from
    /// the fallback table still parses.
    #[test]
    fn stride_is_derived_not_matched_against_a_table() {
        assert!(!FALLBACK_STRIDES.contains(&STRIDE_26100));
        let buf = fixture(CULPRIT_BASE);
        assert_eq!(parse_triage_drivers(&buf).unwrap().drivers.len(), modules().len());
    }

    /// End to end, a 0xD1 whose RIP is inside a third-party driver names it —
    /// the case that previously reported no blame at all.
    #[test]
    fn build_26100_dump_blames_the_driver_at_rip() {
        let triage = crate::analyze_prefix(&fixture(CULPRIT_BASE + 0x1234)).unwrap();
        assert_eq!(triage.bugcheck_code, "0xd1");
        assert_eq!(triage.rip_module.as_deref(), Some(CULPRIT));
        assert_eq!(triage.blamed_module.as_deref(), Some(CULPRIT));
        assert!(!triage.rip_in_kernel_image);
        assert_eq!(triage.drivers.len(), modules().len());
        assert!(triage.warnings.is_empty(), "{:?}", triage.warnings);
    }

    /// Guard: nt's range must contain the header's anchors. An out-of-phase
    /// parse puts nt somewhere else, and blame from it would be fiction.
    #[test]
    fn anchors_outside_the_kernel_image_reject_the_parse() {
        for anchor in [0x20usize, 0x80] {
            let mut buf = fixture(CULPRIT_BASE);
            put_u64(&mut buf, anchor, NT_BASE_26100 + NT_SIZE_26100 + 0x1000);
            let err = parse_triage_drivers(&buf).unwrap_err();
            assert!(
                err.contains("field offsets are wrong"),
                "anchor {anchor:#x} should be rejected, got {err:?}"
            );
        }
    }

    /// Guard: entry 0 of a triage driver list is always the kernel image.
    #[test]
    fn non_kernel_first_entry_rejects_the_parse() {
        let mut buf = fixture(CULPRIT_BASE);
        // Swap entry 0's name for a driver's, leaving the anchors alone.
        let list = 0x2100usize;
        let culprit_name_off = u32_at(&buf, list + 5 * STRIDE_26100).unwrap();
        put_u32(&mut buf, list, culprit_name_off);
        let err = parse_triage_drivers(&buf).unwrap_err();
        assert!(err.contains("expected the kernel image"), "got {err:?}");
    }

    /// `BaseDllName.Buffer` is a kernel VA, so when `DriverNameOffset` is junk
    /// the names come from pool order — accepted only because every entry's
    /// `BaseDllName.Length` matches the record it is paired with.
    #[test]
    fn pool_order_resolves_names_when_offsets_are_unusable() {
        let mut buf = fixture(CULPRIT_BASE + 0x1234);
        let list = 0x2100usize;
        for i in 0..modules().len() {
            put_u32(&mut buf, list + i * STRIDE_26100, 0xDEAD_BEEF);
        }
        let t = parse_triage_drivers(&buf).unwrap();
        assert_eq!(t.drivers.len(), modules().len());
        let by_base: Vec<&str> = {
            let mut v: Vec<&DriverEntry> = t.drivers.iter().collect();
            v.sort_by_key(|d| d.base);
            v.into_iter().map(|d| d.name.as_str()).collect()
        };
        assert!(by_base.contains(&CULPRIT), "{by_base:?}");
    }

    /// Pool order is not trusted on its own: with the corroborating
    /// `BaseDllName.Length` fields cleared, junk offsets fail rather than
    /// pairing names with whatever entry happens to sit at that index.
    #[test]
    fn pool_order_is_rejected_without_length_corroboration() {
        let mut buf = fixture(CULPRIT_BASE);
        let list = 0x2100usize;
        for i in 0..modules().len() {
            put_u32(&mut buf, list + i * STRIDE_26100, 0xDEAD_BEEF);
            put_u32(&mut buf, list + i * STRIDE_26100 + DE_BASE_NAME_LEN, 0);
        }
        assert!(parse_triage_drivers(&buf).is_err());
    }

    /// A name paired with the wrong entry is caught by the length cross-check.
    #[test]
    fn length_cross_check_catches_an_off_by_one_pairing() {
        let mut buf = fixture(CULPRIT_BASE);
        let list = 0x2100usize;
        let mods = modules();
        for i in 0..mods.len() {
            put_u32(&mut buf, list + i * STRIDE_26100, 0xDEAD_BEEF);
            // Shift each entry's declared name length one slot along.
            let wrong = (mods[(i + 1) % mods.len()].0.encode_utf16().count() * 2) as u16;
            buf[list + i * STRIDE_26100 + DE_BASE_NAME_LEN
                ..list + i * STRIDE_26100 + DE_BASE_NAME_LEN + 2]
                .copy_from_slice(&wrong.to_le_bytes());
        }
        assert!(parse_triage_drivers(&buf).is_err());
    }

    /// An unreadable driver list must say so. An empty `drivers` with no
    /// warning reads as "no third-party driver involved", which is how a
    /// driver-caused BSOD got filed as unattributed.
    #[test]
    fn unreadable_driver_list_warns_instead_of_reporting_no_drivers() {
        let mut buf = fixture(CULPRIT_BASE);
        // Point the driver list at the PAGE padding the old parser read.
        put_u32(&mut buf, 0x2000 + T_DRIVER_LIST_OFFSET, 0x1000);
        let triage = crate::analyze_prefix(&buf).unwrap();
        assert!(triage.drivers.is_empty());
        assert_eq!(triage.blamed_module, None);
        assert_eq!(triage.warnings.len(), 1, "{:?}", triage.warnings);
        assert!(
            triage.warnings[0].contains("not an absence of third-party drivers"),
            "{:?}",
            triage.warnings
        );
    }

    /// Opt-in pass over real `C:\Windows\Minidump` dumps:
    /// `DUMP_TRIAGE_TEST_DUMPS=<dir> cargo test -p dump-triage -- --ignored`
    #[test]
    #[ignore = "needs real dumps; set DUMP_TRIAGE_TEST_DUMPS"]
    fn real_triage_dumps_resolve_their_driver_lists() {
        let Ok(dir) = std::env::var("DUMP_TRIAGE_TEST_DUMPS") else {
            panic!("set DUMP_TRIAGE_TEST_DUMPS to a directory of .dmp files");
        };
        let mut checked = 0;
        for entry in std::fs::read_dir(&dir).expect("read dump dir") {
            let path = entry.expect("dir entry").path();
            if path.extension().is_none_or(|e| !e.eq_ignore_ascii_case("dmp")) {
                continue;
            }
            let triage = crate::analyze_file(&path).expect("analyze");
            if triage.dump_type != 4 {
                continue;
            }
            checked += 1;
            assert!(triage.warnings.is_empty(), "{path:?}: {:?}", triage.warnings);
            assert!(!triage.drivers.is_empty(), "{path:?}: empty driver list");
            assert!(
                triage.drivers.iter().any(|d| crate::is_kernel_image(&d.name)),
                "{path:?}: no kernel image in the driver list"
            );
            assert!(triage.rip_module.is_some(), "{path:?}: RIP matched no module");
        }
        assert!(checked > 0, "no DumpType 4 dumps found in {dir}");
    }
}
