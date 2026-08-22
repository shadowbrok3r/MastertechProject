//! Extracts the embedded WinRing0 driver and runs it as a kernel service.

use std::ffi::{c_void, CString};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::sync::Mutex;

use winapi::um::winnt::{SERVICE_DEMAND_START, SERVICE_ERROR_NORMAL, SERVICE_KERNEL_DRIVER};
use winapi::um::winsvc::{
    CloseServiceHandle, ControlService, CreateServiceW, DeleteService, OpenSCManagerW,
    OpenServiceW, StartServiceW, SC_HANDLE, SC_MANAGER_ALL_ACCESS, SERVICE_ALL_ACCESS,
    SERVICE_CONTROL_STOP, SERVICE_STATUS,
};
use winapi::shared::sddl;
use winapi::um::{
    errhandlingapi, fileapi, handleapi, minwinbase, securitybaseapi, winbase, winnt,
};

/// Device name baked into this WinRing0 build; the user-mode path and the
/// service name both use it.
pub const SERVICE_NAME: &str = "WinRing0_1_2_0";
pub const DEVICE_PATH: &str = r"\\.\WinRing0_1_2_0";
const DRIVER_BYTES: &[u8] = include_bytes!("../../../drivers/WinRing0x64.sys");

const ERROR_SHARING_VIOLATION: u32 = 32;
const ERROR_FILE_NOT_FOUND: u32 = 2;
const ERROR_ACCESS_DENIED: u32 = 5;
const ERROR_SERVICE_ALREADY_RUNNING: u32 = 1056;
const ERROR_SERVICE_MARKED_FOR_DELETE: u32 = 1072;
const ERROR_SERVICE_EXISTS: u32 = 1073;
const ERROR_DRIVER_BLOCKED: u32 = 1275;

pub fn wide(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

const DRIVER_FILE: &str = "WinRing0_mtech.sys";

/// Grants only SYSTEM and Administrators, inherited by files created inside.
const DRIVER_DIR_SDDL: &str = "D:P(A;OICI;GA;;;SY)(A;OICI;GA;;;BA)";

const ERROR_ALREADY_EXISTS: u32 = 183;

/// Staging directory for the driver image, locked to SYSTEM and Administrators.
///
/// `StartServiceW` loads whatever sits at this path, so a world-writable
/// directory lets any non-admin process swap the image between the write and the
/// load and reach ring 0. Returns `None` when the directory cannot be created or
/// locked down, in which case the driver is not staged at all.
fn driver_dir() -> Option<PathBuf> {
    let base = std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".to_string());
    let parent = Path::new(&base).join("Mastertech");
    std::fs::create_dir_all(&parent).ok()?;
    let dir = parent.join("drivers");
    locked_dir(&dir).then_some(dir)
}

fn driver_path() -> Option<PathBuf> {
    Some(driver_dir()?.join(DRIVER_FILE))
}

/// Creates `path` with [`DRIVER_DIR_SDDL`], or re-applies that DACL when a
/// previous build already created it.
fn locked_dir(path: &Path) -> bool {
    let Some(sd) = SecurityDescriptor::from_sddl(DRIVER_DIR_SDDL) else {
        log::warn!("stress-kit/winring0: could not build the driver-directory security descriptor");
        return false;
    };
    let wide_path = wide(&path.to_string_lossy());
    let mut attrs = minwinbase::SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<minwinbase::SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd.0,
        bInheritHandle: 0,
    };
    if unsafe { fileapi::CreateDirectoryW(wide_path.as_ptr(), &mut attrs) } != 0 {
        return true;
    }
    if unsafe { errhandlingapi::GetLastError() } != ERROR_ALREADY_EXISTS {
        return false;
    }
    // Re-apply, so a directory left by an older build cannot stay user-writable.
    let applied = unsafe {
        securitybaseapi::SetFileSecurityW(
            wide_path.as_ptr(),
            winnt::DACL_SECURITY_INFORMATION,
            sd.0,
        )
    } != 0;
    if !applied {
        log::warn!(
            "stress-kit/winring0: could not lock {} down to SYSTEM and Administrators",
            path.display()
        );
    }
    applied
}

/// Security descriptor parsed from SDDL, freed on drop.
struct SecurityDescriptor(*mut c_void);

