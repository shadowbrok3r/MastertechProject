//! SATA/ATA SMART collection over `EFI_ATA_PASS_THRU`.
//!
//! Reads SMART READ DATA (0xB0/0xD0) and SMART READ THRESHOLDS (0xB0/0xD1) as
//! PIO data-in transfers, pairs attributes with thresholds by id, and reduces
//! each attribute to an explicit judgment so the renderer never re-derives a
//! verdict from a raw number. Absence of SMART is reported as its own status,
//! never as health.
//!
//! All reads are best-effort and never panic.

use core::time::Duration;
use std::alloc::{Layout, alloc_zeroed, dealloc};

use uefi::Status;
use uefi::boot::{self, OpenProtocolAttributes, OpenProtocolParams};
use uefi::proto::ata::pass_thru::AtaPassThru;
use uefi_raw::protocol::ata::{
    AtaCommandBlock, AtaPassThruCommandPacket, AtaPassThruCommandProtocol, AtaPassThruLength,
    AtaPassThruProtocol, AtaStatusBlock,
};

use crate::logln;
use crate::stress::{calibrate_tsc_hz, rdtsc};

/// Deadline for a SMART page read; blocks the UI thread for its duration.
const CMD_TIMEOUT: Duration = Duration::from_secs(3);
/// Deadline for the IDENTIFY probe issued at every enumerated address.
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
/// Aggregate deadline for the whole ATA walk, across every controller.
const SCAN_BUDGET: Duration = Duration::from_secs(15);
/// Upper bound on the hand-rolled port walk.
const MAX_PORTS: usize = 32;
/// Upper bound on the hand-rolled port-multiplier walk per port.
const MAX_PMP: usize = 16;
/// Upper bound on device addresses probed across all ports.
const MAX_DEVICES: usize = 16;
/// Sector size of every page this module reads.
const PAGE_LEN: usize = 512;
/// Largest I/O alignment accepted from firmware.
const MAX_IO_ALIGN: usize = 4096;
/// Attribute and threshold entries per page.
const ENTRY_COUNT: usize = 30;
/// Byte width of one attribute or threshold entry.
const ENTRY_LEN: usize = 12;
/// First entry offset; the two preceding bytes are the page revision.
const ENTRY_BASE: usize = 2;

/// Register set for one PIO data-in command.
struct PioCmd {
    command: u8,
    features: u8,
    lba_mid: u8,
    lba_high: u8,
    timeout: Duration,
}

/// ATA IDENTIFY DEVICE.
const IDENTIFY: PioCmd = PioCmd {
    command: 0xEC,
    features: 0,
    lba_mid: 0,
    lba_high: 0,
    timeout: PROBE_TIMEOUT,
};
/// ATA SMART READ DATA, with the SMART signature in LBA mid/high.
const SMART_READ_DATA: PioCmd = PioCmd {
    command: 0xB0,
    features: 0xD0,
    lba_mid: 0x4F,
    lba_high: 0xC2,
    timeout: CMD_TIMEOUT,
};
/// ATA SMART READ THRESHOLDS.
const SMART_READ_THRESH: PioCmd = PioCmd {
    features: 0xD1,
    ..SMART_READ_DATA
};

/// A single device address on one ATA controller.
struct Dev {
    raw: *mut AtaPassThruProtocol,
    port: u16,
    pmp: u16,
    align: usize,
}

/// One enumerated address and whether the controller positively reported it.
struct Addr {
    port: u16,
    pmp: u16,
    reported: bool,
}

/// TSC deadline bounding the whole walk.
struct Budget {
    deadline: u64,
}

impl Budget {
    fn new(d: Duration) -> Self {
        let hz = calibrate_tsc_hz().max(1);
        Self {
            deadline: rdtsc().saturating_add(hz.saturating_mul(d.as_secs())),
        }
    }

    fn spent(&self) -> bool {
        rdtsc() >= self.deadline
    }
}

