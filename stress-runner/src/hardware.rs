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

/// Components resolved for one run. `gpus` is kept separate from `all` so the
/// controller can target a GPU component on a GPU-only plan instead of
/// defaulting every run to the CPU.
#[derive(Debug, Clone, Default)]
pub struct ResolvedComponents {
    pub cpu: Option<RecordId>,
    pub gpus: Vec<RecordId>,
    /// Every component touched, CPU and GPUs together.
    pub all: Vec<RecordId>,
    pub notices: HardwareNotices,
}

/// Poll the agent briefly, then fall back to a synchronous sysinfo capture.
/// Returns `(cpu_component_id, all_component_ids, notices)` where `notices`
/// contains any user-visible diagnostics (empty snapshot, upsert failures).
pub fn ensure_components_for_run(telemetry: &TelemetryAgent) -> ResolvedComponents {
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
pub fn ensure_components_from_snapshot(snapshot: &TelemetrySnapshot) -> ResolvedComponents {
    let mut all = Vec::new();
    let mut gpu_ids: Vec<RecordId> = Vec::new();
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
        let cpu_specs = (!snapshot.cores.is_empty())
            .then(|| serde_json::json!({ "logical_cores": snapshot.cores.len() }));
        match upsert_blocking(HardwareKind::Cpu, &vendor, &model, cpu_specs) {
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
        let gpu_specs = gpu
            .memory_total_mb
            .map(|mb| serde_json::json!({ "vram_mb": mb }));
        match upsert_blocking(HardwareKind::Gpu, &gpu.vendor, &gpu.name, gpu_specs) {
            Ok(id) => {
                log::info!(
                    "[hw_middleware] gpu upserted: {} / {} -> {id:?}",
                    gpu.vendor, gpu.name
                );
                gpu_ids.push(id.clone());
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

    // Telemetry-derived GPUs come from NVML plus sysinfo Components, so an AMD
    // or Intel card yields nothing and the machine gets no GPU component even
    // though the stressors bind it happily. Fall back to the wgpu adapter list,
    // which is vendor-neutral and is what the GPU stressors actually run on.
    if gpu_upserts == 0 {
        match wgpu_gpu_identities() {
            Ok(identities) if !identities.is_empty() => {
                for (vendor, model) in identities {
                    match upsert_blocking(HardwareKind::Gpu, &vendor, &model, None) {
                        Ok(id) => {
                            log::info!(
                                "[hw_middleware] gpu upserted from wgpu adapter: {vendor} / {model} -> {id:?}"
                            );
                            gpu_ids.push(id.clone());
                            all.push(id);
                            gpu_upserts += 1;
                        }
                        Err(e) => {
                            let msg =
                                format!("gpu upsert from wgpu adapter ({vendor}, {model}) failed: {e}");
                            log::warn!("[hw_middleware] {msg}");
                            notices.push(msg);
                        }
                    }
                }
                if gpu_upserts > 0 {
                    notices.push(format!(
                        "GPU telemetry reported nothing (NVML is NVIDIA-only); recorded \
                         {gpu_upserts} hardware_component.gpu row(s) from the wgpu adapter list \
                         instead. GPU thermal/power readings are still unavailable."
                    ));
                }
            }
            Ok(_) => {
                let msg = "no GPUs in telemetry snapshot and wgpu enumerated no hardware adapter \
                           — hardware_component.gpu skipped"
                    .to_string();
                log::warn!("[hw_middleware] {msg}");
                notices.push(msg);
            }
            Err(msg) => {
                log::warn!("[hw_middleware] {msg}");
                notices.push(msg);
            }
        }
    }

    if gpu_upserts == 0 && !snapshot.gpus.is_empty() {
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
    ResolvedComponents { cpu: cpu_id, gpus: gpu_ids, all, notices }
}

/// Block-on adapter so the sync `RunController::worker` thread can call
/// [`HardwareComponent::upsert_seen`] without standing up its own runtime.
/// `specs` is merged NONE-safely by the upsert SQL, so an identity-only pass
/// never wipes specs an earlier richer pass stored.
fn upsert_blocking(
    kind: HardwareKind,
    vendor: &str,
    model: &str,
    specs: Option<serde_json::Value>,
) -> anyhow::Result<RecordId> {
    let mut component = HardwareComponent::new(kind, vendor, model);
    component.specs = specs;
    let comp_for_async = component.clone();
    runtime::block_on(async move { HardwareComponent::upsert_seen(&comp_for_async).await })
}

/// `(vendor, model)` for every real hardware GPU wgpu can see, software
/// rasterizers excluded. Vendor-neutral, unlike the NVML telemetry path.
///
/// `Err` carries an operator-facing reason: the `gpu` feature being off is a
/// build choice, not a fault, but either way no GPU component can be derived.
fn wgpu_gpu_identities() -> Result<Vec<(String, String)>, String> {
    let report = stress_kit::gpu_stack::check_gpu_stack();
    if !report.wgpu_checked {
        return Err("no GPUs in telemetry snapshot and wgpu was not checked (stress-kit built \
                    without the `gpu` feature) — hardware_component.gpu skipped"
            .to_string());
    }
    if !report.has_hardware_gpu() {
        return Ok(Vec::new());
    }
    // wgpu reports one adapter per backend, so a single card shows up twice
    // ("… (Vulkan, vendor 0x1002)" and "… (Dx12, vendor 0x1002)"). Strip the
    // backend suffix and dedupe, otherwise one GPU becomes two components.
    let mut seen = std::collections::HashSet::new();
    Ok(report
        .wgpu_adapters
        .iter()
        .filter(|a| is_hardware_adapter(a))
        .map(|a| adapter_model_name(a))
        .filter(|model| seen.insert(model.to_ascii_lowercase()))
        .map(|model| (classify_gpu_vendor(&model), model))
        .collect())
}

/// Adapter label with wgpu's trailing `(<Backend>, vendor 0x….)` annotation
/// removed: `AMD Radeon RX 7900 XTX (Vulkan, vendor 0x1002)` → `AMD Radeon RX
/// 7900 XTX`. Labels without that suffix pass through untouched.
fn adapter_model_name(label: &str) -> String {
    let trimmed = label.trim();
    match trimmed.rfind('(') {
        Some(i) if trimmed.ends_with(')') && trimmed[i..].contains("vendor") => {
            trimmed[..i].trim().to_string()
        }
        _ => trimmed.to_string(),
    }
}

/// Excludes software rasterizers and the Microsoft Basic fallback, which are
/// not hardware worth recording as a component.
fn is_hardware_adapter(label: &str) -> bool {
    let n = label.to_lowercase();
    !(n.contains("microsoft basic")
        || n.contains("llvmpipe")
        || n.contains("softwarerasterizer")
        || n.contains("swiftshader")
        || n.trim().is_empty())
}

/// Best-effort vendor classifier from a GPU adapter label.
fn classify_gpu_vendor(label: &str) -> String {
    let n = label.to_lowercase();
    if n.contains("nvidia") || n.contains("geforce") || n.contains("quadro") || n.contains("rtx") {
        "NVIDIA".into()
    } else if n.contains("amd") || n.contains("radeon") || n.contains("ati ") {
        "AMD".into()
    } else if n.contains("intel") || n.contains("arc ") || n.contains("iris") || n.contains("uhd") {
        "Intel".into()
    } else if n.contains("apple") {
        "Apple".into()
    } else {
        "Unknown".into()
    }
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
