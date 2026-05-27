//! GPU VRAM write-verify pattern walker. Reports MiB/s; surfaces mismatches via `last_error`.

#![cfg(feature = "gpu")]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Instant;

use wgpu::util::DeviceExt;

use crate::Metrics;

use super::gpu_common::{emit_fatal_tick, emit_tick, run_unsupported, GpuContext, TICK};

/// Bail out after this many consecutive readback failures. See the same
/// constant in `gpu_pcie.rs` for the rationale.
const MAX_CONSECUTIVE_READBACK_ERRORS: u32 = 3;

const WG_SIZE: u32 = 64;
const MIN_BUFFER_BYTES: u64 = 16 * 1024 * 1024;

const SHADER: &str = r#"
struct Params {
    len:    u32,   // number of u32 elements
    seed:   u32,
    _pad0:  u32,
    _pad1:  u32,
};

@group(0) @binding(0) var<storage, read_write> buf:     array<u32>;
@group(0) @binding(1) var<storage, read_write> errors:  array<atomic<u32>>;
@group(0) @binding(2) var<uniform>             params:  Params;

fn pattern(i: u32, seed: u32) -> u32 {
    var x: u32 = (i ^ seed) ^ ((i << 13u) | (i >> 19u));
    x = x ^ (x << 7u);
    x = x ^ (x >> 9u);
    x = x * 2246822519u + 1u;
    return x;
}

@compute @workgroup_size(64)
fn write_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= params.len) { return; }
    buf[i] = pattern(i, params.seed);
}

@compute @workgroup_size(64)
fn verify_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= params.len) { return; }
    let expected = pattern(i, params.seed);
    let actual = buf[i];
    if (actual != expected) {
        atomicAdd(&errors[0], 1u);
    }
}
"#;