impl SecurityDescriptor {
    fn from_sddl(sddl_text: &str) -> Option<Self> {
        let mut sd: *mut c_void = null_mut();
        let ok = unsafe {
            sddl::ConvertStringSecurityDescriptorToSecurityDescriptorW(
                wide(sddl_text).as_ptr(),
                1, // SDDL_REVISION_1
                &mut sd,
                null_mut(),
            )
        };
        (ok != 0 && !sd.is_null()).then_some(Self(sd))
    }
}

impl Drop for SecurityDescriptor {
    fn drop(&mut self) {
        unsafe { winbase::LocalFree(self.0) };
    }
}

/// Removes the driver image older builds staged in the world-writable temp
/// directory.
fn remove_legacy_temp_copy() {
    let _ = std::fs::remove_file(std::env::temp_dir().join(DRIVER_FILE));
}

pub fn open_device() -> Option<winnt::HANDLE> {
    let path = CString::new(DEVICE_PATH).ok()?;
    let handle = unsafe {
        fileapi::CreateFileA(
            path.as_ptr(),
            winnt::GENERIC_READ | winnt::GENERIC_WRITE,
            0,
            null_mut(),
            fileapi::OPEN_EXISTING,
            winnt::FILE_ATTRIBUTE_NORMAL,
            null_mut(),
        )
    };
    if handle == handleapi::INVALID_HANDLE_VALUE {
        None
    } else {
        Some(handle)
    }
}

/// Callers currently holding the driver loaded. Guards the stop-delete-rewrite
/// sequence below, which would otherwise pull the driver out from under a
/// handle another caller in this process is still using.
static LOADS: Mutex<usize> = Mutex::new(0);

/// Extracts the embedded driver and starts it as a kernel service. A stale
/// service of the same name is removed first so our `.sys` is the one loaded.
/// Reference-counted: a load while the driver is already up just takes a share.
pub fn load_driver() -> Result<(), String> {
    let mut loads = LOADS.lock().unwrap_or_else(|e| e.into_inner());
    if *loads > 0 {
        *loads += 1;
        return Ok(());
    }
    load_driver_uncounted()?;
    *loads = 1;
    Ok(())
}

fn load_driver_uncounted() -> Result<(), String> {
    let Some(path) = driver_path() else {
        return Err(
            "could not create a staging directory for the driver under \
             %ProgramData%\\Mastertech\\drivers restricted to SYSTEM and Administrators; \
             refusing to load a kernel image from a world-writable path"
                .to_string(),
        );
    };
    remove_legacy_temp_copy();

    unsafe {
        let scm = OpenSCManagerW(null(), null(), SC_MANAGER_ALL_ACCESS);
        if scm.is_null() {
            return Err(classify("open service manager", errhandlingapi::GetLastError()));
        }

        // Clear any stale instance FIRST. A prior run — or a force-kill that
        // skipped Drop — leaves the driver loaded, and a running kernel service
        // keeps its .sys file open, so the file can't be rewritten until the
        // service is stopped. Doing this before the write makes load self-healing
        // across crashes and re-runs.
        remove_service(scm);

        if let Err(e) = write_driver_with_retry(&path) {
            CloseServiceHandle(scm);
            return Err(e);
        }

        let name = wide(SERVICE_NAME);
        let display = wide("Mastertech CPU Thermal (WinRing0)");
        let bin = wide(&path.to_string_lossy());
        let mut svc: SC_HANDLE = null_mut();
        let mut create_err = 0u32;
        for attempt in 0..5 {
            svc = CreateServiceW(
                scm,
                name.as_ptr(),
                display.as_ptr(),
                SERVICE_ALL_ACCESS,
                SERVICE_KERNEL_DRIVER,
                SERVICE_DEMAND_START,
                SERVICE_ERROR_NORMAL,
                bin.as_ptr(),
                null(),
                null_mut(),
                null(),
                null(),
                null(),
            );
            if !svc.is_null() {
                break;
            }
            create_err = errhandlingapi::GetLastError();
            if create_err == ERROR_SERVICE_EXISTS {
                svc = OpenServiceW(scm, name.as_ptr(), SERVICE_ALL_ACCESS);
                break;
            }
            // A just-deleted service can linger "marked for delete" until its last
            // handle closes; back off and retry.
            if create_err != ERROR_SERVICE_MARKED_FOR_DELETE {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(150 * (attempt + 1)));
        }
        if svc.is_null() {
            CloseServiceHandle(scm);
            return Err(classify("create driver service", create_err));
        }

        if StartServiceW(svc, 0, null_mut()) == 0 {
            let err = errhandlingapi::GetLastError();
            if err != ERROR_SERVICE_ALREADY_RUNNING {
                CloseServiceHandle(svc);
                CloseServiceHandle(scm);
                return Err(classify("start driver service", err));
            }
        }

        CloseServiceHandle(svc);
        CloseServiceHandle(scm);
    }
    Ok(())
}

