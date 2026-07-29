//! Volume signature triage: name the filesystem on every partition, pre-boot.
//!
//! The boot doctor counts partition type GUIDs and discards the handles. This
//! keeps them, reads one 512-byte sector per partition and classifies what
//! actually lives there — NTFS, BitLocker, FAT, exFAT, blank or unrecognised.
//! A BitLocker volume with no recovery key changes the whole repair plan, so it
//! has to surface before anything is touched.
//!
//! All reads are best-effort and never panic.

use uefi::boot::{self, OpenProtocolAttributes, OpenProtocolParams};
use uefi::{Handle, Status};
use uefi::proto::media::block::{BlockIO, BlockIoProtocol};
use uefi::proto::media::disk::DiskIo;
use uefi::proto::media::partition::PartitionInfo;
use uefi::{Guid, guid};

use crate::bootdiag::Severity;
use crate::logln;

/// Windows/basic data partition (the OS lives here).
const WIN_DATA_GUID: Guid = guid!("ebd0a0a2-b9e5-4433-87c0-68b6b72699c7");

/// Bytes classified per volume.
const SECTOR: usize = 512;
/// Upper bound on partition handles inspected.
const MAX_HANDLES: usize = 16;
/// Consecutive unreadable partitions after which the remaining ones are skipped.
const MAX_CONSECUTIVE_FAILS: usize = 2;
/// Largest logical block size accepted for a bounce read.
const MAX_BLOCK_SIZE: u32 = 65536;
/// Largest buffer alignment accepted from `io_align`.
const MAX_IO_ALIGN: u32 = 65536;

/// BitLocker (Vista) volume identifier at offset 0x0B.
const BL_GUID_VISTA: [u8; 16] = [
    0x3b, 0xd6, 0x67, 0x49, 0x29, 0x2e, 0xd8, 0x4a, 0x83, 0x99, 0xf6, 0xa3, 0x39, 0xe3, 0xd0, 0x01,
];
/// BitLocker (Windows 7+) volume identifier at offset 0x0B.
const BL_GUID_WIN7: [u8; 16] = [
    0x3b, 0x4d, 0xa8, 0x92, 0x80, 0xdd, 0x0e, 0x4d, 0x9e, 0x4e, 0xb1, 0xe3, 0x28, 0x4e, 0xae, 0xd8,
];

#[derive(Clone, PartialEq, Eq)]
pub enum VolKind {
    Ntfs,
    BitLocker,
    Fat12,
    Fat16,
    Fat32,
    FatUnknown,
    ExFat,
    Unformatted,
    Unknown { oem: String },
}

impl VolKind {
    /// Short filesystem name for tables and logs.
    pub fn label(&self) -> String {
        match self {
            Self::Ntfs => "NTFS".into(),
            Self::BitLocker => "BitLocker".into(),
            Self::Fat12 => "FAT12".into(),
            Self::Fat16 => "FAT16".into(),
            Self::Fat32 => "FAT32".into(),
            Self::FatUnknown => "FAT".into(),
            Self::ExFat => "exFAT".into(),
            Self::Unformatted => "unformatted".into(),
            Self::Unknown { oem } if oem.is_empty() => "unknown".into(),
            Self::Unknown { oem } => format!("unknown ({oem})"),
        }
    }
}

/// Which protocol produced the sector.
#[derive(Clone, PartialEq, Eq)]
pub enum ReadPath {
    DiskIo,
    BlockIo,
    Failed(String),
    Skipped,
}

impl ReadPath {
    pub fn label(&self) -> &'static str {
        match self {
            Self::DiskIo => "DiskIo",
            Self::BlockIo => "BlockIo",
            Self::Failed(_) => "failed",
            Self::Skipped => "skipped",
        }
    }
}

#[derive(Clone)]
pub struct VolumeSig {
    pub kind: VolKind,
    pub start_lba: u64,
    pub end_lba: u64,
    pub is_windows_data: bool,
    pub read_via: ReadPath,
}

