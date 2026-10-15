//! Record-time bounds: impossible readings are stored as null and reported as an event.

use std::collections::HashSet;

use stress_kit::telemetry::{GpuSample, TelemetrySnapshot};
use stress_kit::{Metrics, Stressor};

/// `stress_test_event.code` of a reading cleared as physically impossible.
pub const IMPLAUSIBLE_VALUE_CODE: &str = "implausible_value";

/// Above any single desktop card's board power (12V-2x6 plus slot is 675 W).
pub const DESKTOP_GPU_MAX_W: f32 = 1_000.0;
/// Above the highest laptop GPU TGP including Dynamic Boost (175 W).
pub const LAPTOP_GPU_MAX_W: f32 = 250.0;
/// Multiple of a desktop card's enforced power limit that no 1 s NVML power average reaches.
pub const ENFORCED_LIMIT_MULTIPLE: f32 = 2.0;

/// 64 FP32 flops per cycle (two 512-bit FMA ports) at 6.5 GHz: no x86 logical CPU exceeds it.
pub const CPU_MAX_GFLOPS_PER_LOGICAL_CPU: f64 = 416.0;
/// Above the fastest desktop GPU's FP32 peak (RTX PRO 6000 Blackwell, about 126 TFLOPS).
pub const DESKTOP_GPU_MAX_GFLOPS: f64 = 130_000.0;
/// Above the fastest laptop GPU's FP32 peak (RTX 5090 Laptop, about 45 TFLOPS).
pub const LAPTOP_GPU_MAX_GFLOPS: f64 = 60_000.0;
/// Above the fastest integrated GPU's FP32 peak (Radeon 8060S, about 30 TFLOPS dual-issue).
pub const INTEGRATED_GPU_MAX_GFLOPS: f64 = 40_000.0;

/// Form factor that bounds what a GPU can draw and compute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuClass {
    Desktop,
    Laptop,
    Integrated,
}

impl GpuClass {
    /// Class from the adapter name and, when known, wgpu's `DeviceType` label.
    pub fn of(name: &str, device_type: Option<&str>) -> Self {
        if device_type == Some("IntegratedGpu") {
            return Self::Integrated;
        }
        let name = name.to_ascii_lowercase();
        if ["laptop", "mobile", "max-q"]
            .iter()
            .any(|m| name.contains(m))
        {
            return Self::Laptop;
        }
        Self::Desktop
    }

    /// Class of the adapter the current run's GPU work bound; `None` before any GPU stage.
    fn of_selected_adapter() -> Option<Self> {
        stress_kit::last_selected_adapter().map(|a| Self::of(&a.name, Some(&a.device_type)))
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Desktop => "desktop GPU",
            Self::Laptop => "laptop GPU",
            Self::Integrated => "integrated GPU",
        }
    }

    /// Board power ceiling; an integrated GPU is held to the desktop number.
    pub fn max_watts(self) -> f32 {
        match self {
            Self::Laptop => LAPTOP_GPU_MAX_W,
            Self::Desktop | Self::Integrated => DESKTOP_GPU_MAX_W,
        }
    }

    pub fn max_gflops(self) -> f64 {
        match self {
            Self::Desktop => DESKTOP_GPU_MAX_GFLOPS,
            Self::Laptop => LAPTOP_GPU_MAX_GFLOPS,
            Self::Integrated => INTEGRATED_GPU_MAX_GFLOPS,
        }
    }
}

/// Highest believable board power for one GPU sample and what it derives from.
#[derive(Debug, Clone, PartialEq)]
pub struct PowerCeiling {
    pub watts: f32,
    pub basis: String,
}

/// Form-factor ceiling, lowered for a desktop card to a multiple of its enforced power limit.
pub fn gpu_power_ceiling(sample: &GpuSample) -> PowerCeiling {
    let class = GpuClass::of(&sample.name, None);
    let form_factor = class.max_watts();
    match sample
        .power_limit_w
        .map(|limit| (limit, limit * ENFORCED_LIMIT_MULTIPLE))
    {
        Some((limit, watts))
            if class == GpuClass::Desktop
                && limit.is_finite()
                && limit > 0.0
                && watts < form_factor =>
        {
            PowerCeiling {
                watts,
                basis: format!(
                    "{ENFORCED_LIMIT_MULTIPLE:.0}x its {limit:.0} W enforced power limit"
                ),
            }
        }
        _ => PowerCeiling {
            watts: form_factor,
            basis: class.label().to_string(),
        },
    }
}

