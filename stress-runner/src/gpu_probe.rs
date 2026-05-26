//! Curated 4-stage GPU probe preset shared by qc-app MCP, Mastertech scripts,
//! and stress_test_run persistence.

use database::schema::{RecordId, TargetKind, TestTool};

use crate::{RunPlan, RunSpec, RunStage};

/// Stable preset identifier persisted into `stress_test_run.preset_label`.
pub const GPU_PROBE_PRESET: &str = "qc-mcp:gpu-probe-v1";

/// Build the 4-stage GPU probe, scaled by `mult` (clamped at the caller).
///
/// `mult = 1.0` → ~125 s planned (~2 min wall).
pub fn gpu_probe_stages(mult: f32) -> Vec<RunStage> {
    fn dur(base: u64, mult: f32) -> u64 {
        ((base as f32) * mult).round().max(1.0) as u64
    }
    let mk = |label: &str, stressor: stress_kit::Stressor, base_secs: u64, mem_mb: u64| RunStage {
        label: label.to_string(),
        stressor,
        threads: 0,
        duration_secs: dur(base_secs, mult),
        memory_cap_mb: mem_mb,
        disk_file_mb: 16,
    };
    vec![
        mk("gpu_compute", stress_kit::Stressor::Gpu, 30, 256),
        mk("gpu_matmul", stress_kit::Stressor::GpuMatmul, 30, 256),
        mk("gpu_vram", stress_kit::Stressor::GpuVram, 45, 1024),
        mk("gpu_pcie", stress_kit::Stressor::GpuPcie, 20, 64),
    ]
}

/// RunSpec for a GPU probe scenario against `computer`.
pub fn gpu_probe_spec(computer: RecordId, mult: f32) -> RunSpec {
    let stages = gpu_probe_stages(mult);
    RunSpec {
        computer,
        tool: TestTool::StressKitScenario {
            name: Some(GPU_PROBE_PRESET.into()),
        },
        target_kind: TargetKind::Gpu,
        target_component: None,
        touched_components: Vec::new(),
        service_order: None,
        session_ref: None,
        task_ref: None,
        tech: None,
        hostname: None,
        machine_id: None,
        bios_settings: Default::default(),
        driver_versions: Default::default(),
        notes: None,
        preset_label: Some(GPU_PROBE_PRESET.into()),
        tags: vec!["preset:gpu-probe".into()],
        plan: RunPlan::Scenario {
            stages,
            total_wall_secs: None,
            repeat_until_total: false,
        },
    }
}
