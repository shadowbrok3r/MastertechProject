//! GPU telemetry sampling: NVML for NVIDIA, WDDM performance data for every other Windows adapter
//! (ADLX adds AMD board power and hotspot), sysinfo last.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use sysinfo::Components;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GpuSample {
    pub index: usize,
    pub vendor: String,
    pub name: String,
    pub temp_c: Option<f32>,
    pub usage_pct: Option<f32>,
    pub memory_used_mb: Option<u64>,
    pub memory_total_mb: Option<u64>,
    pub power_w: Option<f32>,
    pub power_limit_w: Option<f32>,
    pub gpu_clock_mhz: Option<u32>,
    pub mem_clock_mhz: Option<u32>,
    pub pcie_replay_counter: Option<u32>,
    pub pcie_link_gen: Option<u32>,
    pub pcie_link_width: Option<u32>,
    pub ecc_errors_corrected: Option<u64>,
    pub ecc_errors_uncorrected: Option<u64>,
    pub fan_pct: Option<u32>,
    pub throttle_reasons: Vec<String>,
    pub driver_version: Option<String>,
    /// Power draw as a percentage of the adapter's own limit.
    #[serde(default)]
    pub power_pct: Option<f32>,
    #[serde(default)]
    pub fan_rpm: Option<u32>,
    /// Hottest on-die sensor, when the vendor library reports one.
    #[serde(default)]
    pub hotspot_c: Option<f32>,
    /// Temperature at which the driver starts throttling, when it publishes one.
    #[serde(default)]
    pub throttle_temp_c: Option<f32>,
    #[serde(default)]
    pub source: GpuSource,
}

/// Reader that produced a [`GpuSample`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuSource {
    Nvml,
    /// Windows adapter performance data; which sensor its temperature is depends on the vendor.
    Wddm,
    Sysinfo,
    #[default]
    #[serde(other)]
    Unknown,
}

#[cfg(feature = "nvml")]
mod nvml_sampler {
    use super::{GpuSample, GpuSource};
    use nvml_wrapper::{
        enum_wrappers::device::{Clock, TemperatureSensor},
        Nvml,
    };
    use std::sync::{Mutex, OnceLock};

    static NVML: OnceLock<Mutex<Option<Nvml>>> = OnceLock::new();

    fn nvml() -> &'static Mutex<Option<Nvml>> {
        NVML.get_or_init(|| {
            let n = Nvml::init().ok();
            if n.is_none() {
                log::debug!("[stress-kit/gpu] NVML init failed; NVIDIA telemetry disabled");
            }
            Mutex::new(n)
        })
    }

    pub fn sample() -> Vec<GpuSample> {
        let Ok(guard) = nvml().lock() else { return Vec::new() };
        let Some(nv) = guard.as_ref() else { return Vec::new() };

        let driver = nv.sys_driver_version().ok();
        let count = match nv.device_count() {
            Ok(c) => c,
            Err(e) => {
                log::debug!("[stress-kit/gpu] nvml device_count failed: {e}");
                return Vec::new();
            }
        };

        let mut out = Vec::with_capacity(count as usize);
        for idx in 0..count {
            let Ok(dev) = nv.device_by_index(idx) else { continue };
            let name = dev.name().unwrap_or_else(|_| format!("GPU {idx}"));
            let temp_c = dev.temperature(TemperatureSensor::Gpu).ok().map(|t| t as f32);
            let util = dev.utilization_rates().ok();
            let usage_pct = util.as_ref().map(|u| u.gpu as f32);
            let mem = dev.memory_info().ok();
            let memory_used_mb = mem.as_ref().map(|m| m.used / (1024 * 1024));
            let memory_total_mb = mem.as_ref().map(|m| m.total / (1024 * 1024));
            let power_w = dev.power_usage().ok().map(|mw| mw as f32 / 1000.0);
            let power_limit_w = dev
                .enforced_power_limit()
                .ok()
                .map(|mw| mw as f32 / 1000.0);
            let gpu_clock_mhz = dev.clock_info(Clock::Graphics).ok();
            let mem_clock_mhz = dev.clock_info(Clock::Memory).ok();
            let pcie_replay_counter = dev.pcie_replay_counter().ok();
            let pcie_link_gen = dev.current_pcie_link_gen().ok();
            let pcie_link_width = dev.current_pcie_link_width().ok();
            let ecc_errors_corrected = dev
                .total_ecc_errors(
                    nvml_wrapper::enum_wrappers::device::MemoryError::Corrected,
                    nvml_wrapper::enum_wrappers::device::EccCounter::Aggregate,
                )
                .ok();
            let ecc_errors_uncorrected = dev
                .total_ecc_errors(
                    nvml_wrapper::enum_wrappers::device::MemoryError::Uncorrected,
                    nvml_wrapper::enum_wrappers::device::EccCounter::Aggregate,
                )
                .ok();
            let fan_pct = dev.fan_speed(0).ok();
            let throttle_reasons = dev
                .current_throttle_reasons()
                .map(|r| format!("{:?}", r))
                .ok()
                .into_iter()
                .filter(|s| s != "(empty)")
                .collect();

            out.push(GpuSample {
                index: idx as usize,
                vendor: "NVIDIA".into(),
                name,
                temp_c,
                usage_pct,
                memory_used_mb,
                memory_total_mb,
                power_w,
                power_limit_w,
                gpu_clock_mhz,
                mem_clock_mhz,
                pcie_replay_counter,
                pcie_link_gen,
                pcie_link_width,
                ecc_errors_corrected,
                ecc_errors_uncorrected,
                fan_pct,
                throttle_reasons,
                driver_version: driver.clone(),
                power_pct: None,
                fan_rpm: None,
                hotspot_c: None,
                throttle_temp_c: None,
                source: GpuSource::Nvml,
            });
        }
        out
    }
}

