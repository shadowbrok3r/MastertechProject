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

use std::thread;
use std::time::{Duration, Instant};

use database::schema::{HardwareComponent, HardwareKind, RecordId};
use stress_kit::telemetry::{TelemetryAgent, TelemetrySnapshot};

use crate::runtime;

/// Notes from the hardware middleware pass: each entry is a human-readable
/// line the worker surfaces to the operator (UI toast / MCP report). Empty
/// vec means everything succeeded.
pub type HardwareNotices = Vec<String>;

/// Poll the agent briefly, then fall back to a synchronous sysinfo capture.
/// Returns `(cpu_component_id, all_component_ids, notices)` where `notices`
/// contains any user-visible diagnostics (empty snapshot, upsert failures).
pub fn ensure_components_for_run(
    telemetry: &TelemetryAgent,
) -> (Option<RecordId>, Vec<RecordId>, HardwareNotices) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        let snap = telemetry.snapshot();
        if !snap.cores.is_empty() {
            log::info!(
                "[hw_middleware] using live telemetry snapshot: {} core(s), {} gpu(s)",
                snap.cores.len(),
                snap.gpus.len()
            );
            return ensure_components_from_snapshot(&snap);
        }
        thread::sleep(Duration::from_millis(50));
    }
    log::info!(
        "[hw_middleware] live telemetry empty after 2s; falling back to synchronous sysinfo capture"
    );
    let fallback = TelemetryAgent::capture_now();
    log::info!(
        "[hw_middleware] sync sysinfo capture: {} core(s), {} gpu(s)",
        fallback.cores.len(),
        fallback.gpus.len()
    );
    ensure_components_from_snapshot(&fallback)
}

/// Discover hardware from `snapshot` and upsert one `hardware_component` row
/// per unique CPU + GPU. Returns CPU id, all touched component ids, and any
/// diagnostic notices (each surfaced to the operator as a `RunUpdate::Warning`).
pub fn ensure_components_from_snapshot(
    snapshot: &TelemetrySnapshot,
) -> (Option<RecordId>, Vec<RecordId>, HardwareNotices) {
    let mut all = Vec::new();
    let mut cpu_id = None;
    let mut notices: HardwareNotices = Vec::new();

    // CPU. sysinfo reports the same brand string on every logical core for a
    // single-socket box, so the first non-empty brand is canonical. Multi-
    // socket workstations need a smarter walk (group by socket), but the
    // overwhelming majority of QC machines are single-socket consumer parts.
    let raw_brand = snapshot.cores.first().map(|c| c.brand.clone()).unwrap_or_default();
    let model = raw_brand.trim().to_string();
    if model.is_empty() {
        let msg = format!(
            "no CPU brand in telemetry snapshot ({} cores reported) — hardware_component.cpu skipped",
            snapshot.cores.len()
        );
        log::warn!("[hw_middleware] {msg}");
        notices.push(msg);
    } else {
        let vendor = classify_cpu_vendor(&model);
        match upsert_blocking(HardwareKind::Cpu, &vendor, &model) {
            Ok(id) => {
                log::info!("[hw_middleware] cpu upserted: {vendor} / {model} -> {id:?}");
                cpu_id = Some(id.clone());
                all.push(id);
            }
            Err(e) => {
                let msg = format!("cpu upsert ({vendor}, {model}) failed: {e}");
                log::warn!("[hw_middleware] {msg}");
                notices.push(msg);
            }
        }
    }

    // GPUs. Each `GpuSample` already carries a classified vendor + name
    // (sysinfo Component label, e.g. `"amdgpu edge"` or `"NVIDIA GeForce …"`).
    // Skip rows where either is empty — those are usually CPU package sensors
    // the GPU classifier mis-labelled.
    let mut gpu_upserts = 0;
    let mut gpu_skipped = 0;
    for gpu in &snapshot.gpus {
        if gpu.vendor.is_empty() || gpu.name.is_empty() {
            gpu_skipped += 1;
            continue;
        }
        match upsert_blocking(HardwareKind::Gpu, &gpu.vendor, &gpu.name) {
            Ok(id) => {
                log::info!(
                    "[hw_middleware] gpu upserted: {} / {} -> {id:?}",
                    gpu.vendor, gpu.name
                );
                all.push(id);
                gpu_upserts += 1;
            }
            Err(e) => {
                let msg = format!("gpu upsert ({}, {}) failed: {e}", gpu.vendor, gpu.name);
                log::warn!("[hw_middleware] {msg}");
                notices.push(msg);
            }
        }
    }

    if snapshot.gpus.is_empty() {
        let msg = "no GPUs in telemetry snapshot — hardware_component.gpu skipped (NVML disabled or sysinfo Components didn't enumerate any)".to_string();
        log::warn!("[hw_middleware] {msg}");
        notices.push(msg);
    } else if gpu_upserts == 0 {
        let msg = format!(
            "snapshot listed {} GPU sample(s) but all had empty vendor or name; nothing upserted ({gpu_skipped} skipped)",
            snapshot.gpus.len()
        );
        log::warn!("[hw_middleware] {msg}");
        notices.push(msg);
    }

    log::info!(
        "[hw_middleware] result: {} component(s) upserted; cpu={:?}, gpus={}, notices={}",
        all.len(),
        cpu_id,
        gpu_upserts,
        notices.len()
    );
    (cpu_id, all, notices)
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
