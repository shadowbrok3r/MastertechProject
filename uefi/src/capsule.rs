//! BIOS update orchestration via the EFI System Resource Table (ESRT) and
//! UEFI capsule services.
//!
//! Reading is safe and always on: the ESRT reports the current firmware
//! version, the lowest version the platform will accept, and the outcome of
//! the last update attempt. Applying is firewalled — [`preflight`] must pass,
//! then [`apply_capsule`] calls `QueryCapsuleCapabilities` (the firmware
//! validates the signed vendor capsule) and only then hands it to
//! `UpdateCapsule`, which flashes and resets.

use uefi::boot::{self, AllocateType, MemoryType};
use uefi::runtime::{self, VariableVendor};
use uefi::{Status, guid};
use uefi_raw::capsule::{CapsuleBlockDescriptor, CapsuleFlags, CapsuleHeader};

use crate::logln;

/// EFI System Resource Table configuration-table GUID.
const ESRT_GUID: uefi::Guid = guid!("b122a263-3661-4f68-9929-78f8b0d62180");

/// EFI_FIRMWARE_MANAGEMENT_CAPSULE_ID_GUID — the outer GUID of an FMP capsule.
const FMP_CAPSULE_GUID: uefi::Guid = guid!("6dcbd5ed-e82d-4c44-bda1-7194199ad92a");

/// EFI_OS_INDICATIONS_FILE_CAPSULE_DELIVERY_SUPPORTED.
const OS_IND_CAPSULE_ON_DISK: u64 = 0x0000_0000_0000_0004;

/// Minimum battery charge required to flash a portable machine.
const MIN_BATTERY_PCT: u8 = 50;

/// Keep the capsule and its descriptor list under 4 GiB; 32-bit PEI coalescing
/// cannot reach higher.
const MAX_CAPSULE_ADDR: u64 = 0xFFFF_F000;

#[derive(Clone, Default)]
pub struct EsrtEntry {
    pub fw_class: String,
    pub fw_type: u32,
    pub fw_version: u32,
    pub lowest_supported: u32,
    /// Capsule flags bits 0..=15, meaningful only for this entry's capsule GUID.
    pub capsule_flags: u32,
    pub last_attempt_version: u32,
    pub last_attempt_status: u32,
}

impl EsrtEntry {
    pub fn type_name(&self) -> &'static str {
        match self.fw_type {
            0 => "unknown",
            1 => "system firmware",
            2 => "device firmware",
            3 => "UEFI driver",
            _ => "reserved",
        }
    }

    /// EFI_SYSTEM_RESOURCE_ENTRY LastAttemptStatus decode. Values 0..=8 are
    /// spec-defined; 0x1000 and above are edk2 FmpDevicePkg vendor ranges.
    pub fn last_status_name(&self) -> String {
        match self.last_attempt_status {
            0 => "success".into(),
            1 => "unsuccessful".into(),
            2 => "insufficient resources".into(),
            3 => "incorrect version".into(),
            4 => "invalid image format".into(),
            5 => "auth error".into(),
            6 => "AC not connected".into(),
            7 => "insufficient battery".into(),
            8 => "unsatisfied dependencies".into(),
            v if v >= 0x1000 => format!("vendor-defined (0x{v:04x})"),
            v => format!("unknown (0x{v:04x})"),
        }
    }
}

#[derive(Default)]
pub struct EsrtInfo {
    pub present: bool,
    pub fw_resource_count: u32,
    /// Slots the table was sized for; entries beyond `fw_resource_count` are unused.
    pub fw_resource_count_max: u32,
    pub entries: Vec<EsrtEntry>,
    /// OsIndicationsSupported raw value.
    pub os_indications_supported: u64,
    /// Firmware advertises capsule-on-disk delivery.
    pub capsule_on_disk: bool,
}

impl EsrtInfo {
    /// The primary system-firmware entry (the BIOS itself), if any.
    pub fn system_entry(&self) -> Option<&EsrtEntry> {
        self.entries.iter().find(|e| e.fw_type == 1)
    }
}

/// Machine power state, gating a flash on portables.
#[derive(Clone, Default)]
pub struct PowerState {
    pub portable: bool,
    pub battery_present: bool,
    pub charge_pct: Option<u8>,
    /// Battery is draining, i.e. no AC. `None` when it could not be read.
    pub discharging: Option<bool>,
    /// Where the readings came from, for display.
    pub source: &'static str,
}

