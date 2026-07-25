//! WinRing0-backed CPU temperature reader (opt-in `winring0-thermal`).
//!
//! Reads Intel digital-thermal-sensor MSRs and AMD Zen SMU Tctl over a kernel
//! driver, since CPU temps are not exposed to user mode. The embedded WinRing0
//! driver is CVE-2020-14979 and only loads when Windows driver protections are
//! lowered; [`CpuThermalMonitor::open`] classifies the load failure and returns
//! `None` (logging the required toggle) rather than failing silently.

use std::ffi::{c_void, CString};
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};

use winapi::shared::minwindef::DWORD;
use winapi::um::winnt::{SERVICE_DEMAND_START, SERVICE_ERROR_NORMAL, SERVICE_KERNEL_DRIVER};
use winapi::um::winsvc::{
    CloseServiceHandle, ControlService, CreateServiceW, DeleteService, OpenSCManagerW,
    OpenServiceW, StartServiceW, SC_HANDLE, SC_MANAGER_ALL_ACCESS, SERVICE_ALL_ACCESS,
    SERVICE_CONTROL_STOP, SERVICE_STATUS,
};
use winapi::um::{
    errhandlingapi, fileapi, handleapi, ioapiset, processthreadsapi, synchapi, winbase, winnt,
};

use super::{CpuDieReader, CpuDieThermal};

/// Device name baked into this WinRing0 build; the user-mode path and the
/// service name both use it.
const SERVICE_NAME: &str = "WinRing0_1_2_0";
const DEVICE_PATH: &str = r"\\.\WinRing0_1_2_0";
const DRIVER_BYTES: &[u8] = include_bytes!("../../drivers/WinRing0x64.sys");

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

const MSR_TEMPERATURE_TARGET: u32 = 0x1A2;
const IA32_THERM_STATUS: u32 = 0x19C;
const IA32_PACKAGE_THERM_STATUS: u32 = 0x1B1;
/// Architectural on every x86 part, so a failed read means the device handle is
/// gone rather than that this CPU lacks the register.
const IA32_TIME_STAMP_COUNTER: u32 = 0x10;

const AMD_D0F0: u32 = 0;
const AMD_SMU_INDEX_REG: u32 = 0x60;
const AMD_SMU_DATA_REG: u32 = 0x64;
const AMD_SMN_THM_CUR_TEMP: u32 = 0x00059800;

const MIN_POLL_INTERVAL: Duration = Duration::from_millis(500);
/// Age of the last successful read at which cached temps are dropped.
const STALE_AFTER: Duration = Duration::from_secs(3);
const MAX_CORES: usize = 64;
/// Floor for a die reading; the degenerate Intel readout (== TjMax) lands on
/// exactly 0 °C, and no powered die sits below a serviceable room's ambient.
const CPU_MIN_PLAUSIBLE_C: f32 = 5.0;
const CPU_MAX_PLAUSIBLE_C: f32 = 125.0;

const _: () = assert!(
    STALE_AFTER.as_millis() >= MIN_POLL_INTERVAL.as_millis(),
    "cache would expire before the next read could refresh it"
);

/// Bounded wait for the shared PCI mutex; proceed unlocked past this so a stuck
/// peer can't stall the sampler thread.
const PCI_MUTEX_WAIT_MS: u32 = 200;
const WAIT_OBJECT_0: u32 = 0;
const WAIT_ABANDONED: u32 = 0x80;

#[derive(Clone, Copy, PartialEq)]
enum Vendor {
    Intel,
    Amd,
    Other,
}

pub struct CpuThermalMonitor {
    device: winnt::HANDLE,
    vendor: Vendor,
    tj_max: u32,
    cached: Option<CpuDieThermal>,
    last_polled: Instant,
    /// Timestamp of the last read that produced a die value.
    last_good: Instant,
    /// `Global\Access_PCI`; null when unavailable. Serializes SMN index/data
    /// access against other sensor tools (LHM/HWiNFO/AIDA64/CPU-Z).
    pci_mutex: winnt::HANDLE,
    /// Set once the device handle stops answering; no further readings publish.
    device_lost: bool,
}