/// One GPU's first power reading above its ceiling.
#[derive(Debug, Clone, PartialEq)]
pub struct PowerBreach {
    pub gpu_index: usize,
    pub gpu_name: String,
    pub watts: f32,
    pub ceiling: PowerCeiling,
}

impl PowerBreach {
    pub fn detail(&self) -> String {
        format!(
            "GPU {} '{}' reported {:.0} W, above its {:.0} W ceiling ({}); its power readings \
             above that are stored as null",
            self.gpu_index, self.gpu_name, self.watts, self.ceiling.watts, self.ceiling.basis
        )
    }

    pub fn data(&self) -> serde_json::Value {
        serde_json::json!({
            "field": "power_w",
            "gpu_index": self.gpu_index,
            "gpu_name": self.gpu_name,
            "value": finite_or_null(f64::from(self.watts)),
            "ceiling": self.ceiling.watts,
            "unit": "W",
            "basis": self.ceiling.basis,
        })
    }
}

/// Clears GPU power readings above their ceiling, reporting each GPU's first breach once per run.
#[derive(Debug, Default)]
pub struct PowerGate {
    reported: HashSet<(usize, String)>,
}

impl PowerGate {
    pub fn screen(&mut self, snapshot: &mut TelemetrySnapshot) -> Vec<PowerBreach> {
        let mut first = Vec::new();
        for gpu in &mut snapshot.gpus {
            let Some(watts) = gpu.power_w else { continue };
            let ceiling = gpu_power_ceiling(gpu);
            if watts.is_finite() && (0.0..=ceiling.watts).contains(&watts) {
                continue;
            }
            gpu.power_w = None;
            if self.reported.insert((gpu.index, gpu.name.clone())) {
                first.push(PowerBreach {
                    gpu_index: gpu.index,
                    gpu_name: gpu.name.clone(),
                    watts,
                    ceiling,
                });
            }
        }
        first
    }
}

/// Logical CPUs the machine exposes, taking the larger of telemetry and the OS count.
pub fn logical_cpus(snapshot: &TelemetrySnapshot) -> usize {
    let os = std::thread::available_parallelism().map_or(1, |n| n.get());
    snapshot.cores.len().max(os)
}

/// Throughput ceiling in the stressor's own unit, or `None` for stressors not measured in flops.
pub fn throughput_ceiling(stressor: Stressor, logical_cpus: usize, gpu: GpuClass) -> Option<f64> {
    let cpu = logical_cpus.max(1) as f64 * CPU_MAX_GFLOPS_PER_LOGICAL_CPU;
    match stressor {
        Stressor::Fp | Stressor::Matrix => Some(cpu * 1_000.0),
        Stressor::Linpack | Stressor::Avx2 => Some(cpu),
        Stressor::Gpu | Stressor::GpuMatmul => Some(gpu.max_gflops()),
        Stressor::Psu | Stressor::PsuTransient | Stressor::Combined => Some(cpu + gpu.max_gflops()),
        _ => None,
    }
}

/// Human-readable derivation of a stressor's ceiling.
fn ceiling_basis(stressor: Stressor, logical_cpus: usize, gpu: GpuClass) -> String {
    let cpu = format!("{logical_cpus} logical CPUs x {CPU_MAX_GFLOPS_PER_LOGICAL_CPU:.0} GFLOPS");
    let gpu = format!("{} {:.0} GFLOPS", gpu.label(), gpu.max_gflops());
    if stressor.is_gpu() {
        gpu
    } else if stressor.has_gpu_leg() {
        format!("{cpu} + {gpu}")
    } else {
        cpu
    }
}

/// A stage's first throughput sample above its stressor's ceiling.
#[derive(Debug, Clone, PartialEq)]
pub struct ThroughputBreach {
    pub stage: String,
    pub stressor: Stressor,
    pub value: f64,
    pub ceiling: f64,
    pub basis: String,
}

impl ThroughputBreach {
    pub fn detail(&self) -> String {
        let unit = self.stressor.throughput_unit();
        format!(
            "stage '{}' ({}): {:.0} {unit} exceeds the {:.0} {unit} ceiling ({}); samples above \
             it are stored as null",
            self.stage,
            self.stressor.as_str(),
            self.value,
            self.ceiling,
            self.basis
        )
    }