fn rd_u32(p: usize) -> u32 {
    unsafe { core::ptr::read_unaligned(p as *const u32) }
}
fn rd_u64(p: usize) -> u64 {
    unsafe { core::ptr::read_unaligned(p as *const u64) }
}

fn guid_at(p: usize) -> uefi::Guid {
    unsafe {
        let mut b = [0u8; 16];
        core::ptr::copy_nonoverlapping(p as *const u8, b.as_mut_ptr(), 16);
        uefi::Guid::from_bytes(b)
    }
}

/// Read the ESRT from the config tables and the capsule-on-disk capability bit.
pub fn collect() -> EsrtInfo {
    let mut info = EsrtInfo::default();

    let mut base = 0usize;
    uefi::system::with_config_table(|entries| {
        for e in entries {
            if e.guid == ESRT_GUID {
                base = e.address as usize;
            }
        }
    });

    if base != 0 {
        // EFI_SYSTEM_RESOURCE_TABLE: count(u32) countMax(u32) version(u64) entries[].
        let count = rd_u32(base);
        let count_max = rd_u32(base + 4);
        let version = rd_u64(base + 8);
        if version == 1 && count <= 256 && (count_max == 0 || count <= count_max) {
            info.present = true;
            info.fw_resource_count = count;
            info.fw_resource_count_max = count_max;
            let entry_base = base + 16;
            for i in 0..count as usize {
                let ep = entry_base + i * 40;
                info.entries.push(EsrtEntry {
                    fw_class: format!("{}", guid_at(ep)),
                    fw_type: rd_u32(ep + 16),
                    fw_version: rd_u32(ep + 20),
                    lowest_supported: rd_u32(ep + 24),
                    capsule_flags: rd_u32(ep + 28),
                    last_attempt_version: rd_u32(ep + 32),
                    last_attempt_status: rd_u32(ep + 36),
                });
            }
        } else {
            logln(format!(
                "esrt: rejected table (version={version} count={count} max={count_max})"
            ));
        }
    }

    let mut buf = [0u8; 8];
    if let Ok((data, _)) = runtime::get_variable(
        uefi::cstr16!("OsIndicationsSupported"),
        &VariableVendor::GLOBAL_VARIABLE,
        &mut buf,
    ) {
        let mut v = 0u64;
        for (i, &b) in data.iter().take(8).enumerate() {
            v |= (b as u64) << (8 * i);
        }
        info.os_indications_supported = v;
        info.capsule_on_disk = v & OS_IND_CAPSULE_ON_DISK != 0;
    }

    logln(format!(
        "esrt: present={} count={} capsule_on_disk={}",
        info.present, info.fw_resource_count, info.capsule_on_disk
    ));
    info
}

/// Outcome of comparing a candidate version against the live ESRT.
pub enum VersionGate {
    Newer(String),
    /// Same version or a downgrade the platform would still accept — a
    /// deliberate reflash, allowed only when explicitly forced.
    NotNewer(String),
    /// Below the ESRT lowest-supported version; the firmware will refuse it.
    BelowFloor(String),
}

/// Classify `new_version` against the system-firmware ESRT entry.
pub fn version_gate(info: &EsrtInfo, new_version: u32) -> Result<VersionGate, String> {
    let e = info.system_entry().ok_or("no system-firmware ESRT entry")?;
    if new_version < e.lowest_supported {
        return Ok(VersionGate::BelowFloor(format!(
            "0x{:08x} is below lowest-supported 0x{:08x} (rollback blocked)",
            new_version, e.lowest_supported
        )));
    }
    if new_version <= e.fw_version {
        return Ok(VersionGate::NotNewer(format!(
            "0x{:08x} is not newer than current 0x{:08x}",
            new_version, e.fw_version
        )));
    }
    Ok(VersionGate::Newer(format!(
        "0x{:08x} accepted (current 0x{:08x}, floor 0x{:08x})",
        new_version, e.fw_version, e.lowest_supported
    )))
}