/// Tag carried by every attribute that the alarm gate cannot evaluate.
pub const RAW_ONLY_TAG: &str = "raw-only (no normalized value) - not a failure";
/// Note emitted once per drive when SandForce-class raw aliasing is detected.
pub const SANDFORCE_NOTE: &str =
    "SandForce-class controller: raw fields aliased, raw values not meaningful";

/// Outcome of the SMART read for one drive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SmartStatus {
    /// SMART READ DATA succeeded; `attrs` is populated.
    Read,
    /// The drive or host adapter rejected the SMART command.
    Unsupported,
    /// No `AtaPassThru` handle exists (RST/RAID mode, or a USB-SATA bridge).
    NoHandle,
    /// The command did not complete within `CMD_TIMEOUT`.
    Timeout,
    /// Any other failure, with the firmware status or device error.
    Error(String),
}

impl SmartStatus {
    /// Human-readable status text.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::Read => "SMART read".to_string(),
            Self::Unsupported => "SMART not supported by drive or adapter".to_string(),
            Self::NoHandle => {
                "no ATA pass-thru handle (RAID/RST mode or USB bridge) - SMART unavailable"
                    .to_string()
            }
            Self::Timeout => "SMART read timed out".to_string(),
            Self::Error(e) => format!("SMART read failed: {e}"),
        }
    }

    /// True when attribute data was actually read from the drive.
    #[must_use]
    pub const fn has_data(&self) -> bool {
        matches!(self, Self::Read)
    }
}

/// Verdict for one attribute, decided once at collection time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AttrJudgment {
    /// Pre-fail attribute at or below its non-zero threshold.
    Failing,
    /// Pre-fail attribute above its non-zero threshold.
    Prefail,
    /// Populated attribute that cannot fail.
    Ok,
    /// Normalized value is reserved or unthresholded; only the raw counter means anything.
    RawOnly,
    /// Normalized value is zero, i.e. the drive never filled it in.
    NotPopulated,
}

impl AttrJudgment {
    /// True only for the one verdict that may be shown as a failure.
    #[must_use]
    pub const fn is_alarm(&self) -> bool {
        matches!(self, Self::Failing)
    }

    /// Tag for attributes the alarm gate cannot evaluate.
    #[must_use]
    pub const fn note(&self) -> Option<&'static str> {
        match self {
            Self::RawOnly | Self::NotPopulated => Some(RAW_ONLY_TAG),
            _ => None,
        }
    }

    /// Short verdict text.
    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Failing => "FAILING",
            Self::Prefail => "pre-fail (passing)",
            Self::Ok => "ok",
            Self::RawOnly => "raw-only",
            Self::NotPopulated => "not populated",
        }
    }
}

/// One SMART attribute paired with its threshold and verdict.
#[derive(Clone, Debug)]
pub struct SmartAttr {
    pub id: u8,
    pub name: String,
    pub flags: u16,
    pub value: u8,
    pub worst: u8,
    pub threshold: Option<u8>,
    pub raw: u64,
    pub judgment: AttrJudgment,
    pub note: Option<String>,
}

impl SmartAttr {
    /// True when the attribute is flagged pre-fail (flags bit 0).
    #[must_use]
    pub const fn is_prefail_flagged(&self) -> bool {
        (self.flags & 0x0001) != 0
    }
}

/// One ATA device reached through an `AtaPassThru` controller.
#[derive(Clone, Debug)]
pub struct SataDrive {
    pub model: String,
    pub serial: String,
    pub firmware: String,
    pub capacity: u64,
    pub port: u16,
    pub pmp: u16,
    pub attrs: Vec<SmartAttr>,
    pub quirk: Option<String>,
    pub status: SmartStatus,
    /// SMART READ THRESHOLDS completed.
    pub thresholds_read: bool,
    /// SMART RETURN STATUS result, when the drive answered it.
    pub threshold_exceeded: Option<bool>,
}

