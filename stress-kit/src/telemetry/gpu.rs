//! GPU telemetry sampling. NVML-backed for NVIDIA; sysinfo fallback otherwise.

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
}

#[cfg(feature = "nvml")]
mod nvml_sampler {
    use super::GpuSample;
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

pub fn sample_gpus(components: &Components) -> Vec<GpuSample> {
    let mut out = nvml_sampler::sample();

    let seen_names: std::collections::HashSet<String> =
        out.iter().map(|g| g.name.to_lowercase()).collect();

    for c in components.iter() {
        let label = c.label();
        if !is_gpu_label(label) {
            continue;
        }
        if seen_names.contains(&label.to_lowercase()) {
            continue;
        }
        out.push(GpuSample {
            index: out.len(),
            vendor: classify_vendor(label),
            name: label.to_string(),
            temp_c: c.temperature(),
            ..Default::default()
        });
    }
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