impl VolumeSig {
    /// Number of blocks spanned, when the partition entry gave a usable range.
    pub fn block_count(&self) -> Option<u64> {
        self.end_lba.checked_sub(self.start_lba)?.checked_add(1)
    }

    /// Bench-facing finding for this volume. The only sanctioned wording.
    pub fn verdict_text(&self) -> String {
        let span = format!("LBA {}-{}", self.start_lba, self.end_lba);
        match &self.read_via {
            ReadPath::Failed(why) => {
                return format!("{span}: first sector unreadable ({why}) - volume not classified");
            }
            ReadPath::Skipped => {
                return format!(
                    "{span}: not read - earlier partitions on this media were unreadable"
                );
            }
            _ => {}
        }
        match &self.kind {
            VolKind::BitLocker => format!(
                "{span}: BitLocker metadata present - volume contents unreadable pre-boot"
            ),
            VolKind::Ntfs => format!("{span}: NTFS volume"),
            VolKind::Fat12 => format!("{span}: FAT12 volume"),
            VolKind::Fat16 => format!("{span}: FAT16 volume"),
            VolKind::Fat32 => format!("{span}: FAT32 volume"),
            VolKind::FatUnknown => format!("{span}: FAT volume"),
            VolKind::ExFat => format!("{span}: exFAT volume"),
            VolKind::Unformatted => {
                format!("{span}: first sector is all zeroes - unformatted or wiped")
            }
            VolKind::Unknown { oem } if oem.is_empty() => {
                format!("{span}: no recognised filesystem signature")
            }
            VolKind::Unknown { oem } => {
                format!("{span}: no recognised filesystem signature (OEM ID \"{oem}\")")
            }
        }
    }

    /// Severity a technician should act on.
    pub fn severity(&self) -> Severity {
        match (&self.read_via, &self.kind) {
            (ReadPath::Failed(_) | ReadPath::Skipped, _) => Severity::Warn,
            (_, VolKind::BitLocker) => Severity::Warn,
            (_, VolKind::Unformatted | VolKind::Unknown { .. }) if self.is_windows_data => {
                Severity::Warn
            }
            _ => Severity::Ok,
        }
    }
}

/// True if any volume carries BitLocker metadata.
pub fn any_bitlocker(vols: &[VolumeSig]) -> bool {
    vols.iter().any(|v| v.kind == VolKind::BitLocker)
}

/// Findings worth showing, in the boot doctor's verdict shape.
pub fn verdicts(vols: &[VolumeSig]) -> Vec<(Severity, String)> {
    vols.iter()
        .filter(|v| v.severity() != Severity::Ok)
        .map(|v| {
            let tag = if v.is_windows_data { "Windows data partition" } else { "Volume" };
            (v.severity(), format!("{tag} {}", v.verdict_text()))
        })
        .collect()
}

/// Printable ASCII with everything else folded to a dot.
fn ascii_id(b: &[u8]) -> String {
    b.iter()
        .map(|&c| if (0x20..0x7f).contains(&c) { c as char } else { '.' })
        .collect()
}

fn is_bitlocker_guid(sec: &[u8]) -> bool {
    match sec.get(0x0b..0x1b) {
        Some(g) => g == BL_GUID_VISTA.as_slice() || g == BL_GUID_WIN7.as_slice(),
        None => false,
    }
}

