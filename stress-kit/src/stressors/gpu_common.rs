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

/// Wall-clock bound on a buffer-map callback once the queue has been polled.
pub(super) const MAP_WAIT_TIMEOUT: Duration = Duration::from_secs(5);

/// Bound on a single device wait; exceeding it yields `PollError::Timeout`.
pub(super) const DEVICE_WAIT_TIMEOUT: Duration = Duration::from_secs(60);

/// Bounded wait on the most recent submission.
pub(super) fn wait_latest() -> wgpu::PollType {
    wgpu::PollType::Wait {
        submission_index: None,
        timeout: Some(DEVICE_WAIT_TIMEOUT),
    }
}

/// Bounded wait on a specific submission.
pub(super) fn wait_for(index: wgpu::SubmissionIndex) -> wgpu::PollType {
    wgpu::PollType::Wait {
        submission_index: Some(index),
        timeout: Some(DEVICE_WAIT_TIMEOUT),
    }
}

/// Uncaptured device errors logged in full before suppression kicks in.
const LOGGED_DEVICE_ERRORS: u64 = 5;

/// Longest an error detail may be before it is truncated.
const MAX_DETAIL_CHARS: usize = 200;

/// How long [`run_unsupported`] re-emits its fatal before returning.
const UNSUPPORTED_LINGER: Duration = Duration::from_secs(2);

/// Phrases stress-runner reads as hardware evidence of a lost GPU.
const DEVICE_LOSS_PHRASES: [&str; 7] = [
    "device is lost",
    "device lost",
    "device removed",
    "dxgi_error",
    "gpu device failed",
    "gpu unavailable",
    "gpu leg stopped",
];

fn mentions_device_loss(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    DEVICE_LOSS_PHRASES.iter().any(|p| m.contains(p))
}

/// Whitespace-collapsed, length-capped copy of `s`.
fn one_line(s: &str) -> String {
    let flat = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= MAX_DETAIL_CHARS {
        return flat;
    }
    flat.chars().take(MAX_DETAIL_CHARS).collect::<String>() + "..."
}

fn first_or(slot: &Mutex<Option<String>>, fallback: &str) -> String {
    slot.lock()
        .ok()
        .and_then(|g| g.clone())
        .unwrap_or_else(|| fallback.to_string())
}

/// `Tooling` = wgpu rejected commands this crate issued, so the load never ran.
/// `Device` = the driver reported a failure of its own.
#[derive(Clone, Copy)]
enum ErrorClass {
    Tooling,
    Device,
}

/// Shared device-failure state fed by the uncaptured-error handler and the
/// device-lost callback. Stressor loops poll [`GpuHealth::failure`] each
/// iteration and abort the stage instead of spinning on a dead device.
#[derive(Clone, Default)]
pub(super) struct GpuHealth {
    device_errors: Arc<AtomicU64>,
    tooling_errors: Arc<AtomicU64>,
    lost: Arc<AtomicBool>,
    first_device_error: Arc<Mutex<Option<String>>>,
    first_tooling_error: Arc<Mutex<Option<String>>>,
}

impl GpuHealth {
    fn note_error(&self, err: &wgpu::Error) {
        let text = one_line(&err.to_string());
        // wgpu routes device loss to the device-lost callback and never here, so
        // an uncaptured internal error is driver-side and the rest are ours.
        let (class, kind) = match err {
            wgpu::Error::Validation { .. } => (ErrorClass::Tooling, "validation error"),
            wgpu::Error::OutOfMemory { .. } => (ErrorClass::Tooling, "out-of-memory error"),
            wgpu::Error::Internal { .. } => (ErrorClass::Device, "internal driver error"),
        };
        // Device-loss text is hardware evidence whichever variant carried it.
        let class = if mentions_device_loss(&text) {
            ErrorClass::Device
        } else {
            class
        };
        let (counter, first) = match class {
            ErrorClass::Tooling => (&self.tooling_errors, &self.first_tooling_error),
            ErrorClass::Device => (&self.device_errors, &self.first_device_error),
        };
        let n = counter.fetch_add(1, Ordering::Relaxed);
        if n < LOGGED_DEVICE_ERRORS {
            log::error!("[stress-kit/gpu] uncaptured {kind}: {text}");
        } else if n % 1000 == 0 {
            log::error!("[stress-kit/gpu] {kind}s continue (total {n}); suppressing");
        }
        if let Ok(mut g) = first.lock() {
            g.get_or_insert(text);
        }
    }

