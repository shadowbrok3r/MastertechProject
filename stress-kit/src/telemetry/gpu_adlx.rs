//! AMD total board power and hotspot temperature from ADLX, the library AMD's driver installs.

use std::ffi::{CStr, c_char, c_void};
use std::ptr::{NonNull, null, null_mut};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::LibraryLoader::{
    GetProcAddress, LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};
use windows::core::{PCSTR, PCWSTR, s, w};

use super::gpu::GpuSample;
use super::gpu_wddm::LuidKey;

#[cfg(target_pointer_width = "64")]
const LIBRARY: PCWSTR = w!("amdadlx64.dll");
#[cfg(not(target_pointer_width = "64"))]
const LIBRARY: PCWSTR = w!("amdadlx32.dll");

/// ADLX 2.0.0.125, the SDK release the vtable layouts below were checked against.
const SDK_VERSION: u64 = 0x0002_0000_0000_007D;
/// Delay before another start attempt after ADLX failed to start.
const RETRY_AFTER: Duration = Duration::from_secs(60);
/// Upper bound on GPUs read from one list.
const MAX_GPUS: u32 = 16;

const ADLX_OK: i32 = 0;
const ADLX_ALREADY_INITIALIZED: i32 = 2;

type Slot = *const c_void;
type InitializeFn = unsafe extern "C" fn(version: u64, system: *mut *mut c_void) -> i32;
type QueryFullVersionFn = unsafe extern "C" fn(version: *mut u64) -> i32;
type TerminateFn = unsafe extern "C" fn() -> i32;
type ObjectGetter = unsafe extern "system" fn(this: *mut c_void, out: *mut *mut c_void) -> i32;
type IntGetter = unsafe extern "system" fn(this: *mut c_void, data: *mut i32) -> i32;
type DoubleGetter = unsafe extern "system" fn(this: *mut c_void, data: *mut f64) -> i32;

/// `IADLXInterface`, the head of every reference-counted ADLX vtable.
#[repr(C)]
struct InterfaceVtbl {
    _acquire: Slot,
    release: unsafe extern "system" fn(this: *mut c_void) -> i32,
    query_interface:
        unsafe extern "system" fn(this: *mut c_void, iid: *const u16, out: *mut *mut c_void) -> i32,
}

/// `IADLXSystem` through `GetPerformanceMonitoringServices`.
#[repr(C)]
struct SystemVtbl {
    _get_hybrid_graphics_type: Slot,
    get_gpus: ObjectGetter,
    _query_interface_to_get_gpu_tuning_services: [Slot; 7],
    get_performance_monitoring_services: ObjectGetter,
}

/// `IADLXGPUList` through `At_GPUList`.
#[repr(C)]
struct GpuListVtbl {
    interface: InterfaceVtbl,
    _size_and_empty: [Slot; 2],
    begin: unsafe extern "system" fn(this: *mut c_void) -> u32,
    end: unsafe extern "system" fn(this: *mut c_void) -> u32,
    _at_to_add_back: [Slot; 4],
    at_gpu_list:
        unsafe extern "system" fn(this: *mut c_void, at: u32, out: *mut *mut c_void) -> i32,
}

/// `IADLXGPU` through `Name`.
#[repr(C)]
struct GpuVtbl {
    interface: InterfaceVtbl,
    _vendor_id_to_is_external: [Slot; 4],
    name: unsafe extern "system" fn(this: *mut c_void, name: *mut *const c_char) -> i32,
}

/// `IADLXGPU2` through `LUID`.
#[repr(C)]
struct Gpu2Vtbl {
    interface: InterfaceVtbl,
    _gpu_and_gpu1: [Slot; 20],
    is_power_off: unsafe extern "system" fn(this: *mut c_void, state: *mut u8) -> i32,
    _power_on_to_amd_windows_driver_version: [Slot; 10],
    luid: unsafe extern "system" fn(this: *mut c_void, luid: *mut Luid) -> i32,
}

/// `IADLXPerformanceMonitoringServices` through `GetCurrentGPUMetrics`.
#[repr(C)]
struct PerfVtbl {
    interface: InterfaceVtbl,
    _sampling_interval_to_current_all_metrics: [Slot; 15],
    get_current_gpu_metrics: unsafe extern "system" fn(
        this: *mut c_void,
        gpu: *mut c_void,
        out: *mut *mut c_void,
    ) -> i32,
}