/// Whether the machine is on stable power. Desktops pass unconditionally;
/// portables must be on AC with a healthy charge, and an unreadable battery is
/// a refusal rather than an assumption.
pub fn power_verdict(power: &PowerState) -> Result<String, String> {
    if !power.portable {
        return Ok("desktop/AC-only chassis".into());
    }
    if power.discharging == Some(true) {
        return Err("portable is running on battery; connect AC".into());
    }
    match power.charge_pct {
        Some(p) if p < MIN_BATTERY_PCT => {
            Err(format!("battery {p}% is below the {MIN_BATTERY_PCT}% floor"))
        }
        Some(p) => Ok(format!("portable on AC, battery {p}%")),
        None if power.discharging == Some(false) => Ok("portable on AC, charge unknown".into()),
        None => Err(format!(
            "portable power state unreadable (source: {})",
            power.source
        )),
    }
}

/// Every gate that must pass before a capsule is downloaded or flashed.
/// Returns advisory warnings on success. `force` permits a same-version
/// reflash, a missing version gate, and a retry after a prior format/auth
/// failure. It never bypasses the ESRT version floor or the power check —
/// flashing a portable off AC is how a machine gets bricked.
pub fn preflight(
    info: &EsrtInfo,
    power: &PowerState,
    expected_version: Option<u32>,
    force: bool,
) -> Result<Vec<String>, String> {
    let mut warns = Vec::new();

    if !info.present {
        return Err("no ESRT; platform does not advertise capsule firmware update".into());
    }
    let entry = info
        .system_entry()
        .ok_or("no fw_type==1 system-firmware ESRT entry")?;

    match expected_version {
        Some(v) => match version_gate(info, v)? {
            VersionGate::Newer(m) => warns.push(m),
            VersionGate::NotNewer(m) if force => warns.push(format!("{m} - forced reflash")),
            VersionGate::NotNewer(m) => return Err(format!("{m} (no-op/downgrade blocked)")),
            VersionGate::BelowFloor(m) => return Err(m),
        },
        None if force => {
            warns.push(format!(
                "no expected version supplied; current is 0x{:08x}",
                entry.fw_version
            ));
        }
        None => return Err("expected_version is required (or force:true to override)".into()),
    }

    // A prior attempt that failed authentication or format will fail again.
    match entry.last_attempt_status {
        3 | 4 | 5 => {
            let msg = format!(
                "previous attempt v0x{:08x} failed: {}",
                entry.last_attempt_version,
                entry.last_status_name()
            );
            if force {
                warns.push(format!("{msg} - retrying anyway"));
            } else {
                return Err(msg);
            }
        }
        6 | 7 => warns.push(format!(
            "previous attempt failed on power: {}",
            entry.last_status_name()
        )),
        _ => {}
    }

    warns.push(power_verdict(power)?);
    Ok(warns)
}

/// One payload inside an FMP capsule.
pub struct FmpPayload {
    /// UpdateImageTypeId — matches the ESRT `fw_class` of the target device.
    pub image_type_id: String,
    pub image_index: u8,
    pub image_size: u32,
}

/// Parsed EFI_FIRMWARE_MANAGEMENT_CAPSULE_HEADER contents.
pub struct FmpInfo {
    pub embedded_driver_count: u16,
    pub payloads: Vec<FmpPayload>,
}

/// Parse the FMP capsule header chain that follows the EFI_CAPSULE_HEADER.
/// Returns `None` for a non-FMP capsule or a malformed chain.
pub fn parse_fmp(bytes: &[u8], header_size: u32) -> Option<FmpInfo> {
    let base = header_size as usize;
    if guid_from_slice(bytes.get(0..16)?)? != FMP_CAPSULE_GUID {
        return None;
    }
    let h = bytes.get(base..base + 8)?;
    if u32::from_le_bytes(h[0..4].try_into().ok()?) != 1 {
        return None;
    }
    let drivers = u16::from_le_bytes(h[4..6].try_into().ok()?);
    let items = u16::from_le_bytes(h[6..8].try_into().ok()?);
    if items == 0 || drivers as usize + items as usize > 64 {
        return None;
    }

    let mut payloads = Vec::new();
    for i in 0..items as usize {
        let off_at = base + 8 + (drivers as usize + i) * 8;
        let off = u64::from_le_bytes(bytes.get(off_at..off_at + 8)?.try_into().ok()?) as usize;
        let p = base.checked_add(off)?;
        let ih = bytes.get(p..p + 28)?;
        if u32::from_le_bytes(ih[0..4].try_into().ok()?) == 0 {
            return None;
        }
        payloads.push(FmpPayload {
            image_type_id: format!("{}", guid_from_slice(&ih[4..20])?),
            image_index: ih[20],
            image_size: u32::from_le_bytes(ih[24..28].try_into().ok()?),
        });
    }
    Some(FmpInfo { embedded_driver_count: drivers, payloads })
}