impl SataDrive {
    /// Attributes that cleared the alarm gate, minus the drive's aliased raw fields.
    pub fn alarm_attrs(&self) -> impl Iterator<Item = &SmartAttr> {
        self.attrs
            .iter()
            .filter(|a| a.judgment.is_alarm() && !is_aliased(self, a.id))
    }

    /// True when at least one attribute cleared the alarm gate.
    #[must_use]
    pub fn has_alarm(&self) -> bool {
        self.alarm_attrs().next().is_some()
    }

    /// True when the drive itself, or a judged attribute, reports a failure.
    #[must_use]
    pub fn is_failing(&self) -> bool {
        self.threshold_exceeded == Some(true) || self.has_alarm()
    }

    /// Why the alarm gate could not run on this drive, if it could not.
    #[must_use]
    pub fn unverified_reason(&self) -> Option<String> {
        if !self.status.has_data() {
            return Some(self.status.label());
        }
        if self.attrs.is_empty() {
            return Some("SMART page returned no attributes".to_string());
        }
        if self.threshold_exceeded.is_some() {
            return None;
        }
        if !self.thresholds_read {
            return Some("SMART thresholds unreadable - attributes not judged".to_string());
        }
        if !self.attrs.iter().any(|a| a.threshold.is_some()) {
            return Some("SMART thresholds page carries no entries - attributes not judged".to_string());
        }
        None
    }

    /// True when the drive answered enough for health to be asserted.
    #[must_use]
    pub fn is_verified(&self) -> bool {
        self.unverified_reason().is_none()
    }
}

/// True for the raw counters a SandForce-class controller aliases together.
#[must_use]
pub fn is_aliased(d: &SataDrive, id: u8) -> bool {
    d.quirk.is_some() && SANDFORCE_IDS.contains(&id)
}

/// Attribute names for the ids that matter on a bench; everything else is `attr NNN`.
const ATTR_NAMES: &[(u8, &str)] = &[
    (1, "Raw Read Error Rate"),
    (5, "Reallocated Sectors Count"),
    (9, "Power-On Hours"),
    (10, "Spin Retry Count"),
    (12, "Power Cycle Count"),
    (171, "Program Fail Count"),
    (172, "Erase Fail Count"),
    (174, "Unexpected Power Loss Count"),
    (177, "Wear Leveling Count"),
    (181, "Program Fail Count Total"),
    (182, "Erase Fail Count Total"),
    (184, "End-to-End Error"),
    (187, "Reported Uncorrectable Errors"),
    (188, "Command Timeout"),
    (190, "Airflow Temperature"),
    (194, "Temperature"),
    (195, "Hardware ECC Recovered"),
    (196, "Reallocation Event Count"),
    (197, "Current Pending Sector Count"),
    (198, "Offline Uncorrectable Sector Count"),
    (199, "UltraDMA CRC Error Count"),
    (231, "SSD Life Left"),
    (232, "Available Reserved Space"),
    (233, "Media Wearout Indicator"),
    (241, "Total LBAs Written"),
    (242, "Total LBAs Read"),
];

/// Attribute ids SandForce-class controllers alias onto one raw field.
const SANDFORCE_IDS: [u8; 4] = [1, 195, 201, 204];

fn attr_name(id: u8) -> String {
    match ATTR_NAMES.iter().find(|(k, _)| *k == id) {
        Some((_, n)) => (*n).to_string(),
        None => format!("attr {id}"),
    }
}

/// Heap block with a firmware-requested alignment, freed on drop.
struct AlignedBuf {
    ptr: *mut u8,
    layout: Layout,
}

impl AlignedBuf {
    fn new(len: usize, align: usize) -> Option<Self> {
        if len == 0 {
            return None;
        }
        let layout = Layout::from_size_align(len, align).ok()?;
        let ptr = unsafe { alloc_zeroed(layout) };
        if ptr.is_null() {
            None
        } else {
            Some(Self { ptr, layout })
        }
    }

