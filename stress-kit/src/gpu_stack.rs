//! GPU user-mode driver-stack integrity check.
//!
//! Detects the fault class where Windows' kernel display path works (the
//! card is active in `Win32_VideoController` and rendering the desktop) but
//! the user-mode stack is corrupted: wgpu (Dx12/Vulkan ICDs) cannot see the
//! discrete adapter and NVML enumerates zero devices. On such machines GPU
//! stress/benchmark work silently lands on the iGPU and produces an
//! iGPU-class score that reads as "no GPU" to the operator.
//!
//! Evidence sources, each best-effort:
//!   * WMI `Win32_VideoController` — kernel/PnP view (Windows only).
//!   * wgpu adapter enumeration over `Backends::PRIMARY` (`gpu` feature).
//!   * NVML device count (`nvml` feature; NVIDIA only).
//!
//! A discrete controller that is WMI-active but absent from wgpu (with NVML
//! corroborating for NVIDIA) yields one verdict line in
//! [`GpuStackReport::broken`], prefixed `GPU driver stack broken:` so every
//! downstream log surface stays greppable.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Refresh interval for the cached report; driver state only changes on
/// reinstall, but techs do that mid-session.
const CACHE_TTL: Duration = Duration::from_secs(300);

static CACHE: Mutex<Option<(Instant, GpuStackReport)>> = Mutex::new(None);

/// PCI vendor ids as parsed from `PNPDeviceID` / reported by wgpu.
const VENDOR_NVIDIA: u32 = 0x10DE;
const VENDOR_AMD: u32 = 0x1002;
const VENDOR_INTEL: u32 = 0x8086;
/// Microsoft software rasterizers (WARP / Basic Render Driver).
const VENDOR_MICROSOFT: u32 = 0x1414;

/// One `Win32_VideoController` row, reduced to what the cross-check needs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VideoControllerInfo {
    pub name: String,
    pub pnp_device_id: String,
    pub driver_version: Option<String>,
    /// Device Manager error code; `Some(0)` means the kernel driver is loaded
    /// and the device reports "working properly".
    pub config_manager_error_code: Option<u32>,
    /// Non-zero when the controller is scanning out a desktop resolution.
    pub current_horizontal_resolution: Option<u32>,
    /// PCI vendor id parsed from `pnp_device_id`; 0 for non-PCI controllers.
    pub vendor_id: u32,
    pub active: bool,
    pub rendering: bool,
    pub discrete: bool,
}

/// Cross-check result. Empty `broken` means no integrity fault was proven —
/// not necessarily that every source was available; see `wgpu_checked` /
/// `nvml_status`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GpuStackReport {
    /// WMI controllers seen (Windows only; empty elsewhere).
    pub wmi_controllers: Vec<VideoControllerInfo>,
    /// Human-readable wgpu adapter labels, software rasterizers included.
    pub wgpu_adapters: Vec<String>,
    /// False when the `gpu` feature is off and no wgpu evidence exists.
    pub wgpu_checked: bool,
    /// `Some(n)` only when NVML initialized; `None` covers feature-off and
    /// init failure (see `nvml_status`).
    pub nvml_device_count: Option<u32>,
    pub nvml_status: String,
    /// One verdict line per WMI-active discrete controller missing from the
    /// user-mode stack. Prefix: `GPU driver stack broken:`.
    pub broken: Vec<String>,
}

impl GpuStackReport {
    pub fn is_broken(&self) -> bool {
        !self.broken.is_empty()
    }

    /// True when wgpu found an adapter the GPU stressors can actually run on.
    /// CPU-backed rasterizers answer wgpu but exercise no GPU, so they do not
    /// count. Gate GPU benchmarks on this rather than on NVML telemetry, which
    /// is NVIDIA-only and reports nothing on AMD or Intel.
    pub fn has_hardware_gpu(&self) -> bool {
        self.wgpu_adapters.iter().any(|a| {
            let n = a.to_lowercase();
            !(n.contains("microsoft basic")
                || n.contains("llvmpipe")
                || n.contains("swiftshader"))
        })
    }