/// `IADLXGPUMetrics` through `GPUVRAM`.
#[repr(C)]
struct MetricsVtbl {
    interface: InterfaceVtbl,
    _time_stamp_and_usage: [Slot; 2],
    gpu_clock_speed: IntGetter,
    gpu_vram_clock_speed: IntGetter,
    gpu_temperature: DoubleGetter,
    gpu_hotspot_temperature: DoubleGetter,
    _gpu_power: Slot,
    gpu_total_board_power: DoubleGetter,
    gpu_fan_speed: IntGetter,
    gpu_vram: IntGetter,
}

/// `ADLX_LUID`.
#[repr(C)]
#[derive(Default)]
struct Luid {
    low_part: u32,
    high_part: i32,
}

/// The vtable of the ADLX object at `this`.
///
/// # Safety
/// `this` must be a live ADLX object whose vtable starts with layout `V`.
unsafe fn vtbl<'a, V>(this: *mut c_void) -> &'a V {
    unsafe { &**this.cast::<*const V>() }
}

/// An acquired ADLX interface, released on drop.
struct Owned(NonNull<c_void>);

impl Owned {
    /// Takes over a reference ADLX handed out acquired; `None` for null.
    fn new(raw: *mut c_void) -> Option<Self> {
        NonNull::new(raw).map(Self)
    }

    fn ptr(&self) -> *mut c_void {
        self.0.as_ptr()
    }

    /// # Safety
    /// `V` must be a prefix of this interface's vtable layout.
    unsafe fn vtbl<V>(&self) -> &V {
        unsafe { vtbl(self.ptr()) }
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: every reference-counted ADLX vtable starts with `IADLXInterface`.
        unsafe { (self.vtbl::<InterfaceVtbl>().release)(self.ptr()) };
    }
}

/// A typed export of `library`.
///
/// # Safety
/// `F` must be the export's function pointer type.
unsafe fn export<F: Copy>(library: HMODULE, name: PCSTR) -> Option<F> {
    const { assert!(size_of::<F>() == size_of::<usize>()) };
    let proc = unsafe { GetProcAddress(library, name) }?;
    Some(unsafe { std::mem::transmute_copy(&proc) })
}

fn succeeded(status: i32) -> bool {
    (ADLX_OK..=ADLX_ALREADY_INITIALIZED).contains(&status)
}

/// Degrees from a plausible sensor reading.
fn celsius(raw: f64) -> Option<f32> {
    (raw > 0.0 && raw < 150.0).then_some(raw as f32)
}

fn positive(raw: i32) -> Option<u32> {
    u32::try_from(raw).ok().filter(|v| *v > 0)
}

/// One GPU as ADLX reports it.
#[derive(Debug, Clone, Default, PartialEq)]
struct Reading {
    luid: Option<LuidKey>,
    name: String,
    board_power_w: Option<f32>,
    hotspot_c: Option<f32>,
    temp_c: Option<f32>,
    gpu_clock_mhz: Option<u32>,
    mem_clock_mhz: Option<u32>,
    fan_rpm: Option<u32>,
    vram_used_mb: Option<u64>,
}

impl Reading {
    /// Board power and hotspot replace; the rest only fills what WDDM did not answer.
    fn apply(&self, gpu: &mut GpuSample) {
        gpu.power_w = self.board_power_w.or(gpu.power_w);
        gpu.hotspot_c = self.hotspot_c.or(gpu.hotspot_c);
        gpu.temp_c = gpu.temp_c.or(self.temp_c);
        gpu.gpu_clock_mhz = gpu.gpu_clock_mhz.or(self.gpu_clock_mhz);
        gpu.mem_clock_mhz = gpu.mem_clock_mhz.or(self.mem_clock_mhz);
        gpu.fan_rpm = gpu.fan_rpm.or(self.fan_rpm);
        gpu.memory_used_mb = gpu.memory_used_mb.or(self.vram_used_mb);
    }
}

/// A started ADLX instance, kept for the life of the process.
struct Runtime {
    system: NonNull<c_void>,
    perf: Owned,
}

// SAFETY: ADLX objects are process-global and are only used while `STATE` is locked.
unsafe impl Send for Runtime {}

enum State {
    Untried,
    Ready(Runtime),
    Failed(Instant),
}

static STATE: Mutex<State> = Mutex::new(State::Untried);