    fn as_slice(&self) -> &[u8] {
        unsafe { core::slice::from_raw_parts(self.ptr, self.layout.size()) }
    }
}

impl Drop for AlignedBuf {
    fn drop(&mut self) {
        unsafe { dealloc(self.ptr, self.layout) };
    }
}

/// Alignment required by the controller, clamped to a usable power of two.
fn io_align(raw: *mut AtaPassThruProtocol) -> usize {
    let mode = unsafe { (*raw).mode };
    if mode.is_null() {
        return 1;
    }
    let a = unsafe { (*mode).io_align } as usize;
    let a = if a == 0 { 1 } else { a };
    if a.is_power_of_two() && a <= MAX_IO_ALIGN {
        a
    } else {
        1
    }
}

/// Reads a function-pointer field as a nullable address.
unsafe fn fn_addr<T>(p: *const T) -> *const () {
    unsafe { p.cast::<*const ()>().read_unaligned() }
}

/// False when the protocol structure carries a null function pointer.
fn vtable_ok(raw: *mut AtaPassThruProtocol) -> bool {
    unsafe {
        !fn_addr(&raw const (*raw).pass_thru).is_null()
            && !fn_addr(&raw const (*raw).get_next_port).is_null()
            && !fn_addr(&raw const (*raw).get_next_device).is_null()
    }
}

/// Bounded port / port-multiplier walk; any unexpected status ends the list.
fn enumerate_devices(raw: *mut AtaPassThruProtocol, budget: &Budget) -> Vec<Addr> {
    let mut found = Vec::new();
    let mut port: u16 = 0xFFFF;
    for _ in 0..MAX_PORTS {
        if budget.spent() {
            break;
        }
        let st = unsafe { ((*raw).get_next_port)(raw, &mut port) };
        if st != Status::SUCCESS {
            break;
        }
        let mut pmp: u16 = 0xFFFF;
        let mut first = true;
        for _ in 0..MAX_PMP {
            let prev = pmp;
            let st = unsafe { ((*raw).get_next_device)(raw, port, &mut pmp) };
            if st == Status::SUCCESS {
                if !first && pmp == prev {
                    break;
                }
                found.push(Addr {
                    port,
                    pmp,
                    reported: true,
                });
                if pmp == 0xFFFF || found.len() >= MAX_DEVICES {
                    break;
                }
            } else if st == Status::NOT_FOUND {
                if first {
                    found.push(Addr {
                        port,
                        pmp: 0xFFFF,
                        reported: false,
                    });
                }
                break;
            } else {
                break;
            }
            first = false;
        }
        if found.len() >= MAX_DEVICES {
            break;
        }
    }
    found
}

/// Issues one PIO data-in command and returns the transferred page.
fn pio_in(dev: &Dev, cmd: &PioCmd) -> Result<Vec<u8>, SmartStatus> {
    let data = match AlignedBuf::new(PAGE_LEN, dev.align) {
        Some(b) => b,
        None => return Err(SmartStatus::Error("buffer allocation failed".to_string())),
    };
    let asb = match AlignedBuf::new(size_of::<AtaStatusBlock>(), dev.align) {
        Some(b) => b,
        None => return Err(SmartStatus::Error("buffer allocation failed".to_string())),
    };
    let acb = AtaCommandBlock {
        command: cmd.command,
        features: cmd.features,
        cylinder_low: cmd.lba_mid,
        cylinder_high: cmd.lba_high,
        sector_count: 1,
        ..Default::default()
    };
    let mut packet = AtaPassThruCommandPacket {
        asb: asb.ptr.cast(),
        acb: &acb,
        timeout: (cmd.timeout.as_nanos() / 100) as u64,
        in_data_buffer: data.ptr.cast(),
        out_data_buffer: core::ptr::null(),
        in_transfer_length: PAGE_LEN as u32,
        out_transfer_length: 0,
        protocol: AtaPassThruCommandProtocol::PIO_DATA_IN,
        length: AtaPassThruLength::BYTES,
    };
    let raw = dev.raw;
    let st =
        unsafe { ((*raw).pass_thru)(raw, dev.port, dev.pmp, &mut packet, core::ptr::null_mut()) };
    if st != Status::SUCCESS {
        return Err(status_to_smart(st));
    }
    // AtaStatusBlock is repr(C) all-u8: [2]=status, [3]=error.
    let sb = asb.as_slice();
    let dev_status = sb.get(2).copied().unwrap_or(0);
    let dev_error = sb.get(3).copied().unwrap_or(0);
    if (dev_status & 0x21) != 0 {
        return Err(SmartStatus::Error(format!(
            "device status 0x{dev_status:02X} error 0x{dev_error:02X}"
        )));
    }
    Ok(data.as_slice().to_vec())
}

