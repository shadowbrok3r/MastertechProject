//! Pulsed CPU + GPU load for PSU / 12V transient stress. The GPU leg
//! square-waves between compute bursts and idle windows while CPU FMA workers
//! run continuously. Submits are sized so a burst can be cut on the clock, and
//! the achieved duty cycle is measured rather than assumed. Reports combined
//! GFLOPS from confirmed-complete burst-phase GPU work. A run with no GPU load
//! is not a valid transient test: it goes fatal and stays fatal on every later
//! tick.

#![cfg(feature = "gpu")]

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use wgpu::util::DeviceExt;

use crate::Metrics;

use super::gpu_common::{emit_fatal_tick, emit_tick, GpuContext, TICK};

/// Target length of the GPU burst; submits stop as soon as it elapses.
const PULSE_ON: Duration = Duration::from_millis(100);
/// Idle window between GPU bursts.
const PULSE_OFF: Duration = Duration::from_millis(100);

const CHAIN_DEPTH: usize = 8;
const ITERS_PER_BURST: u64 = 200_000;
const CPU_FLOPS_PER_BURST: u64 = ITERS_PER_BURST * CHAIN_DEPTH as u64 * 2;

const WG_SIZE: u32 = 64;
/// Workgroups per submit; one unit must run far shorter than `PULSE_ON`.
const WG_PER_SUBMIT: u32 = 1024;
const INVOCATIONS_PER_SUBMIT: u64 = (WG_SIZE as u64) * (WG_PER_SUBMIT as u64);
const INNER_ITERS: u32 = 512;
// 6 flops per shader iteration plus 2 more on every fourth iteration.
const GPU_OPS_PER_INVOCATION: u64 = (INNER_ITERS as u64) * 6 + (INNER_ITERS as u64) / 2;
/// Queued submits tolerated before the driver thread waits on the oldest.
const MAX_INFLIGHT_SUBMITS: usize = 3;
/// Consecutive device-wait timeouts tolerated before the GPU leg is declared stopped.
const MAX_DRAIN_FAILURES: u32 = 3;
/// Timed single-unit submits taken before the burst loop starts.
const CALIBRATION_SAMPLES: u32 = 3;
/// Bursts between measured duty-cycle logs.
const DUTY_LOG_CYCLES: u64 = 25;
/// Measured duty cycle above which the pulse is reported as degraded.
const DUTY_CEILING: f64 = 0.65;
const SCATTER_FLOATS: usize = (64 * 1024 * 1024) / std::mem::size_of::<f32>();

const SHADER: &str = r#"
struct Params {
    inner_iters: u32,
    buffer_len:  u32,
    seed:        u32,
    _pad:        u32,
};

@group(0) @binding(0) var<storage, read>       scatter: array<f32>;
@group(0) @binding(1) var<storage, read_write> sink:    array<f32>;
@group(0) @binding(2) var<uniform>             params:  Params;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    let len = params.buffer_len;
    if (i >= arrayLength(&sink)) { return; }

    var s1: u32 = (i * 2654435761u) ^ params.seed;
    var s2: u32 = (i * 1597334677u) ^ (params.seed ^ 0x9e3779b9u);

    var x: f32 = f32(i & 0xffu) * 0.001 + 1.001;
    var y: f32 = 0.5;

    for (var k: u32 = 0u; k < params.inner_iters; k = k + 1u) {
        s1 = s1 * 1664525u + 1013904223u;
        let v1 = scatter[s1 % len];

        let t = x * 1.000001 + 0.0001;
        y = fma(t, v1, y);
        x = x * 0.99999 + 0.0001;

        if ((k & 3u) == 0u) {
            s2 = s2 * 22695477u + 1u;
            y = y + scatter[s2 % len] * 1e-9;
        }
    }

    sink[i] = x * 1e-9 + y * 1e-9;
}
"#;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    inner_iters: u32,
    buffer_len: u32,
    seed: u32,
    _pad: u32,
}