    /// Joined verdict lines, `None` when healthy.
    pub fn summary(&self) -> Option<String> {
        if self.broken.is_empty() {
            None
        } else {
            Some(self.broken.join("\n"))
        }
    }
}

/// Cached cross-check; recomputes at most every [`CACHE_TTL`].
pub fn check_gpu_stack() -> GpuStackReport {
    if let Ok(guard) = CACHE.lock() {
        if let Some((at, report)) = guard.as_ref() {
            if at.elapsed() < CACHE_TTL {
                return report.clone();
            }
        }
    }
    let report = check_gpu_stack_uncached();
    if let Ok(mut guard) = CACHE.lock() {
        *guard = Some((Instant::now(), report.clone()));
    }
    report
}

/// Run the WMI / wgpu / NVML cross-check now, bypassing the cache. Blocking
/// (COM + adapter enumeration); call from a worker thread.
pub fn check_gpu_stack_uncached() -> GpuStackReport {
    let wmi_controllers = wmi_scan::query_video_controllers();
    let (wgpu_checked, wgpu_raw) = wgpu_scan::enumerate();
    let (nvml_device_count, nvml_status) = nvml_scan::device_count();

    let wgpu_adapters = wgpu_raw
        .iter()
        .map(|a| format!("{} ({}, vendor 0x{:04x})", a.name, a.backend, a.vendor))
        .collect();

    let mut broken = Vec::new();
    for ctl in wmi_controllers.iter().filter(|c| c.discrete && c.active) {
        let in_wgpu = wgpu_raw.iter().any(|a| adapter_matches(ctl, a));
        if wgpu_checked && in_wgpu {
            continue;
        }
        let nvml_zero = matches!(nvml_device_count, Some(0));
        let nvml_dead = nvml_device_count.is_none();
        let is_nvidia = ctl.vendor_id == VENDOR_NVIDIA;

        // wgpu absence is the primary signal; NVML alone (zero devices after a
        // clean init) is enough for NVIDIA when wgpu evidence is unavailable.
        let proven = if wgpu_checked {
            !in_wgpu
        } else {
            is_nvidia && nvml_zero
        };
        if !proven {
            continue;
        }

        let mut line = format!(
            "GPU driver stack broken: {} is active per Win32_VideoController{} but absent from wgpu Dx12/Vulkan adapter enumeration",
            ctl.name,
            if ctl.rendering { " and driving a display" } else { "" },
        );
        if is_nvidia {
            if nvml_zero {
                line.push_str("; NVML initialized but enumerates 0 devices");
            } else if nvml_dead {
                line.push_str(&format!("; NVML unavailable ({})", nvml_status));
            }
        }
        line.push_str(
            ". The kernel display driver is rendering while user-mode compute/3D \
             (ICDs/NVML) is unavailable — GPU tests fall back to the iGPU and score \
             iGPU-class. Fix: DDU + clean driver reinstall.",
        );
        log::warn!("[stress-kit/gpu_stack] {line}");
        broken.push(line);
    }

    GpuStackReport {
        wmi_controllers,
        wgpu_adapters,
        wgpu_checked,
        nvml_device_count,
        nvml_status,
        broken,
    }
}

struct WgpuAdapter {
    name: String,
    vendor: u32,
    backend: String,
    discrete_name: bool,
}

/// Vendor-id match, with a name check for AMD where iGPU and dGPU share the
/// vendor id. Software rasterizers never match.
fn adapter_matches(ctl: &VideoControllerInfo, adapter: &WgpuAdapter) -> bool {
    if adapter.vendor == VENDOR_MICROSOFT || is_software_adapter(&adapter.name) {
        return false;
    }
    if adapter.vendor != ctl.vendor_id {
        return false;
    }
    if ctl.vendor_id == VENDOR_AMD {
        let a = normalize_name(&adapter.name);
        let c = normalize_name(&ctl.name);
        return a.contains(&c) || c.contains(&a) || adapter.discrete_name;
    }
    true
}

fn is_software_adapter(name: &str) -> bool {
    let n = name.to_lowercase();
    n.contains("basic render") || n.contains("llvmpipe") || n.contains("swiftshader")
}

