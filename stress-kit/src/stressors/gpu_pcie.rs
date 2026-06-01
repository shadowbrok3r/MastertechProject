//! CPU↔GPU upload-touch-download round-trip. Reports GB/s.

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
const MIN_BUFFER_BYTES: u64 = 4 * 1024 * 1024;

const TOUCH_SHADER: &str = r#"
struct Params { len: u32, _p0: u32, _p1: u32, _p2: u32 };

@group(0) @binding(0) var<storage, read_write> buf:    array<u32>;
@group(0) @binding(1) var<uniform>             params: Params;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if (gid.x >= params.len) { return; }
    buf[gid.x] = buf[gid.x] ^ 0x5a5a5a5au;
}
"#;

fn capped_element_count(memory_cap_mb: u64) -> (u64, u64) {
    let cap_bytes = (memory_cap_mb.max(4).min(256)) * 1024 * 1024;
    let buffer_bytes = cap_bytes.max(MIN_BUFFER_BYTES);
    let max_elements = (MAX_DISPATCH_GROUPS as u64) * (WG_SIZE as u64);
    let wg = WG_SIZE as u64;
    let elements = ((buffer_bytes / 4) / wg) * wg;
    let elements = elements.min(max_elements);
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
        Err(e) => return run_unsupported(format!("gpu_pcie acquire failed: {e}"), cancel, tx, started_at),
    };

    let (elements, buffer_bytes) = capped_element_count(memory_cap_mb);
    let groups = (elements as u32).div_ceil(WG_SIZE);

    log::info!(
        "[stress-kit/gpu_pcie] acquired {} ({}), {} MiB round-trip buffer ({} workgroups)",
        ctx.vendor_label, ctx.backend_label, buffer_bytes / (1024 * 1024), groups
    );

    let staging_upload: Vec<u32> = (0..elements as usize)
        .map(|i| (i as u32).wrapping_mul(2654435761))
        .collect();

    let gpu_buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("gpu_pcie buf"),
        size: buffer_bytes,
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_DST
            | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback_buf = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("gpu_pcie readback"),
        size: buffer_bytes,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    #[repr(C)]
    #[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
    struct Params { len: u32, _p0: u32, _p1: u32, _p2: u32 }
    let params_buf = ctx.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("gpu_pcie params"),
        contents: bytemuck::bytes_of(&Params { len: elements as u32, _p0: 0, _p1: 0, _p2: 0 }),
        usage: wgpu::BufferUsages::UNIFORM,
    });

    let module = ctx.device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("gpu_pcie touch module"),
        source: wgpu::ShaderSource::Wgsl(TOUCH_SHADER.into()),
    });
    let pipeline = ctx.device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("gpu_pcie touch pipeline"),
        layout: None,
        module: &module,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let bind_group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("gpu_pcie bind group"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: gpu_buf.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: params_buf.as_entire_binding() },
        ],
    });

    let mut last_tick = Instant::now();
    let mut bytes_in_tick: u64 = 0;
    let mut consecutive_readback_errors: u32 = 0;

    while !cancel.load(Ordering::Relaxed) {
        ctx.queue.write_buffer(&gpu_buf, 0, bytemuck::cast_slice(&staging_upload));

        let mut encoder = ctx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("gpu_pcie encoder"),
        });
        {
            let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("gpu_pcie touch"),
                timestamp_writes: None,
            });
            cpass.set_pipeline(&pipeline);
            cpass.set_bind_group(0, &bind_group, &[]);
            cpass.dispatch_workgroups(groups, 1, 1);
        }
        encoder.copy_buffer_to_buffer(&gpu_buf, 0, &readback_buf, 0, buffer_bytes);
        ctx.queue.submit(std::iter::once(encoder.finish()));
        let _ = ctx.device.poll(wgpu::PollType::Wait);

        let slice = readback_buf.slice(..);
        let (tx_map, rx_map) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |res| {
            let _ = tx_map.send(res);
        });
        let _ = ctx.device.poll(wgpu::PollType::Wait);
        match rx_map.recv() {
            Ok(Ok(())) => {
                let view = slice.get_mapped_range();
                std::hint::black_box(view[0]);
                drop(view);
                readback_buf.unmap();
                consecutive_readback_errors = 0;
            }
            _ => {
                readback_buf.unmap();
                consecutive_readback_errors += 1;
                if consecutive_readback_errors >= MAX_CONSECUTIVE_READBACK_ERRORS {
                    let msg = format!(
                        "gpu_pcie: {consecutive_readback_errors} consecutive readback failures; aborting stage"
                    );
                    log::error!("[stress-kit/gpu_pcie] {msg}");
                    emit_fatal_tick(tx, started_at, msg);
                    return;
                }
                emit_tick(tx, started_at, 0.0, Some(format!(
                    "readback map failed ({consecutive_readback_errors}/{MAX_CONSECUTIVE_READBACK_ERRORS})"
                )));
                continue;
            }
        }

        bytes_in_tick += buffer_bytes * 2;

        if last_tick.elapsed() >= TICK {
            let dt = last_tick.elapsed().as_secs_f64().max(f64::EPSILON);
            let gb_per_sec = (bytes_in_tick as f64) / dt / 1e9;
            emit_tick(tx, started_at, gb_per_sec, None);
            last_tick = Instant::now();
            bytes_in_tick = 0;
        }
    }

    log::debug!("[stress-kit/gpu_pcie] cancellation received, exiting");
}
