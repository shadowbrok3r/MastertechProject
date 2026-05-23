//! General GPU compute stressor.
//!
//! WGSL kernel hammers two things every workgroup:
//!   1. A long chain of FMA-ish floating-point math (`fma(x, c, y)` style) —
//!      keeps the shader cores busy on the FP ALUs.
//!   2. A scattered read from a sizeable storage buffer — keeps the memory
//!      subsystem busy and forces cache misses so the GPU's L2 and VRAM
//!      controllers are actually doing work.
//!
//! Reports GFLOPS computed from a known op count per invocation × workgroup
//! count × dispatches per tick.
//!
//! Why this and not FurMark: FurMark stresses *one* dimension (power draw via
//! shader load) at insane levels that some BIOS profiles actively throttle.
//! This is meant for diagnostic triage — exercise the realistic gaming path
//! (math + memory together), not abuse the power limiter.

#![cfg(feature = "gpu")]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Instant;

use wgpu::util::DeviceExt;

use crate::Metrics;

use super::gpu_common::{emit_tick, run_unsupported, GpuContext, TICK};

/// Compute shader. One invocation does `INNER_ITERS` FMA-style ops on a
/// register-resident accumulator, plus one scattered storage-buffer read.
///
/// Op count per invocation (for GFLOPS math): `INNER_ITERS * 4` (each loop body
/// does 1 mul + 1 add + 1 fma + 1 multiply-into-x = 4 fp32 ops). Scattered
/// reads aren't counted as "flops" — they're throughput overhead.
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

const WG_SIZE: u32 = 64u;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i  = gid.x;
    let len = params.buffer_len;
    if (i >= arrayLength(&sink)) { return; }

    // Two seed streams so each invocation reads different addresses.
    var s1: u32 = (i * 2654435761u) ^ params.seed;
    var s2: u32 = (i * 1597334677u) ^ (params.seed ^ 0x9e3779b9u);

    var x: f32 = f32(i & 0xffu) * 0.001 + 1.001;
    var y: f32 = 0.5;

    for (var k: u32 = 0u; k < params.inner_iters; k = k + 1u) {
        // Scattered load
        s1 = s1 * 1664525u + 1013904223u;
        let idx1 = s1 % len;
        let v1 = scatter[idx1];

        // Math: 4 fp32 ops (mul, add, fma, mul-into-x)
        let t = x * 1.000001 + 0.0001;
        y = fma(t, v1, y);
        x = x * 0.99999 + 0.0001;

        // Second scatter every 4 iterations to keep cache cold
        if ((k & 3u) == 0u) {
            s2 = s2 * 22695477u + 1u;
            let idx2 = s2 % len;
            y = y + scatter[idx2] * 1e-9;
        }
    }

    // Keep the optimizer from killing the loop. One store per invocation —
    // negligible bandwidth, but mandatory for the shader to have side effects.
    sink[i] = x * 1e-9 + y * 1e-9;
}
"#;

// Workgroup config: WG_SIZE in the shader × WG_COUNT here = invocations per dispatch.
const WG_SIZE: u32 = 64;
const WG_COUNT: u32 = 4096;
const INVOCATIONS_PER_DISPATCH: u64 = (WG_SIZE as u64) * (WG_COUNT as u64);
const INNER_ITERS: u32 = 1024;
/// FP ops per invocation: 4 in the main loop body × INNER_ITERS, plus the
/// `+ scatter[idx2] * 1e-9` adds ≈ 2 more ops every 4 iters (averaged: 0.5/iter).
/// Round to 4.5 to keep the math honest; integers preferred for u64 arithmetic.
const OPS_PER_INVOCATION: u64 = (INNER_ITERS as u64) * 4 + (INNER_ITERS as u64) / 2;

/// Scatter buffer size — 64 MiB worth of fp32. Large enough to defeat L2 on
/// every consumer card; small enough to fit on a 4 GB iGPU partition.
const SCATTER_FLOATS: usize = (64 * 1024 * 1024) / std::mem::size_of::<f32>();

pub(crate) fn run(
    _thread_count: usize,
    cancel: &Arc<AtomicBool>,
    tx: &mpsc::Sender<Metrics>,
    started_at: Instant,
) {
    let ctx = match GpuContext::acquire(true) {
        Ok(c) => c,
        Err(e) => return run_unsupported(format!("gpu acquire failed: {e}"), cancel, tx, started_at),
    };
    log::info!(
        "[stress-kit/gpu] acquired {} on {} backend",
        ctx.vendor_label, ctx.backend_label
    );

    // ── Resources ──────────────────────────────────────────────────────────
    let scatter_data: Vec<f32> = (0..SCATTER_FLOATS)
        .map(|i| ((i as u32).wrapping_mul(2246822519)) as f32 * 1e-7 + 0.5)
        .collect();
    let scatter_buf = ctx
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("gpu-stressor scatter"),
            contents: bytemuck::cast_slice(&scatter_data),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        });
    drop(scatter_data); // Don't keep the CPU-side copy alive.

    let sink_buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("gpu-stressor sink"),
        size: (INVOCATIONS_PER_DISPATCH as u64) * 4,
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
    let mut params = Params {
        inner_iters: INNER_ITERS,
        buffer_len: SCATTER_FLOATS as u32,
        seed: 0xc0ffee,
        _pad: 0,
    };
    let params_buf = ctx
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("gpu-stressor params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

    let module = ctx
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gpu-stressor module"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
    let pipeline = ctx
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("gpu-stressor pipeline"),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });

    let bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("gpu-stressor bind group"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: scatter_buf.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: sink_buf.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: params_buf.as_entire_binding() },
        ],
    });

    // ── Run loop ───────────────────────────────────────────────────────────
    let mut last_tick = Instant::now();
    let mut dispatches_in_tick: u64 = 0;

    while !cancel.load(Ordering::Relaxed) {
        // Rotate the seed each dispatch so the compiler can't cache loads.
        params.seed = params.seed.wrapping_mul(1103515245).wrapping_add(12345);
        ctx.queue.write_buffer(&params_buf, 0, bytemuck::bytes_of(&params));

        let mut encoder = ctx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("gpu-stressor encoder"),
            });
        {
            let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("gpu-stressor pass"),
                timestamp_writes: None,
            });
            cpass.set_pipeline(&pipeline);
            cpass.set_bind_group(0, &bind_group, &[]);
            cpass.dispatch_workgroups(WG_COUNT, 1, 1);
        }
        ctx.queue.submit(std::iter::once(encoder.finish()));
        // Wait for completion — keeps the work synchronous, makes cancel
        // responsive, and lets us count dispatches per tick deterministically.
        let _ = ctx.device.poll(wgpu::PollType::Wait);
        dispatches_in_tick += 1;

        if last_tick.elapsed() >= TICK {
            let dt = last_tick.elapsed().as_secs_f64().max(f64::EPSILON);
            let invocations = INVOCATIONS_PER_DISPATCH * dispatches_in_tick;
            let total_ops = invocations * OPS_PER_INVOCATION;
            let gflops = (total_ops as f64) / dt / 1e9;
            emit_tick(tx, started_at, gflops, None);
            last_tick = Instant::now();
            dispatches_in_tick = 0;
        }
    }

    log::debug!("[stress-kit/gpu] cancellation received, exiting");
}
