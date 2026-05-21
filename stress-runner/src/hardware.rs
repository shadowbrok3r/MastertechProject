//! Hardware-component upsert middleware.
//!
//! Runs at the top of [`crate::controller::worker`] so every stress run has a
//! linked [`database::schema::HardwareComponent`] row before the
//! `stress_test_run` row is built. Without this:
//!   * `stress_test_run.target_component` stays `None`.
//!   * `hardware_test_baseline` (the materialized view that joins runs to
//!     components) never sees this machine's hardware.
//!   * The `compare_to_baseline` MCP tool has nothing to compare against.
//!
//! Best-effort by design — any upsert failure logs at warn level and the run
//! still proceeds with whatever component IDs were successfully resolved.
//! Telemetry sampling + metric/event writes are independent of this path, so
//! a transient SurrealDB hiccup here doesn't lose run data.

use database::schema::{HardwareComponent, HardwareKind, RecordId};
use stress_kit::telemetry::TelemetrySnapshot;

use crate::runtime;

/// Discover hardware from the current `TelemetrySnapshot` and upsert one
/// `hardware_component` row per unique CPU + GPU. Returns:
///
///   * `cpu_id` — the CPU's canonical record (for `RunSpec.target_component`).
///   * `touched_ids` — every component upserted in this call, in the order
///     `[cpu, gpu0, gpu1, …]` (for `RunSpec.touched_components`).
///
/// Both halves are empty on the no-telemetry-yet path; the caller treats
/// them as "skip the link" rather than as failure.
pub fn ensure_components_from_snapshot(
    snapshot: &TelemetrySnapshot,
) -> (Option<RecordId>, Vec<RecordId>) {
    let mut all = Vec::new();
    let mut cpu_id = None;

    // CPU. sysinfo reports the same brand string on every logical core for a
    // single-socket box, so the first non-empty brand is canonical. Multi-
    // socket workstations need a smarter walk (group by socket), but the
    // overwhelming majority of QC machines are single-socket consumer parts.
    if let Some(brand) = snapshot.cores.first().map(|c| c.brand.clone()) {
        let model = brand.trim().to_string();
        if !model.is_empty() {
            let vendor = classify_cpu_vendor(&model);
            match upsert_blocking(HardwareKind::Cpu, &vendor, &model) {
                Ok(id) => {
                    cpu_id = Some(id.clone());
                    all.push(id);
                }
                Err(e) => log::warn!(
                    "[hw_middleware] cpu upsert ({vendor}, {model}) failed: {e}"
                ),
            }
        }
    }

    // GPUs. Each `GpuSample` already carries a classified vendor + name
    // (sysinfo Component label, e.g. `"amdgpu edge"` or `"NVIDIA GeForce …"`).
    // Skip rows where either is empty — those are usually CPU package sensors
    // the GPU classifier mis-labelled.
    for gpu in &snapshot.gpus {
        if gpu.vendor.is_empty() || gpu.name.is_empty() {
            continue;
        }
        match upsert_blocking(HardwareKind::Gpu, &gpu.vendor, &gpu.name) {
            Ok(id) => all.push(id),
            Err(e) => log::warn!(
                "[hw_middleware] gpu upsert ({}, {}) failed: {e}",
                gpu.vendor,
                gpu.name
            ),
        }
    }

    log::debug!(
        "[hw_middleware] upserted {} component(s); cpu={:?}",
        all.len(),
        cpu_id
    );
    (cpu_id, all)
}

/// Block-on adapter so the sync `RunController::worker` thread can call
/// [`HardwareComponent::upsert_seen`] without standing up its own runtime.
fn upsert_blocking(
    kind: HardwareKind,
    vendor: &str,
    model: &str,
) -> anyhow::Result<RecordId> {
    let component = HardwareComponent::new(kind, vendor, model);
    let comp_for_async = component.clone();
    runtime::block_on(async move { HardwareComponent::upsert_seen(&comp_for_async).await })
}

/// Best-effort vendor classifier from a CPU brand string. Returns the
/// canonical vendor name we want in `hardware_component.vendor`; the full
/// brand string stays in `model`.
fn classify_cpu_vendor(brand: &str) -> String {
    let b = brand.to_lowercase();
    if b.contains("amd") || b.contains("ryzen") || b.contains("epyc") || b.contains("threadripper") {
        "AMD".into()
    } else if b.contains("intel") || b.contains("xeon") || b.contains("core(tm)") || b.contains("pentium") {
        "Intel".into()
    } else if b.contains("apple") {
        "Apple".into()
    } else if b.contains("arm") {
        "ARM".into()
    } else {
        "Unknown".into()
    }
}