/// SMART RETURN STATUS; `Some(true)` when the drive reports a threshold exceeded.
fn smart_return_status(dev: &Dev) -> Option<bool> {
    let asb = AlignedBuf::new(size_of::<AtaStatusBlock>(), dev.align)?;
    let acb = AtaCommandBlock {
        command: 0xB0,
        features: 0xDA,
        cylinder_low: 0x4F,
        cylinder_high: 0xC2,
        ..Default::default()
    };
    let mut packet = AtaPassThruCommandPacket {
        asb: asb.ptr.cast(),
        acb: &acb,
        timeout: (CMD_TIMEOUT.as_nanos() / 100) as u64,
        in_data_buffer: core::ptr::null_mut(),
        out_data_buffer: core::ptr::null(),
        in_transfer_length: 0,
        out_transfer_length: 0,
        protocol: AtaPassThruCommandProtocol::ATA_NON_DATA,
        length: AtaPassThruLength::NO_DATA_TRANSFER,
    };
    let raw = dev.raw;
    let st =
        unsafe { ((*raw).pass_thru)(raw, dev.port, dev.pmp, &mut packet, core::ptr::null_mut()) };
    if st != Status::SUCCESS {
        return None;
    }
    // AtaStatusBlock is repr(C) all-u8: [5]=cylinder_low, [6]=cylinder_high.
    let sb = asb.as_slice();
    match (sb.get(5).copied()?, sb.get(6).copied()?) {
        (0x4F, 0xC2) => Some(false),
        (0xF4, 0x2C) => Some(true),
        _ => None,
    }
}

fn status_to_smart(st: Status) -> SmartStatus {
    if st == Status::UNSUPPORTED || st == Status::INVALID_PARAMETER {
        SmartStatus::Unsupported
    } else if st == Status::TIMEOUT {
        SmartStatus::Timeout
    } else {
        SmartStatus::Error(format!("{st:?}"))
    }
}

/// Decodes byte-swapped ASCII from `words` IDENTIFY words starting at `word`.
fn ata_string(buf: &[u8], word: usize, words: usize) -> String {
    let mut s = String::new();
    for w in word..word.saturating_add(words) {
        let lo = w.saturating_mul(2);
        let (Some(&a), Some(&b)) = (buf.get(lo.saturating_add(1)), buf.get(lo)) else {
            break;
        };
        for c in [a, b] {
            s.push(if (0x20..=0x7E).contains(&c) {
                c as char
            } else {
                ' '
            });
        }
    }
    s.trim().to_string()
}

/// Little-endian IDENTIFY word `n`, or 0 when out of range.
fn ata_word(buf: &[u8], n: usize) -> u64 {
    let lo = n.saturating_mul(2);
    let a = buf.get(lo).copied().unwrap_or(0);
    let b = buf.get(lo.saturating_add(1)).copied().unwrap_or(0);
    u64::from(u16::from_le_bytes([a, b]))
}