pub(crate) fn run(
    _thread_count: usize,
    memory_cap_mb: u64,
    cancel: &Arc<AtomicBool>,
    tx: &mpsc::Sender<Metrics>,
    started_at: Instant,
) {
    let ctx = match GpuContext::acquire(true) {
        Ok(c) => c,
        Err(e) => return run_unsupported(format!("gpu_vram acquire failed: {e}"), cancel, tx, started_at),
    };

    let cap_bytes = (memory_cap_mb.max(16)) * 1024 * 1024;
    let buffer_bytes = cap_bytes.max(MIN_BUFFER_BYTES);
    let element_bytes = 4u64;
    let elements = ((buffer_bytes / element_bytes) / (WG_SIZE as u64)) * (WG_SIZE as u64);
    let buffer_bytes = elements * element_bytes;

    log::info!(
        "[stress-kit/gpu_vram] acquired {} ({}), allocating {} MiB VRAM ({} elements)",
        ctx.vendor_label, ctx.backend_label,
        buffer_bytes / (1024 * 1024), elements
    );

    let buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("gpu_vram buf"),
        size: buffer_bytes,
        usage: wgpu::BufferUsages::STORAGE,
        mapped_at_creation: false,
    });

    let errors_buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("gpu_vram errors"),
        size: 16,
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_SRC
            | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let readback_buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("gpu_vram readback"),
        size: 16,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    #[repr(C)]
    #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
    struct Params { len: u32, seed: u32, _pad0: u32, _pad1: u32 }
    let mut params = Params { len: elements as u32, seed: 0xdeadbeef, _pad0: 0, _pad1: 0 };
    let params_buf = ctx.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("gpu_vram params"),
        contents: bytemuck::bytes_of(&params),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });

    let module = ctx.device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("gpu_vram module"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });
    let write_pipeline = ctx.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("gpu_vram write pipeline"),
        layout: None,
        module: &module,
        entry_point: Some("write_main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let verify_pipeline = ctx.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("gpu_vram verify pipeline"),
        layout: None,
        module: &module,
        entry_point: Some("verify_main"),
        compilation_options: Default::default(),
        cache: None,
    });

    let bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("gpu_vram bind group"),
        layout: &write_pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: buf.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: errors_buf.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: params_buf.as_entire_binding() },
        ],
    });

    let groups = (elements as u32).div_ceil(WG_SIZE);

    let mut last_tick = Instant::now();
    let mut bytes_touched_in_tick: u64 = 0;
    let mut total_errors_observed: u64 = 0;
    let mut consecutive_readback_errors: u32 = 0;

    while !cancel.load(Ordering::Relaxed) {
        params.seed = params.seed.wrapping_mul(1103515245).wrapping_add(12345);
        ctx.queue.write_buffer(&params_buf, 0, bytemuck::bytes_of(&params));
        ctx.queue.write_buffer(&errors_buf, 0, bytemuck::bytes_of(&[0u32; 4]));

        let mut encoder = ctx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("gpu_vram encoder"),
        });
        {
            let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("gpu_vram write"),
                timestamp_writes: None,
            });
            cpass.set_pipeline(&write_pipeline);
            cpass.set_bind_group(0, &bind_group, &[]);
            cpass.dispatch_workgroups(groups, 1, 1);
        }
        {
            let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("gpu_vram verify"),
                timestamp_writes: None,
            });
            cpass.set_pipeline(&verify_pipeline);
            cpass.set_bind_group(0, &bind_group, &[]);
            cpass.dispatch_workgroups(groups, 1, 1);
        }
        encoder.copy_buffer_to_buffer(&errors_buf, 0, &readback_buf, 0, 16);
        ctx.queue.submit(std::iter::once(encoder.finish()));

        let slice = readback_buf.slice(..);
        let (tx_map, rx_map) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |res| {
            let _ = tx_map.send(res);
        });
        let _ = ctx.device.poll(wgpu::PollType::Wait);
        let err_count = match rx_map.recv() {
            Ok(Ok(())) => {
                let view = slice.get_mapped_range();
                let count = u32::from_le_bytes([view[0], view[1], view[2], view[3]]) as u64;
                drop(view);
                readback_buf.unmap();
                consecutive_readback_errors = 0;
                count
            }
            _ => {
                // map_async arms map_context.initial_range before the callback
                // fires; unmap() must reset it on failure or the next iteration
                // panics with "Buffer is already mapped".
                readback_buf.unmap();
                consecutive_readback_errors += 1;
                if consecutive_readback_errors >= MAX_CONSECUTIVE_READBACK_ERRORS {
                    let msg = format!(
                        "gpu_vram: {consecutive_readback_errors} consecutive readback failures; aborting stage"
                    );
                    log::error!("[stress-kit/gpu_vram] {msg}");
                    emit_fatal_tick(tx, started_at, msg);
                    return;
                }
                emit_tick(tx, started_at, 0.0, Some(format!(
                    "readback map failed ({consecutive_readback_errors}/{MAX_CONSECUTIVE_READBACK_ERRORS})"
                )));
                continue;
            }
        };

        total_errors_observed += err_count;
        bytes_touched_in_tick += buffer_bytes * 2;

        if last_tick.elapsed() >= TICK {
            let dt = last_tick.elapsed().as_secs_f64().max(f64::EPSILON);
            let mib_per_sec = (bytes_touched_in_tick as f64) / dt / (1024.0 * 1024.0);
            let err_msg = if total_errors_observed > 0 {
                Some(format!(
                    "{} VRAM mismatches detected (cumulative); this is a hardware fault",
                    total_errors_observed
                ))
            } else {
                None
            };
            emit_tick(tx, started_at, mib_per_sec, err_msg);
            last_tick = Instant::now();
            bytes_touched_in_tick = 0;
        }
    }

    if total_errors_observed > 0 {
        log::error!(
            "[stress-kit/gpu_vram] final error count: {} — card has bad VRAM cells",
            total_errors_observed
        );
    }
    log::debug!("[stress-kit/gpu_vram] cancellation received, exiting");
}
