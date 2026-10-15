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
    /// Temperature at which the hardware slows the GPU, when the driver publishes one.
    #[serde(default)]
    pub slowdown_temp_c: Option<f32>,
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
        Device, Nvml,
        enum_wrappers::device::{Clock, TemperatureSensor, TemperatureThreshold},
    };
    use std::ops::RangeInclusive;
    use std::sync::{Mutex, OnceLock};

    /// NVML thresholds outside this range are treated as unreported.
    const PLAUSIBLE_THRESHOLD_C: RangeInclusive<u32> = 80..=110;

    /// One device's NVML temperature thresholds in degrees C.
    #[derive(Debug, Clone, Copy, Default, PartialEq)]
    pub(super) struct Thresholds {
        /// GPU max operating temperature, when plausible and below `slowdown_c`.
        pub throttle_c: Option<f32>,
        /// HW slowdown temperature, when plausible.
        pub slowdown_c: Option<f32>,
    }

    impl Thresholds {
        /// Plausibility-filtered thresholds from raw NVML readings.
        pub(super) fn from_raw(gpu_max: Option<u32>, slowdown: Option<u32>) -> Self {
            let plausible = |c: Option<u32>| c.filter(|c| PLAUSIBLE_THRESHOLD_C.contains(c));
            let slowdown = plausible(slowdown);
            let throttle = plausible(gpu_max).filter(|g| slowdown.is_some_and(|s| *g < s));
            Self {
                throttle_c: throttle.map(|c| c as f32),
                slowdown_c: slowdown.map(|c| c as f32),
            }
        }
    }

    struct NvmlState {
        nvml: Nvml,
        /// Per device index, read once at init.
        thresholds: Vec<Thresholds>,
    }

    impl NvmlState {
        fn new(nvml: Nvml) -> Self {
            let thresholds = (0..nvml.device_count().unwrap_or(0))
                .map(|idx| {
                    nvml.device_by_index(idx)
                        .map(|dev| device_thresholds(idx, &dev))
                        .unwrap_or_default()
                })
                .collect();
            Self { nvml, thresholds }
        }
    }

    static NVML: OnceLock<Mutex<Option<NvmlState>>> = OnceLock::new();

    fn nvml() -> &'static Mutex<Option<NvmlState>> {
        NVML.get_or_init(|| {
            let n = Nvml::init().ok().map(NvmlState::new);
            if n.is_none() {
                log::debug!("[stress-kit/gpu] NVML init failed; NVIDIA telemetry disabled");
            }
            Mutex::new(n)
        })
    }

    fn device_thresholds(idx: u32, dev: &Device) -> Thresholds {
        let gpu_max = dev.temperature_threshold(TemperatureThreshold::GpuMax).ok();
        let slowdown = dev
            .temperature_threshold(TemperatureThreshold::Slowdown)
            .ok();
        let picked = Thresholds::from_raw(gpu_max, slowdown);
        log::info!(
            "[stress-kit/gpu] NVML device {idx}: max operating {gpu_max:?} C, slowdown {slowdown:?} C, graded throttle {:?} C, slowdown {:?} C",
            picked.throttle_c,
            picked.slowdown_c
        );
        picked
    }

    pub fn sample() -> Vec<GpuSample> {
        let Ok(guard) = nvml().lock() else { return Vec::new() };
        let Some(state) = guard.as_ref() else { return Vec::new() };
        let nv = &state.nvml;

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
            let thresholds = state
                .thresholds
                .get(idx as usize)
                .copied()
                .unwrap_or_default();

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
                throttle_temp_c: thresholds.throttle_c,
                slowdown_temp_c: thresholds.slowdown_c,
                source: GpuSource::Nvml,
            });
        }
        out
    }
}

#[cfg(all(test, feature = "nvml"))]
mod nvml_threshold_tests {
    use super::nvml_sampler::Thresholds;

    fn picked(gpu_max: Option<u32>, slowdown: Option<u32>) -> (Option<f32>, Option<f32>) {
        let t = Thresholds::from_raw(gpu_max, slowdown);
        (t.throttle_c, t.slowdown_c)
    }

    #[test]
    fn gpu_max_operating_temperature_is_the_throttle_temperature() {
        assert_eq!(picked(Some(93), Some(95)), (Some(93.0), Some(95.0)));
        assert_eq!(picked(Some(87), Some(102)), (Some(87.0), Some(102.0)));
    }

    #[test]
    fn slowdown_never_stands_in_for_a_missing_gpu_max() {
        assert_eq!(picked(None, Some(93)), (None, Some(93.0)));
    }

    #[test]
    fn a_gpu_max_without_a_plausible_slowdown_above_it_is_unused() {
        assert_eq!(picked(Some(93), None), (None, None));
        assert_eq!(picked(Some(93), Some(u32::MAX - 1)), (None, None));
        assert_eq!(picked(Some(95), Some(95)), (None, Some(95.0)));
        assert_eq!(picked(Some(97), Some(95)), (None, Some(95.0)));
    }

    #[test]
    fn implausible_thresholds_are_unused() {
        assert_eq!(picked(None, None), (None, None));
        assert_eq!(picked(Some(0), Some(0)), (None, None));
        assert_eq!(picked(Some(57), Some(97)), (None, Some(97.0)));
        assert_eq!(picked(Some(65), Some(97)), (None, Some(97.0)));
        assert_eq!(picked(Some(79), Some(95)), (None, Some(95.0)));
        assert_eq!(picked(Some(255), Some(111)), (None, None));
        assert_eq!(picked(Some(80), Some(110)), (Some(80.0), Some(110.0)));
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
