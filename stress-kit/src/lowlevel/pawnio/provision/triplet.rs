//! Provisioning route that installs the embedded x64 driver package directly.
//!
//! Three steps, none of them optional. `SetupCopyOEMInfW` stages the package
//! into the driver store, which is what registers the catalog carrying the
//! Microsoft attestation signature — without it the `.sys` fails kernel signing
//! policy, since its own embedded signature is only the author's code-signing
//! cert. Then a root-enumerated devnode is created, because PawnIO builds its
//! device object in AddDevice and the PnP manager calls that only for a devnode
//! whose hardware id matches. Finally the staged package is bound to that node.

use std::path::{Path, PathBuf};

use windows::core::{BOOL, PCWSTR};
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    SetupCopyOEMInfW, SetupDiCallClassInstaller, SetupDiCreateDeviceInfoList,
    SetupDiCreateDeviceInfoW, SetupDiDestroyDeviceInfoList, SetupDiSetDeviceRegistryPropertyW,
    UpdateDriverForPlugAndPlayDevicesW, DICD_GENERATE_ID, DIF_REGISTERDEVICE, HDEVINFO,
    INSTALLFLAG_FORCE, SPDRP_HARDWAREID, SPOST_PATH, SP_COPY_STYLE, SP_DEVINFO_DATA,
};

use crate::lowlevel::busmutex::wide;
use crate::lowlevel::staging;
use crate::lowlevel::pawnio::devnode::{DEVCLASS_SOFTWAREDEVICE, HARDWARE_ID};

const SYS: (&str, &[u8]) = (
    "PawnIO.sys",
    include_bytes!("../../../../drivers/pawnio/x64/PawnIO.sys"),
);
const INF: (&str, &[u8]) = (
    "PawnIO.inf",
    include_bytes!("../../../../drivers/pawnio/x64/PawnIO.inf"),
);
const CAT: (&str, &[u8]) = (
    "PawnIO.cat",
    include_bytes!("../../../../drivers/pawnio/x64/PawnIO.cat"),
);

/// Subdirectory of the locked staging directory holding the package.
const PACKAGE_DIR: &str = "pawnio";

pub fn install() -> Result<(), String> {
    if cfg!(not(target_arch = "x86_64")) {
        return Err("only the x64 PawnIO package is embedded".to_string());
    }
    let inf = stage()?;
    stage_package(&inf)?;
    create_devnode()?;
    bind_driver(&inf)
}

/// Writes the package where only SYSTEM and Administrators can reach it. The
/// three files must land in one directory and keep these names: the INF names
/// its catalog and its `.sys` by name, and the catalog hashes them by name.
fn stage() -> Result<PathBuf, String> {
    let dir = staging::driver_dir()
        .map(|base| base.join(PACKAGE_DIR))
        .ok_or_else(|| {
            "could not create the Mastertech drivers directory under %ProgramData% restricted \
             to SYSTEM and Administrators; refusing to install a driver from a world-writable \
             path"
                .to_string()
        })?;
    if !staging::locked_dir(&dir) {
        return Err(format!(
            "could not lock {} down to SYSTEM and Administrators",
            dir.display()
        ));
    }
    for (name, bytes) in [SYS, INF, CAT] {
        std::fs::write(dir.join(name), bytes)
            .map_err(|e| format!("could not stage {name}: {e}"))?;
    }
    Ok(dir.join(INF.0))
}

/// Copies the package into the driver store and registers its catalog.
fn stage_package(inf: &Path) -> Result<(), String> {
    let source = wide(&inf.to_string_lossy());
    unsafe {
        SetupCopyOEMInfW(
            PCWSTR(source.as_ptr()),
            PCWSTR::null(),
            SPOST_PATH,
            SP_COPY_STYLE(0),
            None,
            None,
            None,
        )
    }
    .map(|_| ())
    .map_err(|e| classify("stage the PawnIO driver package", e))
}

/// Registers a root-enumerated node carrying our hardware id, so the PnP
/// manager has something to call PawnIO's AddDevice for.
fn create_devnode() -> Result<(), String> {
    let set = unsafe { SetupDiCreateDeviceInfoList(Some(&DEVCLASS_SOFTWAREDEVICE as *const _), None) }
        .map_err(|e| classify("create the PawnIO device list", e))?;
    let result = register_devnode(set);
    let _ = unsafe { SetupDiDestroyDeviceInfoList(set) };
    result
}