fn guid_from_slice(s: &[u8]) -> Option<uefi::Guid> {
    let b: [u8; 16] = s.try_into().ok()?;
    Some(uefi::Guid::from_bytes(b))
}

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

fn sha256_block(h: &mut [u32; 8], block: &[u8]) {
    let mut w = [0u32; 64];
    for i in 0..16 {
        w[i] = u32::from_be_bytes([
            block[4 * i],
            block[4 * i + 1],
            block[4 * i + 2],
            block[4 * i + 3],
        ]);
    }
    for i in 16..64 {
        let a = w[i - 15];
        let b = w[i - 2];
        let s0 = a.rotate_right(7) ^ a.rotate_right(18) ^ (a >> 3);
        let s1 = b.rotate_right(17) ^ b.rotate_right(19) ^ (b >> 10);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }
    let mut v = *h;
    for i in 0..64 {
        let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
        let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
        let t1 = v[7]
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(SHA256_K[i])
            .wrapping_add(w[i]);
        let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
        let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
        let t2 = s0.wrapping_add(maj);
        v[7] = v[6];
        v[6] = v[5];
        v[5] = v[4];
        v[4] = v[3].wrapping_add(t1);
        v[3] = v[2];
        v[2] = v[1];
        v[1] = v[0];
        v[0] = t1.wrapping_add(t2);
    }
    for i in 0..8 {
        h[i] = h[i].wrapping_add(v[i]);
    }
}

/// SHA-256 as lowercase hex, hashed in place so a 64 MiB capsule is not copied.
/// Hand-rolled because the firmware crate carries no hashing dependency.
pub fn sha256_hex(data: &[u8]) -> String {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let full = data.len() / 64 * 64;
    let mut off = 0;
    while off < full {
        sha256_block(&mut h, &data[off..off + 64]);
        off += 64;
    }

    let rem = &data[full..];
    let mut block = [0u8; 64];
    block[..rem.len()].copy_from_slice(rem);
    let mut i = rem.len();
    block[i] = 0x80;
    i += 1;
    if i > 56 {
        sha256_block(&mut h, &block);
        block = [0u8; 64];
        i = 0;
    }
    block[i..56].fill(0);
    block[56..].copy_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    sha256_block(&mut h, &block);

    let mut out = String::with_capacity(64);
    for x in h {
        out.push_str(&format!("{x:08x}"));
    }
    out
}

/// Compare a capsule against an expected lowercase-hex SHA-256.
pub fn verify_sha256(data: &[u8], expected: &str) -> Result<(), String> {
    let got = sha256_hex(data);
    if got.eq_ignore_ascii_case(expected.trim()) {
        Ok(())
    } else {
        Err(format!("sha256 mismatch (expected {expected}, got {got})"))
    }
}

/// Describe a capsule's headers so an operator can check it before arming.
pub fn inspect(bytes: &[u8]) -> Result<Vec<String>, String> {
    if bytes.len() < core::mem::size_of::<CapsuleHeader>() {
        return Err("capsule shorter than its header".into());
    }
    let guid = guid_from_slice(&bytes[0..16]).ok_or("unreadable capsule GUID")?;
    let header_size = u32::from_le_bytes(bytes[16..20].try_into().unwrap());
    let raw_flags = u32::from_le_bytes(bytes[20..24].try_into().unwrap());
    let image_size = u32::from_le_bytes(bytes[24..28].try_into().unwrap());
    let flags = CapsuleFlags::from_bits_truncate(raw_flags);

    let mut out = vec![
        format!("guid    {guid}"),
        format!(
            "size    header {header_size}, image {image_size}, loaded {}",
            bytes.len()
        ),
        format!(
            "flags   0x{raw_flags:08x} persist={} populate={} reset={}",
            flags.contains(CapsuleFlags::PERSIST_ACROSS_RESET),
            flags.contains(CapsuleFlags::POPULATE_SYSTEM_TABLE),
            flags.contains(CapsuleFlags::INITIATE_RESET),
        ),
    ];
    if guid == FMP_CAPSULE_GUID {
        match parse_fmp(bytes, header_size) {
            Some(f) => {
                out.push(format!(
                    "fmp     {} payload(s), {} embedded driver(s)",
                    f.payloads.len(),
                    f.embedded_driver_count
                ));
                for p in &f.payloads {
                    out.push(format!(
                        "target  {} idx {} ({} bytes)",
                        p.image_type_id, p.image_index, p.image_size
                    ));
                }
            }
            None => out.push("fmp     header unparseable".into()),
        }
    } else {
        out.push("fmp     not an FMP capsule - target cannot be verified".into());
    }
    Ok(out)
}

