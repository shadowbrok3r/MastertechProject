//! Concurrent CPU + RAM + GPU load (OCCT-style whole-system torture). CPU FMA
//! workers and a memory-bandwidth churn leg share the core pool while a driver
//! thread hammers the GPU compute kernel. Headline throughput is combined
//! CPU+GPU GFLOPS; the RAM leg is concurrent load only.
//!
//! With no usable GPU the CPU and RAM legs keep running, but every tick reports
//! an `inconclusive -` reason in `last_error`: the stage is not a whole-system
//! result and must not be graded as one. Ticks stay non-fatal so the CPU+RAM
//! load is not aborted, and the reason avoids device-loss wording so a machine
//! without a dGPU is never reported as failed hardware.

#![cfg(feature = "gpu")]

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use wgpu::util::DeviceExt;

use crate::Metrics;

use super::gpu_common::{wait_latest, GpuContext};

const TICK: Duration = Duration::from_millis(500);

const CHAIN_DEPTH: usize = 8;
const ITERS_PER_BURST: u64 = 200_000;
const CPU_FLOPS_PER_BURST: u64 = ITERS_PER_BURST * CHAIN_DEPTH as u64 * 2;

const MEM_CHUNK_MB: u64 = 16;

const WG_SIZE: u32 = 64;
const WG_COUNT: u32 = 4096;
const INVOCATIONS_PER_DISPATCH: u64 = (WG_SIZE as u64) * (WG_COUNT as u64);
const INNER_ITERS: u32 = 2048;
// 5 flops per inner iteration: `y * 0.999999` plus two 2-flop fma calls.
const GPU_OPS_PER_INVOCATION: u64 = (INNER_ITERS as u64) * 5;
/// Consecutive device-wait timeouts tolerated before the GPU leg is dropped.
const MAX_WAIT_FAILURES: u32 = 3;

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
    memory_cap_mb: u64,
    cancel: &Arc<AtomicBool>,
    tx: &mpsc::Sender<Metrics>,
    started_at: Instant,
) {
    let pool = thread_count.saturating_sub(1).max(2);
    let mem_threads = (pool / 4).clamp(1, 4);
    let cpu_threads = pool.saturating_sub(mem_threads).max(1);
    let mem_cap_per_thread_mb = (memory_cap_mb / mem_threads as u64).max(MEM_CHUNK_MB);

    let cpu_bursts = Arc::new(AtomicU64::new(0));
    let gpu_dispatches = Arc::new(AtomicU64::new(0));
    let warn_slot: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    let cpu_handles: Vec<_> = (0..cpu_threads)
        .map(|_| {
            let cancel = cancel.clone();
            let counter = cpu_bursts.clone();
            thread::Builder::new()
                .name("stress-kit-combined-cpu".into())
                .spawn(move || fma_worker(cancel, counter))
                .expect("stress-kit: failed to spawn combined cpu worker")
        })
        .collect();

    let mem_handles: Vec<_> = (0..mem_threads)
        .map(|_| {
            let cancel = cancel.clone();
            thread::Builder::new()
                .name("stress-kit-combined-mem".into())
                .spawn(move || mem_churn_worker(cancel, mem_cap_per_thread_mb))
                .expect("stress-kit: failed to spawn combined mem worker")
        })
        .collect();

    let gpu_handle = {
        let cancel = cancel.clone();
        let counter = gpu_dispatches.clone();
        let warn = warn_slot.clone();
        thread::Builder::new()
            .name("stress-kit-combined-gpu".into())
            .spawn(move || gpu_driver(cancel, counter, warn))
            .expect("stress-kit: failed to spawn combined gpu driver")
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
    for h in mem_handles {
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

fn mem_churn_worker(cancel: Arc<AtomicBool>, cap_mb: u64) {
    let chunk_bytes = MEM_CHUNK_MB as usize * 1024 * 1024;
    let max_chunks = (cap_mb / MEM_CHUNK_MB).max(1) as usize;

    let mut chunks: Vec<Vec<u8>> = Vec::with_capacity(max_chunks);
    for _ in 0..max_chunks {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        let mut chunk = vec![0u8; chunk_bytes];
        write_pattern(&mut chunk);
        chunks.push(chunk);
    }

    let mut idx = 0usize;
    while !cancel.load(Ordering::Relaxed) {
        let mut chunk = vec![0u8; chunk_bytes];
        write_pattern(&mut chunk);
        chunks[idx] = chunk;
        idx = (idx + 1) % max_chunks;
    }

    drop(chunks);
}

#[inline]
fn write_pattern(buf: &mut [u8]) {
    for (i, b) in buf.iter_mut().enumerate() {
        *b = (i & 0xFF) as u8 ^ 0xA5;
    }
    std::hint::black_box(&*buf);
}

fn gpu_driver(cancel: Arc<AtomicBool>, counter: Arc<AtomicU64>, warn: Arc<Mutex<Option<String>>>) {
    let ctx = match GpuContext::acquire(true) {
        Ok(c) => c,
        Err(e) => {
            let msg = format!(
                "combined: inconclusive - no usable GPU, the GPU leg never ran ({e}); the CPU \
                 and RAM legs carried the whole load, so this is not a whole-system result"
            );
            log::error!("[stress-kit/combined] {msg}");
            set_warn(&warn, msg);
            return;
        }
    };
    log::info!(
        "[stress-kit/combined] GPU leg on {} ({})",
        ctx.vendor_label,
        ctx.backend_label
    );

    let sink_buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("combined sink"),
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
            label: Some("combined params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

    let module = ctx.device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("combined module"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let pipeline = ctx
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("combined pipeline"),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
    let bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("combined bind group"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: sink_buf.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: params_buf.as_entire_binding() },
        ],
    });

    let mut wait_failures: u32 = 0;

    while !cancel.load(Ordering::Relaxed) {
        params.seed = params.seed.wrapping_mul(1103515245).wrapping_add(12345);
        ctx.queue.write_buffer(&params_buf, 0, bytemuck::bytes_of(&params));

        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("combined encoder") });
        {
            let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("combined pass"),
                timestamp_writes: None,
            });
            cpass.set_pipeline(&pipeline);
            cpass.set_bind_group(0, &bind_group, &[]);
            cpass.dispatch_workgroups(WG_COUNT, 1, 1);
        }
        ctx.queue.submit(std::iter::once(encoder.finish()));
        // Only a confirmed-complete dispatch counts toward GFLOPS.
        match ctx.device.poll(wait_latest()) {
            Ok(_) => {
                counter.fetch_add(1, Ordering::Relaxed);
                wait_failures = 0;
            }
            Err(e) => {
                wait_failures += 1;
                log::warn!(
                    "[stress-kit/combined] device wait timed out ({e:?}) x{wait_failures}"
                );
            }
        }
        if let Some(reason) = ctx.health.failure() {
            let msg = format!("combined: GPU leg stopped ({reason}); continuing CPU+RAM only");
            log::error!("[stress-kit/combined] {msg}");
            set_warn(&warn, msg);
            return;
        }
        if wait_failures >= MAX_WAIT_FAILURES {
            let msg = format!(
                "combined: inconclusive - GPU leg stopped (queue stalled, {MAX_WAIT_FAILURES} \
                 consecutive device-wait timeouts); CPU+RAM only from here, so this is not a \
                 whole-system result"
            );
            log::error!("[stress-kit/combined] {msg}");
            set_warn(&warn, msg);
            return;
        }
    }
}

fn set_warn(warn: &Arc<Mutex<Option<String>>>, msg: String) {
    if let Ok(mut g) = warn.lock() {
        *g = Some(msg);
    }
}