fn register_devnode(set: HDEVINFO) -> Result<(), String> {
    let class_name = wide("PawnIO");
    let mut data = SP_DEVINFO_DATA {
        cbSize: size_of::<SP_DEVINFO_DATA>() as u32,
        ..Default::default()
    };
    unsafe {
        SetupDiCreateDeviceInfoW(
            set,
            PCWSTR(class_name.as_ptr()),
            &DEVCLASS_SOFTWAREDEVICE,
            PCWSTR::null(),
            None,
            DICD_GENERATE_ID,
            Some(&mut data),
        )
    }
    .map_err(|e| classify("create the PawnIO device node", e))?;

    // REG_MULTI_SZ: the id, its terminator, and the list terminator.
    let mut ids = wide(HARDWARE_ID);
    ids.push(0);
    let bytes: Vec<u8> = ids.iter().flat_map(|unit| unit.to_le_bytes()).collect();
    unsafe { SetupDiSetDeviceRegistryPropertyW(set, &mut data, SPDRP_HARDWAREID, Some(&bytes)) }
        .map_err(|e| classify("set the PawnIO hardware id", e))?;

    unsafe { SetupDiCallClassInstaller(DIF_REGISTERDEVICE, set, Some(&data as *const _)) }
        .map_err(|e| classify("register the PawnIO device node", e))
}

/// Binds the staged package to the node we just created. Only ever called for a
/// fresh node, so there is no running device for the install to tear down.
fn bind_driver(inf: &Path) -> Result<(), String> {
    let hardware_id = wide(HARDWARE_ID);
    let path = wide(&inf.to_string_lossy());
    let mut reboot = BOOL(0);
    unsafe {
        UpdateDriverForPlugAndPlayDevicesW(
            None,
            PCWSTR(hardware_id.as_ptr()),
            PCWSTR(path.as_ptr()),
            INSTALLFLAG_FORCE,
            Some(&mut reboot),
        )
    }
    .map_err(|e| classify("install the PawnIO driver on its device node", e))?;
    // A reboot request means the device is not coming up in this boot, so it is
    // reported rather than warned about and then waited on.
    if reboot.as_bool() {
        return Err(
            "the PawnIO driver install needs a reboot to finish, so its device will not appear \
             until the machine restarts"
                .to_string(),
        );
    }
    Ok(())
}

/// Maps a driver-install failure to guidance an operator can act on.
fn classify(action: &str, error: windows::core::Error) -> String {
    const ERROR_ACCESS_DENIED: i32 = 0x8007_0005u32 as i32;
    const ERROR_IN_WOW64: i32 = 0x8007_0216u32 as i32;
    const ERROR_SIGNATURE_OSATTRIBUTE_MISMATCH: i32 = 0x800B_0110u32 as i32;

    let code = error.code().0;
    match code {
        ERROR_ACCESS_DENIED => format!("could not {action}: denied; run elevated"),
        ERROR_IN_WOW64 => format!(
            "could not {action}: a 32-bit process cannot install a 64-bit driver package"
        ),
        ERROR_SIGNATURE_OSATTRIBUTE_MISMATCH => format!(
            "could not {action}: the package signature was rejected. The .inf, .cat and .sys are \
             signed as one set, so any edit to one invalidates all three"
        ),
        _ => format!("could not {action}: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The staged file names are the ones the INF and its catalog reference, and
    /// the hardware id is the one the devnode is registered with. Renaming any
    /// of them breaks the install with no diagnostic beyond a Win32 error.
    #[test]
    fn the_embedded_package_is_the_x64_one_and_agrees_with_the_inf() {
        let inf = String::from_utf8_lossy(INF.1);
        assert!(inf.contains("ntamd64"), "the embedded INF is not the x64 one");
        assert!(inf.contains(SYS.0), "the INF does not name {}", SYS.0);
        assert!(inf.contains(CAT.0), "the INF does not name {}", CAT.0);
        assert!(inf.contains(HARDWARE_ID), "the INF does not bind {HARDWARE_ID}");
    }

    /// The catalog carries the Microsoft attestation signature and is a signed
    /// PKCS#7 blob; an empty or text file here would fail at install time.
    #[test]
    fn the_catalog_is_a_der_sequence() {
        assert_eq!(&CAT.1[..2], &[0x30, 0x82], "the catalog is not DER");
    }

    /// The driver is the x64 build, not one of the ARM64 ones the installer
    /// also carries.
    #[test]
    fn the_driver_is_an_amd64_image() {
        let pe = u32::from_le_bytes(SYS.1[0x3C..0x40].try_into().unwrap()) as usize;
        let machine = u16::from_le_bytes(SYS.1[pe + 4..pe + 6].try_into().unwrap());
        assert_eq!(machine, 0x8664, "the embedded driver is not AMD64");
    }
}