#[cfg(not(feature = "nvml"))]
mod nvml_sampler {
    use super::GpuSample;
    pub fn sample() -> Vec<GpuSample> { Vec::new() }
}

/// GPU readers that keep state between ticks.
#[derive(Default)]
pub(crate) struct GpuSampler {
    #[cfg(target_os = "windows")]
    wddm: super::gpu_wddm::WddmGpuMonitor,
}

impl GpuSampler {
    /// NVML devices, then WDDM adapters NVML does not cover, then sysinfo labels no reader named.
    pub(crate) fn sample(&mut self, components: &Components) -> Vec<GpuSample> {
        let nvml = nvml_sampler::sample();
        #[cfg(target_os = "windows")]
        let mut out = {
            let mut wddm = self.wddm.sample();
            super::gpu_adlx::enrich(&mut wddm);
            with_wddm(nvml, wddm.into_iter().map(|(_, gpu)| gpu).collect())
        };
        #[cfg(not(target_os = "windows"))]
        let mut out = nvml;

        let named: HashSet<String> = out.iter().map(|g| g.name.to_lowercase()).collect();
        for c in components.iter() {
            let label = c.label();
            if !is_gpu_label(label) || named.contains(&label.to_lowercase()) {
                continue;
            }
            out.push(GpuSample {
                vendor: classify_vendor(label),
                name: label.to_string(),
                temp_c: c.temperature(),
                source: GpuSource::Sysinfo,
                ..Default::default()
            });
        }
        for (i, gpu) in out.iter_mut().enumerate() {
            gpu.index = i;
        }
        out
    }
}

/// NVML devices followed by the WDDM adapters NVML does not cover.
#[cfg(target_os = "windows")]
fn with_wddm(nvml: Vec<GpuSample>, wddm: Vec<GpuSample>) -> Vec<GpuSample> {
    let nvml_answered = !nvml.is_empty();
    let mut out = nvml;
    out.extend(
        wddm.into_iter()
            .filter(|gpu| !(nvml_answered && gpu.vendor == "NVIDIA")),
    );
    out
}

fn is_gpu_label(label: &str) -> bool {
    let l = label.to_lowercase();
    l.contains("gpu")
        || l.contains("gfx")
        || l.contains("radeon")
        || l.contains("nvidia")
        || l.contains("amdgpu")
        || l == "edge"
}

fn classify_vendor(label: &str) -> String {
    let l = label.to_lowercase();
    if l.contains("nvidia") {
        "NVIDIA".into()
    } else if l.contains("amd") || l.contains("radeon") || l.contains("amdgpu") {
        "AMD".into()
    } else if l.contains("intel") || l.contains("i915") {
        "Intel".into()
    } else {
        "Unknown".into()
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    fn gpu(vendor: &str, name: &str, source: GpuSource) -> GpuSample {
        GpuSample {
            vendor: vendor.into(),
            name: name.into(),
            source,
            ..Default::default()
        }
    }

    #[test]
    fn nvml_covers_every_nvidia_adapter_wddm_lists() {
        let nvml = vec![gpu("NVIDIA", "GeForce GTX 1080", GpuSource::Nvml)];
        let wddm = vec![
            gpu("NVIDIA", "NVIDIA GeForce GTX 1080", GpuSource::Wddm),
            gpu("Intel", "Intel(R) UHD Graphics 630", GpuSource::Wddm),
        ];
        let names: Vec<String> = with_wddm(nvml, wddm).into_iter().map(|g| g.name).collect();
        assert_eq!(names, ["GeForce GTX 1080", "Intel(R) UHD Graphics 630"]);
    }

    #[test]
    fn wddm_reports_nvidia_when_nvml_is_silent() {
        let wddm = vec![gpu("NVIDIA", "NVIDIA GeForce RTX 4070", GpuSource::Wddm)];
        assert_eq!(with_wddm(Vec::new(), wddm).len(), 1);
    }

    #[test]
    fn identical_cards_stay_separate() {
        let wddm = vec![
            gpu("AMD", "AMD Radeon RX 7900 XTX", GpuSource::Wddm),
            gpu("AMD", "AMD Radeon RX 7900 XTX", GpuSource::Wddm),
        ];
        assert_eq!(with_wddm(Vec::new(), wddm).len(), 2);
    }

    #[test]
    fn source_round_trips_and_absorbs_unknown_readers() {
        assert_eq!(serde_json::to_string(&GpuSource::Wddm).unwrap(), "\"wddm\"");
        let old: GpuSample = serde_json::from_str(r#"{"index":0,"vendor":"AMD","name":"x","temp_c":null,"usage_pct":null,"memory_used_mb":null,"memory_total_mb":null,"power_w":null,"power_limit_w":null,"gpu_clock_mhz":null,"mem_clock_mhz":null,"pcie_replay_counter":null,"pcie_link_gen":null,"pcie_link_width":null,"ecc_errors_corrected":null,"ecc_errors_uncorrected":null,"fan_pct":null,"throttle_reasons":[],"driver_version":null}"#).unwrap();
        assert_eq!(old.source, GpuSource::Unknown);
        let future: GpuSource = serde_json::from_str("\"adlx\"").unwrap();
        assert_eq!(future, GpuSource::Unknown);
    }
}
