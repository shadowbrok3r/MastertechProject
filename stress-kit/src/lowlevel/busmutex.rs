//! Named bus mutexes shared with peer sensor tools.
//!
//! Sequences a driver cannot make atomic on our behalf — a PCI config
//! index/data pair, a SuperIO config-mode session — are serialized against
//! other tools through the same well-known mutex names they take.

use std::ptr::null_mut;
use std::sync::Mutex;

use winapi::um::{handleapi, synchapi, winnt};

use super::protocol;

const WAIT_OBJECT_0: u32 = 0;
const WAIT_ABANDONED: u32 = 0x80;

pub fn wide(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// Opens a shared named mutex; null on failure, which callers treat as "proceed
/// unlocked" rather than as an error.
fn open_mutex(name: &str) -> winnt::HANDLE {
    let handle = unsafe { synchapi::CreateMutexW(null_mut(), 0, wide(name).as_ptr()) };
    if handle.is_null() {
        log::warn!("stress-kit/lowlevel: cannot open shared mutex {name}");
    }
    handle
}

/// Bounded, best-effort hold of one shared mutex; proceeds unlocked past the
/// timeout so a stuck peer cannot stall the sampler thread.
pub struct MutexGuardHandle {
    handle: winnt::HANDLE,
    acquired: bool,
}

impl MutexGuardHandle {
    fn acquire(handle: winnt::HANDLE) -> Self {
        let acquired = !handle.is_null() && {
            let r = unsafe { synchapi::WaitForSingleObject(handle, protocol::MUTEX_WAIT_MS) };
            r == WAIT_OBJECT_0 || r == WAIT_ABANDONED
        };
        Self { handle, acquired }
    }
}

impl Drop for MutexGuardHandle {
    fn drop(&mut self) {
        if self.acquired {
            unsafe { synchapi::ReleaseMutex(self.handle) };
        }
    }
}

/// `Global\Access_PCI`, held across a config-space index/data pair.
pub struct PciMutex(winnt::HANDLE);

// A named mutex handle is process-global and the Win32 wait/release calls are
// thread-safe; the guard tracks ownership per acquisition.
unsafe impl Send for PciMutex {}
unsafe impl Sync for PciMutex {}

impl PciMutex {
    pub fn open() -> Self {
        Self(open_mutex(protocol::PCI_MUTEX_NAME))
    }

    pub fn lock(&self) -> MutexGuardHandle {
        MutexGuardHandle::acquire(self.0)
    }
}

impl Drop for PciMutex {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { handleapi::CloseHandle(self.0) };
        }
    }
}

/// Both ISA-bus mutex names, taken together so either peer convention
/// interlocks with us. Null entries mean the bus stays untouched.
pub struct IsaBus {
    handles: [winnt::HANDLE; 2],
    held: Mutex<bool>,
}

unsafe impl Send for IsaBus {}
unsafe impl Sync for IsaBus {}

impl IsaBus {
    pub fn open() -> Self {
        Self {
            handles: [
                open_mutex(protocol::ISA_MUTEX_NAMES[0]),
                open_mutex(protocol::ISA_MUTEX_NAMES[1]),
            ],
            held: Mutex::new(false),
        }
    }

    /// All-or-nothing across both names; a partial take is released before
    /// returning false so a peer is never blocked by a lease we did not get.
    pub fn acquire(&self) -> bool {
        let Ok(mut held) = self.held.lock() else {
            return false;
        };
        if *held {
            return false;
        }
        let mut taken = 0usize;
        let mut abandoned = false;
        for &handle in &self.handles {
            if handle.is_null() {
                break;
            }
            match unsafe { synchapi::WaitForSingleObject(handle, protocol::MUTEX_WAIT_MS) } {
                WAIT_OBJECT_0 => taken += 1,
                WAIT_ABANDONED => {
                    taken += 1;
                    abandoned = true;
                }
                _ => break,
            }
        }
        if taken < self.handles.len() {
            for &handle in self.handles.iter().take(taken).rev() {
                unsafe { synchapi::ReleaseMutex(handle) };
            }
            log::debug!("stress-kit/lowlevel: ISA-bus mutex not fully acquired; skipping port access");
            return false;
        }
        if abandoned {
            log::warn!(
                "stress-kit/lowlevel: ISA-bus mutex was abandoned; a peer may have left the \
                 SuperIO in config mode"
            );
        }
        *held = true;
        true
    }

    pub fn release(&self) {
        let Ok(mut held) = self.held.lock() else {
            return;
        };
        if !*held {
            return;
        }
        for &handle in self.handles.iter().rev() {
            if !handle.is_null() {
                unsafe { synchapi::ReleaseMutex(handle) };
            }
        }
        *held = false;
    }
}

impl Drop for IsaBus {
    fn drop(&mut self) {
        self.release();
        for &handle in &self.handles {
            if !handle.is_null() {
                unsafe { handleapi::CloseHandle(handle) };
            }
        }
    }
}

/// A named mutex held across a long operation, with its own timeout.
///
/// Separate from [`MutexGuardHandle`], whose bounded wait is sized for a bus
/// sequence: serializing something like a driver install needs to wait much
/// longer than a sampler ever should.
pub struct NamedLease {
    handle: winnt::HANDLE,
    acquired: bool,
}

unsafe impl Send for NamedLease {}
unsafe impl Sync for NamedLease {}

impl NamedLease {
    /// Waits up to `timeout_ms` for `name`. A lease that could not be taken is
    /// still returned: serialization here is best effort, and refusing to
    /// proceed because a peer is slow is worse than proceeding unserialized.
    pub fn acquire(name: &str, timeout_ms: u32) -> Self {
        let handle = unsafe { synchapi::CreateMutexW(null_mut(), 0, wide(name).as_ptr()) };
        if handle.is_null() {
            log::debug!("stress-kit/lowlevel: cannot open lease {name}; proceeding unserialized");
            return Self { handle, acquired: false };
        }
        let acquired = match unsafe { synchapi::WaitForSingleObject(handle, timeout_ms) } {
            WAIT_OBJECT_0 => true,
            // The holder died mid-operation; the lease is ours and whatever it
            // was doing is half-finished.
            WAIT_ABANDONED => {
                log::warn!("stress-kit/lowlevel: lease {name} was abandoned by its holder");
                true
            }
            _ => {
                log::debug!("stress-kit/lowlevel: lease {name} not taken within {timeout_ms}ms");
                false
            }
        };
        Self { handle, acquired }
    }
}

impl Drop for NamedLease {
    fn drop(&mut self) {
        if self.acquired {
            unsafe { synchapi::ReleaseMutex(self.handle) };
        }
        if !self.handle.is_null() {
            unsafe { handleapi::CloseHandle(self.handle) };
        }
    }
}
