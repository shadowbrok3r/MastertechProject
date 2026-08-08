//! Driverless storage temperatures via `IOCTL_STORAGE_QUERY_PROPERTY`.
//!
//! NVMe exposes a SMART/Health log page (composite temperature in Kelvin) that
//! Windows surfaces in user mode with no kernel driver — unlike CPU temps. SATA
//! SSD/HDD temps come from the generic `StorageDeviceTemperatureProperty`, which
//! the storage stack sources from the drive's SMART temperature. Each
//! `\\.\PhysicalDriveN` is queried; drives that report neither are skipped.

use std::ptr::null_mut;
use std::time::{Duration, Instant};

use winapi::shared::minwindef::DWORD;
use winapi::um::{fileapi, handleapi, ioapiset, winnt};

use super::ThermalReading;

const IOCTL_STORAGE_QUERY_PROPERTY: u32 = 0x002D_1400;
const STORAGE_DEVICE_PROTOCOL_SPECIFIC_PROPERTY: u32 = 50;
const STORAGE_DEVICE_TEMPERATURE_PROPERTY: u32 = 52;
const PROPERTY_STANDARD_QUERY: u32 = 0;
const PROTOCOL_TYPE_NVME: u32 = 3;
const NVME_DATA_TYPE_LOG_PAGE: u32 = 2;
const NVME_LOG_PAGE_HEALTH: u32 = 0x02;
const NVME_HEALTH_LOG_SIZE: usize = 512;

const MAX_PHYSICAL_DRIVES: u32 = 16;
const MIN_POLL_INTERVAL: Duration = Duration::from_secs(5);
const MIN_PLAUSIBLE_C: f32 = -10.0;
const MAX_PLAUSIBLE_C: f32 = 150.0;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct ProtocolSpecificData {
    protocol_type: u32,
    data_type: u32,
    request_value: u32,
    request_sub_value: u32,
    data_offset: u32,
    data_length: u32,
    fixed_return: u32,
    request_sub_value2: u32,
    request_sub_value3: u32,
    request_sub_value4: u32,
}

/// `STORAGE_PROPERTY_QUERY` on input, `STORAGE_PROTOCOL_DATA_DESCRIPTOR` on
/// output (the first two u32s alias Version/Size); the log lands in `data`.
#[repr(C)]
struct ProtocolDataQuery {
    property_id: u32,
    query_type: u32,
    protocol: ProtocolSpecificData,
    data: [u8; NVME_HEALTH_LOG_SIZE],
}

pub struct StorageThermalMonitor {
    cached: Vec<ThermalReading>,
    last_polled: Instant,
}

impl StorageThermalMonitor {
    pub fn open() -> Option<Self> {
        let cached = read_all();
        log::debug!("stress-kit/storage-thermal: {} disk sensor(s)", cached.len());
        Some(Self {
            cached,
            last_polled: Instant::now(),
        })
    }

    pub fn poll(&mut self) -> Vec<ThermalReading> {
        if self.last_polled.elapsed() < MIN_POLL_INTERVAL {
            return self.cached.clone();
        }
        self.last_polled = Instant::now();
        let readings = read_all();
        if !readings.is_empty() {
            self.cached = readings;
        }
        self.cached.clone()
    }
}

fn read_all() -> Vec<ThermalReading> {
    let mut out = Vec::new();
    for n in 0..MAX_PHYSICAL_DRIVES {
        if let Some((label, temp_c)) = read_disk_temp(n) {
            if (MIN_PLAUSIBLE_C..=MAX_PLAUSIBLE_C).contains(&temp_c) {
                out.push(ThermalReading { label, temp_c });
            }
        }
    }
    out
}

/// One reading per physical drive: NVMe health log first (composite temp), else
/// the generic storage temperature property — which covers SATA SSD/HDD (the
/// storage stack sources it from the drive's SMART temperature).
fn read_disk_temp(drive: u32) -> Option<(String, f32)> {
    let handle = open_drive(drive)?;
    let result = if let Some(t) = query_nvme_health(handle) {
        Some((format!("NVMe Disk {drive}"), t))
    } else if let Some(t) = query_storage_temperature(handle) {
        Some((format!("Disk {drive}"), t))
    } else {
        None
    };
    unsafe { handleapi::CloseHandle(handle) };
    result
}

