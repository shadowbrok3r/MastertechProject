//! Counts the `Root\PawnIO` device nodes on this machine.
//!
//! Every install route creates a node, and none of them checks first, so this
//! is what stops a second route from adding one beside a node that is already
//! there. Two nodes for one hardware id is not a benign duplicate: only one of
//! them can own the `\Device\PawnIO` symlink, and upstream's installer starts
//! failing with `STATUS_OBJECT_NAME_COLLISION`.

use windows::core::{GUID, PCWSTR};
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
    SetupDiGetDeviceRegistryPropertyW, HDEVINFO, SETUP_DI_GET_CLASS_DEVS_FLAGS, SPDRP_HARDWAREID,
    SP_DEVINFO_DATA,
};

/// Hardware id every PawnIO node carries.
pub const HARDWARE_ID: &str = r"Root\PawnIO";

/// `SoftwareDevice`, the setup class the INF declares.
pub const DEVCLASS_SOFTWAREDEVICE: GUID = GUID::from_u128(0x62f9c741_b25a_46ce_b54c_9bccce08b6f2);

/// How many nodes carry our hardware id. Counts nodes that are registered but
/// not started as well: those still occupy the id, and installing beside one
/// is what produces a duplicate.
pub fn count() -> usize {
    let Ok(set) = (unsafe {
        SetupDiGetClassDevsW(
            Some(&DEVCLASS_SOFTWAREDEVICE as *const _),
            PCWSTR::null(),
            None,
            SETUP_DI_GET_CLASS_DEVS_FLAGS(0),
        )
    }) else {
        // Enumeration itself failed, so nothing is known. Reporting one node
        // keeps callers from installing on a guess.
        log::debug!("stress-kit/pawnio: could not enumerate SoftwareDevice nodes");
        return 1;
    };
    let mut index = 0u32;
    let mut found = 0usize;
    loop {
        let mut data = SP_DEVINFO_DATA {
            cbSize: size_of::<SP_DEVINFO_DATA>() as u32,
            ..Default::default()
        };
        if unsafe { SetupDiEnumDeviceInfo(set, index, &mut data) }.is_err() {
            break;
        }
        index += 1;
        if carries_our_hardware_id(set, &data) {
            found += 1;
        }
    }
    let _ = unsafe { SetupDiDestroyDeviceInfoList(set) };
    found
}

fn carries_our_hardware_id(set: HDEVINFO, data: &SP_DEVINFO_DATA) -> bool {
    let mut buffer = [0u8; 512];
    let mut needed = 0u32;
    let read = unsafe {
        SetupDiGetDeviceRegistryPropertyW(
            set,
            data,
            SPDRP_HARDWAREID,
            None,
            Some(&mut buffer),
            Some(&mut needed),
        )
    };
    if read.is_err() {
        return false;
    }
    let units: Vec<u16> = buffer
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    units
        .split(|&unit| unit == 0)
        .filter(|id| !id.is_empty())
        .any(|id| String::from_utf16_lossy(id).eq_ignore_ascii_case(HARDWARE_ID))
}
