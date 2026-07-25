//! GPU VRAM write-verify pattern walker. Reports MiB/s; mismatches accumulate
//! in `Metrics::errors` with detail in `last_error`.
//!
//! The buffer covers the full requested footprint; dispatches are chunked so
//! the D3D12 65535-workgroup cap no longer truncates coverage. Device loss or
//! validation errors abort the stage with one fatal tick carrying the reason.

#![cfg(feature = "gpu")]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Instant;

use wgpu::util::DeviceExt;

use crate::Metrics;

use super::gpu_common::{
    emit_fatal_tick, emit_tick, run_unsupported, GpuContext, MAX_DISPATCH_GROUPS, TICK, WG_SIZE,
};

const MAX_CONSECUTIVE_READBACK_ERRORS: u32 = 3;
const MIN_BUFFER_BYTES: u64 = 16 * 1024 * 1024;
/// Elements one dispatch can cover (D3D12 workgroup cap × workgroup size).
const CHUNK_ELEMENTS: u64 = (MAX_DISPATCH_GROUPS as u64) * (WG_SIZE as u64);

const SHADER: &str = r#"
struct Params {
    len:    u32,
    seed:   u32,
    base:   u32,
    _pad:   u32,
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
    let i = params.base + gid.x;
    if (i >= params.len) { return; }
    buf[i] = pattern(i, params.seed);
}

@compute @workgroup_size(64)
fn verify_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = params.base + gid.x;
    if (i >= params.len) { return; }
    let expected = pattern(i, params.seed);
    if (buf[i] != expected) {
        atomicAdd(&errors[0], 1u);
    }
}
"#;

/// Buffer sizing honoring the device's storage-binding limit.
fn element_count(memory_cap_mb: u64, ctx: &GpuContext) -> (u64, u64) {
    let device_limits = ctx.device.limits();
    let max_bytes = (device_limits.max_storage_buffer_binding_size as u64)
        .min(device_limits.max_buffer_size);
    let cap_bytes = (memory_cap_mb.max(16) * 1024 * 1024)
        .clamp(MIN_BUFFER_BYTES, max_bytes);
    // u32 indexing in the shader.
    let elements = (cap_bytes / 4).min(u32::MAX as u64);
    (elements, elements * 4)
}