    pub fn data(&self) -> serde_json::Value {
        serde_json::json!({
            "field": "throughput",
            "stage": self.stage,
            "stressor": self.stressor.as_str(),
            "value": finite_or_null(self.value),
            "ceiling": self.ceiling,
            "unit": self.stressor.throughput_unit(),
            "basis": self.basis,
        })
    }
}

/// Screens one stage's throughput samples against its stressor's ceiling.
#[derive(Debug)]
pub struct ThroughputGate {
    stage: String,
    stressor: Stressor,
    logical_cpus: usize,
    reported: bool,
    last_breached: bool,
}

impl ThroughputGate {
    pub fn new(stage: &str, stressor: Stressor, logical_cpus: usize) -> Self {
        Self {
            stage: stage.to_string(),
            stressor,
            logical_cpus,
            reported: false,
            last_breached: false,
        }
    }

    /// Zeroes a throughput above the ceiling; returns the breach the first time the stage breaks it.
    pub fn screen(&mut self, metrics: &mut Metrics) -> Option<ThroughputBreach> {
        let gpu = GpuClass::of_selected_adapter().unwrap_or(GpuClass::Desktop);
        self.screen_with(metrics, gpu)
    }

    fn screen_with(&mut self, metrics: &mut Metrics, gpu: GpuClass) -> Option<ThroughputBreach> {
        let value = metrics.throughput;
        let ceiling = throughput_ceiling(self.stressor, self.logical_cpus, gpu);
        self.last_breached = match ceiling {
            Some(c) => !value.is_finite() || value > c,
            None => !value.is_finite(),
        };
        if !self.last_breached {
            return None;
        }
        metrics.throughput = 0.0;
        if std::mem::replace(&mut self.reported, true) {
            return None;
        }
        Some(ThroughputBreach {
            stage: self.stage.clone(),
            stressor: self.stressor,
            value,
            ceiling: ceiling.unwrap_or(f64::INFINITY),
            basis: ceiling_basis(self.stressor, self.logical_cpus, gpu),
        })
    }

    /// The value to store for the latest screened sample: `None` when it was cleared.
    pub fn recorded(&self, throughput: f64) -> Option<f64> {
        (!self.last_breached).then_some(throughput)
    }
}

