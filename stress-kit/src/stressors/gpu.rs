//! Mixed FMA + scattered-load compute shader. Reports GFLOPS.

#![cfg(feature = "gpu")]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Instant;

use wgpu::util::DeviceExt;

use crate::Metrics;

use super::gpu_common::{emit_fatal_tick, emit_tick, run_unsupported, GpuContext, TICK};

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

const WG_SIZE: u32 = 64;
const WG_COUNT: u32 = 4096;
const INVOCATIONS_PER_DISPATCH: u64 = (WG_SIZE as u64) * (WG_COUNT as u64);
const INNER_ITERS: u32 = 1024;
// 6 flops per inner iteration, plus 2 more on every 4th iteration.
const OPS_PER_INVOCATION: u64 = (INNER_ITERS as u64) * 6 + (INNER_ITERS as u64) / 2;
const SCATTER_FLOATS: usize = (64 * 1024 * 1024) / std::mem::size_of::<f32>();
/// Consecutive device-wait timeouts tolerated before the run is declared stalled.
const MAX_WAIT_FAILURES: u32 = 3;

pub(crate) fn run(
    _thread_count: usize,
    cancel: &Arc<AtomicBool>,
    tx: &mpsc::Sender<Metrics>,
    started_at: Instant,
) {
    let ctx = match GpuContext::acquire(true) {
        Ok(c) => c,
        Err(e) => {
            return run_unsupported("gpu", "GPU compute load", &e, cancel, tx, started_at)
        }
    };
    log::info!(
        "[stress-kit/gpu] acquired {} on {} backend",
        ctx.vendor_label, ctx.backend_label
    );

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
    drop(scatter_data);

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

    let mut last_tick = Instant::now();
    // Confirmed-complete dispatches only; a timed-out wait is not work done.
    let mut dispatches_in_tick: u64 = 0;
    let mut wait_failures: u32 = 0;
    let mut warn: Option<String> = None;

    while !cancel.load(Ordering::Relaxed) {
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
        // A wait timeout is neither an uncaptured error nor device-lost, so it has
        // to be counted here or a hung device looks healthy.
        match ctx.device.poll(wgpu::PollType::Wait) {
            Ok(_) => {
                dispatches_in_tick += 1;
                wait_failures = 0;
            }
            Err(e) => {
                wait_failures += 1;
                let msg = format!("gpu: device wait timed out ({e:?}) x{wait_failures}");
                log::warn!("[stress-kit/gpu] {msg}");
                warn = Some(msg);
            }
        }
        if let Some(reason) = ctx.health.failure() {
            emit_fatal_tick(tx, started_at, format!("gpu: {reason}"), 0);
            return;
        }
        if wait_failures >= MAX_WAIT_FAILURES {
            emit_fatal_tick(
                tx,
                started_at,
                format!(
                    "gpu: inconclusive - queue stalled, {MAX_WAIT_FAILURES} consecutive \
                     device-wait timeouts; the compute load is not completing"
                ),
                0,
            );
            return;
        }

        if last_tick.elapsed() >= TICK {
            let dt = last_tick.elapsed().as_secs_f64().max(f64::EPSILON);
            let invocations = INVOCATIONS_PER_DISPATCH * dispatches_in_tick;
            let total_ops = invocations * OPS_PER_INVOCATION;
            let gflops = (total_ops as f64) / dt / 1e9;
            emit_tick(tx, started_at, gflops, warn.take(), 0);
            last_tick = Instant::now();
            dispatches_in_tick = 0;
        }
    }

    log::debug!("[stress-kit/gpu] cancellation received, exiting");
}