/// Flash a signed vendor capsule. DANGEROUS and irreversible: on success this
/// hands the image to the firmware and resets — it does not return. The payload
/// and the scatter-gather list are both copied into page-aligned
/// RUNTIME_SERVICES_DATA below 4 GiB so the firmware can still reach them after
/// the reset.
pub fn apply_capsule(bytes: &[u8], esrt: &EsrtInfo) -> Result<String, String> {
    if bytes.len() < core::mem::size_of::<CapsuleHeader>() {
        return Err("capsule shorter than its header".into());
    }
    // Validate the declared sizes before trusting the image.
    let header_size = u32::from_le_bytes(bytes[16..20].try_into().unwrap());
    let raw_flags = u32::from_le_bytes(bytes[20..24].try_into().unwrap());
    let image_size = u32::from_le_bytes(bytes[24..28].try_into().unwrap());
    if (header_size as usize) < core::mem::size_of::<CapsuleHeader>()
        || (image_size as usize) < header_size as usize
        || image_size as usize > bytes.len()
    {
        return Err(format!(
            "capsule header inconsistent (header={header_size} image={image_size} buf={})",
            bytes.len()
        ));
    }
    let image_size = image_size as usize;
    if image_size < bytes.len() {
        logln(format!(
            "capsule: ignoring {} trailing byte(s) past CapsuleImageSize",
            bytes.len() - image_size
        ));
    }

    let flags = CapsuleFlags::from_bits_truncate(raw_flags);
    let persist = flags.contains(CapsuleFlags::PERSIST_ACROSS_RESET);
    logln(format!(
        "capsule: guid={} flags=0x{:08x} persist={} initiate_reset={} image={}B",
        guid_from_slice(&bytes[0..16]).map(|g| format!("{g}")).unwrap_or_default(),
        raw_flags,
        persist,
        flags.contains(CapsuleFlags::INITIATE_RESET),
        image_size
    ));
    if !persist {
        logln("capsule: PERSIST_ACROSS_RESET not set; firmware must process in place".into());
    }

    // An FMP capsule names the device it targets; refuse a capsule built for a
    // different board rather than relying on the vendor signature alone.
    if let Some(fmp) = parse_fmp(bytes, header_size) {
        let ids: Vec<&str> = fmp.payloads.iter().map(|p| p.image_type_id.as_str()).collect();
        logln(format!(
            "capsule: FMP with {} payload(s) {:?}, {} embedded driver(s)",
            fmp.payloads.len(),
            ids,
            fmp.embedded_driver_count
        ));
        if let Some(sys) = esrt.system_entry() {
            let want = sys.fw_class.to_ascii_lowercase();
            let matched = fmp
                .payloads
                .iter()
                .any(|p| p.image_type_id.to_ascii_lowercase() == want);
            if !matched && !esrt.entries.iter().any(|e| {
                ids.iter().any(|i| i.eq_ignore_ascii_case(&e.fw_class))
            }) {
                return Err(format!(
                    "capsule targets {:?}, none of which match any ESRT fw_class (system is {})",
                    ids, sys.fw_class
                ));
            }
            if !matched {
                logln("capsule: targets a device-firmware ESRT entry, not system firmware".into());
            }
        }
    } else {
        logln("capsule: not an FMP capsule; cannot verify the target device".into());
    }

    // Page-aligned, reset-surviving copies of the payload and the descriptor
    // list. The firmware reads the list by physical address after the reset, so
    // it cannot live on the stack.
    let pages = image_size.div_ceil(4096);
    let mem = boot::allocate_pages(
        AllocateType::MaxAddress(MAX_CAPSULE_ADDR),
        MemoryType::RUNTIME_SERVICES_DATA,
        pages,
    )
    .map_err(|e| format!("allocate_pages({pages}) for capsule: {e:?}"))?;
    let sgl = match boot::allocate_pages(
        AllocateType::MaxAddress(MAX_CAPSULE_ADDR),
        MemoryType::RUNTIME_SERVICES_DATA,
        1,
    ) {
        Ok(p) => p,
        Err(e) => {
            unsafe {
                let _ = boot::free_pages(mem, pages);
            }
            return Err(format!("allocate_pages(1) for descriptor list: {e:?}"));
        }
    };

    let dst = mem.as_ptr();
    unsafe {
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, image_size);
    }

    let desc = sgl.as_ptr() as *mut CapsuleBlockDescriptor;
    unsafe {
        core::ptr::write_unaligned(
            desc,
            CapsuleBlockDescriptor { length: image_size as u64, address: dst as u64 },
        );
        core::ptr::write_unaligned(
            desc.add(1),
            CapsuleBlockDescriptor { length: 0, address: 0 },
        );
    }

    let result = flash(dst, desc, image_size, persist);
    if result.is_err() {
        unsafe {
            let _ = boot::free_pages(mem, pages);
            let _ = boot::free_pages(sgl, 1);
        }
    }
    result
}