impl CpuThermalMonitor {
    /// Loads WinRing0 and opens its device. `None` when the load is blocked,
    /// quarantined, or unelevated — the reason is logged.
    pub fn open() -> Option<Self> {
        if let Err(e) = load_driver() {
            log::warn!("stress-kit/cpu-thermal: {e} — CPU temp via WinRing0 disabled");
            return None;
        }
        let device = match open_device() {
            Some(d) => d,
            None => {
                let code = unsafe { errhandlingapi::GetLastError() };
                log::warn!(
                    "stress-kit/cpu-thermal: driver loaded but device open failed (err {code}); unloading"
                );
                unload_driver();
                return None;
            }
        };

        let vendor = detect_vendor();
        let mut me = Self {
            device,
            vendor,
            tj_max: 100,
            cached: None,
            last_polled: Instant::now() - MIN_POLL_INTERVAL,
            last_good: Instant::now(),
            pci_mutex: open_pci_mutex(),
            device_lost: false,
        };
        if vendor == Vendor::Intel {
            me.tj_max = me.read_tjmax();
        }
        me.cached = me.read_all();
        me.last_good = Instant::now();
        match me.cached.as_ref() {
            Some(die) => log::info!(
                "stress-kit/cpu-thermal: WinRing0 loaded; {:?} die sensor, package {:?}, {} \
                 per-core value(s)",
                die.reader,
                die.package_c,
                die.core_temp_count()
            ),
            None => log::warn!(
                "stress-kit/cpu-thermal: WinRing0 loaded but no plausible CPU die reading; temps \
                 stay absent (IO ports remain available for board rails)"
            ),
        }
        Some(me)
    }

    /// Latest die readings; throttled to [`MIN_POLL_INTERVAL`]. A failed read keeps
    /// the prior cache only until it is [`STALE_AFTER`] old; once the device handle
    /// is proven gone, the cache is dropped and this returns `None` for the rest of
    /// the run.
    pub fn poll(&mut self) -> Option<CpuDieThermal> {
        if self.device_lost {
            return None;
        }
        self.drop_stale_cache();
        if self.last_polled.elapsed() < MIN_POLL_INTERVAL {
            return self.cached.clone();
        }
        self.last_polled = Instant::now();
        match self.read_all() {
            Some(die) => {
                self.last_good = Instant::now();
                self.cached = Some(die);
            }
            None => {
                if let Some(err) = self.device_probe_error() {
                    self.device_lost = true;
                    self.cached = None;
                    log::warn!(
                        "stress-kit/cpu-thermal: WinRing0 device stopped answering (err {err}); \
                         another process loading this driver stops and deletes the service, which \
                         invalidates this handle. CPU temps end here instead of repeating the last \
                         reading"
                    );
                } else {
                    self.drop_stale_cache();
                }
            }
        }
        self.cached.clone()
    }

    /// Clears the cache when the last successful read is [`STALE_AFTER`] old.
    fn drop_stale_cache(&mut self) {
        let age = self.last_good.elapsed();
        if self.cached.is_none() || age < STALE_AFTER {
            return;
        }
        log::warn!(
            "stress-kit/cpu-thermal: no CPU die reading for {age:.1?} while the device still \
             answers; dropping cached temps instead of republishing them"
        );
        self.cached = None;
    }

    /// `None` while the driver answers a TSC read on our handle; `Some(win32
    /// error)` once the device is gone.
    fn device_probe_error(&self) -> Option<u32> {
        if self.read_msr(IA32_TIME_STAMP_COUNTER).is_some() {
            return None;
        }
        Some(unsafe { errhandlingapi::GetLastError() })
    }

    fn read_all(&self) -> Option<CpuDieThermal> {
        match self.vendor {
            Vendor::Intel => self.read_intel(),
            Vendor::Amd => self.read_amd(),
            Vendor::Other => None,
        }
    }

    /// Package DTS plus one DTS read per logical core; a core whose sensor does not
    /// answer stays `None` in its own slot.
    fn read_intel(&self) -> Option<CpuDieThermal> {
        let package_c = dts_temp(self.read_msr(IA32_PACKAGE_THERM_STATUS), self.tj_max)
            .and_then(plausible_cpu_temp);
        let count = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
            .min(MAX_CORES);
        let prev = set_affinity(0);
        let cores: Vec<Option<f32>> = (0..count)
            .map(|core| {
                set_affinity(core);
                dts_temp(self.read_msr(IA32_THERM_STATUS), self.tj_max).and_then(plausible_cpu_temp)
            })
            .collect();
        restore_affinity(prev);

        let any_core = cores.iter().any(Option::is_some);
        if package_c.is_none() && !any_core {
            return None;
        }
        Some(CpuDieThermal {
            package_c,
            cores: if any_core { cores } else { Vec::new() },
            reader: CpuDieReader::IntelDts,
        })
    }

