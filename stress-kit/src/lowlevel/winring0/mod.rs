//! Legacy WinRing0 backend (CVE-2020-14979).
//!
//! Loads only with Memory Integrity and the Vulnerable Driver Blocklist off, and
//! current Defender definitions quarantine the `.sys` on disk. Kept behind
//! `backend-winring0` for benches where we control those settings; it is never
//! the preferred backend.

mod loader;

use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::Mutex;

use winapi::shared::minwindef::DWORD;
use winapi::um::{errhandlingapi, handleapi, ioapiset, synchapi, winnt};

use super::protocol;
use super::{
    BackendId, Capabilities, LowLevelBackend, LpcAccess, LpcSlot, MsrAccess, SmnAccess,
    SuperIoFamily,
};

const OLS_TYPE: u32 = 40000;
const METHOD_BUFFERED: u32 = 0;
const FILE_ANY_ACCESS: u32 = 0;
const FILE_READ_ACCESS: u32 = 1;
const FILE_WRITE_ACCESS: u32 = 2;

const fn ctl_code(function: u32, access: u32) -> u32 {
    (OLS_TYPE << 16) | (access << 14) | (function << 2) | METHOD_BUFFERED
}
const IOCTL_READ_MSR: u32 = ctl_code(0x821, FILE_ANY_ACCESS);
const IOCTL_READ_PCI_CONFIG: u32 = ctl_code(0x851, FILE_READ_ACCESS);
const IOCTL_WRITE_PCI_CONFIG: u32 = ctl_code(0x852, FILE_WRITE_ACCESS);
const IOCTL_READ_IO_PORT_BYTE: u32 = ctl_code(0x833, FILE_READ_ACCESS);
const IOCTL_WRITE_IO_PORT_BYTE: u32 = ctl_code(0x836, FILE_WRITE_ACCESS);

/// Architectural on every x86 part, so a failed read means the device handle is
/// gone rather than that this CPU lacks the register.
const IA32_TIME_STAMP_COUNTER: u32 = 0x10;

const WAIT_OBJECT_0: u32 = 0;
const WAIT_ABANDONED: u32 = 0x80;

pub struct WinRing0Backend {
    device: winnt::HANDLE,
    /// `Global\Access_PCI`; null when unavailable (caller proceeds unlocked).
    pci_mutex: winnt::HANDLE,
    /// Both ISA-bus mutex names; null entries mean the bus stays untouched.
    isa_mutexes: [winnt::HANDLE; 2],
    isa_held: Mutex<bool>,
    /// Currently admitted hardware-monitor window.
    window: Mutex<Option<(u16, u16)>>,
}

// Windows handles are process-global and DeviceIoControl is thread-safe; the
// non-atomic bus sequences are serialized by the ISA and PCI mutexes.
unsafe impl Send for WinRing0Backend {}
unsafe impl Sync for WinRing0Backend {}

impl WinRing0Backend {
    /// Loads the driver and opens its device. `Err` carries operator-actionable
    /// guidance, already classified from the Win32 error.
    pub fn open() -> Result<Self, String> {
        loader::load_driver()?;
        let Some(device) = loader::open_device() else {
            let code = unsafe { errhandlingapi::GetLastError() };
            loader::unload_driver();
            return Err(loader::classify("open driver device", code));
        };
        Ok(Self {
            device,
            pci_mutex: open_mutex(protocol::PCI_MUTEX_NAME),
            isa_mutexes: [
                open_mutex(protocol::ISA_MUTEX_NAMES[0]),
                open_mutex(protocol::ISA_MUTEX_NAMES[1]),
            ],
            isa_held: Mutex::new(false),
            window: Mutex::new(None),
        })
    }

    fn ioctl(&self, code: u32, input: &[u8], out: &mut [u8]) -> Option<DWORD> {
        let mut returned: DWORD = 0;
        let out_ptr = if out.is_empty() {
            null_mut()
        } else {
            out.as_mut_ptr() as *mut c_void
        };
        let ok = unsafe {
            ioapiset::DeviceIoControl(
                self.device,
                code,
                input.as_ptr() as *mut c_void,
                input.len() as DWORD,
                out_ptr,
                out.len() as DWORD,
                &mut returned,
                null_mut(),
            )
        };
        (ok != 0).then_some(returned)
    }

    fn read_pci_config(&self, pci_address: u32, reg: u32) -> Option<u32> {
        let mut input = [0u8; 8];
        input[..4].copy_from_slice(&pci_address.to_le_bytes());
        input[4..].copy_from_slice(&reg.to_le_bytes());
        let mut out = [0u8; 4];
        let returned = self.ioctl(IOCTL_READ_PCI_CONFIG, &input, &mut out)?;
        (returned >= 4).then(|| u32::from_le_bytes(out))
    }

