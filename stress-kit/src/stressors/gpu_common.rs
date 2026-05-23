//! Shared wgpu plumbing for the GPU stressors.
//!
//! All four GPU stressors (`gpu`, `gpu_matmul`, `gpu_vram`, `gpu_pcie`) need
//! the same boilerplate: pick a high-performance adapter, create a device and
//! queue, derive a label for telemetry. Centralized here so each stressor
//! module only contains the shader + run loop unique to that test.
//!
//! ## Vendor preference
//!
//! `request_adapter` is given `HighPerformance` power preference — on
//! laptops with hybrid graphics (5600G iGPU + RTX 2070 SUPER dGPU, like the
//! machine we're chasing) this picks the discrete card. That's intentional:
//! the customer's complaint is about the dGPU, so we should hammer the dGPU.
//!
//! ## Cancellation
//!
//! `wgpu::Queue::submit` returns immediately; the actual work happens
//! asynchronously on the device. We wait for completion via `device.poll`
//! with `wgpu::PollType::Wait` between dispatches, then check the cancel
//! atomic. Worst-case latency on cancel = one dispatch's wall time.
//!
//! ## Why a single host thread per GPU stressor
//!
//! Stress-kit's CPU stressors fan out one worker per logical CPU because the
//! parallelism unit is "core". For GPU work the parallelism unit is "compute
//! unit", and that's controlled by the workgroup dispatch size, not by host
//! threads. Spawning N host threads contending for one queue would just
//! serialize at the driver. The dispatcher in `stressors/mod.rs` already
//! treats GPU stressors specially via `Stressor::is_gpu()` — they get one
//! supervisor thread, period.

#![cfg(feature = "gpu")]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use wgpu::{
    Adapter, Backends, Device, DeviceDescriptor, Features, Instance,
    InstanceDescriptor, Limits, PowerPreference, Queue, RequestAdapterOptions,
};

use crate::Metrics;

/// Reporting cadence — matches the CPU stressors' 500 ms tick so the UI
/// doesn't have to special-case GPU runs.
pub(super) const TICK: Duration = Duration::from_millis(500);

/// One acquired GPU device + queue + adapter info, ready for shader dispatch.
pub(super) struct GpuContext {
    pub adapter: Adapter,
    pub device: Device,
    pub queue: Queue,
    pub vendor_label: String,
    pub backend_label: String,
}

impl GpuContext {
    /// Block-on (we're on a worker thread, not the async runtime). Returns
    /// `Err` if no compatible adapter is available, with a message we can
    /// surface through `Metrics::last_error`.
    pub(super) fn acquire(prefer_discrete: bool) -> Result<Self, String> {
        let instance = Instance::new(&InstanceDescriptor {
            backends: Backends::PRIMARY,
            ..Default::default()
        });

        let power = if prefer_discrete {
            PowerPreference::HighPerformance
        } else {
            PowerPreference::LowPower
        };

        let adapter = pollster::block_on(instance.request_adapter(&RequestAdapterOptions {
            power_preference: power,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
        .map_err(|e| format!("no compatible GPU adapter: {e}"))?;

        let info = adapter.get_info();
        let vendor_label = format!(
            "{} / {} (vendor 0x{:04x}, device 0x{:04x})",
            info.name, info.driver, info.vendor, info.device
        );
        let backend_label = format!("{:?}", info.backend);

        // No exotic features — keep the requirement set tight so the test runs
        // on every consumer card we'd ever see in the shop. f32 atomics would
        // simplify VRAM error counting but they're not core wgpu yet; we use a
        // u32 atomic counter instead.
        let (device, queue) = pollster::block_on(adapter.request_device(&DeviceDescriptor {
            label: Some("stress-kit GPU stressor"),
            required_features: Features::empty(),
            required_limits: Limits::default(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        }))
        .map_err(|e| format!("GPU device request failed: {e}"))?;

        // Install an error scope-ish hook: wgpu surfaces validation/internal
        // errors through `on_uncaptured_error`. Anything that lands here gets
        // logged so a TDR-style failure during a stressor isn't silent.
        device.on_uncaptured_error(Box::new(|err| {
            log::error!("[stress-kit/gpu] uncaptured device error: {err}");
        }));

        Ok(Self {
            adapter,
            device,
            queue,
            vendor_label,
            backend_label,
        })
    }
}

/// Emit a `Metrics` tick. Mirrors the CPU stressors' 500 ms reporting cadence.
pub(super) fn emit_tick(
    tx: &mpsc::Sender<Metrics>,
    started_at: Instant,
    throughput: f64,
    last_error: Option<String>,
) {
    let _ = tx.send(Metrics {
        elapsed_secs: started_at.elapsed().as_secs_f64(),
        throughput,
        last_error,
    });
}

/// Common "we couldn't even acquire a GPU" path: emit one error metric and
/// idle until the cancel flag flips, so the supervisor still sees a finite
/// run rather than a thread that exits early.
pub(super) fn run_unsupported(
    reason: String,
    cancel: &Arc<AtomicBool>,
    tx: &mpsc::Sender<Metrics>,
    started_at: Instant,
) {
    log::warn!("[stress-kit/gpu] stressor inactive: {reason}");
    emit_tick(tx, started_at, 0.0, Some(reason));
    // Park until cancelled so the run-controller's `duration_secs` is honored.
    while !cancel.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(100));
    }
}