fn finite_or_null(v: f64) -> serde_json::Value {
    if v.is_finite() {
        serde_json::json!(v)
    } else {
        serde_json::Value::Null
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gpu(index: usize, name: &str, watts: f32) -> GpuSample {
        GpuSample {
            index,
            name: name.to_string(),
            power_w: Some(watts),
            ..Default::default()
        }
    }

    fn tick(throughput: f64) -> Metrics {
        Metrics {
            throughput,
            ..Default::default()
        }
    }

    #[test]
    fn classifies_gpus_by_form_factor() {
        assert_eq!(
            GpuClass::of("NVIDIA GeForce RTX 4060 Laptop GPU", None),
            GpuClass::Laptop
        );
        assert_eq!(
            GpuClass::of("NVIDIA GeForce RTX 3080 Max-Q", None),
            GpuClass::Laptop
        );
        assert_eq!(
            GpuClass::of("NVIDIA GeForce RTX 5090", Some("DiscreteGpu")),
            GpuClass::Desktop
        );
        assert_eq!(
            GpuClass::of("AMD Radeon(TM) Graphics", Some("IntegratedGpu")),
            GpuClass::Integrated
        );
    }

    /// Snapshot: an idle RTX 4060 Laptop read 590 W on 46 of 150 ticks of run 7697c4cf.
    #[test]
    fn laptop_gpu_power_above_any_laptop_tgp_is_cleared_once() {
        let mut gate = PowerGate::default();
        let mut snap = TelemetrySnapshot {
            gpus: vec![gpu(0, "NVIDIA GeForce RTX 4060 Laptop GPU", 590.006)],
            ..Default::default()
        };
        let breaches = gate.screen(&mut snap);
        assert_eq!(snap.gpus[0].power_w, None);
        assert_eq!(breaches.len(), 1);
        assert_eq!(breaches[0].ceiling.watts, LAPTOP_GPU_MAX_W);
        assert!(
            breaches[0].detail().contains("590 W"),
            "{}",
            breaches[0].detail()
        );

        let mut again = TelemetrySnapshot {
            gpus: vec![gpu(0, "NVIDIA GeForce RTX 4060 Laptop GPU", 588.0)],
            ..Default::default()
        };
        assert!(gate.screen(&mut again).is_empty());
        assert_eq!(again.gpus[0].power_w, None);
    }

    /// Snapshot: the lab's two RTX 5090 desktops peaked at 518 W.
    #[test]
    fn real_high_desktop_power_is_kept() {
        let mut gate = PowerGate::default();
        let mut snap = TelemetrySnapshot {
            gpus: vec![
                gpu(0, "NVIDIA GeForce RTX 5090", 518.0),
                gpu(1, "NVIDIA GeForce RTX 4060 Laptop GPU", 140.0),
            ],
            ..Default::default()
        };
        assert!(gate.screen(&mut snap).is_empty());
        assert_eq!(snap.gpus[0].power_w, Some(518.0));
        assert_eq!(snap.gpus[1].power_w, Some(140.0));
    }

    fn limited(name: &str, watts: f32, limit_w: f32) -> GpuSample {
        GpuSample {
            power_limit_w: Some(limit_w),
            ..gpu(0, name, watts)
        }
    }

    #[test]
    fn desktop_power_above_twice_the_enforced_limit_is_cleared() {
        let mut gate = PowerGate::default();
        let mut snap = TelemetrySnapshot {
            gpus: vec![limited("NVIDIA GeForce RTX 4060", 590.006, 115.0)],
            ..Default::default()
        };
        let breaches = gate.screen(&mut snap);
        assert_eq!(snap.gpus[0].power_w, None);
        assert_eq!(breaches.len(), 1);
        assert_eq!(breaches[0].ceiling.watts, 230.0);
        let detail = breaches[0].detail();
        assert!(
            detail.contains("above its 230 W ceiling (2x its 115 W enforced power limit)"),
            "{detail}"
        );
        assert_eq!(
            breaches[0].data()["basis"],
            "2x its 115 W enforced power limit"
        );
    }

    /// Snapshot: real desktop peaks reach about 1.01x the card's limit (4080 SUPER 323 W, GTX 1060 121 W).
    #[test]
    fn desktop_power_near_the_enforced_limit_is_kept() {
        let cases = [
            ("NVIDIA GeForce RTX 4080 SUPER", 323.0, 320.0),
            ("NVIDIA GeForce GTX 1060 6GB", 121.0, 120.0),
            ("NVIDIA GeForce RTX 5090", 518.0, 575.0),
            ("NVIDIA GeForce RTX 4060", 230.0, 115.0),
        ];
        let mut gate = PowerGate::default();
        for (name, watts, limit_w) in cases {
            let mut snap = TelemetrySnapshot {
                gpus: vec![limited(name, watts, limit_w)],
                ..Default::default()
            };
            assert!(gate.screen(&mut snap).is_empty(), "{name}");
            assert_eq!(snap.gpus[0].power_w, Some(watts), "{name}");
        }
    }

    #[test]
    fn laptops_and_unusable_limits_keep_the_form_factor_ceiling() {
        let laptop = limited("NVIDIA GeForce RTX 4060 Laptop GPU", 140.0, 35.0);
        assert_eq!(gpu_power_ceiling(&laptop).watts, LAPTOP_GPU_MAX_W);
        for limit_w in [0.0, -5.0, f32::NAN, 600.0] {
            let desktop = limited("NVIDIA GeForce RTX 5090", 900.0, limit_w);
            let ceiling = gpu_power_ceiling(&desktop);
            assert_eq!(ceiling.watts, DESKTOP_GPU_MAX_W, "{limit_w}");
            assert_eq!(ceiling.basis, "desktop GPU");
        }
        assert_eq!(
            gpu_power_ceiling(&gpu(0, "NVIDIA GeForce RTX 5090", 900.0)).watts,
            DESKTOP_GPU_MAX_W
        );
    }

    #[test]
    fn non_finite_and_negative_power_are_cleared() {
        let mut gate = PowerGate::default();
        let mut snap = TelemetrySnapshot {
            gpus: vec![
                gpu(0, "a", f32::NAN),
                gpu(1, "b", -3.0),
                gpu(2, "c", 1_500.0),
            ],
            ..Default::default()
        };
        assert_eq!(gate.screen(&mut snap).len(), 3);
        assert!(snap.gpus.iter().all(|g| g.power_w.is_none()));
    }

    #[test]
    fn every_flops_stressor_has_a_ceiling_and_no_other_does() {
        for &s in Stressor::all() {
            let flops = matches!(s.throughput_unit(), "GFLOPS" | "Mflop/s");
            assert_eq!(
                throughput_ceiling(s, 16, GpuClass::Desktop).is_some(),
                flops,
                "{s:?} ({})",
                s.throughput_unit()
            );
        }
    }

    /// Snapshot: psu 122,396 GFLOPS on a 7800X3D with no discrete GPU; GT 710 gpu_matmul 241,530.
    #[test]
    fn snapshot_impossible_flops_are_cleared() {
        let mut psu_on_igpu = ThroughputGate::new("psu_combined", Stressor::Psu, 16);
        let mut m = tick(122_396.0);
        let breach = psu_on_igpu
            .screen_with(&mut m, GpuClass::Integrated)
            .expect("breach");
        assert_eq!(m.throughput, 0.0);
        assert_eq!(psu_on_igpu.recorded(m.throughput), None);
        assert_eq!(
            breach.ceiling,
            16.0 * CPU_MAX_GFLOPS_PER_LOGICAL_CPU + INTEGRATED_GPU_MAX_GFLOPS
        );

        let mut matmul = ThroughputGate::new("gpu_matmul", Stressor::GpuMatmul, 8);
        let mut m = tick(241_530.0);
        assert!(matmul.screen_with(&mut m, GpuClass::Desktop).is_some());
        assert_eq!(m.throughput, 0.0);
    }

    /// Snapshot maxima per stressor, e.g. matrix 603,710 Mflop/s on a 24-thread 275HX.
    #[test]
    fn snapshot_real_maxima_are_kept() {
        let cases = [
            (Stressor::Matrix, 24, GpuClass::Laptop, 603_710.0),
            (Stressor::GpuMatmul, 16, GpuClass::Desktop, 3_219.0),
            (Stressor::Psu, 16, GpuClass::Desktop, 3_333.0),
            (Stressor::Combined, 16, GpuClass::Desktop, 5_406.7),
            (Stressor::Linpack, 24, GpuClass::Desktop, 53.1),
            (Stressor::Fp, 32, GpuClass::Desktop, 43_506.0),
        ];
        for (stressor, cpus, gpu, value) in cases {
            let mut gate = ThroughputGate::new("stage", stressor, cpus);
            let mut m = tick(value);
            assert!(
                gate.screen_with(&mut m, gpu).is_none(),
                "{stressor:?} {value}"
            );
            assert_eq!(m.throughput, value);
            assert_eq!(gate.recorded(m.throughput), Some(value));
        }
    }

    #[test]
    fn a_stage_reports_its_first_breach_only() {
        let mut gate = ThroughputGate::new("gpu", Stressor::Gpu, 8);
        assert!(
            gate.screen_with(&mut tick(200_000.0), GpuClass::Desktop)
                .is_some()
        );
        assert!(
            gate.screen_with(&mut tick(300_000.0), GpuClass::Desktop)
                .is_none()
        );
        let mut ok = tick(500.0);
        assert!(gate.screen_with(&mut ok, GpuClass::Desktop).is_none());
        assert_eq!(gate.recorded(ok.throughput), Some(500.0));
    }

    #[test]
    fn non_flops_stressors_only_lose_non_finite_samples() {
        let mut gate = ThroughputGate::new("memtest", Stressor::MemTest, 8);
        let mut high = tick(1e12);
        assert!(gate.screen_with(&mut high, GpuClass::Desktop).is_none());
        assert_eq!(high.throughput, 1e12);
        let mut nan = tick(f64::NAN);
        let breach = gate
            .screen_with(&mut nan, GpuClass::Desktop)
            .expect("breach");
        assert_eq!(nan.throughput, 0.0);
        assert_eq!(breach.data()["value"], serde_json::Value::Null);
    }

    #[test]
    fn breach_detail_names_the_stage_value_and_basis() {
        let mut gate = ThroughputGate::new("psu_combined", Stressor::Psu, 16);
        let breach = gate
            .screen_with(&mut tick(140_000.0), GpuClass::Desktop)
            .expect("breach");
        let detail = breach.detail();
        assert!(detail.contains("psu_combined"), "{detail}");
        assert!(detail.contains("140000 GFLOPS"), "{detail}");
        assert!(
            detail.contains("16 logical CPUs x 416 GFLOPS + desktop GPU 130000 GFLOPS"),
            "{detail}"
        );
        assert_eq!(breach.data()["field"], "throughput");
    }
}
