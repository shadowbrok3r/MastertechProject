//! CPU↔GPU upload-touch-download round-trip. Reports GB/s; every word of the
//! readback is verified against `upload ^ seed`, with mismatches accumulating
//! in `Metrics::errors`. The XOR seed changes each pass so the downstream
//! transfer pattern varies without re-generating the upload buffer.

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
/// Elements one dispatch can cover (D3D12 workgroup cap × workgroup size).
const CHUNK_ELEMENTS: u64 = (MAX_DISPATCH_GROUPS as u64) * (WG_SIZE as u64);

const TOUCH_SHADER: &str = r#"
struct Params { len: u32, base: u32, seed: u32, _p2: u32 };

@group(0) @binding(0) var<storage, read_write> buf:    array<u32>;
@group(0) @binding(1) var<uniform>             params: Params;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = params.base + gid.x;
    if (i >= params.len) { return; }
    buf[i] = buf[i] ^ params.seed;
}
"#;

fn capped_element_count(memory_cap_mb: u64) -> (u64, u64) {
    let cap_bytes = (memory_cap_mb.max(4).min(256)) * 1024 * 1024;
    let buffer_bytes = cap_bytes.max(MIN_BUFFER_BYTES);
    let elements = buffer_bytes / 4;
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
                "gpu_pcie",
                "PCIe round-trip load",
                &e,
                cancel,
                tx,
                started_at,
            )
        }
    };

    let (elements, buffer_bytes) = capped_element_count(memory_cap_mb);
    let chunk_count = elements.div_ceil(CHUNK_ELEMENTS);

    log::info!(
        "[stress-kit/gpu_pcie] acquired {} ({}), {} MiB round-trip buffer ({} chunk(s))",
        ctx.vendor_label, ctx.backend_label, buffer_bytes / (1024 * 1024), chunk_count
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
    struct Params { len: u32, base: u32, seed: u32, _p2: u32 }
    // Seed is rewritten every pass, so the buffers need COPY_DST.
    let params_bufs: Vec<wgpu::Buffer> = (0..chunk_count)
        .map(|c| {
            ctx.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("gpu_pcie params"),
                contents: bytemuck::bytes_of(&Params {
                    len: elements as u32,
                    base: (c * CHUNK_ELEMENTS) as u32,
                    seed: 0,
                    _p2: 0,
                }),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            })
        })
        .collect();

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
    let bind_groups: Vec<wgpu::BindGroup> = params_bufs
        .iter()
        .map(|params_buf| {
            ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("gpu_pcie bind group"),
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: gpu_buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: params_buf.as_entire_binding() },
                ],
            })
        })
        .collect();

    let mut last_tick = Instant::now();
    let mut bytes_in_tick: u64 = 0;
    let mut consecutive_readback_errors: u32 = 0;
    let mut total_errors_observed: u64 = 0;
    let mut seed: u32 = 0x5a5a_5a5a;

    while !cancel.load(Ordering::Relaxed) {
        seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
        for (c, params_buf) in params_bufs.iter().enumerate() {
            let params = Params {
                len: elements as u32,
                base: (c as u64 * CHUNK_ELEMENTS) as u32,
                seed,
                _p2: 0,
            };
            ctx.queue.write_buffer(params_buf, 0, bytemuck::bytes_of(&params));
        }
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
            for (c, bind_group) in bind_groups.iter().enumerate() {
                let start = c as u64 * CHUNK_ELEMENTS;
                let chunk_elems = (elements - start).min(CHUNK_ELEMENTS);
                cpass.set_bind_group(0, bind_group, &[]);
                cpass.dispatch_workgroups(chunk_elems.div_ceil(WG_SIZE as u64) as u32, 1, 1);
            }
        }
        encoder.copy_buffer_to_buffer(&gpu_buf, 0, &readback_buf, 0, buffer_bytes);
        ctx.queue.submit(std::iter::once(encoder.finish()));
        let _ = ctx.device.poll(wgpu::PollType::Wait);

        // Bail before interpreting the readback on a failed device.
        if let Some(reason) = ctx.health.failure() {
            emit_fatal_tick(tx, started_at, format!("gpu_pcie: {reason}"), total_errors_observed);
            return;
        }

        let slice = readback_buf.slice(..);
        let (tx_map, rx_map) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |res| {
            let _ = tx_map.send(res);
        });
        let _ = ctx.device.poll(wgpu::PollType::Wait);
        match rx_map.recv() {
            Ok(Ok(())) => {
                let view = slice.get_mapped_range();
                let got: &[u32] = bytemuck::cast_slice(&view);
                let mismatches = verify_round_trip(got, &staging_upload, seed);
                drop(view);
                readback_buf.unmap();
                consecutive_readback_errors = 0;

                // A device that died mid-readback yields garbage, not PCIe
                // data errors — abort without polluting the count.
                if mismatches > 0 {
                    if let Some(reason) = ctx.health.failure() {
                        emit_fatal_tick(
                            tx,
                            started_at,
                            format!("gpu_pcie: {reason}"),
                            total_errors_observed,
                        );
                        return;
                    }
                    total_errors_observed += mismatches;
                }
            }
            _ => {
                readback_buf.unmap();
                consecutive_readback_errors += 1;
                if consecutive_readback_errors >= MAX_CONSECUTIVE_READBACK_ERRORS {
                    let msg = format!(
                        "gpu_pcie: {consecutive_readback_errors} consecutive readback failures; aborting stage"
                    );
                    log::error!("[stress-kit/gpu_pcie] {msg}");
                    emit_fatal_tick(tx, started_at, msg, total_errors_observed);
                    return;
                }
                emit_tick(tx, started_at, 0.0, Some(format!(
                    "readback map failed ({consecutive_readback_errors}/{MAX_CONSECUTIVE_READBACK_ERRORS})"
                )), total_errors_observed);
                continue;
            }
        }

        bytes_in_tick += buffer_bytes * 2;

        if last_tick.elapsed() >= TICK {
            let dt = last_tick.elapsed().as_secs_f64().max(f64::EPSILON);
            let gb_per_sec = (bytes_in_tick as f64) / dt / 1e9;
            let err_msg = if total_errors_observed > 0 {
                Some(format!(
                    "{} PCIe round-trip mismatches detected (cumulative); this is a hardware fault",
                    total_errors_observed
                ))
            } else {
                None
            };
            emit_tick(tx, started_at, gb_per_sec, err_msg, total_errors_observed);
            last_tick = Instant::now();
            bytes_in_tick = 0;
        }
    }

    if total_errors_observed > 0 {
        log::error!(
            "[stress-kit/gpu_pcie] final error count: {} — transfers are corrupting data",
            total_errors_observed
        );
    }
    log::debug!("[stress-kit/gpu_pcie] cancellation received, exiting");
}

