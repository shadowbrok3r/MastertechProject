//! Console attachment for a `windows_subsystem = "windows"` build.
//!
//! A GUI-subsystem process starts with no console and invalid standard handles, so `eprintln!`,
//! `env_logger`, and ratatui's `io::stdout()` backend all write nowhere. [`attach_parent`] adopts
//! the launching terminal's console when there is one; [`ensure_console`] allocates a fresh one
//! for terminal mode when there is not. Both rebind the process standard handles afterwards —
//! attaching alone does not repoint handles the loader already resolved.

/// Whether the process currently owns a console.
pub fn has_console() -> bool {
    imp::has_console()
}

/// Adopt the launching terminal's console, if the process was started from one.
/// Returns whether a console is attached afterwards.
pub fn attach_parent() -> bool {
    imp::attach_parent()
}

/// Guarantee a console: adopt the parent's, else allocate one. Returns whether that succeeded.
pub fn ensure_console() -> bool {
    imp::ensure_console()
}

#[cfg(target_os = "windows")]
mod imp {
    use std::fs::OpenOptions;
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AllocConsole, AttachConsole, GetConsoleWindow, GetStdHandle,
        STD_ERROR_HANDLE, STD_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, SetStdHandle,
    };

    pub(super) fn has_console() -> bool {
        unsafe { !GetConsoleWindow().0.is_null() }
    }

    pub(super) fn attach_parent() -> bool {
        if has_console() {
            rebind_std_handles();
            return true;
        }
        let attached = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) }.is_ok();
        if attached {
            rebind_std_handles();
        }
        attached
    }

    pub(super) fn ensure_console() -> bool {
        if attach_parent() {
            return true;
        }
        let allocated = unsafe { AllocConsole() }.is_ok();
        if allocated {
            rebind_std_handles();
        }
        allocated
    }

    /// Point any unset standard handle at the attached console device.
    fn rebind_std_handles() {
        bind("CONIN$", STD_INPUT_HANDLE);
        bind("CONOUT$", STD_OUTPUT_HANDLE);
        bind("CONOUT$", STD_ERROR_HANDLE);
    }

    /// Install a console device as `slot`, leaking the handle so it outlives this call. A slot that
    /// already holds a valid handle is left alone, so a redirected pipe is never clobbered.
    fn bind(device: &str, slot: STD_HANDLE) {
        if unsafe { GetStdHandle(slot) }.is_ok_and(|h| !h.is_invalid()) {
            return;
        }
        let Ok(file) = OpenOptions::new().read(true).write(true).open(device) else {
            return;
        };
        let handle = HANDLE(file.as_raw_handle() as _);
        if unsafe { SetStdHandle(slot, handle) }.is_ok() {
            std::mem::forget(file);
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod imp {
    pub(super) fn has_console() -> bool {
        true
    }

    pub(super) fn attach_parent() -> bool {
        true
    }

    pub(super) fn ensure_console() -> bool {
        true
    }
}
