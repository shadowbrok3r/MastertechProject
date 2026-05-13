//! Per-logical-core CPU sample. Mirrors the legacy `qc-app::hw_sampler::CoreRow`
//! so the qc-app egui table can switch over with minimal churn.

use serde::{Deserialize, Serialize};
use sysinfo::{Components, System};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CoreSample {
    pub index: usize,
    /// CPU marketing name (same for every core on a single-socket system).
    pub brand: String,
    /// `sysinfo` core id (e.g. `cpu3`).
    pub name: String,
    /// 0–100.
    pub usage_pct: f32,
    /// MHz.
    pub freq_mhz: u64,
    /// °C when a matching `Component` exists (rare on Windows without vendor drivers).
    pub temp_c: Option<f32>,
}

pub fn sample_cores(sys: &System, components: &Components) -> Vec<CoreSample> {
    sys.cpus()
        .iter()
        .enumerate()
        .map(|(i, cpu)| CoreSample {
            index: i,
            brand: cpu.brand().to_string(),
            name: cpu.name().to_string(),
            usage_pct: cpu.cpu_usage(),
            freq_mhz: cpu.frequency(),
            temp_c: read_core_temp(components, i),
        })
        .collect()
}

fn read_core_temp(components: &Components, idx: usize) -> Option<f32> {
    let core_label = format!("Core {idx}");
    if let Some(c) = components.iter().find(|c| c.label() == core_label) {
        return c.temperature();
    }
    components
        .iter()
        .find(|c| {
            let l = c.label().to_lowercase();
            l.contains("package") || l.contains("physical id 0") || l.contains("cpu")
        })
        .and_then(|c| c.temperature())
}