impl Runtime {
    /// Loads ADLX from System32 and starts it; the library stays loaded either way.
    fn start() -> Result<Self, String> {
        let library = unsafe { LoadLibraryExW(LIBRARY, None, LOAD_LIBRARY_SEARCH_SYSTEM32) }
            .map_err(|e| format!("no ADLX library: {e}"))?;
        // SAFETY: the export types are the ones ADLX.h declares.
        let initialize = unsafe { export::<InitializeFn>(library, s!("ADLXInitialize")) }
            .ok_or("ADLXInitialize is not exported")?;
        let runtime_version =
            unsafe { export::<QueryFullVersionFn>(library, s!("ADLXQueryFullVersion")) }.and_then(
                |query| {
                    let mut version = 0;
                    (unsafe { query(&mut version) } == ADLX_OK).then_some(version)
                },
            );
        let version = runtime_version.map_or(SDK_VERSION, |v| v.min(SDK_VERSION));

        let mut system = null_mut();
        let status = unsafe { initialize(version, &mut system) };
        if !succeeded(status) {
            return Err(format!("ADLXInitialize({version:#x}) returned {status}"));
        }
        let perf = NonNull::new(system).and_then(|system| {
            let mut perf = null_mut();
            // SAFETY: `system` is the IADLXSystem ADLXInitialize returned.
            let status = unsafe {
                (vtbl::<SystemVtbl>(system.as_ptr()).get_performance_monitoring_services)(
                    system.as_ptr(),
                    &mut perf,
                )
            };
            (status == ADLX_OK)
                .then(|| Owned::new(perf))
                .flatten()
                .map(|perf| (system, perf))
        });
        match perf {
            Some((system, perf)) => Ok(Self { system, perf }),
            None => {
                if let Some(terminate) =
                    unsafe { export::<TerminateFn>(library, s!("ADLXTerminate")) }
                {
                    unsafe { terminate() };
                }
                Err("ADLX started without performance monitoring services".to_string())
            }
        }
    }

    /// Every GPU ADLX lists, with the metrics its driver answered.
    fn read(&self) -> Vec<Reading> {
        // SAFETY: `system` is the live IADLXSystem; each object below is used per its vtable.
        let system = unsafe { vtbl::<SystemVtbl>(self.system.as_ptr()) };
        let mut raw = null_mut();
        if unsafe { (system.get_gpus)(self.system.as_ptr(), &mut raw) } != ADLX_OK {
            return Vec::new();
        }
        let Some(list) = Owned::new(raw) else {
            return Vec::new();
        };
        let gpus = unsafe { list.vtbl::<GpuListVtbl>() };
        let (begin, end) = unsafe { ((gpus.begin)(list.ptr()), (gpus.end)(list.ptr())) };
        (begin..end.min(begin.saturating_add(MAX_GPUS)))
            .filter_map(|at| {
                let mut raw = null_mut();
                (unsafe { (gpus.at_gpu_list)(list.ptr(), at, &mut raw) } == ADLX_OK)
                    .then(|| Owned::new(raw))
                    .flatten()
            })
            .map(|gpu| self.reading(&gpu))
            .collect()
    }

    fn reading(&self, gpu: &Owned) -> Reading {
        let gpu2 = gpu2(gpu);
        let mut reading = Reading {
            luid: gpu2.as_ref().and_then(luid),
            name: name(gpu).unwrap_or_default(),
            ..Default::default()
        };
        if gpu2.as_ref().is_some_and(powered_off) {
            return reading;
        }
        let mut raw = null_mut();
        // SAFETY: `perf` is the live performance monitoring service and `gpu` one of its GPUs.
        let status = unsafe {
            (self.perf.vtbl::<PerfVtbl>().get_current_gpu_metrics)(
                self.perf.ptr(),
                gpu.ptr(),
                &mut raw,
            )
        };
        let Some(metrics) = (status == ADLX_OK).then(|| Owned::new(raw)).flatten() else {
            return reading;
        };
        let m = unsafe { metrics.vtbl::<MetricsVtbl>() };
        let double = |get: DoubleGetter| {
            let mut value = 0.0;
            (unsafe { get(metrics.ptr(), &mut value) } == ADLX_OK).then_some(value)
        };
        let int = |get: IntGetter| {
            let mut value = 0;
            (unsafe { get(metrics.ptr(), &mut value) } == ADLX_OK).then_some(value)
        };
        reading.board_power_w = double(m.gpu_total_board_power)
            .filter(|w| *w > 0.0 && *w < 2000.0)
            .map(|w| w as f32);
        reading.hotspot_c = double(m.gpu_hotspot_temperature).and_then(celsius);
        reading.temp_c = double(m.gpu_temperature).and_then(celsius);
        reading.gpu_clock_mhz = int(m.gpu_clock_speed).and_then(positive);
        reading.mem_clock_mhz = int(m.gpu_vram_clock_speed).and_then(positive);
        reading.fan_rpm = int(m.gpu_fan_speed).and_then(positive);
        reading.vram_used_mb = int(m.gpu_vram).and_then(positive).map(u64::from);
        reading
    }
}

