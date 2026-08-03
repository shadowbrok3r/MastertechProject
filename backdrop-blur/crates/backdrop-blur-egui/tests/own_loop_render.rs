//! Gated GPU tier: the own-loop adapter end-to-end on lavapipe. Runs a real egui frame that
//! paints a hard red/blue edge, drives `OwnLoopRenderer::render_frame` (egui → intermediate +
//! target, then blur + composite), reads the target back, and asserts the frosted panel blurred
//! the edge while the rest of the egui UI is untouched. This is the only test that exercises the
//! real egui-wgpu render path; the default tier covers the surface→prepare/record wiring.
//!
//! Runs only with `--features image-snapshots` on a Vulkan software-rasterizer host.
//! `image-snapshots` implies `own-loop`; the `all(...)` gate is defense in depth so this file is
//! never even parsed in a grab-pass-only build.
#![cfg(all(feature = "image-snapshots", feature = "own-loop"))]

use backdrop_blur_core::{BlurRadius, CornerRadius, LinearRgba, Presence, RepaintPolicy, Tint};
use backdrop_blur_egui::{FrameInput, OwnLoopRenderer, Surface};
use backdrop_blur_wgpu::WgpuBlur;

const DIM: u32 = 256;
const EDGE: f32 = 128.0;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

fn software_device() -> (wgpu::Device, wgpu::Queue) {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::LowPower,
        force_fallback_adapter: false,
        compatible_surface: None,
    }))
    .expect("a Vulkan adapter (lavapipe via VK_ICD_FILENAMES) is required for the gated GPU tier");
    let limits = adapter.limits();
    pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("backdrop-blur egui test device"),
        required_features: wgpu::Features::empty(),
        required_limits: limits,
        memory_hints: wgpu::MemoryHints::default(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
        trace: wgpu::Trace::Off,
    }))
    .expect("device creation on the software adapter")
}

/// Run one egui frame painting a red left half and a blue right half across the whole screen.
fn egui_red_blue_frame() -> (
    egui::Context,
    Vec<egui::ClippedPrimitive>,
    egui::TexturesDelta,
) {
    let ctx = egui::Context::default();
    let raw = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            egui::vec2(DIM as f32, DIM as f32),
        )),
        ..Default::default()
    };
    let output = ctx.run_ui(raw, |ui| {
        let painter = ui.painter();
        painter.rect_filled(
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(EDGE, DIM as f32)),
            egui::CornerRadius::ZERO,
            egui::Color32::RED,
        );
        painter.rect_filled(
            egui::Rect::from_min_max(egui::pos2(EDGE, 0.0), egui::pos2(DIM as f32, DIM as f32)),
            egui::CornerRadius::ZERO,
            egui::Color32::BLUE,
        );
    });
    let jobs = ctx.tessellate(output.shapes, output.pixels_per_point);
    (ctx, jobs, output.textures_delta)
}

fn read_back(device: &wgpu::Device, queue: &wgpu::Queue, texture: &wgpu::Texture) -> Vec<u8> {
    let unpadded = DIM * 4;
    let padded =
        unpadded.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: u64::from(padded * DIM),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded),
                rows_per_image: Some(DIM),
            },
        },
        wgpu::Extent3d {
            width: DIM,
            height: DIM,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([encoder.finish()]);
    let slice = buffer.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |res| {
        tx.send(res).expect("map channel send")
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("device poll");
    rx.recv().expect("map callback").expect("buffer map");
    let mapped = slice.get_mapped_range();
    let mut tight = vec![0u8; (unpadded * DIM) as usize];
    for y in 0..DIM as usize {
        let src = y * padded as usize;
        let dst = y * unpadded as usize;
        tight[dst..dst + unpadded as usize].copy_from_slice(&mapped[src..src + unpadded as usize]);
    }
    tight
}

fn pixel(data: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * DIM + x) * 4) as usize;
    [data[i], data[i + 1], data[i + 2], data[i + 3]]
}

#[test]
fn own_loop_frosts_a_panel_over_a_real_egui_frame() {
    let (device, queue) = software_device();
    let (ctx, jobs, textures_delta) = egui_red_blue_frame();

    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("target"),
        size: wgpu::Extent3d {
            width: DIM,
            height: DIM,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());

    let mut adapter =
        OwnLoopRenderer::new(&device, FORMAT).expect("Rgba8Unorm is a supported target");
    let mut blur = WgpuBlur::new(&device).expect("create WgpuBlur");

    // A 100×100 panel centred on the red/blue edge.
    let surface = Surface {
        rect: egui::Rect::from_min_size(
            egui::pos2(EDGE - 50.0, EDGE - 50.0),
            egui::vec2(100.0, 100.0),
        ),
        blur_radius: BlurRadius::new(12.0),
        tint: Tint::new(LinearRgba::new(0.0, 0.0, 0.0, 0.12)),
        corner_radius: CornerRadius::new(20.0),
        presence: Presence::default(),
        repaint: RepaintPolicy::Static,
    };

    adapter
        .render_frame(
            &device,
            &queue,
            &ctx,
            &mut blur,
            FrameInput {
                target: &target_view,
                paint_jobs: &jobs,
                textures_delta: &textures_delta,
                screen: egui_wgpu::ScreenDescriptor {
                    size_in_pixels: [DIM, DIM],
                    pixels_per_point: 1.0,
                },
            },
            &[surface],
        )
        .expect("render_frame succeeds");

    let out = read_back(&device, &queue, &target);

    // Far left, outside the panel: egui's red, untouched by the composite.
    let [r, _, b, _] = pixel(&out, 20, 128);
    assert!(
        r > 200 && b < 40,
        "egui UI outside the panel must be untouched red, got r={r} b={b}"
    );

    // Panel centre sits on the red/blue edge; a real blur bleeds both channels together.
    let [r, _, b, _] = pixel(&out, 128, 128);
    assert!(
        r > 30 && b > 30,
        "the frosted panel centre must show a blurred red↔blue mix, got r={r} b={b}"
    );
}