/// User-addressable capacity from IDENTIFY, preferring the 48-bit sector count.
fn ata_capacity(buf: &[u8]) -> u64 {
    let lba48 = ata_word(buf, 100)
        | (ata_word(buf, 101) << 16)
        | (ata_word(buf, 102) << 32)
        | (ata_word(buf, 103) << 48);
    let lba28 = ata_word(buf, 60) | (ata_word(buf, 61) << 16);
    let sectors = if lba48 > 0 { lba48 } else { lba28 };
    sectors.saturating_mul(512)
}

/// 48-bit little-endian raw value from an attribute entry.
fn raw48(entry: &[u8]) -> u64 {
    match entry.get(5..11) {
        Some(r) => r
            .iter()
            .rev()
            .fold(0u64, |acc, &b| (acc << 8) | u64::from(b)),
        None => 0,
    }
}

/// Threshold for `id` from a SMART READ THRESHOLDS page.
fn threshold_for(page: Option<&[u8]>, id: u8) -> Option<u8> {
    let page = page?;
    for n in 0..ENTRY_COUNT {
        let off = ENTRY_BASE + n * ENTRY_LEN;
        let Some(e) = page.get(off..off + ENTRY_LEN) else {
            break;
        };
        let (Some(&eid), Some(&thr)) = (e.first(), e.get(1)) else {
            break;
        };
        if eid == id {
            return Some(thr);
        }
    }
    None
}

/// The alarm gate: only a flagged pre-fail attribute with a populated value at
/// or below a non-zero threshold may be called failing.
fn judge(flags: u16, value: u8, threshold: Option<u8>) -> AttrJudgment {
    let prefail = (flags & 0x0001) != 0;
    match (value, threshold) {
        (0, _) => AttrJudgment::NotPopulated,
        (0xFE | 0xFF, _) => AttrJudgment::RawOnly,
        (_, None) => AttrJudgment::RawOnly,
        (_, Some(0)) => AttrJudgment::Ok,
        (v, Some(t)) if prefail && v <= t => AttrJudgment::Failing,
        (_, Some(_)) if prefail => AttrJudgment::Prefail,
        _ => AttrJudgment::Ok,
    }
}

/// Decodes a SMART READ DATA page against an optional thresholds page.
fn parse_attrs(data: &[u8], thresholds: Option<&[u8]>) -> Vec<SmartAttr> {
    let mut out = Vec::new();
    for n in 0..ENTRY_COUNT {
        let off = ENTRY_BASE + n * ENTRY_LEN;
        let Some(e) = data.get(off..off + ENTRY_LEN) else {
            break;
        };
        let id = e.first().copied().unwrap_or(0);
        if id == 0 {
            continue;
        }
        let flags = u16::from_le_bytes([
            e.get(1).copied().unwrap_or(0),
            e.get(2).copied().unwrap_or(0),
        ]);
        let value = e.get(3).copied().unwrap_or(0);
        let worst = e.get(4).copied().unwrap_or(0);
        let threshold = threshold_for(thresholds, id);
        let judgment = judge(flags, value, threshold);
        out.push(SmartAttr {
            id,
            name: attr_name(id),
            flags,
            value,
            worst,
            threshold,
            raw: raw48(e),
            judgment,
            note: judgment.note().map(str::to_string),
        });
    }
    out
}

/// Detects SandForce-class raw aliasing by co-equal non-zero raw fields.
fn sandforce_quirk(attrs: &[SmartAttr], model: &str) -> Option<String> {
    let mut raws = Vec::new();
    for id in SANDFORCE_IDS {
        raws.push(attrs.iter().find(|a| a.id == id)?.raw);
    }
    let first = *raws.first()?;
    if first == 0 || !raws.iter().all(|&r| r == first) {
        return None;
    }
    if model.is_empty() {
        Some(SANDFORCE_NOTE.to_string())
    } else {
        Some(format!("{model}: {SANDFORCE_NOTE}"))
    }
}