fn normalize_name(name: &str) -> String {
    name.to_lowercase()
        .replace("(tm)", "")
        .replace("(r)", "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// `PCI\VEN_10DE&DEV_...` → `0x10DE`. 0 when not a PCI id.
fn parse_pci_vendor(pnp_device_id: &str) -> u32 {
    let upper = pnp_device_id.to_uppercase();
    if !upper.starts_with("PCI\\") {
        return 0;
    }
    upper
        .split("VEN_")
        .nth(1)
        .and_then(|s| s.get(..4))
        .and_then(|h| u32::from_str_radix(h, 16).ok())
        .unwrap_or(0)
}

/// Discrete classification mirroring the stressor adapter scoring: NVIDIA is
/// always discrete, AMD unless the iGPU "Graphics" naming, Intel only Arc.
fn is_discrete(vendor_id: u32, name: &str) -> bool {
    let n = name.to_lowercase();
    match vendor_id {
        VENDOR_NVIDIA => true,
        VENDOR_AMD => !(n.contains("graphics") && !n.contains("rx")),
        VENDOR_INTEL => n.contains("arc"),
        _ => false,
    }
}

#[cfg(target_os = "windows")]
mod wmi_scan {
    use super::VideoControllerInfo;
    use serde::Deserialize;
    use wmi::WMIConnection;

    #[derive(Deserialize, Debug)]
    #[serde(rename = "Win32_VideoController")]
    #[serde(rename_all = "PascalCase")]
    struct Win32VideoController {
        name: Option<String>,
        #[serde(rename = "PNPDeviceID")]
        pnp_device_id: Option<String>,
        driver_version: Option<String>,
        config_manager_error_code: Option<u32>,
        current_horizontal_resolution: Option<u32>,
    }

    pub(super) fn query_video_controllers() -> Vec<VideoControllerInfo> {
        let wmi = match WMIConnection::with_namespace_path("ROOT\\CIMV2") {
            Ok(w) => w,
            Err(e) => {
                log::warn!("[stress-kit/gpu_stack] ROOT\\CIMV2 connect failed: {e}");
                return Vec::new();
            }
        };
        let rows: Vec<Win32VideoController> = match wmi.query() {
            Ok(r) => r,
            Err(e) => {
                log::warn!("[stress-kit/gpu_stack] Win32_VideoController query failed: {e}");
                return Vec::new();
            }
        };
        rows.into_iter()
            .filter_map(|r| {
                let name = r.name?.trim().to_string();
                if name.is_empty() {
                    return None;
                }
                let pnp = r.pnp_device_id.unwrap_or_default();
                let vendor_id = super::parse_pci_vendor(&pnp);
                Some(VideoControllerInfo {
                    active: r.config_manager_error_code == Some(0),
                    rendering: r.current_horizontal_resolution.unwrap_or(0) > 0,
                    discrete: vendor_id != 0 && super::is_discrete(vendor_id, &name),
                    vendor_id,
                    name,
                    pnp_device_id: pnp,
                    driver_version: r.driver_version,
                    config_manager_error_code: r.config_manager_error_code,
                    current_horizontal_resolution: r.current_horizontal_resolution,
                })
            })
            .collect()
    }
}

#[cfg(not(target_os = "windows"))]
mod wmi_scan {
    use super::VideoControllerInfo;
    pub(super) fn query_video_controllers() -> Vec<VideoControllerInfo> {
        Vec::new()
    }
}

#[cfg(feature = "gpu")]
mod wgpu_scan {
    use super::WgpuAdapter;
    use wgpu::{Backends, Instance, InstanceDescriptor};

    pub(super) fn enumerate() -> (bool, Vec<WgpuAdapter>) {
        let instance = Instance::new(InstanceDescriptor {
            backends: Backends::PRIMARY,
            ..InstanceDescriptor::new_without_display_handle()
        });
        let adapters = pollster::block_on(instance.enumerate_adapters(Backends::PRIMARY))
            .into_iter()
            .map(|a| {
                let info = a.get_info();
                WgpuAdapter {
                    discrete_name: super::is_discrete(info.vendor, &info.name),
                    name: info.name,
                    vendor: info.vendor,
                    backend: format!("{:?}", info.backend),
                }
            })
            .collect();
        (true, adapters)
    }
}

#[cfg(not(feature = "gpu"))]
mod wgpu_scan {
    use super::WgpuAdapter;
    pub(super) fn enumerate() -> (bool, Vec<WgpuAdapter>) {
        (false, Vec::new())
    }
}

#[cfg(feature = "nvml")]
mod nvml_scan {
    pub(super) fn device_count() -> (Option<u32>, String) {
        match nvml_wrapper::Nvml::init() {
            Ok(nv) => match nv.device_count() {
                Ok(n) => (Some(n), format!("ok: {n} device(s)")),
                Err(e) => (None, format!("device_count failed: {e}")),
            },
            Err(e) => (None, format!("init failed: {e}")),
        }
    }
}

#[cfg(not(feature = "nvml"))]
mod nvml_scan {
    pub(super) fn device_count() -> (Option<u32>, String) {
        (None, "feature disabled".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctl(name: &str, pnp: &str) -> VideoControllerInfo {
        let vendor_id = parse_pci_vendor(pnp);
        VideoControllerInfo {
            name: name.into(),
            pnp_device_id: pnp.into(),
            vendor_id,
            discrete: vendor_id != 0 && is_discrete(vendor_id, name),
            active: true,
            rendering: true,
            ..Default::default()
        }
    }

    #[test]
    fn pci_vendor_parses() {
        assert_eq!(parse_pci_vendor("PCI\\VEN_10DE&DEV_2702&SUBSYS_0001"), 0x10DE);
        assert_eq!(parse_pci_vendor("pci\\ven_8086&dev_a780"), 0x8086);
        assert_eq!(parse_pci_vendor("ROOT\\BasicDisplay\\0000"), 0);
    }

    #[test]
    fn discrete_classification() {
        assert!(ctl("NVIDIA GeForce RTX 4080 SUPER", "PCI\\VEN_10DE&DEV_2702").discrete);
        assert!(ctl("AMD Radeon RX 7800 XT", "PCI\\VEN_1002&DEV_747E").discrete);
        assert!(!ctl("AMD Radeon(TM) Graphics", "PCI\\VEN_1002&DEV_164E").discrete);
        assert!(!ctl("Intel(R) UHD Graphics 770", "PCI\\VEN_8086&DEV_A780").discrete);
        assert!(ctl("Intel(R) Arc(TM) A770 Graphics", "PCI\\VEN_8086&DEV_56A0").discrete);
        assert!(!ctl("Microsoft Basic Display Adapter", "ROOT\\BasicDisplay\\0000").discrete);
    }

    #[test]
    fn nvidia_matches_by_vendor_amd_by_name() {
        let rtx = ctl("NVIDIA GeForce RTX 4080 SUPER", "PCI\\VEN_10DE&DEV_2702");
        let nv_adapter = WgpuAdapter {
            name: "NVIDIA GeForce RTX 4080 SUPER".into(),
            vendor: VENDOR_NVIDIA,
            backend: "Dx12".into(),
            discrete_name: true,
        };
        let igpu_adapter = WgpuAdapter {
            name: "Intel(R) UHD Graphics 770".into(),
            vendor: VENDOR_INTEL,
            backend: "Dx12".into(),
            discrete_name: false,
        };
        let warp = WgpuAdapter {
            name: "Microsoft Basic Render Driver".into(),
            vendor: VENDOR_MICROSOFT,
            backend: "Dx12".into(),
            discrete_name: false,
        };
        assert!(adapter_matches(&rtx, &nv_adapter));
        assert!(!adapter_matches(&rtx, &igpu_adapter));
        assert!(!adapter_matches(&rtx, &warp));

        let rx = ctl("AMD Radeon RX 7800 XT", "PCI\\VEN_1002&DEV_747E");
        let amd_igpu = WgpuAdapter {
            name: "AMD Radeon(TM) Graphics".into(),
            vendor: VENDOR_AMD,
            backend: "Vulkan".into(),
            discrete_name: false,
        };
        assert!(!adapter_matches(&rx, &amd_igpu));
    }
}