/// Compare the readback against `upload ^ seed`, returning the mismatch
/// count; the first offending word is logged with offset/expected/got.
fn verify_round_trip(got: &[u32], upload: &[u32], seed: u32) -> u64 {
    let mut mismatches: u64 = 0;
    for (g, u) in got.iter().zip(upload) {
        mismatches += (*g != *u ^ seed) as u64;
    }
    if mismatches > 0 {
        if let Some((i, g)) = got
            .iter()
            .enumerate()
            .find(|(i, g)| **g != upload[*i] ^ seed)
            .map(|(i, g)| (i, *g))
        {
            log::error!(
                "[stress-kit/gpu_pcie] round-trip mismatch: offset 0x{:X} expected 0x{:08X} got 0x{:08X} ({} total this pass)",
                i * 4,
                upload[i] ^ seed,
                g,
                mismatches
            );
        }
    }
    mismatches
}

#[cfg(test)]
mod tests {
    use super::verify_round_trip;

    #[test]
    fn verify_counts_corrupted_words() {
        let upload: Vec<u32> = (0..4096u32).map(|i| i.wrapping_mul(2654435761)).collect();
        let seed = 0xA5A5_1234;
        let mut got: Vec<u32> = upload.iter().map(|u| u ^ seed).collect();
        got[17] ^= 1 << 9;
        got[4000] = 0;
        assert_eq!(verify_round_trip(&got, &upload, seed), 2);
    }

    #[test]
    fn verify_clean_round_trip_counts_nothing() {
        let upload: Vec<u32> = (0..4096u32).map(|i| i.wrapping_mul(2654435761)).collect();
        let seed = 0x0BAD_F00D;
        let got: Vec<u32> = upload.iter().map(|u| u ^ seed).collect();
        assert_eq!(verify_round_trip(&got, &upload, seed), 0);
    }
}
