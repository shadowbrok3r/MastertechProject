//! Combined CPU + GPU load for power-supply / VRM stress. CPU workers run FMA
//! chains while a raised-priority driver thread keeps a bounded number of
//! single-dispatch mixed-FMA + scattered-load submissions in flight on the GPU.
//! Reports combined GFLOPS.
//!
//! When the GPU leg cannot run, the stage keeps loading every core, reports the
//! reason on each tick, then marks every tick past the grace window fatal: a
//! CPU-only run never loads the rails the +12V / GPU rules grade, so it must not
//! yield a PSU verdict.

#![cfg(feature = "gpu")]

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use wgpu::util::DeviceExt;

use crate::Metrics;

use super::gpu_common::{emit_tick, GpuContext};

const TICK: Duration = Duration::from_millis(500);

const CHAIN_DEPTH: usize = 8;
const ITERS_PER_BURST: u64 = 200_000;
const CPU_FLOPS_PER_BURST: u64 = ITERS_PER_BURST * CHAIN_DEPTH as u64 * 2;

const WG_SIZE: u32 = 64;
const WG_COUNT: u32 = 4096;
const INVOCATIONS_PER_DISPATCH: u64 = (WG_SIZE as u64) * (WG_COUNT as u64);
const INNER_ITERS: u32 = 1024;
// 6 flops per inner iteration, plus 2 more on every 4th iteration.
const GPU_OPS_PER_INVOCATION: u64 = (INNER_ITERS as u64) * 6 + (INNER_ITERS as u64) / 2;
const SCATTER_FLOATS: usize = (64 * 1024 * 1024) / std::mem::size_of::<f32>();

/// Distinct seed slots cycled through, one per submission.
const DISPATCH_SLOTS: usize = 8;
/// Single-dispatch submissions queued before the driver thread waits on the oldest.
const MAX_INFLIGHT_DISPATCHES: usize = 32;
/// Consecutive queue-drain timeouts tolerated before the GPU leg is declared stopped.
const MAX_DRAIN_FAILURES: u32 = 3;
/// Logical cores held back for the GPU driver thread and the tick loop.
const RESERVED_CORES: usize = 2;
/// Ticks that carry the GPU-leg failure reason before every later tick goes fatal.
const GPU_LOSS_GRACE_TICKS: u32 = 6;
/// Reason reported when the GPU-leg failure detail is unreadable.
const GPU_LEG_DOWN: &str = "psu: inconclusive - GPU unavailable, GPU leg never ran";

const SHADER: &str = r#"
struct Params {
    inner_iters: u32,
    buffer_len:  u32,
    seed:        u32,
    _pad:        u32,
};

@group(0) @binding(0) var<storage, read>        scatter:  array<f32>;
@group(0) @binding(1) var<storage, read_write>  sink:     array<f32>;
@group(0) @binding(2) var<uniform>              params:   Params;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i  = gid.x;
    let len = params.buffer_len;
    if (i >= arrayLength(&sink)) { return; }

    var s1: u32 = (i * 2654435761u) ^ params.seed;
    var s2: u32 = (i * 1597334677u) ^ (params.seed ^ 0x9e3779b9u);

    var x: f32 = f32(i & 0xffu) * 0.001 + 1.001;
    var y: f32 = 0.5;

    for (var k: u32 = 0u; k < params.inner_iters; k = k + 1u) {
        s1 = s1 * 1664525u + 1013904223u;
        let idx1 = s1 % len;
        let v1 = scatter[idx1];

        let t = x * 1.000001 + 0.0001;
        y = fma(t, v1, y);
        x = x * 0.99999 + 0.0001;

        if ((k & 3u) == 0u) {
            s2 = s2 * 22695477u + 1u;
            let idx2 = s2 % len;
            y = y + scatter[idx2] * 1e-9;
        }
    }

    sink[i] = x * 1e-9 + y * 1e-9;
}
"#;