/// Classify a volume from its first sector.
fn classify(sec: &[u8]) -> VolKind {
    let oem = sec.get(3..11).unwrap_or_default();
    if oem == b"NTFS    ".as_slice() {
        return VolKind::Ntfs;
    }
    if oem == b"-FVE-FS-".as_slice() || is_bitlocker_guid(sec) {
        return VolKind::BitLocker;
    }
    if oem == b"EXFAT   ".as_slice() {
        return VolKind::ExFat;
    }
    let fs32 = sec.get(0x52..0x5a).unwrap_or_default();
    let fs16 = sec.get(0x36..0x3e).unwrap_or_default();
    if fs32.starts_with(b"FAT32") || fs16.starts_with(b"FAT32") {
        return VolKind::Fat32;
    }
    if fs16.starts_with(b"FAT16") {
        return VolKind::Fat16;
    }
    if fs16.starts_with(b"FAT12") {
        return VolKind::Fat12;
    }
    let fat_oem = oem == b"MSDOS5.0".as_slice() || oem == b"MSWIN4.1".as_slice();
    if fat_oem || fs16.starts_with(b"FAT") {
        return VolKind::FatUnknown;
    }
    if sec.iter().all(|&b| b == 0) {
        return VolKind::Unformatted;
    }
    VolKind::Unknown { oem: ascii_id(oem) }
}

/// Partition span and Windows-data flag from `PartitionInfo`.
fn partition_span(h: Handle) -> Option<(u64, u64, bool)> {
    let pi = unsafe {
        boot::open_protocol::<PartitionInfo>(
            OpenProtocolParams {
                handle: h,
                agent: boot::image_handle(),
                controller: None,
            },
            OpenProtocolAttributes::GetProtocol,
        )
    }
    .ok()?;
    if let Some(gpt) = pi.gpt_partition_entry() {
        let g = gpt.partition_type_guid.0;
        let (start, end) = (gpt.starting_lba, gpt.ending_lba);
        return Some((start, end, g == WIN_DATA_GUID));
    }
    if let Some(mbr) = pi.mbr_partition_record() {
        let start = mbr.starting_lba as u64;
        let end = start.saturating_add(u64::from(mbr.size_in_lba)).saturating_sub(1);
        return Some((start, end, false));
    }
    Some((0, 0, false))
}

/// Fields of `EFI_BLOCK_IO_MEDIA` needed for a bounded read.
struct Media {
    media_id: u32,
    block_size: u32,
    io_align: u32,
}

/// Read `EFI_BLOCK_IO_MEDIA` through a null-checked pointer, revision-1 fields only.
fn media_of(blk: &BlockIO) -> Option<Media> {
    let raw = unsafe { &*(blk as *const BlockIO).cast::<BlockIoProtocol>() };
    if raw.media.is_null() {
        return None;
    }
    let m = raw.media;
    let present: bool = unsafe { (*m).media_present }.into();
    if !present {
        return None;
    }
    Some(Media {
        media_id: unsafe { (*m).media_id },
        block_size: unsafe { (*m).block_size },
        io_align: unsafe { (*m).io_align },
    })
}

/// Read `total` bytes from LBA 0 into an `io_align`-honouring bounce buffer.
fn bounce_read(blk: &BlockIO, m: &Media, out: &mut [u8; SECTOR]) -> Result<(), String> {
    let bs = m.block_size as usize;
    if m.block_size == 0 || m.block_size > MAX_BLOCK_SIZE {
        return Err(format!("block size {}", m.block_size));
    }
    let blocks = SECTOR.div_ceil(bs);
    let Some(total) = blocks.checked_mul(bs) else {
        return Err("block count overflow".into());
    };
    if m.io_align > MAX_IO_ALIGN {
        return Err(format!("io_align {}", m.io_align));
    }
    let align = match m.io_align as usize {
        0 | 1 => 1,
        a if a.is_power_of_two() => a,
        a => return Err(format!("io_align {a} not a power of two")),
    };
    let Some(cap) = total.checked_add(align) else {
        return Err("buffer size overflow".into());
    };
    let mut raw = vec![0u8; cap];
    let off = raw.as_ptr().align_offset(align);
    let Some(buf) = raw.get_mut(off..).and_then(|s| s.get_mut(..total)) else {
        return Err("alignment slack exhausted".into());
    };
    blk.read_blocks(m.media_id, 0, buf)
        .map_err(|e| format!("read_blocks {:?}", e.status()))?;
    let Some(head) = buf.get(..SECTOR) else {
        return Err("short bounce buffer".into());
    };
    out.copy_from_slice(head);
    Ok(())
}