    fn read_amd(&self) -> Option<CpuDieThermal> {
        Some(CpuDieThermal {
            package_c: Some(self.read_amd_tctl()?),
            cores: Vec::new(),
            reader: CpuDieReader::AmdTctl,
        })
    }

    fn read_tjmax(&self) -> u32 {
        match self.read_msr(MSR_TEMPERATURE_TARGET) {
            Some(v) => {
                let tj = ((v as u32) >> 16) & 0xFF;
                if tj == 0 { 100 } else { tj }
            }
            None => 100,
        }
    }

    fn read_amd_tctl(&self) -> Option<f32> {
        let _guard = PciGuard::acquire(self.pci_mutex);
        self.write_pci_config(AMD_D0F0, AMD_SMU_INDEX_REG, AMD_SMN_THM_CUR_TEMP)?;
        let val = self.read_pci_config(AMD_D0F0, AMD_SMU_DATA_REG)?;
        if val == 0 || val == 0xFFFF_FFFF {
            return None;
        }
        let raw = (val >> 21) & 0x7FF;
        let mut temp = raw as f32 * 0.125;
        if val & 0x8_0000 != 0 || val & 0x3_0000 != 0 {
            temp -= 49.0;
        }
        plausible_cpu_temp(temp)
    }

    /// `RDMSR` via the driver. MSR index is passed little-endian (the fork's
    /// `io()` byte-swapped it, reading the wrong register).
    fn read_msr(&self, msr: u32) -> Option<u64> {
        let input = msr.to_le_bytes();
        let mut out = [0u8; 8];
        let mut returned: DWORD = 0;
        let ok = unsafe {
            ioapiset::DeviceIoControl(
                self.device,
                IOCTL_READ_MSR,
                input.as_ptr() as *mut c_void,
                input.len() as DWORD,
                out.as_mut_ptr() as *mut c_void,
                out.len() as DWORD,
                &mut returned,
                null_mut(),
            )
        };
        if ok != 0 && returned >= 8 {
            Some(u64::from_le_bytes(out))
        } else {
            None
        }
    }

    fn read_pci_config(&self, pci_address: u32, reg: u32) -> Option<u32> {
        let mut input = [0u8; 8];
        input[..4].copy_from_slice(&pci_address.to_le_bytes());
        input[4..].copy_from_slice(&reg.to_le_bytes());
        let mut out = [0u8; 4];
        let mut returned: DWORD = 0;
        let ok = unsafe {
            ioapiset::DeviceIoControl(
                self.device,
                IOCTL_READ_PCI_CONFIG,
                input.as_ptr() as *mut c_void,
                input.len() as DWORD,
                out.as_mut_ptr() as *mut c_void,
                out.len() as DWORD,
                &mut returned,
                null_mut(),
            )
        };
        if ok != 0 && returned >= 4 {
            Some(u32::from_le_bytes(out))
        } else {
            None
        }
    }

    fn write_pci_config(&self, pci_address: u32, reg: u32, value: u32) -> Option<()> {
        let mut input = [0u8; 12];
        input[..4].copy_from_slice(&pci_address.to_le_bytes());
        input[4..8].copy_from_slice(&reg.to_le_bytes());
        input[8..].copy_from_slice(&value.to_le_bytes());
        let mut returned: DWORD = 0;
        let ok = unsafe {
            ioapiset::DeviceIoControl(
                self.device,
                IOCTL_WRITE_PCI_CONFIG,
                input.as_ptr() as *mut c_void,
                input.len() as DWORD,
                null_mut(),
                0,
                &mut returned,
                null_mut(),
            )
        };
        if ok != 0 { Some(()) } else { None }
    }

    /// IO-port accessor for other readers on this driver (SuperIO board voltages).
    pub fn io_ports(&self) -> IoPorts {
        IoPorts { device: self.device }
    }
}

