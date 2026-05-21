//! GPU sampler.
//!
//! Best-effort, vendor-agnostic. We surface:
//!   * `name`  — derived from the matching sysinfo `Component` label, or "GPU N".
//!   * `temp_c` — the matching `Component` temperature if any.
//!
//! Live `usage_pct` and VRAM accounting are vendor-specific (NVML for NVIDIA,
//! ADL/ROCm-SMI for AMD, DXGI on Windows). Those fields are `None` here on
//! purpose — wire them in once we pick a vendor dependency.
//!
//! The sampler doesn't enumerate the bus; it harvests anything the
//! `sysinfo` Components list calls `"GPU"`, `"gfx"`, `"radeon"`, `"nvidia"`,
//! `"amdgpu"`, or `"edge"` (NVIDIA temp on Linux). Empty result = no GPU
//! sensors found, which is fine.

use serde::{Deserialize, Serialize};
use sysinfo::Components;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GpuSample {
    pub index: usize,
    /// Best-effort vendor string ("NVIDIA", "AMD", "Intel", or "Unknown").
    pub vendor: String,
    /// Component label as reported by sysinfo (often the model name on Linux).
    pub name: String,
    pub temp_c: Option<f32>,
    /// 0–100, when a vendor probe is wired up. `None` for now.
    pub usage_pct: Option<f32>,
    /// VRAM used in MB, when available.
    pub memory_used_mb: Option<u64>,
    /// VRAM total in MB, when available.
    pub memory_total_mb: Option<u64>,
}

pub fn sample_gpus(components: &Components) -> Vec<GpuSample> {
    let mut out: Vec<GpuSample> = Vec::new();
    for c in components.iter() {
        let label = c.label();
        if !is_gpu_label(label) {
            continue;
        }
        let vendor = classify_vendor(label);
        out.push(GpuSample {
            index: out.len(),
            vendor,
            name: label.to_string(),
            temp_c: c.temperature(),
            usage_pct: None,
            memory_used_mb: None,
            memory_total_mb: None,
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