/// Write the embedded driver, retrying briefly on a sharing violation (the prior
/// driver still unloading, or AV momentarily holding the file).
fn write_driver_with_retry(path: &PathBuf) -> Result<(), String> {
    let mut last = 0u32;
    for attempt in 0..5 {
        match std::fs::write(path, DRIVER_BYTES) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last = e.raw_os_error().unwrap_or(0) as u32;
                if last != ERROR_SHARING_VIOLATION {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(150 * (attempt + 1)));
            }
        }
    }
    Err(classify("write driver", last))
}

/// Releases one share; the last one out stops the service and removes the image.
pub fn unload_driver() {
    let mut loads = LOADS.lock().unwrap_or_else(|e| e.into_inner());
    *loads = loads.saturating_sub(1);
    if *loads > 0 {
        return;
    }
    unsafe {
        let scm = OpenSCManagerW(null(), null(), SC_MANAGER_ALL_ACCESS);
        if !scm.is_null() {
            remove_service(scm);
            CloseServiceHandle(scm);
        }
    }
    if let Some(path) = driver_path() {
        let _ = std::fs::remove_file(path);
    }
    remove_legacy_temp_copy();
}

/// Best-effort stop + delete of our service. Caller owns the SCM handle.
fn remove_service(scm: SC_HANDLE) {
    let name = wide(SERVICE_NAME);
    unsafe {
        let svc = OpenServiceW(scm, name.as_ptr(), SERVICE_ALL_ACCESS);
        if !svc.is_null() {
            let mut status: SERVICE_STATUS = std::mem::zeroed();
            ControlService(svc, SERVICE_CONTROL_STOP, &mut status);
            DeleteService(svc);
            CloseServiceHandle(svc);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_staging_dacl_parses_and_a_bad_one_does_not() {
        assert!(SecurityDescriptor::from_sddl(DRIVER_DIR_SDDL).is_some());
        assert!(SecurityDescriptor::from_sddl("not-a-security-descriptor").is_none());
    }

    /// The image must never be staged under the world-writable temp directory,
    /// where another process could swap it between the write and `StartService`.
    #[test]
    fn the_driver_is_not_staged_in_the_temp_directory() {
        let Some(path) = driver_path() else {
            return; // Directory could not be locked down, so nothing is staged.
        };
        assert!(!path.starts_with(std::env::temp_dir()));
        assert!(path.ends_with(DRIVER_FILE));
        assert!(
            path.to_string_lossy().contains("Mastertech"),
            "staged outside the app's own directory: {}",
            path.display()
        );
    }
}

/// Map a Win32 error code to operator-actionable guidance.
pub fn classify(action: &str, code: u32) -> String {
    match code {
        ERROR_DRIVER_BLOCKED => format!(
            "{action} blocked by policy (err 1275). The legacy WinRing0 backend needs Memory \
             Integrity (Core Isolation) AND the Microsoft Vulnerable Driver Blocklist \
             (VulnerableDriverBlocklistEnable=0) off, then a reboot; a signed backend does not"
        ),
        ERROR_FILE_NOT_FOUND => format!(
            "{action} failed — driver file missing (err 2); Defender likely quarantined it, add an exclusion"
        ),
        ERROR_ACCESS_DENIED => format!("{action} denied (err 5); run elevated"),
        ERROR_SHARING_VIOLATION => format!(
            "{action} failed — driver file locked (err 32); a prior instance's driver is still \
             loaded. Close other Mastertech instances (a reboot clears it)."
        ),
        other => format!("{action} failed (err {other})"),
    }
}
