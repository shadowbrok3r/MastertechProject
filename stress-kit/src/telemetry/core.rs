//! Per-logical-core CPU sample. Mirrors the legacy `qc-app::hw_sampler::CoreRow`
//! so the qc-app egui table can switch over with minimal churn.

use serde::{Deserialize, Serialize};
use sysinfo::{Components, System};

use super::CpuDieThermal;

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
    /// °C from this core's own sensor; `None` when this core has none. A
    /// package-level reading is never copied here.
    pub temp_c: Option<f32>,
}

/// Per-core sample with no die-sensor input; per-core temps come only from a
/// `Component` naming that core.
pub fn sample_cores(sys: &System, components: &Components) -> Vec<CoreSample> {
    sample_cores_with_die(sys, components, None)
}

/// Per-core sample where `die` supplies Intel DTS per-core temperatures, which
/// take precedence over `sysinfo`'s component list.
pub fn sample_cores_with_die(
    sys: &System,
    components: &Components,
    die: Option<&CpuDieThermal>,
) -> Vec<CoreSample> {
    sys.cpus()
        .iter()
        .enumerate()
        .map(|(i, cpu)| CoreSample {
            index: i,
            brand: cpu.brand().to_string(),
            name: cpu.name().to_string(),
            usage_pct: cpu.cpu_usage(),
            freq_mhz: cpu.frequency(),
            temp_c: core_temp_c(components, die, i),
        })
        .collect()
}

/// This core's own temperature: its die-sensor value, else a component naming it.
fn core_temp_c(components: &Components, die: Option<&CpuDieThermal>, idx: usize) -> Option<f32> {
    die.and_then(|d| d.core_c(idx))
        .or_else(|| component_core_temp(components, idx))
}

/// Temperature of the component whose label names exactly this core.
fn component_core_temp(components: &Components, idx: usize) -> Option<f32> {
    components
        .iter()
        .find(|c| is_core_component_label(c.label(), idx))
        .and_then(|c| c.temperature())
}

/// True for a `Core N` / `CPU Core N` label naming exactly this core index.
fn is_core_component_label(label: &str, idx: usize) -> bool {
    let l = label.trim().to_lowercase();
    l == format!("core {idx}") || l == format!("cpu core {idx}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::CpuDieReader;

    #[test]
    fn only_a_label_naming_this_core_matches() {
        assert!(is_core_component_label("Core 1", 1));
        assert!(is_core_component_label("CPU Core 1", 1));
        assert!(!is_core_component_label("Core 10", 1));
        assert!(!is_core_component_label("Core 1", 10));
        assert!(!is_core_component_label("Package id 0", 0));
        assert!(!is_core_component_label("CPU", 0));
        assert!(!is_core_component_label("Physical id 0", 0));
        assert!(!is_core_component_label("CPUZ_0", 0));
    }

    #[test]
    fn a_package_only_die_leaves_every_core_without_a_temperature() {
        let die = CpuDieThermal {
            package_c: Some(70.0),
            cores: Vec::new(),
            reader: CpuDieReader::AmdTctl,
        };
        let components = Components::new();
        assert_eq!(core_temp_c(&components, Some(&die), 0), None);
        assert_eq!(core_temp_c(&components, Some(&die), 7), None);
    }

    #[test]
    fn a_core_takes_only_its_own_die_value() {
        let die = CpuDieThermal {
            package_c: Some(70.0),
            cores: vec![Some(68.0), None],
            reader: CpuDieReader::IntelDts,
        };
        let components = Components::new();
        assert_eq!(core_temp_c(&components, Some(&die), 0), Some(68.0));
        assert_eq!(core_temp_c(&components, Some(&die), 1), None);
        assert_eq!(core_temp_c(&components, None, 0), None);
    }
}