/// Empty drive record carrying one status, for an address that did not answer.
fn blank_entry(port: u16, pmp: u16, status: SmartStatus) -> SataDrive {
    SataDrive {
        model: String::new(),
        serial: String::new(),
        firmware: String::new(),
        capacity: 0,
        port,
        pmp,
        attrs: Vec::new(),
        quirk: None,
        status,
        thresholds_read: false,
        threshold_exceeded: None,
    }
}

/// Probes one device address, returning `None` when the address holds no device.
fn probe_device(dev: &Dev, reported: bool) -> Option<SataDrive> {
    let id = match pio_in(dev, &IDENTIFY) {
        Ok(b) => b,
        Err(s) => {
            logln(format!(
                "sata: port {} pmp {} IDENTIFY {s:?}",
                dev.port, dev.pmp
            ));
            // An address the controller never reported, or one that rejects
            // IDENTIFY outright, is an empty slot.
            if !reported || s == SmartStatus::Unsupported {
                return None;
            }
            return Some(blank_entry(dev.port, dev.pmp, s));
        }
    };
    let mut drive = SataDrive {
        model: ata_string(&id, 27, 20),
        serial: ata_string(&id, 10, 10),
        firmware: ata_string(&id, 23, 4),
        capacity: ata_capacity(&id),
        port: dev.port,
        pmp: dev.pmp,
        attrs: Vec::new(),
        quirk: None,
        status: SmartStatus::Read,
        thresholds_read: false,
        threshold_exceeded: None,
    };
    let data = match pio_in(dev, &SMART_READ_DATA) {
        Ok(d) => d,
        Err(s) => {
            logln(format!(
                "sata: port {} pmp {} SMART READ DATA {s:?}",
                dev.port, dev.pmp
            ));
            drive.status = s;
            return Some(drive);
        }
    };
    let thresholds = match pio_in(dev, &SMART_READ_THRESH) {
        Ok(t) => {
            drive.thresholds_read = true;
            Some(t)
        }
        Err(s) => {
            logln(format!(
                "sata: port {} pmp {} SMART READ THRESHOLDS {s:?}",
                dev.port, dev.pmp
            ));
            None
        }
    };
    drive.attrs = parse_attrs(&data, thresholds.as_deref());
    drive.quirk = sandforce_quirk(&drive.attrs, &drive.model);
    // Ask the drive for its own verdict only when the thresholds page cannot give one.
    if !drive.thresholds_read || !drive.attrs.iter().any(|a| a.threshold.is_some()) {
        drive.threshold_exceeded = smart_return_status(dev);
        logln(format!(
            "sata: port {} pmp {} SMART RETURN STATUS {:?}",
            dev.port, dev.pmp, drive.threshold_exceeded
        ));
    }
    Some(drive)
}