pub(crate) fn run(
    _thread_count: usize,
    memory_cap_mb: u64,
    cancel: &Arc<AtomicBool>,
    tx: &mpsc::Sender<Metrics>,
    started_at: Instant,
) {
    let ctx = match GpuContext::acquire(true) {
        Ok(c) => c,
        Err(e) => {
            return run_unsupported(
                "gpu_vram",
                "VRAM write-verify load",
                &e,
                cancel,
                tx,
                started_at,
            )
        }
    };

    let (elements, buffer_bytes) = element_count(memory_cap_mb, &ctx);
    let chunk_count = elements.div_ceil(CHUNK_ELEMENTS);

    log::info!(
        "[stress-kit/gpu_vram] acquired {} ({}), {} MiB VRAM in {} chunk(s)",
        ctx.vendor_label, ctx.backend_label,
        buffer_bytes / (1024 * 1024), chunk_count
    );

    let buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("gpu_vram buf"),
        size: buffer_bytes,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
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
    struct Params { len: u32, seed: u32, base: u32, _pad: u32 }

    // One uniform + bind group per chunk; the seed is rewritten each pass.
    let params_bufs: Vec<wgpu::Buffer> = (0..chunk_count)
        .map(|c| {
            ctx.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("gpu_vram params"),
                contents: bytemuck::bytes_of(&Params {
                    len: elements as u32,
                    seed: 0,
                    base: (c * CHUNK_ELEMENTS) as u32,
                    _pad: 0,
                }),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            })
        })
        .collect();

    let module = ctx.device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("gpu_vram module"),
        source: wgpu::ShaderSource::Wgsl(SHADER.into()),
    });

    // Explicit layout shared by both pipelines. Auto-derived (`layout: None`)
    // layouts prune bindings an entry point doesn't reference — write_main
    // never reads `errors`, so its derived layout had 2 bindings and the
    // 3-entry bind group failed validation on every device.
    let storage_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only: false },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    };
    let bind_layout = ctx.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("gpu_vram bind layout"),
        entries: &[
            storage_entry(0),
            storage_entry(1),
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    });
    let pipeline_layout = ctx.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("gpu_vram pipeline layout"),
        bind_group_layouts: &[&bind_layout],
        push_constant_ranges: &[],
    });

    let write_pipeline = ctx.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("gpu_vram write pipeline"),
        layout: Some(&pipeline_layout),
        module: &module,
        entry_point: Some("write_main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let verify_pipeline = ctx.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("gpu_vram verify pipeline"),
        layout: Some(&pipeline_layout),
        module: &module,
        entry_point: Some("verify_main"),
        compilation_options: Default::default(),
        cache: None,
    });

    let bind_groups: Vec<wgpu::BindGroup> = params_bufs
        .iter()
        .map(|params_buf| {
            ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("gpu_vram bind group"),
                layout: &bind_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: errors_buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: params_buf.as_entire_binding() },
                ],
            })
        })
        .collect();

    let mut last_tick = Instant::now();
    let mut bytes_touched_in_tick: u64 = 0;
    let mut total_errors_observed: u64 = 0;
    let mut consecutive_readback_errors: u32 = 0;
    let mut seed: u32 = 0xdeadbeef;

    while !cancel.load(Ordering::Relaxed) {
        seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
        for (c, params_buf) in params_bufs.iter().enumerate() {
            let params = Params {
                len: elements as u32,
                seed,
                base: (c as u64 * CHUNK_ELEMENTS) as u32,
                _pad: 0,
            };
            ctx.queue.write_buffer(params_buf, 0, bytemuck::bytes_of(&params));
        }
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
            for (c, bind_group) in bind_groups.iter().enumerate() {
                let chunk_elems = chunk_len(c as u64, elements);
                cpass.set_bind_group(0, bind_group, &[]);
                cpass.dispatch_workgroups(chunk_elems.div_ceil(WG_SIZE as u64) as u32, 1, 1);
            }
        }
        {
            let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("gpu_vram verify"),
                timestamp_writes: None,
            });
            cpass.set_pipeline(&verify_pipeline);
            for (c, bind_group) in bind_groups.iter().enumerate() {
                let chunk_elems = chunk_len(c as u64, elements);
                cpass.set_bind_group(0, bind_group, &[]);
                cpass.dispatch_workgroups(chunk_elems.div_ceil(WG_SIZE as u64) as u32, 1, 1);
            }
        }
        encoder.copy_buffer_to_buffer(&errors_buf, 0, &readback_buf, 0, 16);
        ctx.queue.submit(std::iter::once(encoder.finish()));
        let _ = ctx.device.poll(wgpu::PollType::Wait);

        // A failed device renders every readback meaningless — a stale map
        // can "succeed" with zeros and masquerade as a clean pass.
        if let Some(reason) = ctx.health.failure() {
            emit_fatal_tick(tx, started_at, format!("gpu_vram: {reason}"), total_errors_observed);
            return;
        }

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
                readback_buf.unmap();
                consecutive_readback_errors += 1;
                if consecutive_readback_errors >= MAX_CONSECUTIVE_READBACK_ERRORS {
                    let msg = format!(
                        "gpu_vram: {consecutive_readback_errors} consecutive readback failures; aborting stage"
                    );
                    log::error!("[stress-kit/gpu_vram] {msg}");
                    emit_fatal_tick(tx, started_at, msg, total_errors_observed);
                    return;
                }
                emit_tick(tx, started_at, 0.0, Some(format!(
                    "readback map failed ({consecutive_readback_errors}/{MAX_CONSECUTIVE_READBACK_ERRORS})"
                )), total_errors_observed);
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
            emit_tick(tx, started_at, mib_per_sec, err_msg, total_errors_observed);
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

/// Elements in chunk `c` of a buffer with `total` elements.
fn chunk_len(c: u64, total: u64) -> u64 {
    let start = c * CHUNK_ELEMENTS;
    (total - start).min(CHUNK_ELEMENTS)
}