fn name(gpu: &Owned) -> Option<String> {
    let mut raw: *const c_char = null();
    // SAFETY: `gpu` is an IADLXGPU; the name stays valid while it is held.
    let status = unsafe { (gpu.vtbl::<GpuVtbl>().name)(gpu.ptr(), &mut raw) };
    (status == ADLX_OK && !raw.is_null()).then(|| {
        unsafe { CStr::from_ptr(raw) }
            .to_string_lossy()
            .trim()
            .to_string()
    })
}

/// `gpu` as `IADLXGPU2`; `None` on runtimes without it.
fn gpu2(gpu: &Owned) -> Option<Owned> {
    let mut raw = null_mut();
    // SAFETY: `gpu` is an IADLXGPU; a successful query returns an acquired IADLXGPU2.
    let status = unsafe {
        (gpu.vtbl::<InterfaceVtbl>().query_interface)(gpu.ptr(), w!("IADLXGPU2").as_ptr(), &mut raw)
    };
    (status == ADLX_OK).then(|| Owned::new(raw)).flatten()
}

fn luid(gpu2: &Owned) -> Option<LuidKey> {
    let mut luid = Luid::default();
    // SAFETY: `gpu2` is an IADLXGPU2.
    (unsafe { (gpu2.vtbl::<Gpu2Vtbl>().luid)(gpu2.ptr(), &mut luid) } == ADLX_OK)
        .then_some((luid.low_part, luid.high_part))
}

/// `true` when the driver reports the GPU powered off, so its metrics are not read.
fn powered_off(gpu2: &Owned) -> bool {
    let mut state = 0u8;
    // SAFETY: `gpu2` is an IADLXGPU2.
    (unsafe { (gpu2.vtbl::<Gpu2Vtbl>().is_power_off)(gpu2.ptr(), &mut state) } == ADLX_OK)
        && state != 0
}

/// Readings from every GPU ADLX lists; empty when no AMD driver library answers.
fn readings() -> Vec<Reading> {
    let Ok(mut state) = STATE.lock() else {
        return Vec::new();
    };
    if let State::Failed(at) = *state
        && at.elapsed() < RETRY_AFTER
    {
        return Vec::new();
    }
    if !matches!(*state, State::Ready(_)) {
        *state = match Runtime::start() {
            Ok(runtime) => {
                log::info!("[stress-kit/gpu] ADLX started; AMD board power and hotspot enabled");
                State::Ready(runtime)
            }
            Err(e) => {
                log::debug!("[stress-kit/gpu] ADLX unavailable: {e}");
                State::Failed(Instant::now())
            }
        };
    }
    match &*state {
        State::Ready(runtime) => runtime.read(),
        _ => Vec::new(),
    }
}

/// The WDDM adapter `reading` describes: by LUID, else by a name only one AMD adapter carries.
fn matching<'a>(
    gpus: &'a mut [(LuidKey, GpuSample)],
    reading: &Reading,
) -> Option<&'a mut GpuSample> {
    if let Some(luid) = reading.luid {
        return gpus
            .iter_mut()
            .find(|(key, _)| *key == luid)
            .map(|(_, gpu)| gpu);
    }
    let mut named = gpus
        .iter_mut()
        .filter(|(_, gpu)| gpu.vendor == "AMD" && gpu.name.eq_ignore_ascii_case(&reading.name));
    let (_, first) = named.next()?;
    named.next().is_none().then_some(first)
}

