//! PawnIO device handle and loaded bytecode modules.
//!
//! One device handle per module: `IOCTL_PIO_LOAD_BINARY` binds the module to the
//! handle it arrived on, and closing that handle unloads it.

use std::ffi::c_void;
use std::ptr::null_mut;

use winapi::shared::minwindef::DWORD;
use winapi::um::{errhandlingapi, fileapi, handleapi, ioapiset, winnt};

use crate::lowlevel::busmutex::wide;

/// Device object PawnIO creates from its PnP AddDevice routine, so it exists
/// only once the `Root\PawnIO` devnode does.
const DEVICE_PATH: &str = r"\\?\GLOBALROOT\Device\PawnIO";

const DEVICE_TYPE: u32 = 41394;
const METHOD_BUFFERED: u32 = 0;

const fn ctl_code(function: u32) -> u32 {
    (DEVICE_TYPE << 16) | (function << 2) | METHOD_BUFFERED
}
const IOCTL_LOAD_BINARY: u32 = ctl_code(0x821);
const IOCTL_EXECUTE_FN: u32 = ctl_code(0x841);
const IOCTL_VERSION: u32 = ctl_code(0x861);

/// Leading bytes of an execute request holding the ASCII function name.
const FN_NAME_LEN: usize = 32;
/// Argument slots the fixed request buffer carries.
const MAX_ARGS: usize = 4;

fn last_error() -> u32 {
    unsafe { errhandlingapi::GetLastError() }
}

fn open_handle() -> Result<winnt::HANDLE, Absent> {
    let path = wide(DEVICE_PATH);
    let handle = unsafe {
        fileapi::CreateFileW(
            path.as_ptr(),
            winnt::GENERIC_READ | winnt::GENERIC_WRITE,
            winnt::FILE_SHARE_READ | winnt::FILE_SHARE_WRITE,
            null_mut(),
            fileapi::OPEN_EXISTING,
            winnt::FILE_ATTRIBUTE_NORMAL,
            null_mut(),
        )
    };
    if handle == handleapi::INVALID_HANDLE_VALUE {
        return Err(Absent::Open(last_error()));
    }
    Ok(handle)
}

/// Why a version read produced nothing. Carried rather than discarded: without
/// the Win32 code there is no way to tell an absent driver from a refused open
/// from a rejected request, and all three read the same from outside.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Absent {
    /// `CreateFileW` on the device path failed. Error 2 means no device object,
    /// which is the ordinary "PawnIO is not installed" answer.
    Open(u32),
    /// The device opened but rejected the version request.
    Ioctl(u32),
}

impl std::fmt::Display for Absent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Open(2) => write!(f, "no PawnIO device object (err 2)"),
            Self::Open(code) => write!(f, "PawnIO device would not open (err {code})"),
            Self::Ioctl(code) => {
                write!(f, "PawnIO device opened but refused the version request (err {code})")
            }
        }
    }
}

/// Driver version as `(major, minor, patch)`, and the cheapest proof the driver
/// is installed and running. The driver rejects any output length but four.
pub fn version() -> Result<(u16, u8, u8), Absent> {
    let handle = open_handle()?;
    let mut out = [0u8; 4];
    let mut returned: DWORD = 0;
    let ok = unsafe {
        ioapiset::DeviceIoControl(
            handle,
            IOCTL_VERSION,
            null_mut(),
            0,
            out.as_mut_ptr() as *mut c_void,
            out.len() as DWORD,
            &mut returned,
            null_mut(),
        )
    };
    let failed = Absent::Ioctl(last_error());
    unsafe { handleapi::CloseHandle(handle) };
    if ok == 0 {
        return Err(failed);
    }
    let raw = u32::from_le_bytes(out);
    Ok(((raw >> 16) as u16, (raw >> 8) as u8, raw as u8))
}

pub struct Module {
    device: winnt::HANDLE,
    label: &'static str,
}

