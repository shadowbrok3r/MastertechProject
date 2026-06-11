//! Shared wgpu device acquisition and tick emission for the GPU stressors.

#![cfg(feature = "gpu")]

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use wgpu::{
    Adapter, Backends, Device, DeviceDescriptor, Features, Instance,
    InstanceDescriptor, Limits, PowerPreference, Queue, RequestAdapterOptions,
};

use crate::Metrics;

pub(super) const TICK: Duration = Duration::from_millis(500);

/// D3D12 caps each dispatch dimension at 65535 workgroups.
pub(super) const MAX_DISPATCH_GROUPS: u32 = 65535;
pub(super) const WG_SIZE: u32 = 64;

/// Uncaptured device errors logged in full before suppression kicks in.
const LOGGED_DEVICE_ERRORS: u64 = 5;

/// Shared device-failure state fed by the uncaptured-error handler and the
/// device-lost callback. Stressor loops poll [`GpuHealth::failure`] each
/// iteration and abort the stage instead of spinning on a dead device.
#[derive(Clone, Default)]
pub(super) struct GpuHealth {
    error_count: Arc<AtomicU64>,
    lost: Arc<AtomicBool>,
    first_error: Arc<Mutex<Option<String>>>,
}

impl GpuHealth {
    fn note_error(&self, msg: String) {
        let n = self.error_count.fetch_add(1, Ordering::Relaxed);
        if n < LOGGED_DEVICE_ERRORS {
            log::error!("[stress-kit/gpu] uncaptured device error: {msg}");
        } else if n % 1000 == 0 {
            log::error!("[stress-kit/gpu] device errors continue (total {n}); suppressing");
        }
        if let Ok(mut g) = self.first_error.lock() {
            g.get_or_insert(msg);
        }
    }

    fn note_lost(&self, reason: String) {
        self.lost.store(true, Ordering::SeqCst);
        log::error!("[stress-kit/gpu] device lost: {reason}");
        if let Ok(mut g) = self.first_error.lock() {
            *g = Some(reason);
        }
    }

    pub(super) fn error_count(&self) -> u64 {
        self.error_count.load(Ordering::Relaxed)
    }

    /// `Some(reason)` once the device has been lost or has reported any
    /// validation error — both invalidate the stage's results.
    pub(super) fn failure(&self) -> Option<String> {
        if self.lost.load(Ordering::Relaxed) || self.error_count() > 0 {
            let detail = self
                .first_error
                .lock()
                .ok()
                .and_then(|g| g.clone())
                .unwrap_or_else(|| "unknown device error".to_string());
            Some(format!(
                "GPU device failed ({} error(s)): {}",
                self.error_count().max(1),
                detail
            ))
        } else {
            None
        }
    }
}

pub(super) struct GpuContext {
    #[allow(dead_code)]
    pub adapter: Adapter,
    pub device: Device,
    pub queue: Queue,
    pub vendor_label: String,
    pub backend_label: String,
    pub health: GpuHealth,
}

fn adapter_score(info: &wgpu::AdapterInfo, prefer_discrete: bool) -> i32 {
    let name = info.name.to_lowercase();
    if name.contains("microsoft basic")
        || name.contains("llvmpipe")
        || name.contains("swiftshader")
    {
        return -1000;
    }

    if prefer_discrete {
        if info.vendor == 0x10DE {
            return 1000;
        }
        if info.vendor == 0x1002 {
            if name.contains("rx ") || name.contains("radeon rx") || name.contains("vega") {
                return 900;
            }
            if name.contains("graphics") && !name.contains("rx") {
                return 100;
            }
            return 500;
        }
        if info.vendor == 0x8086 {
            return 200;
        }
        400
    } else if info.vendor == 0x8086 {
        1000
    } else if info.vendor == 0x1002 && name.contains("graphics") {
        900
    } else if info.vendor == 0x10DE {
        100
    } else {
        400
    }
}

fn pick_adapter(adapters: Vec<Adapter>, prefer_discrete: bool) -> Option<Adapter> {
    adapters
        .into_iter()
        .map(|a| (adapter_score(&a.get_info(), prefer_discrete), a))
        .max_by_key(|(score, _)| *score)
        .filter(|(score, _)| *score > 0)
        .map(|(_, a)| a)
}

