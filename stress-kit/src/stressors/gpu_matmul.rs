//! Repeated NxN fp32 matmul on the GPU. Reports GFLOPS via `2 * N^3` per matmul.

#![cfg(feature = "gpu")]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Instant;

use wgpu::util::DeviceExt;

use crate::Metrics;

use super::gpu_common::{emit_fatal_tick, emit_tick, run_unsupported, GpuContext, TICK};

const N: u32 = 2048;
const TILE: u32 = 16;
const OPS_PER_MATMUL: u64 = 2 * (N as u64) * (N as u64) * (N as u64);

const SHADER: &str = r#"
struct Dims { n: u32, _pad0: u32, _pad1: u32, _pad2: u32 };

@group(0) @binding(0) var<storage, read>       a:    array<f32>;
@group(0) @binding(1) var<storage, read>       b:    array<f32>;
@group(0) @binding(2) var<storage, read_write> c:    array<f32>;
@group(0) @binding(3) var<uniform>             dims: Dims;

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let n = dims.n;
    let row = gid.y;
    let col = gid.x;
    if (row >= n || col >= n) { return; }

    var acc: f32 = 0.0;
    for (var k: u32 = 0u; k < n; k = k + 1u) {
        acc = fma(a[row * n + k], b[k * n + col], acc);
    }
    c[row * n + col] = acc;
}
"#;

pub(crate) fn run(
    _thread_count: usize,
    cancel: &Arc<AtomicBool>,
    tx: &mpsc::Sender<Metrics>,
    started_at: Instant,
) {
    let ctx = match GpuContext::acquire(true) {
        Ok(c) => c,
        Err(e) => return run_unsupported(format!("gpu_matmul acquire failed: {e}"), cancel, tx, started_at),
    };
    log::info!(
        "[stress-kit/gpu_matmul] acquired {} on {} backend, N={N}",
        ctx.vendor_label, ctx.backend_label
    );

    let n_f32 = (N as usize) * (N as usize);
    let bytes = (n_f32 * std::mem::size_of::<f32>()) as u64;

    let mut a_data = vec![0f32; n_f32];
    let mut b_data = vec![0f32; n_f32];
    for i in 0..n_f32 {
        a_data[i] = ((i as u32).wrapping_mul(2654435761) as f32) * 1e-10 + 0.001;
        b_data[i] = ((i as u32).wrapping_mul(1597334677) as f32) * 1e-10 + 0.001;
    }

    let a_buf = ctx.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("matmul A"),
        contents: bytemuck::cast_slice(&a_data),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let b_buf = ctx.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("matmul B"),
        contents: bytemuck::cast_slice(&b_data),
        usage: wgpu::BufferUsages::STORAGE,
    });
    drop(a_data);
    drop(b_data);
    let c_buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("matmul C"),
        size: bytes,
        usage: wgpu::BufferUsages::STORAGE,
        mapped_at_creation: false,
    });

    #[repr(C)]
    #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
    struct Dims { n: u32, _pad0: u32, _pad1: u32, _pad2: u32 }
    let dims_buf = ctx.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("matmul dims"),
        contents: bytemuck::bytes_of(&Dims { n: N, _pad0: 0, _pad1: 0, _pad2: 0 }),
        usage: wgpu::BufferUsages::UNIFORM,
    });

    let module = ctx.device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("matmul module"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let pipeline = ctx.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("matmul pipeline"),
        layout: None,
        module: &module,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("matmul bind group"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: a_buf.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: b_buf.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: c_buf.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 3, resource: dims_buf.as_entire_binding() },
        ],
    });

    let groups = N.div_ceil(TILE);
    let mut last_tick = Instant::now();
    let mut matmuls_in_tick: u64 = 0;

    while !cancel.load(Ordering::Relaxed) {
        let mut encoder = ctx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("matmul encoder"),
        });
        {
            let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("matmul pass"),
                timestamp_writes: None,
            });
            cpass.set_pipeline(&pipeline);
            cpass.set_bind_group(0, &bind_group, &[]);
            cpass.dispatch_workgroups(groups, groups, 1);
        }
        ctx.queue.submit(std::iter::once(encoder.finish()));
        let _ = ctx.device.poll(wgpu::PollType::Wait);
        if let Some(reason) = ctx.health.failure() {
            emit_fatal_tick(tx, started_at, format!("gpu_matmul: {reason}"), 0);
            return;
        }
        matmuls_in_tick += 1;

        if last_tick.elapsed() >= TICK {
            let dt = last_tick.elapsed().as_secs_f64().max(f64::EPSILON);
            let total_ops = matmuls_in_tick * OPS_PER_MATMUL;
            let gflops = (total_ops as f64) / dt / 1e9;
            emit_tick(tx, started_at, gflops, None, 0);
            last_tick = Instant::now();
            matmuls_in_tick = 0;
        }
    }

    log::debug!("[stress-kit/gpu_matmul] cancellation received, exiting");
}