pub(crate) fn run(
    thread_count: usize,
    cancel: &Arc<AtomicBool>,
    tx: &mpsc::Sender<Metrics>,
    started_at: Instant,
) {
    let stop = Arc::new(AtomicBool::new(false));
    let cpu_threads = thread_count.saturating_sub(RESERVED_CORES).max(1);
    let cpu_bursts = Arc::new(AtomicU64::new(0));
    let gpu_dispatches = Arc::new(AtomicU64::new(0));
    let gpu_down = Arc::new(AtomicBool::new(false));
    let warn_slot: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    let mut cpu_handles: Vec<_> = (0..cpu_threads)
        .map(|_| spawn_fma_worker(&stop, &cpu_bursts))
        .collect();

    let gpu_handle = {
        let stop = stop.clone();
        let counter = gpu_dispatches.clone();
        let warn = warn_slot.clone();
        let down = gpu_down.clone();
        thread::Builder::new()
            .name("stress-kit-psu-gpu".into())
            .spawn(move || gpu_driver(stop, counter, warn, down))
            .expect("stress-kit: failed to spawn psu gpu driver")
    };

    let mut last_tick = Instant::now();
    let mut last_cpu: u64 = 0;
    let mut last_gpu: u64 = 0;
    let mut ticks_since_gpu_loss: Option<u32> = None;

    while !cancel.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(50));

        // GPU leg down: put its reserved cores back to work on the CPU chains.
        if ticks_since_gpu_loss.is_none() && gpu_down.load(Ordering::Relaxed) {
            ticks_since_gpu_loss = Some(0);
            for _ in 0..thread_count.saturating_sub(cpu_threads) {
                cpu_handles.push(spawn_fma_worker(&stop, &cpu_bursts));
            }
        }

        if last_tick.elapsed() >= TICK {
            let cpu_now = cpu_bursts.load(Ordering::Relaxed);
            let gpu_now = gpu_dispatches.load(Ordering::Relaxed);
            let dt = last_tick.elapsed().as_secs_f64().max(f64::EPSILON);

            let cpu_flops = cpu_now.saturating_sub(last_cpu) as f64 * CPU_FLOPS_PER_BURST as f64;
            let gpu_flops = gpu_now.saturating_sub(last_gpu) as f64
                * (INVOCATIONS_PER_DISPATCH * GPU_OPS_PER_INVOCATION) as f64;
            let gflops = (cpu_flops + gpu_flops) / dt / 1e9;

            let reason = warn_slot.lock().ok().and_then(|g| g.clone());
            let grace_elapsed = match ticks_since_gpu_loss.as_mut() {
                Some(n) => {
                    *n = n.saturating_add(1);
                    *n >= GPU_LOSS_GRACE_TICKS
                }
                None => false,
            };
            if grace_elapsed {
                // Every tick from here repeats the fatal so a newest-only drain keeps it.
                emit_latched_fatal(
                    tx,
                    started_at,
                    gflops,
                    reason.unwrap_or_else(|| GPU_LEG_DOWN.to_string()),
                );
            } else {
                emit_tick(tx, started_at, gflops, reason, 0);
            }

            last_cpu = cpu_now;
            last_gpu = gpu_now;
            last_tick = Instant::now();
        }
    }

    stop.store(true, Ordering::SeqCst);
    for h in cpu_handles {
        let _ = h.join();
    }
    let _ = gpu_handle.join();
}

fn spawn_fma_worker(stop: &Arc<AtomicBool>, counter: &Arc<AtomicU64>) -> thread::JoinHandle<()> {
    let stop = stop.clone();
    let counter = counter.clone();
    thread::Builder::new()
        .name("stress-kit-psu-cpu".into())
        .spawn(move || fma_worker(stop, counter))
        .expect("stress-kit: failed to spawn psu cpu worker")
}