pub(crate) fn run(
    thread_count: usize,
    cancel: &Arc<AtomicBool>,
    tx: &mpsc::Sender<Metrics>,
    started_at: Instant,
) {
    // Two logical cores left free for the GPU submit thread and the tick loop.
    let cpu_threads = thread_count.saturating_sub(2).max(1);
    let cpu_bursts = Arc::new(AtomicU64::new(0));
    // Confirmed-complete GPU work units, not submits.
    let gpu_units = Arc::new(AtomicU64::new(0));
    let warn_slot: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let fatal_slot: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    let cpu_handles: Vec<_> = (0..cpu_threads)
        .map(|_| {
            let cancel = cancel.clone();
            let counter = cpu_bursts.clone();
            thread::Builder::new()
                .name("stress-kit-pulse-cpu".into())
                .spawn(move || fma_worker(cancel, counter))
                .expect("stress-kit: failed to spawn psu_transient cpu worker")
        })
        .collect();

    let gpu_handle = {
        let cancel = cancel.clone();
        let counter = gpu_units.clone();
        let warn = warn_slot.clone();
        let fatal = fatal_slot.clone();
        let tx = tx.clone();
        thread::Builder::new()
            .name("stress-kit-pulse-gpu".into())
            .spawn(move || gpu_driver(cancel, counter, warn, fatal, tx, started_at))
            .expect("stress-kit: failed to spawn psu_transient gpu driver")
    };

    let mut last_tick = Instant::now();
    let mut last_cpu: u64 = 0;
    let mut last_gpu: u64 = 0;

    while !cancel.load(Ordering::Relaxed) {
        thread::sleep(Duration::from_millis(50));
        if last_tick.elapsed() >= TICK {
            let cpu_now = cpu_bursts.load(Ordering::Relaxed);
            let gpu_now = gpu_units.load(Ordering::Relaxed);
            let dt = last_tick.elapsed().as_secs_f64().max(f64::EPSILON);

            let cpu_flops = cpu_now.saturating_sub(last_cpu) as f64 * CPU_FLOPS_PER_BURST as f64;
            let gpu_flops = gpu_now.saturating_sub(last_gpu) as f64
                * (INVOCATIONS_PER_SUBMIT * GPU_OPS_PER_INVOCATION) as f64;
            let gflops = (cpu_flops + gpu_flops) / dt / 1e9;

            // Every tick after the GPU leg dies repeats the fatal so a newest-only drain keeps it.
            match fatal_slot.lock().ok().and_then(|g| g.clone()) {
                Some(reason) => emit_latched_fatal(tx, started_at, gflops, reason),
                None => emit_tick(
                    tx,
                    started_at,
                    gflops,
                    warn_slot.lock().ok().and_then(|g| g.clone()),
                    0,
                ),
            }

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
        log::debug!(
            "[stress-kit/psu_transient] SetThreadPriority failed; running at default priority"
        );
    }
}

#[cfg(not(windows))]
fn raise_submit_priority() {}

fn gpu_driver(
    cancel: Arc<AtomicBool>,
    counter: Arc<AtomicU64>,
    warn: Arc<Mutex<Option<String>>>,
    fatal: Arc<Mutex<Option<String>>>,
    tx: mpsc::Sender<Metrics>,
    started_at: Instant,
) {
    let ctx = match GpuContext::acquire(true) {
        Ok(c) => c,
        Err(e) => {
            let msg = format!(
                "psu_transient: GPU unavailable ({e}); the pulsed +12V load never ran, \
                 so this is not a valid PSU transient test"
            );
            log::error!("[stress-kit/psu_transient] {msg}");
            set_slot(&warn, msg.clone());
            set_slot(&fatal, msg.clone());
            emit_fatal_tick(&tx, started_at, msg, 0);
            return;
        }
    };
    log::info!(
        "[stress-kit/psu_transient] GPU leg on {} ({}), target pulse {}ms on / {}ms off",
        ctx.vendor_label,
        ctx.backend_label,
        PULSE_ON.as_millis(),
        PULSE_OFF.as_millis()
    );
    raise_submit_priority();

    let scatter_data: Vec<f32> = (0..SCATTER_FLOATS)
        .map(|i| ((i as u32).wrapping_mul(2246822519)) as f32 * 1e-7 + 0.5)
        .collect();
    let scatter_buf = ctx
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("psu-transient scatter"),
            contents: bytemuck::cast_slice(&scatter_data),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        });
    drop(scatter_data);

    let sink_buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("psu-transient sink"),
        size: INVOCATIONS_PER_SUBMIT * 4,
        usage: wgpu::BufferUsages::STORAGE,
        mapped_at_creation: false,
    });

    let mut params = Params {
        inner_iters: INNER_ITERS,
        buffer_len: SCATTER_FLOATS as u32,
        seed: 0xF00D,
        _pad: 0,
    };
    let params_buf = ctx
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("psu-transient params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

    let module = ctx
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("psu-transient module"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
    let pipeline = ctx
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("psu-transient pipeline"),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
    let bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("psu-transient bind group"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: scatter_buf.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: sink_buf.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: params_buf.as_entire_binding() },
        ],
    });

    // Uncounted warm-up submit forces pipeline compilation before the first edge.
    let warmup = submit_unit(&ctx, &pipeline, &bind_group);
    let _ = drain_to(&ctx, warmup, &warn);
    if let Some(reason) = ctx.health.failure() {
        report_gpu_stop(&warn, &fatal, &tx, started_at, reason);
        return;
    }

    let unit_cost = calibrate_unit(&ctx, &pipeline, &bind_group, &warn);
    let inflight_cap = inflight_cap_for(unit_cost);
    log::info!(
        "[stress-kit/psu_transient] submit cost {:.2}ms, ~{} submits per burst, {} in flight",
        unit_cost.as_secs_f64() * 1e3,
        units_per_window(unit_cost),
        inflight_cap
    );
    if unit_cost >= PULSE_ON {
        report_pulse_degraded(&warn, unit_cost, None);
    }
    let mut degraded_measured = false;

    let mut pending: VecDeque<wgpu::SubmissionIndex> = VecDeque::with_capacity(inflight_cap + 1);
    let mut on_total = Duration::ZERO;
    let mut cycle_total = Duration::ZERO;
    let mut cycles: u64 = 0;
    let mut drain_failures: u32 = 0;

    while !cancel.load(Ordering::Relaxed) {
        let cycle_start = Instant::now();
        let on_until = cycle_start + PULSE_ON;
        let mut submits: u64 = 0;

        while !cancel.load(Ordering::Relaxed)
            && Instant::now() < on_until
            && drain_failures < MAX_DRAIN_FAILURES
        {
            params.seed = params.seed.wrapping_mul(1103515245).wrapping_add(12345);
            ctx.queue.write_buffer(&params_buf, 0, bytemuck::bytes_of(&params));

            pending.push_back(submit_unit(&ctx, &pipeline, &bind_group));
            submits += 1;

            match pop_at_cap(&mut pending, inflight_cap) {
                // The cap drain already waits on the oldest unit; its success confirms one unit.
                Some(oldest) => {
                    if drain_to(&ctx, oldest, &warn) {
                        counter.fetch_add(1, Ordering::Relaxed);
                        drain_failures = 0;
                    } else {
                        drain_failures += 1;
                    }
                }
                None => {
                    let _ = ctx.device.poll(wgpu::PollType::Poll);
                }
            }
        }

        // Queued work is drained before the idle window so the trailing edge steps down.
        let queued = pending.len() as u64;
        pending.clear();
        if check_poll(ctx.device.poll(wgpu::PollType::Wait), &warn) {
            // An empty queue confirms every unit still tracked at the boundary.
            counter.fetch_add(queued, Ordering::Relaxed);
            drain_failures = 0;
        } else {
            drain_failures += 1;
        }
        let on_actual = cycle_start.elapsed();

        if let Some(reason) = ctx.health.failure() {
            report_gpu_stop(&warn, &fatal, &tx, started_at, reason);
            return;
        }
        if drain_failures >= MAX_DRAIN_FAILURES {
            report_gpu_stop(
                &warn,
                &fatal,
                &tx,
                started_at,
                format!("queue stalled, {MAX_DRAIN_FAILURES} consecutive device-wait timeouts"),
            );
            return;
        }

        thread::sleep(PULSE_OFF);

        on_total += on_actual;
        cycle_total += cycle_start.elapsed();
        cycles += 1;
        if cycles % DUTY_LOG_CYCLES == 0 {
            let duty = on_total.as_secs_f64() / cycle_total.as_secs_f64().max(f64::EPSILON);
            log::debug!(
                "[stress-kit/psu_transient] measured duty cycle {:.1}% over {cycles} bursts, \
                 {submits} submits in the last burst",
                duty * 100.0
            );
            if duty > DUTY_CEILING && !degraded_measured {
                report_pulse_degraded(&warn, unit_cost, Some(duty));
                degraded_measured = true;
            }
        }
    }

    let _ = check_poll(ctx.device.poll(wgpu::PollType::Wait), &warn);
}