    fn write_pci_config(&self, pci_address: u32, reg: u32, value: u32) -> Option<()> {
        let mut input = [0u8; 12];
        input[..4].copy_from_slice(&pci_address.to_le_bytes());
        input[4..8].copy_from_slice(&reg.to_le_bytes());
        input[8..].copy_from_slice(&value.to_le_bytes());
        self.ioctl(IOCTL_WRITE_PCI_CONFIG, &input, &mut [])?;
        Some(())
    }

    /// `IN AL, DX`. WinRing0 echoes the input length in `returned` for the
    /// read-port IOCTLs and writes only one output byte.
    fn io_in(&self, port: u16) -> Option<u8> {
        let input = (port as u32).to_le_bytes();
        let mut out = [0u8; 4];
        let returned = self.ioctl(IOCTL_READ_IO_PORT_BYTE, &input, &mut out)?;
        (returned >= 1).then_some(out[0])
    }

    /// `OUT DX, AL`. Input is WinRing0's write-port struct: little-endian `u32`
    /// port, then the byte at offset 4.
    fn io_out(&self, port: u16, value: u8) -> Option<()> {
        let mut input = [0u8; 8];
        input[..4].copy_from_slice(&(port as u32).to_le_bytes());
        input[4] = value;
        self.ioctl(IOCTL_WRITE_IO_PORT_BYTE, &input, &mut [])?;
        Some(())
    }

    /// Writes one config-mode exit byte, retrying once before warning.
    fn exit_write(&self, port: u16, value: u8) {
        if self.io_out(port, value).is_some() {
            return;
        }
        if self.io_out(port, value).is_some() {
            log::debug!("stress-kit/winring0: config-mode exit 0x{value:02X} to 0x{port:04X} needed a retry");
            return;
        }
        log::warn!(
            "stress-kit/winring0: config-mode exit 0x{value:02X} to 0x{port:04X} failed twice; \
             SuperIO may be left unlocked"
        );
    }
}

impl MsrAccess for WinRing0Backend {
    /// MSR index is passed little-endian; the fork's `io()` byte-swapped it and
    /// read the wrong register.
    fn read_msr(&self, msr: u32) -> Option<u64> {
        let input = msr.to_le_bytes();
        let mut out = [0u8; 8];
        let returned = self.ioctl(IOCTL_READ_MSR, &input, &mut out)?;
        (returned >= 8).then(|| u64::from_le_bytes(out))
    }
}

impl SmnAccess for WinRing0Backend {
    /// Index/data pair under `Global\Access_PCI`, so peers reading SMN cannot
    /// interleave between our index write and data read.
    fn read_smn(&self, addr: u32) -> Option<u32> {
        let _guard = MutexGuardHandle::acquire(self.pci_mutex);
        self.write_pci_config(protocol::AMD_SMN_BUS_DEVICE_FN, protocol::AMD_SMN_INDEX_REG, addr)?;
        self.read_pci_config(protocol::AMD_SMN_BUS_DEVICE_FN, protocol::AMD_SMN_DATA_REG)
    }
}

