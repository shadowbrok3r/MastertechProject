//! Combined CPU + GPU load for power-supply / VRM stress. CPU workers run FMA
//! chains while a raised-priority driver thread keeps a bounded number of
//! single-dispatch mixed-FMA + scattered-load submissions in flight on the GPU.
//! Reports combined GFLOPS.
//!
//! A CPU-only run never loads the rails the +12V / GPU rules grade, so it cannot
//! yield a PSU verdict: the stage goes fatal the moment the GPU leg is gone, or
//! when a leg that only signalled trouble fails to recover inside
//! `GPU_LOSS_GRACE`, and returns once that fatal is latched.

#![cfg(feature = "gpu")]

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use wgpu::util::DeviceExt;

use crate::Metrics;

use super::gpu_common::{emit_fatal_tick, emit_tick, wait_for, wait_latest, GpuContext, TICK};

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
/// Wall-clock window without confirmed GPU work before the leg is declared stalled.
const DRAIN_STALL_LIMIT: Duration = Duration::from_secs(90);
/// Logical cores held back for the GPU driver thread and the tick loop.
const RESERVED_CORES: usize = 2;
/// Window a mid-run GPU-leg failure is carried as a warning before going fatal.
const GPU_LOSS_GRACE: Duration = Duration::from_secs(3);
/// Reason reported when the GPU-leg failure detail is unreadable.
const GPU_LEG_DOWN: &str =
    "psu: inconclusive - the GPU leg is not running, the combined CPU+GPU load was not applied";

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
    let fatal_slot: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    let mut cpu_handles: Vec<_> = (0..cpu_threads)
        .map(|_| spawn_fma_worker(&stop, &cpu_bursts))
        .collect();

    let gpu_handle = {
        let stop = stop.clone();
        let counter = gpu_dispatches.clone();
        let warn = warn_slot.clone();
        let fatal = fatal_slot.clone();
        let down = gpu_down.clone();
        let tx = tx.clone();
        thread::Builder::new()
            .name("stress-kit-psu-gpu".into())
            .spawn(move || gpu_driver(stop, counter, warn, fatal, down, tx, started_at))
            .expect("stress-kit: failed to spawn psu gpu driver")
    };

    let mut last_tick = Instant::now();
    let mut last_cpu: u64 = 0;
    let mut last_gpu: u64 = 0;
    let mut grace_until: Option<Instant> = None;

    while !cancel.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(50));

        // GPU leg down: put its reserved cores back to work on the CPU chains.
        if grace_until.is_none() && gpu_down.load(Ordering::Relaxed) {
            grace_until = Some(Instant::now() + GPU_LOSS_GRACE);
            for _ in 0..thread_count.saturating_sub(cpu_threads) {
                cpu_handles.push(spawn_fma_worker(&stop, &cpu_bursts));
            }
        }

        if last_tick.elapsed() < TICK {
            continue;
        }

        let cpu_now = cpu_bursts.load(Ordering::Relaxed);
        let gpu_now = gpu_dispatches.load(Ordering::Relaxed);
        let dt = last_tick.elapsed().as_secs_f64().max(f64::EPSILON);

        let cpu_flops = cpu_now.saturating_sub(last_cpu) as f64 * CPU_FLOPS_PER_BURST as f64;
        let gpu_flops = gpu_now.saturating_sub(last_gpu) as f64
            * (INVOCATIONS_PER_DISPATCH * GPU_OPS_PER_INVOCATION) as f64;
        let gflops = (cpu_flops + gpu_flops) / dt / 1e9;

        last_cpu = cpu_now;
        last_gpu = gpu_now;
        last_tick = Instant::now();

        let reason = warn_slot.lock().ok().and_then(|g| g.clone());
        // The driver thread returns early only when its leg is gone for good.
        let leg_gone = gpu_handle.is_finished()
            || grace_until.is_some_and(|deadline| Instant::now() >= deadline);
        let fatal = fatal_slot.lock().ok().and_then(|g| g.clone()).or_else(|| {
            leg_gone.then(|| reason.clone().unwrap_or_else(|| GPU_LEG_DOWN.to_string()))
        });

        match fatal {
            // The verdict is decided; the surviving CPU-only load proves nothing.
            Some(msg) => {
                emit_latched_fatal(tx, started_at, gflops, msg);
                break;
            }
            None => emit_tick(tx, started_at, gflops, reason, 0),
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
    fatal: Arc<Mutex<Option<String>>>,
    down: Arc<AtomicBool>,
    tx: mpsc::Sender<Metrics>,
    started_at: Instant,
) {
    let ctx = match GpuContext::acquire(true) {
        Ok(c) => c,
        Err(e) => {
            // No leg to recover, so the fatal goes out now instead of after the grace window.
            report_gpu_never_ran(
                &warn,
                &fatal,
                &down,
                &tx,
                started_at,
                format!(
                    "psu: inconclusive - GPU unavailable ({e}); the GPU leg never ran, so the \
                     +12V rail was never loaded and this is not a valid PSU test"
                ),
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
    // Reset by every confirmed dispatch; a timing-out device wait leaves it standing.
    let mut last_progress = Instant::now();

    while !stop.load(Ordering::Relaxed) {
        while pending.len() >= MAX_INFLIGHT_DISPATCHES {
            let Some(oldest) = pending.front().cloned() else { break };
            match ctx.device.poll(wait_for(oldest)) {
                Ok(_) => {
                    pending.pop_front();
                    counter.fetch_add(1, Ordering::Relaxed);
                    last_progress = Instant::now();
                }
                // Oldest stays tracked unless a full drain confirms it completed.
                Err(e) => match ctx.device.poll(wait_latest()) {
                    Ok(_) => {
                        counter.fetch_add(pending.len() as u64, Ordering::Relaxed);
                        pending.clear();
                        last_progress = Instant::now();
                    }
                    Err(_) => log::debug!(
                        "[stress-kit/psu] queue wait timed out ({e:?}); {:.0}s without progress",
                        last_progress.elapsed().as_secs_f64()
                    ),
                },
            }
            if stop.load(Ordering::Relaxed)
                || last_progress.elapsed() >= DRAIN_STALL_LIMIT
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
        if last_progress.elapsed() >= DRAIN_STALL_LIMIT {
            report_gpu_stop(
                &warn,
                &down,
                format!(
                    "psu: inconclusive - no GPU dispatch has completed for {}s, device waits keep \
                     timing out; the GPU leg is no longer loading the rail",
                    last_progress.elapsed().as_secs()
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
            last_progress = Instant::now();
        }
    }

    // Skipped once stalled: the wait would block for wgpu's full internal timeout.
    if last_progress.elapsed() < DRAIN_STALL_LIMIT {
        let _ = ctx.device.poll(wait_latest());
    }
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
    set_slot(warn, msg);
    down.store(true, Ordering::SeqCst);
}

/// Publishes the reason and sends the fatal at once, bypassing the grace window.
fn report_gpu_never_ran(
    warn: &Arc<Mutex<Option<String>>>,
    fatal: &Arc<Mutex<Option<String>>>,
    down: &Arc<AtomicBool>,
    tx: &mpsc::Sender<Metrics>,
    started_at: Instant,
    msg: String,
) {
    report_gpu_stop(warn, down, msg.clone());
    set_slot(fatal, msg.clone());
    emit_fatal_tick(tx, started_at, msg, 0);
}

fn set_slot(slot: &Arc<Mutex<Option<String>>>, msg: String) {
    if let Ok(mut g) = slot.lock() {
        *g = Some(msg);
    }
}