fn open_drive(drive: u32) -> Option<winnt::HANDLE> {
    let path = wide(&format!(r"\\.\PhysicalDrive{drive}"));
    let handle = unsafe {
        fileapi::CreateFileW(
            path.as_ptr(),
            0,
            winnt::FILE_SHARE_READ | winnt::FILE_SHARE_WRITE,
            null_mut(),
            fileapi::OPEN_EXISTING,
            0,
            null_mut(),
        )
    };
    if handle == handleapi::INVALID_HANDLE_VALUE {
        None
    } else {
        Some(handle)
    }
}

/// NVMe SMART/Health log composite temperature (bytes 1..3, little-endian Kelvin).
fn query_nvme_health(handle: winnt::HANDLE) -> Option<f32> {
    let mut q: ProtocolDataQuery = unsafe { std::mem::zeroed() };
    q.property_id = STORAGE_DEVICE_PROTOCOL_SPECIFIC_PROPERTY;
    q.query_type = PROPERTY_STANDARD_QUERY;
    q.protocol.protocol_type = PROTOCOL_TYPE_NVME;
    q.protocol.data_type = NVME_DATA_TYPE_LOG_PAGE;
    q.protocol.request_value = NVME_LOG_PAGE_HEALTH;
    q.protocol.data_offset = std::mem::size_of::<ProtocolSpecificData>() as u32;
    q.protocol.data_length = NVME_HEALTH_LOG_SIZE as u32;

    let size = std::mem::size_of::<ProtocolDataQuery>() as DWORD;
    let mut returned: DWORD = 0;
    let ok = unsafe {
        ioapiset::DeviceIoControl(
            handle,
            IOCTL_STORAGE_QUERY_PROPERTY,
            &mut q as *mut _ as *mut _,
            size,
            &mut q as *mut _ as *mut _,
            size,
            &mut returned,
            null_mut(),
        )
    };
    if ok == 0 {
        return None;
    }

    let base = &q.protocol as *const ProtocolSpecificData as *const u8;
    let kelvin = unsafe {
        let data = base.add(q.protocol.data_offset as usize);
        u16::from_le_bytes([*data.add(1), *data.add(2)])
    };
    if kelvin == 0 {
        return None;
    }
    Some(kelvin as f32 - 273.15)
}

/// `StorageDeviceTemperatureProperty` (SATA + NVMe on Win10+). Returns the first
/// reported sensor's Celsius value from the `STORAGE_TEMPERATURE_DATA_DESCRIPTOR`:
/// `InfoCount` at byte 12 (u16), `TemperatureInfo[0].Temperature` at byte 18 (i16).
fn query_storage_temperature(handle: winnt::HANDLE) -> Option<f32> {
    let mut buf = [0u8; 256];
    buf[0..4].copy_from_slice(&STORAGE_DEVICE_TEMPERATURE_PROPERTY.to_le_bytes());
    buf[4..8].copy_from_slice(&PROPERTY_STANDARD_QUERY.to_le_bytes());

    let mut returned: DWORD = 0;
    let ok = unsafe {
        ioapiset::DeviceIoControl(
            handle,
            IOCTL_STORAGE_QUERY_PROPERTY,
            buf.as_mut_ptr() as *mut _,
            buf.len() as DWORD,
            buf.as_mut_ptr() as *mut _,
            buf.len() as DWORD,
            &mut returned,
            null_mut(),
        )
    };
    if ok == 0 || returned < 20 {
        return None;
    }
    let info_count = u16::from_le_bytes([buf[12], buf[13]]);
    if info_count == 0 {
        return None;
    }
    Some(i16::from_le_bytes([buf[18], buf[19]]) as f32)
}

fn wide(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}
