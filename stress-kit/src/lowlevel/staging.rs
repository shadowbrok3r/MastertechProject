//! Staging directory for kernel images and driver installers.
//!
//! Windows loads whatever sits at the staged path, so a world-writable
//! directory lets any non-admin process swap the file between the write and the
//! load and reach ring 0. Everything staged here is locked to SYSTEM and
//! Administrators first.

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::ptr::null_mut;

use winapi::shared::sddl;
use winapi::um::{errhandlingapi, fileapi, minwinbase, securitybaseapi, winbase, winnt};

use super::busmutex::wide;

/// Grants only SYSTEM and Administrators, inherited by files created inside.
const DIR_SDDL: &str = "D:P(A;OICI;GA;;;SY)(A;OICI;GA;;;BA)";

const ERROR_ALREADY_EXISTS: u32 = 183;

/// `%ProgramData%\Mastertech\drivers`, created and locked down. `None` when the
/// directory cannot be created or locked, in which case nothing is staged.
pub fn driver_dir() -> Option<PathBuf> {
    let base = std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".to_string());
    let parent = Path::new(&base).join("Mastertech");
    std::fs::create_dir_all(&parent).ok()?;
    let dir = parent.join("drivers");
    locked_dir(&dir).then_some(dir)
}

/// Creates `path` with [`DIR_SDDL`], or re-applies that DACL when a previous
/// build already created it.
pub fn locked_dir(path: &Path) -> bool {
    let Some(sd) = SecurityDescriptor::from_sddl(DIR_SDDL) else {
        log::warn!("stress-kit/lowlevel: could not build the staging-directory security descriptor");
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
            "stress-kit/lowlevel: could not lock {} down to SYSTEM and Administrators",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_staging_dacl_parses_and_a_bad_one_does_not() {
        assert!(SecurityDescriptor::from_sddl(DIR_SDDL).is_some());
        assert!(SecurityDescriptor::from_sddl("not-a-security-descriptor").is_none());
    }

    /// Nothing is ever staged under the world-writable temp directory, where
    /// another process could swap it between the write and the load.
    #[test]
    fn the_staging_directory_is_not_the_temp_directory() {
        let Some(dir) = driver_dir() else {
            return; // Directory could not be locked down, so nothing is staged.
        };
        assert!(!dir.starts_with(std::env::temp_dir()));
        assert!(dir.to_string_lossy().contains("Mastertech"));
    }
}