/// Encodes and submits one work unit.
fn submit_unit(
    ctx: &GpuContext,
    pipeline: &wgpu::ComputePipeline,
    bind_group: &wgpu::BindGroup,
) -> wgpu::SubmissionIndex {
    let mut encoder = ctx
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("psu-transient encoder"),
        });
    {
        let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("psu-transient pass"),
            timestamp_writes: None,
        });
        cpass.set_pipeline(pipeline);
        cpass.set_bind_group(0, bind_group, &[]);
        cpass.dispatch_workgroups(WG_PER_SUBMIT, 1, 1);
    }
    ctx.queue.submit(std::iter::once(encoder.finish()))
}

/// Pops the oldest queued submission once `cap` is exceeded.
fn pop_at_cap(
    pending: &mut VecDeque<wgpu::SubmissionIndex>,
    cap: usize,
) -> Option<wgpu::SubmissionIndex> {
    if pending.len() > cap {
        pending.pop_front()
    } else {
        None
    }
}

/// Waits for `index` to complete; `false` on a device-wait timeout.
fn drain_to(
    ctx: &GpuContext,
    index: wgpu::SubmissionIndex,
    warn: &Arc<Mutex<Option<String>>>,
) -> bool {
    check_poll(
        ctx.device.poll(wgpu::PollType::WaitForSubmissionIndex(index)),
        warn,
    )
}

