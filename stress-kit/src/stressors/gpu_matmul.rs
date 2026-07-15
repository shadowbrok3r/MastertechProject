//! Repeated NxN fp32 matmul on the GPU. Reports GFLOPS via `2 * N^3` per matmul.
//! Sampled output rows are read back and checked against a CPU reference;
//! mismatches accumulate in `Metrics::errors`.

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

// Output rows checked against the CPU reference, one per matmul, cycled.
const VERIFY_ROWS: usize = 8;
// Per-element mismatch tolerance: |got - ref| > ABS_TOL + REL_TOL * |ref|.
const REL_TOL: f32 = 1e-2;
const ABS_TOL: f32 = 1e-3;

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

    let a_at = |i: usize| ((i as u32).wrapping_mul(2654435761) as f32) * 1e-10 + 0.001;
    let b_at = |i: usize| ((i as u32).wrapping_mul(1597334677) as f32) * 1e-10 + 0.001;

    let mut a_data = vec![0f32; n_f32];
    let mut b_data = vec![0f32; n_f32];
    for i in 0..n_f32 {
        a_data[i] = a_at(i);
        b_data[i] = b_at(i);
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
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });

    let row_bytes = (N as u64) * 4;
    let readback_buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("matmul verify readback"),
        size: row_bytes,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    // CPU reference for each sampled output row.
    let nu = N as usize;
    let row_stride = nu / VERIFY_ROWS;
    let references: Vec<Vec<f32>> = (0..VERIFY_ROWS)
        .map(|s| {
            let row = s * row_stride;
            (0..nu)
                .map(|col| {
                    let mut acc = 0f32;
                    for k in 0..nu {
                        acc = a_at(row * nu + k).mul_add(b_at(k * nu + col), acc);
                    }
                    acc
                })
                .collect()
        })
        .collect();

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
    let mut matmul_count: u64 = 0;
    let mut total_errors: u64 = 0;
    let mut logged_mismatch = false;

    while !cancel.load(Ordering::Relaxed) {
        let sample = (matmul_count % VERIFY_ROWS as u64) as usize;
        let verify_row = (sample * row_stride) as u64;

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
        encoder.copy_buffer_to_buffer(&c_buf, verify_row * row_bytes, &readback_buf, 0, row_bytes);
        ctx.queue.submit(std::iter::once(encoder.finish()));
        let _ = ctx.device.poll(wgpu::PollType::Wait);
        if let Some(reason) = ctx.health.failure() {
            emit_fatal_tick(tx, started_at, format!("gpu_matmul: {reason}"), total_errors);
            return;
        }
        matmuls_in_tick += 1;
        matmul_count += 1;

        // Read back the sampled row and compare against the reference.
        let slice = readback_buf.slice(..);
        let (map_tx, map_rx) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |res| {
            let _ = map_tx.send(res);
        });
        let _ = ctx.device.poll(wgpu::PollType::Wait);
        if let Ok(Ok(())) = map_rx.recv() {
            let view = slice.get_mapped_range();
            let got: &[f32] = bytemuck::cast_slice(&view[..]);
            let reference = &references[sample];
            let mut first_bad: Option<(usize, f32, f32)> = None;
            for col in 0..nu {
                let g = got[col];
                let r = reference[col];
                if !g.is_finite() || (g - r).abs() > ABS_TOL + REL_TOL * r.abs() {
                    total_errors += 1;
                    first_bad.get_or_insert((col, g, r));
                }
            }
            drop(view);
            readback_buf.unmap();

            if !logged_mismatch {
                if let Some((col, g, r)) = first_bad {
                    let row = sample * row_stride;
                    log::error!(
                        "[stress-kit/gpu_matmul] result mismatch at C[{row}][{col}]: got {g}, expected {r} — GPU compute fault"
                    );
                    logged_mismatch = true;
                }
            }
        } else {
            readback_buf.unmap();
        }

        if last_tick.elapsed() >= TICK {
            let dt = last_tick.elapsed().as_secs_f64().max(f64::EPSILON);
            let total_ops = matmuls_in_tick * OPS_PER_MATMUL;
            let gflops = (total_ops as f64) / dt / 1e9;
            let err_msg = if total_errors > 0 {
                Some(format!(
                    "{total_errors} matmul result mismatch(es) (cumulative); GPU is computing wrong arithmetic — hardware fault"
                ))
            } else {
                None
            };
            emit_tick(tx, started_at, gflops, err_msg, total_errors);
            last_tick = Instant::now();
            matmuls_in_tick = 0;
        }
    }

    if total_errors > 0 {
        log::error!(
            "[stress-kit/gpu_matmul] final mismatch count: {total_errors} — GPU compute is unreliable under load"
        );
    }
    log::debug!("[stress-kit/gpu_matmul] cancellation received, exiting");
}