/// Walks every ATA controller and returns one entry per responding device.
///
/// When no `AtaPassThru` handle exists at all, returns a single entry carrying
/// [`SmartStatus::NoHandle`] so the absence is rendered instead of implying health.
#[must_use]
pub fn collect_sata() -> Vec<SataDrive> {
    let mut out: Vec<SataDrive> = Vec::new();
    let handles = match boot::find_handles::<AtaPassThru>() {
        Ok(h) => h,
        Err(e) => {
            logln(format!("sata: find AtaPassThru ERR {e:?}"));
            Vec::new()
        }
    };
    logln(format!("sata: AtaPassThru handles={}", handles.len()));
    if handles.is_empty() {
        out.push(no_handle_entry());
        return out;
    }
    let budget = Budget::new(SCAN_BUDGET);
    let mut truncated = false;
    for handle in handles {
        if budget.spent() {
            truncated = true;
            break;
        }
        let scoped = match unsafe {
            boot::open_protocol::<AtaPassThru>(
                OpenProtocolParams {
                    handle,
                    agent: boot::image_handle(),
                    controller: None,
                },
                OpenProtocolAttributes::GetProtocol,
            )
        } {
            Ok(p) => p,
            Err(_) => continue,
        };
        let Some(proto) = scoped.get() else {
            continue;
        };
        // AtaPassThru is repr(transparent) over UnsafeCell<AtaPassThruProtocol>.
        let raw = core::ptr::from_ref(proto)
            .cast::<AtaPassThruProtocol>()
            .cast_mut();
        if !vtable_ok(raw) {
            logln("sata: AtaPassThru with null entry point, skipped".to_string());
            continue;
        }
        let align = io_align(raw);
        for a in enumerate_devices(raw, &budget) {
            if budget.spent() {
                truncated = true;
                break;
            }
            let dev = Dev {
                raw,
                port: a.port,
                pmp: a.pmp,
                align,
            };
            if let Some(d) = probe_device(&dev, a.reported) {
                out.push(d);
            }
        }
    }
    if truncated {
        logln(format!(
            "sata: scan budget of {}s exhausted, walk truncated",
            SCAN_BUDGET.as_secs()
        ));
        out.push(blank_entry(
            0xFFFF,
            0xFFFF,
            SmartStatus::Error(format!(
                "scan budget of {}s exhausted before every port was probed",
                SCAN_BUDGET.as_secs()
            )),
        ));
    }
    out
}

fn no_handle_entry() -> SataDrive {
    blank_entry(0xFFFF, 0xFFFF, SmartStatus::NoHandle)
}

/// JSON document for one collected drive.
#[must_use]
pub fn drive_json(d: &SataDrive) -> serde_json::Value {
    let attrs: Vec<serde_json::Value> = d
        .attrs
        .iter()
        .map(|a| {
            serde_json::json!({
                "id": a.id,
                "name": a.name,
                "flags": a.flags,
                "prefail_flagged": a.is_prefail_flagged(),
                "value": a.value,
                "worst": a.worst,
                "threshold": a.threshold,
                "raw": a.raw,
                "judgment": a.judgment.label(),
                "alarm": a.judgment.is_alarm() && !is_aliased(d, a.id),
                "aliased": is_aliased(d, a.id),
                "note": a.note,
            })
        })
        .collect();
    serde_json::json!({
        "model": d.model,
        "serial": d.serial,
        "firmware": d.firmware,
        "capacity_bytes": d.capacity,
        "port": d.port,
        "pmp": d.pmp,
        "status": status_json(&d.status),
        "status_text": d.status.label(),
        "smart_available": d.status.has_data(),
        "thresholds_read": d.thresholds_read,
        "threshold_exceeded": d.threshold_exceeded,
        "verified": d.is_verified(),
        "unverified_reason": d.unverified_reason(),
        "has_alarm": d.has_alarm(),
        "failing": d.is_failing(),
        "quirk": d.quirk,
        "attrs": attrs,
    })
}

fn status_json(s: &SmartStatus) -> serde_json::Value {
    match s {
        SmartStatus::Read => serde_json::json!("read"),
        SmartStatus::Unsupported => serde_json::json!("unsupported"),
        SmartStatus::NoHandle => serde_json::json!("no_handle"),
        SmartStatus::Timeout => serde_json::json!("timeout"),
        SmartStatus::Error(e) => serde_json::json!({ "error": e }),
    }
}

/// JSON array for a collected drive list.
#[must_use]
pub fn sata_json(drives: &[SataDrive]) -> serde_json::Value {
    serde_json::Value::Array(drives.iter().map(drive_json).collect())
}

/// Serialized form of [`sata_json`], falling back to an empty array.
#[must_use]
pub fn sata_json_string(drives: &[SataDrive]) -> String {
    serde_json::to_string(&sata_json(drives)).unwrap_or_else(|_| "[]".to_string())
}