/// Borrowed WinRing0 IO-port access. The handle is owned by the
/// [`CpuThermalMonitor`] it came from; calls fail (`None`) once that drops.
#[derive(Clone, Copy)]
pub struct IoPorts {
    device: winnt::HANDLE,
}

impl IoPorts {
    /// `IN AL, DX` via the driver. WinRing0 echoes the input length in
    /// `returned` for the read-port IOCTLs and writes only one output byte.
    pub fn read_io_port_byte(&self, port: u16) -> Option<u8> {
        let input = (port as u32).to_le_bytes();
        let mut out = [0u8; 4];
        let mut returned: DWORD = 0;
        let ok = unsafe {
            ioapiset::DeviceIoControl(
                self.device,
                IOCTL_READ_IO_PORT_BYTE,
                input.as_ptr() as *mut c_void,
                input.len() as DWORD,
                out.as_mut_ptr() as *mut c_void,
                out.len() as DWORD,
                &mut returned,
                null_mut(),
            )
        };
        if ok != 0 && returned >= 1 {
            Some(out[0])
        } else {
            None
        }
    }

    /// `OUT DX, AL` via the driver. Input is WinRing0's write-port struct:
    /// little-endian `u32` port, then the byte at offset 4 (union, 8 bytes total).
    pub fn write_io_port_byte(&self, port: u16, value: u8) -> Option<()> {
        let mut input = [0u8; 8];
        input[..4].copy_from_slice(&(port as u32).to_le_bytes());
        input[4] = value;
        let mut returned: DWORD = 0;
        let ok = unsafe {
            ioapiset::DeviceIoControl(
                self.device,
                IOCTL_WRITE_IO_PORT_BYTE,
                input.as_ptr() as *mut c_void,
                input.len() as DWORD,
                null_mut(),
                0,
                &mut returned,
                null_mut(),
            )
        };
        if ok != 0 { Some(()) } else { None }
    }
}

impl Drop for CpuThermalMonitor {
    fn drop(&mut self) {
        if !self.pci_mutex.is_null() {
            unsafe { handleapi::CloseHandle(self.pci_mutex) };
        }
        unsafe { handleapi::CloseHandle(self.device) };
        unload_driver();
    }
}

/// Opens the shared `Global\Access_PCI` mutex; null on failure (caller proceeds unlocked).
fn open_pci_mutex() -> winnt::HANDLE {
    let name = wide(r"Global\Access_PCI");
    unsafe { synchapi::CreateMutexW(null_mut(), 0, name.as_ptr()) }
}

/// Holds `Global\Access_PCI` for one SMN read; best-effort (proceeds on null/timeout).
struct PciGuard {
    handle: winnt::HANDLE,
    acquired: bool,
}

impl PciGuard {
    fn acquire(handle: winnt::HANDLE) -> Self {
        let acquired = !handle.is_null() && {
            let r = unsafe { synchapi::WaitForSingleObject(handle, PCI_MUTEX_WAIT_MS) };
            r == WAIT_OBJECT_0 || r == WAIT_ABANDONED
        };
        Self { handle, acquired }
    }
}

impl Drop for PciGuard {
    fn drop(&mut self) {
        if self.acquired {
            unsafe { synchapi::ReleaseMutex(self.handle) };
        }
    }
}

/// `TjMax - readout` from a thermal-status MSR value; `None` when the valid bit
/// (31) is clear. Readout is bits 22:16.
fn dts_temp(msr_value: Option<u64>, tj_max: u32) -> Option<f32> {
    let eax = msr_value? as u32;
    if eax & (1 << 31) == 0 {
        return None;
    }
    let readout = (eax >> 16) & 0x7F;
    Some(tj_max.saturating_sub(readout) as f32)
}

/// Drop physically implausible readings (garbage MSR/SMU values — e.g. a VM with
/// no real thermal sensor behind the register, or a readout equal to TjMax).
fn plausible_cpu_temp(t: f32) -> Option<f32> {
    (CPU_MIN_PLAUSIBLE_C..=CPU_MAX_PLAUSIBLE_C)
        .contains(&t)
        .then_some(t)
}

fn detect_vendor() -> Vendor {
    let v = raw_cpuid::CpuId::new()
        .get_vendor_info()
        .map(|v| v.as_str().to_string())
        .unwrap_or_default();
    if v.contains("Intel") {
        Vendor::Intel
    } else if v.contains("AMD") {
        Vendor::Amd
    } else {
        Vendor::Other
    }
}

