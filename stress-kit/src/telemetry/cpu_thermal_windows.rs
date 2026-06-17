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
use winapi::um::{errhandlingapi, fileapi, handleapi, ioapiset, processthreadsapi, winbase, winnt};

use super::ThermalReading;

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

const MSR_TEMPERATURE_TARGET: u32 = 0x1A2;
const IA32_THERM_STATUS: u32 = 0x19C;
const IA32_PACKAGE_THERM_STATUS: u32 = 0x1B1;

const AMD_D0F0: u32 = 0;
const AMD_SMU_INDEX_REG: u32 = 0x60;
const AMD_SMU_DATA_REG: u32 = 0x64;
const AMD_SMN_THM_CUR_TEMP: u32 = 0x00059800;

const MIN_POLL_INTERVAL: Duration = Duration::from_millis(500);
const MAX_CORES: usize = 64;
const CPU_MIN_PLAUSIBLE_C: f32 = 0.0;
const CPU_MAX_PLAUSIBLE_C: f32 = 125.0;

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
    cached: Vec<ThermalReading>,
    last_polled: Instant,
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
            cached: Vec::new(),
            last_polled: Instant::now() - MIN_POLL_INTERVAL,
        };
        if vendor == Vendor::Intel {
            me.tj_max = me.read_tjmax();
        }
        me.cached = me.read_all();
        log::info!(
            "stress-kit/cpu-thermal: WinRing0 loaded ({} reading(s))",
            me.cached.len()
        );
        Some(me)
    }

    /// Latest readings; throttled to [`MIN_POLL_INTERVAL`]. An empty read keeps
    /// the prior cache rather than blanking the chart.
    pub fn poll(&mut self) -> Vec<ThermalReading> {
        if self.last_polled.elapsed() < MIN_POLL_INTERVAL {
            return self.cached.clone();
        }
        self.last_polled = Instant::now();
        let readings = self.read_all();
        if !readings.is_empty() {
            self.cached = readings;
        }
        self.cached.clone()
    }

    fn read_all(&self) -> Vec<ThermalReading> {
        match self.vendor {
            Vendor::Intel => self.read_intel(),
            Vendor::Amd => self.read_amd(),
            Vendor::Other => Vec::new(),
        }
    }

    fn read_intel(&self) -> Vec<ThermalReading> {
        let mut out = Vec::new();
        if let Some(t) = dts_temp(self.read_msr(IA32_PACKAGE_THERM_STATUS), self.tj_max)
            .and_then(plausible_cpu_temp)
        {
            out.push(ThermalReading { label: "CPU Package".into(), temp_c: t });
        }
        let cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
            .min(MAX_CORES);
        let prev = set_affinity(0);
        let mut hottest = None;
        for core in 0..cores {
            set_affinity(core);
            if let Some(t) = dts_temp(self.read_msr(IA32_THERM_STATUS), self.tj_max)
                .and_then(plausible_cpu_temp)
            {
                out.push(ThermalReading { label: format!("CPU Core {core}"), temp_c: t });
                hottest = Some(hottest.map_or(t, |h: f32| h.max(t)));
            }
        }
        restore_affinity(prev);
        // Fall back to hottest core when the package sensor is absent.
        if !out.iter().any(|r| r.label == "CPU Package") {
            if let Some(h) = hottest {
                out.push(ThermalReading { label: "CPU Package".into(), temp_c: h });
            }
        }
        out
    }

    fn read_amd(&self) -> Vec<ThermalReading> {
        match self.read_amd_tctl() {
            Some(t) => vec![ThermalReading { label: "CPU (Tctl)".into(), temp_c: t }],
            None => Vec::new(),
        }
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
}

impl Drop for CpuThermalMonitor {
    fn drop(&mut self) {
        unsafe { handleapi::CloseHandle(self.device) };
        unload_driver();
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
/// no real thermal sensor behind the register).
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
