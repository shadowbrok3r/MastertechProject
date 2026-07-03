//! BIOS update orchestration via the EFI System Resource Table (ESRT) and
//! UEFI capsule services.
//!
//! Reading is safe and always on: the ESRT reports the current firmware
//! version, the lowest version the platform will accept, and the outcome of
//! the last update attempt. Applying is firewalled — [`apply_capsule`] first
//! calls `QueryCapsuleCapabilities` (the firmware validates the signed vendor
//! capsule) and only then hands it to `UpdateCapsule`, which flashes and
//! resets. It is only ever reached from an explicit, confirmed action.

use uefi::boot::{self, AllocateType, MemoryType};
use uefi::runtime::{self, VariableVendor};
use uefi::{Status, guid};
use uefi_raw::capsule::{CapsuleBlockDescriptor, CapsuleHeader};

use crate::logln;

/// EFI System Resource Table configuration-table GUID.
const ESRT_GUID: uefi::Guid = guid!("b122a263-3661-4f68-9929-78f8b0d62180");

/// EFI_OS_INDICATIONS_FILE_CAPSULE_DELIVERY_SUPPORTED.
const OS_IND_CAPSULE_ON_DISK: u64 = 0x0000_0000_0000_0004;

#[derive(Clone, Default)]
pub struct EsrtEntry {
    pub fw_class: String,
    pub fw_type: u32,
    pub fw_version: u32,
    pub lowest_supported: u32,
    pub last_attempt_version: u32,
    pub last_attempt_status: u32,
}

impl EsrtEntry {
    pub fn type_name(&self) -> &'static str {
        match self.fw_type {
            1 => "system firmware",
            2 => "device firmware",
            3 => "UEFI driver",
            4 => "FMP",
            _ => "unknown",
        }
    }

    /// EFI_SYSTEM_RESOURCE_ENTRY LastAttemptStatus decode (subset).
    pub fn last_status_name(&self) -> &'static str {
        match self.last_attempt_status {
            0 => "success",
            1 => "unsuccessful",
            2 => "insufficient resources",
            3 => "incorrect version",
            4 => "invalid image format",
            5 => "auth error",
            6 => "AC not connected",
            7 => "insufficient battery",
            _ => "other",
        }
    }
}

#[derive(Default)]
pub struct EsrtInfo {
    pub present: bool,
    pub fw_resource_count: u32,
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

fn rd_u32(p: usize) -> u32 {
    unsafe { core::ptr::read_unaligned(p as *const u32) }
}
fn rd_u64(p: usize) -> u64 {
    unsafe { core::ptr::read_unaligned(p as *const u64) }
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
        let version = rd_u64(base + 8);
        if version == 1 && count <= 256 {
            info.present = true;
            info.fw_resource_count = count;
            let entry_base = base + 16;
            for i in 0..count as usize {
                let ep = entry_base + i * 40;
                let g = unsafe {
                    let mut b = [0u8; 16];
                    core::ptr::copy_nonoverlapping(ep as *const u8, b.as_mut_ptr(), 16);
                    uefi::Guid::from_bytes(b)
                };
                info.entries.push(EsrtEntry {
                    fw_class: format!("{g}"),
                    fw_type: rd_u32(ep + 16),
                    fw_version: rd_u32(ep + 20),
                    lowest_supported: rd_u32(ep + 24),
                    last_attempt_version: rd_u32(ep + 32),
                    last_attempt_status: rd_u32(ep + 36),
                });
            }
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

/// Whether the platform will accept `new_version` for the system firmware:
/// must be >= the ESRT lowest-supported version and newer than current.
pub fn update_verdict(info: &EsrtInfo, new_version: u32) -> Result<String, String> {
    let e = info.system_entry().ok_or("no system-firmware ESRT entry")?;
    if new_version < e.lowest_supported {
        return Err(format!(
            "0x{:08x} is below lowest-supported 0x{:08x} (rollback blocked)",
            new_version, e.lowest_supported
        ));
    }
    if new_version <= e.fw_version {
        return Ok(format!(
            "0x{:08x} is not newer than current 0x{:08x}",
            new_version, e.fw_version
        ));
    }
    Ok(format!(
        "0x{:08x} accepted (current 0x{:08x}, floor 0x{:08x})",
        new_version, e.fw_version, e.lowest_supported
    ))
}

/// Flash a signed vendor capsule. DANGEROUS and irreversible: on success this
/// hands the image to the firmware and resets — it does not return. The buffer
/// is copied into page-aligned runtime memory so it survives the reset and
/// satisfies `CapsuleHeader` alignment.
pub fn apply_capsule(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() < core::mem::size_of::<CapsuleHeader>() {
        return Err("capsule shorter than its header".into());
    }
    // Validate the declared sizes before trusting the image.
    let header_size = u32::from_le_bytes(bytes[16..20].try_into().unwrap());
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

    // Page-aligned, reset-surviving copy.
    let pages = bytes.len().div_ceil(4096);
    let mem = boot::allocate_pages(AllocateType::AnyPages, MemoryType::RUNTIME_SERVICES_DATA, pages)
        .map_err(|e| format!("allocate_pages: {e:?}"))?;
    let dst = mem.as_ptr();
    unsafe {
        core::ptr::copy_nonoverlapping(bytes.as_ptr(), dst, bytes.len());
    }
    let header = unsafe { &*(dst as *const CapsuleHeader) };
    let headers = [header];

    let reset_type = match runtime::query_capsule_capabilities(&headers) {
        Ok(cap) => {
            logln(format!(
                "capsule: accepted by firmware (max {} bytes, reset {:?})",
                cap.maximum_capsule_size, cap.reset_type
            ));
            cap.reset_type
        }
        Err(e) => return Err(format!("firmware rejected capsule (QueryCapsuleCapabilities: {e:?})")),
    };

    let blocks = [
        CapsuleBlockDescriptor { length: bytes.len() as u64, address: dst as u64 },
        CapsuleBlockDescriptor { length: 0, address: 0 },
    ];
    logln("capsule: calling UpdateCapsule (point of no return)".into());
    runtime::update_capsule(&headers, &blocks).map_err(|e| format!("UpdateCapsule: {e:?}"))?;

    // Vendor capsules usually set INITIATE_RESET and never return here; if the
    // firmware coalesced without resetting, do the required reset ourselves.
    logln("capsule: UpdateCapsule returned; resetting to apply".into());
    runtime::reset(reset_type, Status::SUCCESS, None);
}

/// `firmware_update` object for the fingerprint upload.
pub fn esrt_json(info: &EsrtInfo) -> serde_json::Value {
    let entries: Vec<serde_json::Value> = info
        .entries
        .iter()
        .map(|e| {
            serde_json::json!({
                "fw_class": e.fw_class,
                "type": e.type_name(),
                "version": format!("0x{:08x}", e.fw_version),
                "lowest_supported": format!("0x{:08x}", e.lowest_supported),
                "last_attempt_version": format!("0x{:08x}", e.last_attempt_version),
                "last_attempt_status": e.last_status_name(),
            })
        })
        .collect();
    serde_json::json!({
        "esrt_present": info.present,
        "resource_count": info.fw_resource_count,
        "capsule_on_disk": info.capsule_on_disk,
        "os_indications_supported": format!("0x{:016x}", info.os_indications_supported),
        "entries": entries,
    })
}