/// True for a protocol-level rejection, where the BlockIo path can still work.
/// Everything else means the media already failed the transfer.
fn worth_retrying(st: Status) -> bool {
    st == Status::UNSUPPORTED || st == Status::INVALID_PARAMETER
}

/// First sector of a partition, via DiskIo where the firmware provides it.
fn read_first_sector(h: Handle) -> (Option<[u8; SECTOR]>, ReadPath) {
    let params = || OpenProtocolParams {
        handle: h,
        agent: boot::image_handle(),
        controller: None,
    };
    let blk = match unsafe {
        boot::open_protocol::<BlockIO>(params(), OpenProtocolAttributes::GetProtocol)
    } {
        Ok(b) => b,
        Err(e) => return (None, ReadPath::Failed(format!("BlockIo {:?}", e.status()))),
    };
    let Some(media) = media_of(&blk) else {
        return (None, ReadPath::Failed("no media present".into()));
    };

    let mut buf = [0u8; SECTOR];
    if let Ok(disk) =
        unsafe { boot::open_protocol::<DiskIo>(params(), OpenProtocolAttributes::GetProtocol) }
    {
        match disk.read_disk(media.media_id, 0, &mut buf) {
            Ok(()) => return (Some(buf), ReadPath::DiskIo),
            Err(e) if !worth_retrying(e.status()) => {
                return (None, ReadPath::Failed(format!("read_disk {:?}", e.status())));
            }
            Err(_) => buf = [0u8; SECTOR],
        }
    }
    match bounce_read(&blk, &media, &mut buf) {
        Ok(()) => (Some(buf), ReadPath::BlockIo),
        Err(why) => (None, ReadPath::Failed(why)),
    }
}

/// Classify every partition the firmware exposes.
pub fn collect_volumes() -> Vec<VolumeSig> {
    let mut out = Vec::new();
    let Ok(handles) = boot::find_handles::<PartitionInfo>() else {
        logln("volsig: no PartitionInfo handles".into());
        return out;
    };
    let mut fails = 0usize;
    for h in handles.into_iter().take(MAX_HANDLES) {
        let Some((start_lba, end_lba, is_windows_data)) = partition_span(h) else {
            continue;
        };
        // Each read is an untimed firmware call; a dead disk must not cost one per partition.
        let (sector, read_via) = if fails >= MAX_CONSECUTIVE_FAILS {
            (None, ReadPath::Skipped)
        } else {
            read_first_sector(h)
        };
        if matches!(read_via, ReadPath::Failed(_)) {
            fails += 1;
        } else if read_via != ReadPath::Skipped {
            fails = 0;
        }
        let kind = match &sector {
            Some(s) => classify(s),
            None => VolKind::Unknown { oem: String::new() },
        };
        out.push(VolumeSig {
            kind,
            start_lba,
            end_lba,
            is_windows_data,
            read_via,
        });
    }
    logln(format!(
        "volsig: {} volume(s), bitlocker={}, [{}]",
        out.len(),
        any_bitlocker(&out),
        out.iter()
            .map(|v| format!("{}/{}", v.kind.label(), v.read_via.label()))
            .collect::<Vec<_>>()
            .join(" ")
    ));
    out
}

/// `volume_signatures` array for the fingerprint upload.
pub fn volumes_json(vols: &[VolumeSig]) -> serde_json::Value {
    let items: Vec<serde_json::Value> = vols
        .iter()
        .map(|v| {
            serde_json::json!({
                "kind": v.kind.label(),
                "start_lba": v.start_lba,
                "end_lba": v.end_lba,
                "block_count": v.block_count(),
                "is_windows_data": v.is_windows_data,
                "read_via": v.read_via.label(),
                "read_error": match &v.read_via {
                    ReadPath::Failed(why) => Some(why.clone()),
                    _ => None,
                },
                "verdict": v.verdict_text(),
                "severity": v.severity().key(),
            })
        })
        .collect();
    serde_json::Value::Array(items)
}