/// Surfaces a device-wait timeout through `warn`; `PollError` has no `Display` here.
fn check_poll(
    result: Result<wgpu::PollStatus, wgpu::PollError>,
    warn: &Arc<Mutex<Option<String>>>,
) -> bool {
    match result {
        Ok(_) => true,
        Err(e) => {
            let msg =
                format!("psu_transient: GPU leg stopped responding, device wait timed out ({e:?})");
            log::warn!("[stress-kit/psu_transient] {msg}");
            set_slot(warn, msg);
            false
        }
    }
}

/// Fastest of `CALIBRATION_SAMPLES` timed single-unit submits.
fn calibrate_unit(
    ctx: &GpuContext,
    pipeline: &wgpu::ComputePipeline,
    bind_group: &wgpu::BindGroup,
    warn: &Arc<Mutex<Option<String>>>,
) -> Duration {
    let mut best = Duration::MAX;
    for _ in 0..CALIBRATION_SAMPLES {
        let started = Instant::now();
        let index = submit_unit(ctx, pipeline, bind_group);
        let _ = drain_to(ctx, index, warn);
        best = best.min(started.elapsed());
    }
    best
}

/// Submits that fit in one ON window at the measured unit cost.
fn units_per_window(unit: Duration) -> u64 {
    (PULSE_ON.as_secs_f64() / unit.as_secs_f64().max(1e-9)) as u64
}

/// In-flight cap that holds the post-burst drain to a fraction of the ON window.
fn inflight_cap_for(unit: Duration) -> usize {
    ((units_per_window(unit) / 4) as usize).clamp(1, MAX_INFLIGHT_SUBMITS)
}

/// Flags a pulse rate this card cannot honor: unit cost or measured duty too high.
/// Deliberately avoids the device-loss vocabulary the runner classifies on — a
/// card that is merely too slow to pulse is not a faulty card.
fn report_pulse_degraded(
    warn: &Arc<Mutex<Option<String>>>,
    unit: Duration,
    measured_duty: Option<f64>,
) {
    let measured = match measured_duty {
        Some(d) => format!("measured duty cycle {:.0}%", d * 100.0),
        None => "the burst cannot be subdivided".to_string(),
    };
    let msg = format!(
        "psu_transient: inconclusive - this GPU is too slow to pulse at this rate; one work \
         unit takes {:.0}ms against a {}ms on-window ({measured}), so the load is closer to \
         continuous than square-wave. Test-applicability limit, not a hardware fault.",
        unit.as_secs_f64() * 1e3,
        PULSE_ON.as_millis()
    );
    log::warn!("[stress-kit/psu_transient] {msg}");
    set_slot(warn, msg);
}

/// Ends the stage: the pulsed load stopped, so the rail was never fully loaded.
fn report_gpu_stop(
    warn: &Arc<Mutex<Option<String>>>,
    fatal: &Arc<Mutex<Option<String>>>,
    tx: &mpsc::Sender<Metrics>,
    started_at: Instant,
    reason: String,
) {
    let msg = format!(
        "psu_transient: GPU leg stopped ({reason}); the pulsed +12V load ended, \
         so this is not a valid PSU transient test"
    );
    log::error!("[stress-kit/psu_transient] {msg}");
    set_slot(warn, msg.clone());
    set_slot(fatal, msg.clone());
    emit_fatal_tick(tx, started_at, msg, 0);
}

/// Sends a fatal tick that keeps the measured throughput of the surviving load.
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

fn set_slot(slot: &Arc<Mutex<Option<String>>>, msg: String) {
    if let Ok(mut g) = slot.lock() {
        *g = Some(msg);
    }
}