/// Adds ADLX board power and hotspot temperature to the AMD adapters in `gpus`.
pub(crate) fn enrich(gpus: &mut [(LuidKey, GpuSample)]) {
    if !gpus.iter().any(|(_, gpu)| gpu.vendor == "AMD") {
        return;
    }
    for reading in readings() {
        if let Some(gpu) = matching(gpus, &reading) {
            reading.apply(gpu);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::offset_of;

    const PTR: usize = size_of::<usize>();

    #[test]
    fn vtable_slots_match_the_adlx_headers() {
        assert_eq!(offset_of!(InterfaceVtbl, release), PTR);
        assert_eq!(offset_of!(InterfaceVtbl, query_interface), 2 * PTR);
        assert_eq!(offset_of!(SystemVtbl, get_gpus), PTR);
        assert_eq!(
            offset_of!(SystemVtbl, get_performance_monitoring_services),
            9 * PTR
        );
        assert_eq!(offset_of!(GpuListVtbl, begin), 5 * PTR);
        assert_eq!(offset_of!(GpuListVtbl, end), 6 * PTR);
        assert_eq!(offset_of!(GpuListVtbl, at_gpu_list), 11 * PTR);
        assert_eq!(offset_of!(GpuVtbl, name), 7 * PTR);
        assert_eq!(offset_of!(Gpu2Vtbl, is_power_off), 23 * PTR);
        assert_eq!(offset_of!(Gpu2Vtbl, luid), 34 * PTR);
        assert_eq!(offset_of!(PerfVtbl, get_current_gpu_metrics), 18 * PTR);
        assert_eq!(offset_of!(MetricsVtbl, gpu_clock_speed), 5 * PTR);
        assert_eq!(offset_of!(MetricsVtbl, gpu_vram_clock_speed), 6 * PTR);
        assert_eq!(offset_of!(MetricsVtbl, gpu_temperature), 7 * PTR);
        assert_eq!(offset_of!(MetricsVtbl, gpu_hotspot_temperature), 8 * PTR);
        assert_eq!(offset_of!(MetricsVtbl, gpu_total_board_power), 10 * PTR);
        assert_eq!(offset_of!(MetricsVtbl, gpu_fan_speed), 11 * PTR);
        assert_eq!(offset_of!(MetricsVtbl, gpu_vram), 12 * PTR);
        assert_eq!((size_of::<Luid>(), align_of::<Luid>()), (8, 4));
    }

    fn amd(luid: LuidKey, name: &str) -> (LuidKey, GpuSample) {
        let gpu = GpuSample {
            vendor: "AMD".into(),
            name: name.into(),
            ..Default::default()
        };
        (luid, gpu)
    }

    #[test]
    fn identical_cards_match_by_luid() {
        let mut gpus = [
            amd((1, 0), "AMD Radeon RX 7900 XTX"),
            amd((2, 0), "AMD Radeon RX 7900 XTX"),
        ];
        let second = Reading {
            luid: Some((2, 0)),
            name: "AMD Radeon RX 7900 XTX".into(),
            board_power_w: Some(355.0),
            ..Default::default()
        };
        second.apply(matching(&mut gpus, &second).expect("matched by LUID"));
        assert_eq!(gpus[0].1.power_w, None);
        assert_eq!(gpus[1].1.power_w, Some(355.0));
    }

    #[test]
    fn a_name_matches_only_when_one_adapter_carries_it() {
        let mut gpus = [
            amd((1, 0), "AMD Radeon RX 9070 XT"),
            amd((2, 0), "AMD Radeon(TM) Graphics"),
        ];
        let unique = Reading {
            name: "amd radeon rx 9070 xt".into(),
            ..Default::default()
        };
        assert!(matching(&mut gpus, &unique).is_some());

        let mut twins = [
            amd((1, 0), "AMD Radeon RX 7900 XTX"),
            amd((2, 0), "AMD Radeon RX 7900 XTX"),
        ];
        let ambiguous = Reading {
            name: "AMD Radeon RX 7900 XTX".into(),
            ..Default::default()
        };
        assert!(matching(&mut twins, &ambiguous).is_none());
    }

    #[test]
    fn wddm_readings_win_and_adlx_adds_power_and_hotspot() {
        let mut gpu = GpuSample {
            temp_c: Some(61.0),
            gpu_clock_mhz: Some(2400),
            ..Default::default()
        };
        Reading {
            board_power_w: Some(304.5),
            hotspot_c: Some(78.0),
            temp_c: Some(63.0),
            gpu_clock_mhz: Some(2410),
            fan_rpm: Some(1450),
            ..Default::default()
        }
        .apply(&mut gpu);
        assert_eq!(gpu.power_w, Some(304.5));
        assert_eq!(gpu.hotspot_c, Some(78.0));
        assert_eq!(gpu.temp_c, Some(61.0));
        assert_eq!(gpu.gpu_clock_mhz, Some(2400));
        assert_eq!(gpu.fan_rpm, Some(1450));
    }

    #[test]
    fn implausible_readings_are_no_reading() {
        assert_eq!(celsius(0.0), None);
        assert_eq!(celsius(-1.0), None);
        assert_eq!(celsius(512.0), None);
        assert_eq!(celsius(71.5), Some(71.5));
        assert_eq!(positive(-1), None);
        assert_eq!(positive(0), None);
        assert_eq!(positive(1800), Some(1800));
    }

    #[test]
    fn every_reading_this_machine_returns_is_plausible() {
        for reading in readings() {
            println!("{reading:?}");
            assert!(reading.board_power_w.is_none_or(|w| w > 0.0 && w < 2000.0));
            assert!(reading.hotspot_c.is_none_or(|t| t > 0.0 && t < 150.0));
        }
    }
}