fn fma_worker(stop: Arc<AtomicBool>, counter: Arc<AtomicU64>) {
    let mut acc = [1.000_001f64; CHAIN_DEPTH];
    for (i, a) in acc.iter_mut().enumerate() {
        *a += i as f64 * 1e-6;
    }

    while !stop.load(Ordering::Relaxed) {
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

/// Bumps the calling thread to `THREAD_PRIORITY_ABOVE_NORMAL`; failures are ignored.
#[cfg(windows)]
fn raise_submit_priority() {
    let ok = unsafe {
        winapi::um::processthreadsapi::SetThreadPriority(
            winapi::um::processthreadsapi::GetCurrentThread(),
            winapi::um::winbase::THREAD_PRIORITY_ABOVE_NORMAL as i32,
        )
    };
    if ok == 0 {
        log::debug!("[stress-kit/psu] SetThreadPriority failed; running at default priority");
    }
}

#[cfg(not(windows))]
fn raise_submit_priority() {}

fn gpu_driver(
    stop: Arc<AtomicBool>,
    counter: Arc<AtomicU64>,
    warn: Arc<Mutex<Option<String>>>,
    down: Arc<AtomicBool>,
) {
    let ctx = match GpuContext::acquire(true) {
        Ok(c) => c,
        Err(e) => {
            report_gpu_stop(
                &warn,
                &down,
                format!("psu: inconclusive - GPU unavailable, GPU leg never ran ({e})"),
            );
            return;
        }
    };
    log::info!("[stress-kit/psu] GPU leg on {} ({})", ctx.vendor_label, ctx.backend_label);
    raise_submit_priority();

    let scatter_data: Vec<f32> = (0..SCATTER_FLOATS)
        .map(|i| ((i as u32).wrapping_mul(2246822519)) as f32 * 1e-7 + 0.5)
        .collect();
    let scatter_buf = ctx
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("psu scatter"),
            contents: bytemuck::cast_slice(&scatter_data),
            usage: wgpu::BufferUsages::STORAGE,
        });
    drop(scatter_data);

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
        buffer_len: u32,
        seed: u32,
        _pad: u32,
    }

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
    let layout = pipeline.get_bind_group_layout(0);

    // One write-once params buffer per dispatch slot, each with a distinct seed.
    let mut seed: u32 = 0xc0ffee;
    let params_bufs: Vec<wgpu::Buffer> = (0..DISPATCH_SLOTS)
        .map(|_| {
            seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
            let params = Params {
                inner_iters: INNER_ITERS,
                buffer_len: SCATTER_FLOATS as u32,
                seed,
                _pad: 0,
            };
            ctx.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("psu params"),
                    contents: bytemuck::bytes_of(&params),
                    usage: wgpu::BufferUsages::UNIFORM,
                })
        })
        .collect();
    let bind_groups: Vec<wgpu::BindGroup> = params_bufs
        .iter()
        .map(|params_buf| {
            ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("psu bind group"),
                layout: &layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: scatter_buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: sink_buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: params_buf.as_entire_binding() },
                ],
            })
        })
        .collect();

    // Submitted-but-unconfirmed dispatches, oldest first; drained to keep the cap.
    let mut pending: VecDeque<wgpu::SubmissionIndex> =
        VecDeque::with_capacity(MAX_INFLIGHT_DISPATCHES + 1);
    let mut slot: usize = 0;
    let mut drain_failures: u32 = 0;

    while !stop.load(Ordering::Relaxed) {
        while pending.len() >= MAX_INFLIGHT_DISPATCHES {
            let Some(oldest) = pending.front().cloned() else { break };
            match ctx.device.poll(wgpu::PollType::WaitForSubmissionIndex(oldest)) {
                Ok(_) => {
                    pending.pop_front();
                    counter.fetch_add(1, Ordering::Relaxed);
                    drain_failures = 0;
                }
                // Oldest stays tracked unless a full drain confirms it completed.
                Err(e) => match ctx.device.poll(wgpu::PollType::Wait) {
                    Ok(_) => {
                        counter.fetch_add(pending.len() as u64, Ordering::Relaxed);
                        pending.clear();
                        drain_failures = 0;
                    }
                    Err(_) => {
                        drain_failures += 1;
                        log::debug!(
                            "[stress-kit/psu] queue wait timed out ({e:?}) x{drain_failures}"
                        );
                    }
                },
            }
            if stop.load(Ordering::Relaxed)
                || drain_failures >= MAX_DRAIN_FAILURES
                || ctx.health.failure().is_some()
            {
                break;
            }
        }

        if let Some(reason) = ctx.health.failure() {
            report_gpu_stop(
                &warn,
                &down,
                format!("psu: inconclusive - GPU leg stopped ({reason})"),
            );
            return;
        }
        if drain_failures >= MAX_DRAIN_FAILURES {
            report_gpu_stop(
                &warn,
                &down,
                format!(
                    "psu: inconclusive - GPU leg stopped (queue stalled, \
                     {MAX_DRAIN_FAILURES} consecutive wait timeouts)"
                ),
            );
            return;
        }
        if stop.load(Ordering::Relaxed) {
            break;
        }

        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("psu encoder") });
        {
            let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("psu pass"),
                timestamp_writes: None,
            });
            cpass.set_pipeline(&pipeline);
            cpass.set_bind_group(0, &bind_groups[slot], &[]);
            cpass.dispatch_workgroups(WG_COUNT, 1, 1);
        }
        pending.push_back(ctx.queue.submit(std::iter::once(encoder.finish())));
        slot = (slot + 1) % bind_groups.len();

        if let Ok(wgpu::PollStatus::QueueEmpty) = ctx.device.poll(wgpu::PollType::Poll) {
            counter.fetch_add(pending.len() as u64, Ordering::Relaxed);
            pending.clear();
        }
    }

    let _ = ctx.device.poll(wgpu::PollType::Wait);
}

/// Sends a fatal tick that keeps the measured throughput of the CPU-only load.
fn emit_latched_fatal(
    tx: &mpsc::Sender<Metrics>,
    started_at: Instant,
    throughput: f64,
    reason: String,
) {
    let _ = tx.send(Metrics {
        elapsed_secs: started_at.elapsed().as_secs_f64(),
        throughput,
        last_error: Some(reason),
        fatal: true,
        errors: 0,
    });
}

/// Publishes the GPU-leg stop reason and flags the leg down for the tick loop.
fn report_gpu_stop(warn: &Arc<Mutex<Option<String>>>, down: &Arc<AtomicBool>, msg: String) {
    log::error!("[stress-kit/psu] {msg}");
    if let Ok(mut g) = warn.lock() {
        *g = Some(msg);
    }
    down.store(true, Ordering::SeqCst);
}