impl GpuContext {
    pub(super) fn acquire(prefer_discrete: bool) -> Result<Self, String> {
        let instance = Instance::new(&InstanceDescriptor {
            backends: Backends::PRIMARY,
            ..Default::default()
        });

        let adapters = instance.enumerate_adapters(Backends::PRIMARY);
        if adapters.is_empty() {
            return Err("no GPU adapters found".into());
        }

        for adapter in &adapters {
            let info = adapter.get_info();
            log::info!(
                "[stress-kit/gpu] adapter: {} ({:?}, vendor 0x{:04x}, device 0x{:04x})",
                info.name,
                info.backend,
                info.vendor,
                info.device
            );
        }

        let adapter = pick_adapter(adapters, prefer_discrete).unwrap_or_else(|| {
            let power = if prefer_discrete {
                PowerPreference::HighPerformance
            } else {
                PowerPreference::LowPower
            };
            log::warn!(
                "[stress-kit/gpu] no scored adapter; falling back to wgpu power preference {:?}",
                power
            );
            pollster::block_on(instance.request_adapter(&RequestAdapterOptions {
                power_preference: power,
                compatible_surface: None,
                force_fallback_adapter: false,
            }))
            .expect("enumerate_adapters was non-empty")
        });

        let info = adapter.get_info();
        log::info!(
            "[stress-kit/gpu] selected: {} ({:?}, vendor 0x{:04x}, device 0x{:04x})",
            info.name,
            info.backend,
            info.vendor,
            info.device
        );
        let vendor_label = format!(
            "{} / {} (vendor 0x{:04x}, device 0x{:04x})",
            info.name, info.driver, info.vendor, info.device
        );
        let backend_label = format!("{:?}", info.backend);

        // Default limits cap storage bindings at 128 MiB; lift buffer limits
        // to what the adapter supports so VRAM tests can cover real footprints.
        let adapter_limits = adapter.limits();
        let mut limits = Limits::default();
        limits.max_buffer_size = adapter_limits.max_buffer_size;
        limits.max_storage_buffer_binding_size = adapter_limits.max_storage_buffer_binding_size;

        let (device, queue) = pollster::block_on(adapter.request_device(&DeviceDescriptor {
            label: Some("stress-kit GPU stressor"),
            required_features: Features::empty(),
            required_limits: limits,
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        }))
        .map_err(|e| format!("GPU device request failed: {e}"))?;

        let health = GpuHealth::default();
        let health_err = health.clone();
        device.on_uncaptured_error(Box::new(move |err| {
            health_err.note_error(err.to_string());
        }));
        let health_lost = health.clone();
        device.set_device_lost_callback(move |reason, message| {
            health_lost.note_lost(format!("{reason:?}: {message}"));
        });

        Ok(Self {
            adapter,
            device,
            queue,
            vendor_label,
            backend_label,
            health,
        })
    }
}

pub(super) fn emit_tick(
    tx: &mpsc::Sender<Metrics>,
    started_at: Instant,
    throughput: f64,
    last_error: Option<String>,
    errors: u64,
) {
    let _ = tx.send(Metrics {
        elapsed_secs: started_at.elapsed().as_secs_f64(),
        throughput,
        last_error,
        fatal: false,
        errors,
    });
}

/// Emit a final fatal tick — same shape as `emit_tick` but signals the
/// scenario/controller to abort the current stage. Callers that hit a
/// non-recoverable device error should send this then `return`.
pub(super) fn emit_fatal_tick(
    tx: &mpsc::Sender<Metrics>,
    started_at: Instant,
    reason: String,
    errors: u64,
) {
    let _ = tx.send(Metrics {
        elapsed_secs: started_at.elapsed().as_secs_f64(),
        throughput: 0.0,
        last_error: Some(reason),
        fatal: true,
        errors,
    });
}

pub(super) fn run_unsupported(
    reason: String,
    cancel: &Arc<AtomicBool>,
    tx: &mpsc::Sender<Metrics>,
    started_at: Instant,
) {
    log::warn!("[stress-kit/gpu] stressor inactive: {reason}");
    emit_tick(tx, started_at, 0.0, Some(reason), 0);
    while !cancel.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(100));
    }
}