// The handle is process-global and DeviceIoControl is thread-safe; the module's
// own VM state is serialized by the caller's bus lease.
unsafe impl Send for Module {}
unsafe impl Sync for Module {}

impl Module {
    /// Opens a device handle and loads `blob` into it verbatim: the `.bin` is
    /// already the `[u32 sig_len][signature][bytecode]` the driver expects, and
    /// the driver checks that signature against its trusted keys.
    pub fn load(label: &'static str, blob: &[u8]) -> Result<Self, String> {
        let device = open_handle().map_err(|why| format!("{why}; {label} not loaded"))?;
        // Bound to `me` first so the error paths below close the handle.
        let me = Self { device, label };
        let mut returned: DWORD = 0;
        let ok = unsafe {
            ioapiset::DeviceIoControl(
                device,
                IOCTL_LOAD_BINARY,
                blob.as_ptr() as *mut c_void,
                blob.len() as DWORD,
                null_mut(),
                0,
                &mut returned,
                null_mut(),
            )
        };
        if ok == 0 {
            return Err(format!(
                "PawnIO rejected the {label} module (err {}); its `main` refuses hardware it does \
                 not support",
                last_error()
            ));
        }
        Ok(me)
    }

    /// Calls a named function in this module. `None` on any driver-side failure,
    /// including the module refusing a register outside its allowlist.
    pub fn execute<const IN: usize, const OUT: usize>(
        &self,
        function: &str,
        input: [i64; IN],
    ) -> Option<[i64; OUT]> {
        const { assert!(IN <= MAX_ARGS) };

        let mut request = [0u8; FN_NAME_LEN + MAX_ARGS * 8];
        let name = function.as_bytes();
        let named = name.len().min(FN_NAME_LEN - 1);
        request[..named].copy_from_slice(&name[..named]);
        for (slot, arg) in input.iter().enumerate() {
            let at = FN_NAME_LEN + slot * 8;
            request[at..at + 8].copy_from_slice(&arg.to_le_bytes());
        }

        let mut response = [[0u8; 8]; OUT];
        let mut returned: DWORD = 0;
        let out_ptr = if OUT == 0 {
            null_mut()
        } else {
            response.as_mut_ptr() as *mut c_void
        };
        let ok = unsafe {
            ioapiset::DeviceIoControl(
                self.device,
                IOCTL_EXECUTE_FN,
                request.as_mut_ptr() as *mut c_void,
                (FN_NAME_LEN + IN * 8) as DWORD,
                out_ptr,
                (OUT * 8) as DWORD,
                &mut returned,
                null_mut(),
            )
        };
        if ok == 0 {
            log::debug!(
                "stress-kit/pawnio: {}::{function} failed (err {})",
                self.label,
                last_error()
            );
            return None;
        }
        (returned as usize >= OUT * 8).then(|| response.map(i64::from_le_bytes))
    }
}

impl Drop for Module {
    fn drop(&mut self) {
        unsafe { handleapi::CloseHandle(self.device) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A raw string one backslash short of `\\?\` still compiles, still reads
    /// correctly at a glance, and makes every open fail with
    /// ERROR_INVALID_NAME. Nothing else in the build catches it.
    #[test]
    fn the_device_path_is_a_well_formed_win32_path() {
        assert_eq!(DEVICE_PATH, "\\\\?\\GLOBALROOT\\Device\\PawnIO");
    }

    /// `CTL_CODE(41394, fn, METHOD_BUFFERED, FILE_ANY_ACCESS)`, checked against
    /// the live driver.
    #[test]
    fn control_codes_match_the_driver() {
        assert_eq!(IOCTL_LOAD_BINARY, 0xA1B2_2084);
        assert_eq!(IOCTL_EXECUTE_FN, 0xA1B2_2104);
        assert_eq!(IOCTL_VERSION, 0xA1B2_2184);
    }
}