impl LpcAccess for WinRing0Backend {
    /// All-or-nothing across both mutex names; a partial take is released before
    /// returning false so a peer is never blocked by a lease we did not get.
    fn acquire_bus(&self) -> bool {
        let Ok(mut held) = self.isa_held.lock() else {
            return false;
        };
        if *held {
            return false;
        }
        let mut taken = 0usize;
        let mut abandoned = false;
        for &handle in &self.isa_mutexes {
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
        if taken < self.isa_mutexes.len() {
            for &handle in self.isa_mutexes.iter().take(taken).rev() {
                unsafe { synchapi::ReleaseMutex(handle) };
            }
            log::debug!("stress-kit/winring0: ISA-bus mutex not fully acquired; skipping port access");
            return false;
        }
        if abandoned {
            log::warn!(
                "stress-kit/winring0: ISA-bus mutex was abandoned; a peer may have left the \
                 SuperIO in config mode"
            );
        }
        *held = true;
        true
    }

    fn release_bus(&self) {
        let Ok(mut held) = self.isa_held.lock() else {
            return;
        };
        if !*held {
            return;
        }
        for &handle in self.isa_mutexes.iter().rev() {
            if !handle.is_null() {
                unsafe { synchapi::ReleaseMutex(handle) };
            }
        }
        *held = false;
    }

    fn config_enter(&self, slot: LpcSlot, family: SuperIoFamily) -> Option<()> {
        let index = slot.index_port();
        match family {
            SuperIoFamily::Nuvoton => {
                self.io_out(index, protocol::NUVOTON_ENTER)?;
                self.io_out(index, protocol::NUVOTON_ENTER)?;
            }
            SuperIoFamily::Ite => {
                let last = match slot {
                    LpcSlot::Port4E => protocol::ITE_ENTER_LAST_4E,
                    LpcSlot::Port2E => protocol::ITE_ENTER_LAST_2E,
                };
                for byte in protocol::ITE_ENTER_PREFIX.into_iter().chain([last]) {
                    self.io_out(index, byte)?;
                }
            }
        }
        Some(())
    }

    fn config_exit(&self, slot: LpcSlot, family: SuperIoFamily) {
        match family {
            SuperIoFamily::Nuvoton => self.exit_write(slot.index_port(), protocol::NUVOTON_EXIT),
            SuperIoFamily::Ite => {
                self.exit_write(slot.index_port(), protocol::ITE_EXIT_REG);
                self.exit_write(slot.data_port(), protocol::ITE_EXIT_VALUE);
            }
        }
    }

    fn config_read(&self, slot: LpcSlot, reg: u8) -> Option<u8> {
        self.io_out(slot.index_port(), reg)?;
        self.io_in(slot.data_port())
    }

    fn config_write(&self, slot: LpcSlot, reg: u8, value: u8) -> Option<()> {
        self.io_out(slot.index_port(), reg)?;
        self.io_out(slot.data_port(), value)
    }

    fn window_open(&self, base: u16, len: u16) -> Option<()> {
        if !protocol::window_admissible(base, len) {
            log::debug!(
                "stress-kit/winring0: refusing monitor window 0x{base:04X}+{len}; misaligned, out \
                 of range, or aliasing a legacy ISA device"
            );
            return None;
        }
        *self.window.lock().ok()? = Some((base, len));
        Some(())
    }

    fn window_close(&self, base: u16) {
        if let Ok(mut w) = self.window.lock()
            && w.map(|(b, _)| b) == Some(base)
        {
            *w = None;
        }
    }

    fn window_in(&self, base: u16, offset: u8) -> Option<u8> {
        self.io_in(self.window_port(base, offset)?)
    }

    fn window_out(&self, base: u16, offset: u8, value: u8) -> Option<()> {
        self.io_out(self.window_port(base, offset)?, value)
    }
}

impl WinRing0Backend {
    /// Absolute port for a window-relative offset; `None` unless the window is
    /// the admitted one and the offset is inside it.
    fn window_port(&self, base: u16, offset: u8) -> Option<u16> {
        let (open_base, len) = (*self.window.lock().ok()?)?;
        (open_base == base && (offset as u16) < len).then_some(base + offset as u16)
    }
}

impl LowLevelBackend for WinRing0Backend {
    fn id(&self) -> BackendId {
        BackendId::WinRing0
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            msr: true,
            msr_per_cpu: false,
            smn: true,
            lpc_config: true,
            lpc_window: true,
        }
    }

    fn probe(&self) -> Result<(), String> {
        if self.read_msr(IA32_TIME_STAMP_COUNTER).is_some() {
            return Ok(());
        }
        let err = unsafe { errhandlingapi::GetLastError() };
        Err(format!(
            "WinRing0 device stopped answering (err {err}); another process loading this driver \
             stops and deletes the service, which invalidates this handle"
        ))
    }

    fn msr(&self) -> Option<&dyn MsrAccess> {
        Some(self)
    }

    fn smn(&self) -> Option<&dyn SmnAccess> {
        Some(self)
    }

    fn lpc(&self) -> Option<&dyn LpcAccess> {
        Some(self)
    }
}

impl Drop for WinRing0Backend {
    fn drop(&mut self) {
        self.release_bus();
        for &handle in self.isa_mutexes.iter().chain(std::iter::once(&self.pci_mutex)) {
            if !handle.is_null() {
                unsafe { handleapi::CloseHandle(handle) };
            }
        }
        unsafe { handleapi::CloseHandle(self.device) };
        loader::unload_driver();
    }
}

/// Opens a shared named mutex; null on failure, which callers treat as "proceed
/// unlocked" rather than as an error.
fn open_mutex(name: &str) -> winnt::HANDLE {
    let handle = unsafe { synchapi::CreateMutexW(null_mut(), 0, loader::wide(name).as_ptr()) };
    if handle.is_null() {
        log::warn!("stress-kit/winring0: cannot open shared mutex {name}");
    }
    handle
}

/// Bounded, best-effort hold of one shared mutex; proceeds unlocked past the
/// timeout so a stuck peer cannot stall the sampler thread.
struct MutexGuardHandle {
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