fn set_affinity(core: usize) -> usize {
    unsafe { winbase::SetThreadAffinityMask(processthreadsapi::GetCurrentThread(), 1usize << core) }
}

fn restore_affinity(prev: usize) {
    if prev != 0 {
        unsafe { winbase::SetThreadAffinityMask(processthreadsapi::GetCurrentThread(), prev) };
    }
}

fn driver_path() -> PathBuf {
    std::env::temp_dir().join("WinRing0_mtech.sys")
}

fn open_device() -> Option<winnt::HANDLE> {
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

const ERROR_SHARING_VIOLATION: u32 = 32;
const ERROR_FILE_NOT_FOUND: u32 = 2;
const ERROR_ACCESS_DENIED: u32 = 5;
const ERROR_SERVICE_ALREADY_RUNNING: u32 = 1056;
const ERROR_SERVICE_MARKED_FOR_DELETE: u32 = 1072;
const ERROR_SERVICE_EXISTS: u32 = 1073;
const ERROR_DRIVER_BLOCKED: u32 = 1275;

fn wide(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// Extracts the embedded driver and starts it as a kernel service. A stale
/// service of the same name is removed first so our `.sys` is the one loaded.
fn load_driver() -> Result<(), String> {
    let path = driver_path();

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

fn unload_driver() {
    unsafe {
        let scm = OpenSCManagerW(null(), null(), SC_MANAGER_ALL_ACCESS);
        if !scm.is_null() {
            remove_service(scm);
            CloseServiceHandle(scm);
        }
    }
    let _ = std::fs::remove_file(driver_path());
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

/// Map a Win32 error code to operator-actionable guidance.
fn classify(action: &str, code: u32) -> String {
    match code {
        ERROR_DRIVER_BLOCKED => format!(
            "{action} blocked by policy (err 1275). Disable Memory Integrity (Core Isolation) AND \
             the Microsoft Vulnerable Driver Blocklist (VulnerableDriverBlocklistEnable=0), reboot, retry"
        ),
        ERROR_FILE_NOT_FOUND => format!(
            "{action} failed — driver file missing (err 2); Defender likely quarantined it, add an exclusion"
        ),
        ERROR_ACCESS_DENIED => format!("{action} denied (err 5); run elevated"),
        ERROR_SHARING_VIOLATION => format!(
            "{action} failed — driver file locked (err 32); a prior instance's driver is still \
             loaded. Close other qc-app instances (a reboot clears it)."
        ),
        other => format!("{action} failed (err {other})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Thermal-status MSR value with the valid bit set and this readout.
    fn therm_status(readout: u32) -> Option<u64> {
        Some(((1u32 << 31) | (readout << 16)) as u64)
    }

    #[test]
    fn a_readout_equal_to_tjmax_publishes_nothing() {
        assert_eq!(dts_temp(therm_status(100), 100), Some(0.0));
        assert_eq!(
            dts_temp(therm_status(100), 100).and_then(plausible_cpu_temp),
            None
        );
        assert_eq!(
            dts_temp(therm_status(127), 100).and_then(plausible_cpu_temp),
            None
        );
    }

    #[test]
    fn a_cleared_valid_bit_publishes_nothing() {
        assert_eq!(dts_temp(Some((30u32 << 16) as u64), 100), None);
        assert_eq!(dts_temp(None, 100), None);
    }

    #[test]
    fn a_real_readout_survives_the_floor() {
        assert_eq!(
            dts_temp(therm_status(30), 100).and_then(plausible_cpu_temp),
            Some(70.0)
        );
        assert_eq!(
            dts_temp(therm_status(0), 100).and_then(plausible_cpu_temp),
            Some(100.0)
        );
    }

    #[test]
    fn implausible_values_are_rejected_at_both_ends() {
        assert_eq!(plausible_cpu_temp(-49.0), None);
        assert_eq!(plausible_cpu_temp(0.0), None);
        assert_eq!(plausible_cpu_temp(4.9), None);
        assert_eq!(plausible_cpu_temp(5.0), Some(5.0));
        assert_eq!(plausible_cpu_temp(125.0), Some(125.0));
        assert_eq!(plausible_cpu_temp(125.1), None);
    }
}