/// Hand the staged capsule to the firmware. Diverges on the normal path.
fn flash(
    dst: *mut u8,
    desc: *mut CapsuleBlockDescriptor,
    image_size: usize,
    persist: bool,
) -> Result<String, String> {
    let header = unsafe { &*(dst as *const CapsuleHeader) };
    let headers = [header];
    let blocks = unsafe { core::slice::from_raw_parts(desc as *const CapsuleBlockDescriptor, 2) };

    let cap = runtime::query_capsule_capabilities(&headers)
        .map_err(|e| format!("firmware rejected capsule (QueryCapsuleCapabilities: {e:?})"))?;
    logln(format!(
        "capsule: accepted by firmware (max {} bytes, reset {:?})",
        cap.maximum_capsule_size, cap.reset_type
    ));
    if cap.maximum_capsule_size != 0 && image_size as u64 > cap.maximum_capsule_size {
        return Err(format!(
            "capsule is {image_size} bytes, firmware maximum is {}",
            cap.maximum_capsule_size
        ));
    }

    logln("capsule: calling UpdateCapsule (point of no return)".into());
    runtime::update_capsule(&headers, blocks).map_err(|e| format!("UpdateCapsule: {e:?}"))?;

    // A PERSIST_ACROSS_RESET capsule is only applied by the reset, so reset even
    // if the firmware declined to do it itself. Without that flag the firmware
    // has already processed the capsule in place.
    if !persist {
        logln("capsule: processed in place (no PERSIST_ACROSS_RESET)".into());
        return Ok("capsule processed without reset".into());
    }
    logln("capsule: UpdateCapsule returned; resetting to apply".into());
    runtime::reset(cap.reset_type, Status::SUCCESS, None);
}

/// `firmware_update` object for the fingerprint upload.
pub fn esrt_json(info: &EsrtInfo, power: &PowerState) -> serde_json::Value {
    let entries: Vec<serde_json::Value> = info
        .entries
        .iter()
        .map(|e| {
            serde_json::json!({
                "fw_class": e.fw_class,
                "type": e.type_name(),
                "version": format!("0x{:08x}", e.fw_version),
                "lowest_supported": format!("0x{:08x}", e.lowest_supported),
                "capsule_flags": format!("0x{:08x}", e.capsule_flags),
                "last_attempt_version": format!("0x{:08x}", e.last_attempt_version),
                "last_attempt_status": e.last_status_name(),
                "last_attempt_status_raw": e.last_attempt_status,
            })
        })
        .collect();
    serde_json::json!({
        "esrt_present": info.present,
        "resource_count": info.fw_resource_count,
        "resource_count_max": info.fw_resource_count_max,
        "capsule_on_disk": info.capsule_on_disk,
        "os_indications_supported": format!("0x{:016x}", info.os_indications_supported),
        "entries": entries,
        "power": {
            "portable": power.portable,
            "battery_present": power.battery_present,
            "charge_pct": power.charge_pct,
            "discharging": power.discharging,
            "source": power.source,
            "verdict": match power_verdict(power) {
                Ok(m) => m,
                Err(e) => format!("BLOCKED: {e}"),
            },
        },
    })
}
