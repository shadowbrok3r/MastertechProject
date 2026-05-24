//! Shared wgpu device acquisition and tick emission for the GPU stressors.

#![cfg(feature = "gpu")]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use wgpu::{
    Adapter, Backends, Device, DeviceDescriptor, Features, Instance,
    InstanceDescriptor, Limits, PowerPreference, Queue, RequestAdapterOptions,
};

use crate::Metrics;

pub(super) const TICK: Duration = Duration::from_millis(500);

pub(super) struct GpuContext {
    #[allow(dead_code)]
    pub adapter: Adapter,
    pub device: Device,
    pub queue: Queue,
    pub vendor_label: String,
    pub backend_label: String,
}

impl GpuContext {
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

        let (device, queue) = pollster::block_on(adapter.request_device(&DeviceDescriptor {
            label: Some("stress-kit GPU stressor"),
            required_features: Features::empty(),
            required_limits: Limits::default(),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
        }))
        .map_err(|e| format!("GPU device request failed: {e}"))?;

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

pub(super) fn run_unsupported(
    reason: String,
    cancel: &Arc<AtomicBool>,
    tx: &mpsc::Sender<Metrics>,
    started_at: Instant,
) {
    log::warn!("[stress-kit/gpu] stressor inactive: {reason}");
    emit_tick(tx, started_at, 0.0, Some(reason));
    while !cancel.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(100));
    }
}