    fn note_lost(&self, reason: String) {
        self.lost.store(true, Ordering::SeqCst);
        log::error!("[stress-kit/gpu] device lost: {reason}");
        if let Ok(mut g) = self.first_device_error.lock() {
            *g = Some(one_line(&reason));
        }
    }

    /// `Some(reason)` once the device has been lost or has reported an error.
    /// Driver-side failures read as hardware evidence; errors raised against
    /// this crate's own commands read as inconclusive — no load ran.
    pub(super) fn failure(&self) -> Option<String> {
        let device_errors = self.device_errors.load(Ordering::Relaxed);
        if self.lost.load(Ordering::Relaxed) || device_errors > 0 {
            let detail = first_or(&self.first_device_error, "unknown device error");
            return Some(format!(
                "GPU device failed ({} error(s)): {detail}",
                device_errors.max(1)
            ));
        }
        let tooling_errors = self.tooling_errors.load(Ordering::Relaxed);
        if tooling_errors > 0 {
            let detail = first_or(&self.first_tooling_error, "unknown API error");
            return Some(format!(
                "inconclusive - wgpu rejected this stressor's own commands \
                 ({tooling_errors} API error(s)), so the load never ran: {detail}"
            ));
        }
        None
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

/// CPU-backed rasterizers (WARP / Basic Render Driver, llvmpipe, SwiftShader):
/// they answer wgpu but exercise no GPU.
fn is_software_adapter(info: &wgpu::AdapterInfo) -> bool {
    let name = info.name.to_lowercase();
    name.contains("microsoft basic")
        || name.contains("llvmpipe")
        || name.contains("swiftshader")
}

fn adapter_score(info: &wgpu::AdapterInfo, prefer_discrete: bool) -> i32 {
    let name = info.name.to_lowercase();
    if is_software_adapter(info) {
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
        let instance = Instance::new(InstanceDescriptor {
            backends: Backends::PRIMARY,
            ..InstanceDescriptor::new_without_display_handle()
        });

        let adapters = pollster::block_on(instance.enumerate_adapters(Backends::PRIMARY));
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

        let adapter = match pick_adapter(adapters, prefer_discrete) {
            Some(a) => a,
            None => {
                let power = if prefer_discrete {
                    PowerPreference::HighPerformance
                } else {
                    PowerPreference::LowPower
                };
                log::warn!(
                    "[stress-kit/gpu] no scored adapter; falling back to wgpu power preference {:?}",
                    power
                );
                let fallback = pollster::block_on(instance.request_adapter(&RequestAdapterOptions {
                    power_preference: power,
                    compatible_surface: None,
                    force_fallback_adapter: false,
                    apply_limit_buckets: false,
                }))
                .map_err(|e| format!("no usable GPU adapter: {e}"))?;
                let info = fallback.get_info();
                // The fallback re-offers the adapter the scorer just rejected.
                if is_software_adapter(&info) {
                    return Err(format!(
                        "no usable GPU adapter: '{}' is a software rasterizer",
                        info.name
                    ));
                }
                fallback
            }
        };

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
            experimental_features: wgpu::ExperimentalFeatures::disabled(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        }))
        .map_err(|e| format!("GPU device request failed: {e}"))?;

        let health = GpuHealth::default();
        let health_err = health.clone();
        device.on_uncaptured_error(Arc::new(move |err| {
            health_err.note_error(&err);
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

/// Emits a latched inconclusive fatal so the stage cannot pass, re-emits it for
/// [`UNSUPPORTED_LINGER`], then returns rather than idling out the stage.
/// `stage` is the message prefix, `load` names the work that did not run,
/// `detail` is the acquisition failure.
pub(super) fn run_unsupported(
    stage: &str,
    load: &str,
    detail: &str,
    cancel: &Arc<AtomicBool>,
    tx: &mpsc::Sender<Metrics>,
    started_at: Instant,
) {
    let reason =
        format!("{stage}: inconclusive - no usable GPU, the {load} never ran ({detail})");
    log::error!("[stress-kit/gpu] {reason}");
    emit_fatal_tick(tx, started_at, reason.clone(), 0);
    let mut last_tick = Instant::now();
    let linger_until = Instant::now() + UNSUPPORTED_LINGER;
    while !cancel.load(Ordering::Relaxed) && Instant::now() < linger_until {
        std::thread::sleep(Duration::from_millis(50));
        // Re-emitted every tick so a newest-only drain still sees the fatal.
        if last_tick.elapsed() >= TICK {
            emit_fatal_tick(tx, started_at, reason.clone(), 0);
            last_tick = Instant::now();
        }
    }
    log::debug!("[stress-kit/gpu] {stage}: no GPU to load, ending the stage");
}
