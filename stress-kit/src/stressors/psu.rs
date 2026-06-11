//! Combined CPU + GPU load for power-supply / VRM stress. CPU workers run
//! FMA chains on all but one thread while a driver thread hammers the GPU
//! with the compute-shader FMA kernel. Reports combined GFLOPS. Runs
//! CPU-only (with a warning in `last_error`) when no GPU is available.

#![cfg(feature = "gpu")]

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use wgpu::util::DeviceExt;

use crate::Metrics;

use super::gpu_common::GpuContext;

const TICK: Duration = Duration::from_millis(500);

const CHAIN_DEPTH: usize = 8;
const ITERS_PER_BURST: u64 = 200_000;
const CPU_FLOPS_PER_BURST: u64 = ITERS_PER_BURST * CHAIN_DEPTH as u64 * 2;

const WG_SIZE: u32 = 64;
const WG_COUNT: u32 = 4096;
const INVOCATIONS_PER_DISPATCH: u64 = (WG_SIZE as u64) * (WG_COUNT as u64);
const INNER_ITERS: u32 = 2048;
const GPU_OPS_PER_INVOCATION: u64 = (INNER_ITERS as u64) * 2;

const SHADER: &str = r#"
struct Params { inner_iters: u32, seed: u32, _p0: u32, _p1: u32 };

@group(0) @binding(0) var<storage, read_write> sink: array<f32>;
@group(0) @binding(1) var<uniform> params: Params;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= arrayLength(&sink)) { return; }
    var x: f32 = f32((i ^ params.seed) & 0xffffu) * 1e-5 + 1.0001;
    var y: f32 = 0.5;
    for (var k: u32 = 0u; k < params.inner_iters; k = k + 1u) {
        y = fma(x, 1.000001, y * 0.999999);
        x = fma(y, 1e-7, x);
    }
    sink[i] = x + y;
}
"#;

pub(crate) fn run(
    thread_count: usize,
    cancel: &Arc<AtomicBool>,
    tx: &mpsc::Sender<Metrics>,
    started_at: Instant,
) {
    let cpu_threads = thread_count.saturating_sub(1).max(1);
    let cpu_bursts = Arc::new(AtomicU64::new(0));
    let gpu_dispatches = Arc::new(AtomicU64::new(0));
    let warn_slot: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    let cpu_handles: Vec<_> = (0..cpu_threads)
        .map(|_| {
            let cancel = cancel.clone();
            let counter = cpu_bursts.clone();
            thread::Builder::new()
                .name("stress-kit-psu-cpu".into())
                .spawn(move || fma_worker(cancel, counter))
                .expect("stress-kit: failed to spawn psu cpu worker")
        })
        .collect();

    let gpu_handle = {
        let cancel = cancel.clone();
        let counter = gpu_dispatches.clone();
        let warn = warn_slot.clone();
        thread::Builder::new()
            .name("stress-kit-psu-gpu".into())
            .spawn(move || gpu_driver(cancel, counter, warn))
            .expect("stress-kit: failed to spawn psu gpu driver")
    };

    let mut last_tick = Instant::now();
    let mut last_cpu: u64 = 0;
    let mut last_gpu: u64 = 0;

    while !cancel.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(50));
        if last_tick.elapsed() >= TICK {
            let cpu_now = cpu_bursts.load(Ordering::Relaxed);
            let gpu_now = gpu_dispatches.load(Ordering::Relaxed);
            let dt = last_tick.elapsed().as_secs_f64().max(f64::EPSILON);

            let cpu_flops = cpu_now.saturating_sub(last_cpu) as f64 * CPU_FLOPS_PER_BURST as f64;
            let gpu_flops = gpu_now.saturating_sub(last_gpu) as f64
                * (INVOCATIONS_PER_DISPATCH * GPU_OPS_PER_INVOCATION) as f64;
            let gflops = (cpu_flops + gpu_flops) / dt / 1e9;

            let _ = tx.send(Metrics {
                elapsed_secs: started_at.elapsed().as_secs_f64(),
                throughput: gflops,
                last_error: warn_slot.lock().ok().and_then(|g| g.clone()),
                fatal: false,
                errors: 0,
            });

            last_cpu = cpu_now;
            last_gpu = gpu_now;
            last_tick = Instant::now();
        }
    }

    for h in cpu_handles {
        let _ = h.join();
    }
    let _ = gpu_handle.join();
}

fn fma_worker(cancel: Arc<AtomicBool>, counter: Arc<AtomicU64>) {
    let mut acc = [1.000_001f64; CHAIN_DEPTH];
    for (i, a) in acc.iter_mut().enumerate() {
        *a += i as f64 * 1e-6;
    }

    while !cancel.load(Ordering::Relaxed) {
        for _ in 0..ITERS_PER_BURST {
            for (i, a) in acc.iter_mut().enumerate() {
                *a = a.mul_add(1.000_000_001 + i as f64 * 1e-12, 1e-9);
            }
        }
        for a in acc.iter_mut() {
            if !a.is_finite() || a.abs() > 1e30 {
                *a = 1.000_001;
            }
        }
        std::hint::black_box(&acc);
        counter.fetch_add(1, Ordering::Relaxed);
    }
}

fn gpu_driver(cancel: Arc<AtomicBool>, counter: Arc<AtomicU64>, warn: Arc<Mutex<Option<String>>>) {
    let ctx = match GpuContext::acquire(true) {
        Ok(c) => c,
        Err(e) => {
            let msg = format!("psu: GPU unavailable, running CPU-only ({e})");
            log::warn!("[stress-kit/psu] {msg}");
            if let Ok(mut g) = warn.lock() {
                *g = Some(msg);
            }
            return;
        }
    };
    log::info!("[stress-kit/psu] GPU leg on {} ({})", ctx.vendor_label, ctx.backend_label);

    let sink_buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("psu sink"),
        size: INVOCATIONS_PER_DISPATCH * 4,
        usage: wgpu::BufferUsages::STORAGE,
        mapped_at_creation: false,
    });

    #[repr(C)]
    #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
    struct Params {
        inner_iters: u32,
        seed: u32,
        _p0: u32,
        _p1: u32,
    }
    let mut params = Params {
        inner_iters: INNER_ITERS,
        seed: 0xBEEF,
        _p0: 0,
        _p1: 0,
    };
    let params_buf = ctx
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("psu params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

    let module = ctx
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("psu module"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
    let pipeline = ctx
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("psu pipeline"),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
    let bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("psu bind group"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: sink_buf.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: params_buf.as_entire_binding() },
        ],
    });

    while !cancel.load(Ordering::Relaxed) {
        params.seed = params.seed.wrapping_mul(1103515245).wrapping_add(12345);
        ctx.queue.write_buffer(&params_buf, 0, bytemuck::bytes_of(&params));

        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("psu encoder") });
        {
            let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("psu pass"),
                timestamp_writes: None,
            });
            cpass.set_pipeline(&pipeline);
            cpass.set_bind_group(0, &bind_group, &[]);
            cpass.dispatch_workgroups(WG_COUNT, 1, 1);
        }
        ctx.queue.submit(std::iter::once(encoder.finish()));
        let _ = ctx.device.poll(wgpu::PollType::Wait);
        if let Some(reason) = ctx.health.failure() {
            let msg = format!("psu: GPU leg stopped ({reason}); continuing CPU-only");
            log::error!("[stress-kit/psu] {msg}");
            if let Ok(mut g) = warn.lock() {
                *g = Some(msg);
            }
            return;
        }
        counter.fetch_add(1, Ordering::Relaxed);
    }
}
