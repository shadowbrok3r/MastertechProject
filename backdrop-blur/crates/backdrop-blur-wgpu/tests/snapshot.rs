//! Gated GPU tier (IMPL §2b/§2c): the real end-to-end proof. Builds a `wgpu::Device` on the
//! software (lavapipe) adapter, frosts a panel over a hard-edged backdrop, reads the target
//! back, and asserts the three things that make it *frosted glass*: the backdrop edge got
//! blurred, content outside the panel is untouched, and the rounded corner is masked out.
//!
//! Runs only with `--features image-snapshots` on a host with a Vulkan software rasterizer.
//! The default `cargo test` compiles this file to nothing (the whole module is feature-gated),
//! so it never tries to create a GPU device on a GPU-less machine.
#![cfg(feature = "image-snapshots")]

mod common;

use backdrop_blur_core::{
    BackdropBlur, BlurRadius, BlurRequest, CornerRadius, LinearRgba, Presence, Region, Scale, Tint,
};
use backdrop_blur_wgpu::{SourceColorSpace, SourceView, WgpuBlur};
use common::software_device;

const DIM: u32 = 200;
const EDGE_X: u32 = 100; // backdrop: red left of this, blue right of it

/// A `DIM×DIM` Rgba8Unorm texture filled with a hard vertical edge: red on the left, blue on
/// the right. Returned as `TEXTURE_BINDING | COPY_SRC | COPY_DST` so it can be sampled and copied.
fn backdrop_texture(device: &wgpu::Device, queue: &wgpu::Queue) -> wgpu::Texture {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("backdrop"),
        size: wgpu::Extent3d {
            width: DIM,
            height: DIM,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    let mut pixels = vec![0u8; (DIM * DIM * 4) as usize];
    for y in 0..DIM {
        for x in 0..DIM {
            let i = ((y * DIM + x) * 4) as usize;
            let [r, g, b] = if x < EDGE_X { [255, 0, 0] } else { [0, 0, 255] };
            pixels[i] = r;
            pixels[i + 1] = g;
            pixels[i + 2] = b;
            pixels[i + 3] = 255;
        }
    }

    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(DIM * 4),
            rows_per_image: Some(DIM),
        },
        wgpu::Extent3d {
            width: DIM,
            height: DIM,
            depth_or_array_layers: 1,
        },
    );
    texture
}

fn region(origin: [u32; 2], size: [u32; 2]) -> Region {
    Region {
        origin,
        size,
        scale: Scale::new(1.0),
    }
}

/// A `DIM×DIM` Rgba8Unorm texture filled with a single opaque color.
fn flat_backdrop(device: &wgpu::Device, queue: &wgpu::Queue, rgb: [u8; 3]) -> wgpu::Texture {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("flat backdrop"),
        size: wgpu::Extent3d {
            width: DIM,
            height: DIM,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let pixels: Vec<u8> = (0..(DIM * DIM) as usize)
        .flat_map(|_| [rgb[0], rgb[1], rgb[2], 255])
        .collect();
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(DIM * 4),
            rows_per_image: Some(DIM),
        },
        wgpu::Extent3d {
            width: DIM,
            height: DIM,
            depth_or_array_layers: 1,
        },
    );
    texture
}

/// Frost a panel over `backdrop` and read the target back. The panel covers the centre 100×100;
/// the target is seeded with the backdrop (the host blits intermediate→swapchain first).
fn frost_and_read(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    backdrop: &wgpu::Texture,
    blur_radius: f32,
    tint: Tint,
    presence: f32,
) -> Vec<u8> {
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
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
    let mut blur = WgpuBlur::new(device).expect("create WgpuBlur");
    let panel = region([50, 50], [100, 100]);
    let request = BlurRequest {
        source_region: panel,
        target_rect: panel,
        blur_radius: BlurRadius::new(blur_radius),
        tint,
        corner_radius: CornerRadius::new(24.0),
        presence: Presence::new(presence),
    };
    let source = SourceView {
        view: backdrop.create_view(&wgpu::TextureViewDescriptor::default()),
        size: [DIM, DIM],
        color_space: SourceColorSpace::GammaSrgb,
    };
    let prepared = blur
        .prepare(
            device,
            queue,
            &source,
            wgpu::TextureFormat::Rgba8Unorm,
            &request,
        )
        .expect("prepare")
        .expect("non-empty region");
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("frame"),
    });
    encoder.copy_texture_to_texture(
        wgpu::TexelCopyTextureInfo {
            texture: backdrop,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::Extent3d {
            width: DIM,
            height: DIM,
            depth_or_array_layers: 1,
        },
    );
    blur.record(&mut encoder, &target_view, prepared)
        .expect("record");
    queue.submit([encoder.finish()]);
    read_back(device, queue, &target)
}

/// Read an Rgba8 texture back into a row-major `Vec<u8>` (tightly packed, `DIM*4` per row).
fn read_back(device: &wgpu::Device, queue: &wgpu::Queue, texture: &wgpu::Texture) -> Vec<u8> {
    let unpadded = DIM * 4;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded = unpadded.div_ceil(align) * align;

    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size: u64::from(padded * DIM),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("readback enc"),
    });
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
        tx.send(res).expect("readback map channel send");
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("device poll");
    rx.recv()
        .expect("map callback fired")
        .expect("buffer map succeeded");

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
fn frosted_panel_blurs_the_backdrop_inside_the_masked_rect() {
    let (device, queue) = software_device();
    let backdrop = backdrop_texture(&device, &queue);

    // The target starts as a copy of the backdrop (the host blits intermediate→swapchain before
    // compositing the surface), so anything the surface does NOT touch stays the backdrop.
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
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    let mut blur = WgpuBlur::new(&device).expect("create WgpuBlur");

    // Panel over the centre 100×100; its backdrop is the same screen area, spanning the edge.
    let panel = region([50, 50], [100, 100]);
    let request = BlurRequest {
        source_region: panel,
        target_rect: panel,
        blur_radius: BlurRadius::new(10.0),
        tint: Tint::new(LinearRgba::new(0.0, 0.0, 0.0, 0.15)), // faint darkening film
        corner_radius: CornerRadius::new(24.0),
        presence: Presence::default(),
    };

    let source = SourceView {
        view: backdrop.create_view(&wgpu::TextureViewDescriptor::default()),
        size: [DIM, DIM],
        color_space: SourceColorSpace::GammaSrgb,
    };
    let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());

    let prepared = blur
        .prepare(
            &device,
            &queue,
            &source,
            wgpu::TextureFormat::Rgba8Unorm,
            &request,
        )
        .expect("prepare succeeds")
        .expect("a non-empty region prepares a blur");

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("frame"),
    });
    // Seed the target with the backdrop, then composite the frosted panel over it.
    encoder.copy_texture_to_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &backdrop,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::Extent3d {
            width: DIM,
            height: DIM,
            depth_or_array_layers: 1,
        },
    );
    blur.record(&mut encoder, &target_view, prepared)
        .expect("record");
    queue.submit([encoder.finish()]);

    let out = read_back(&device, &queue, &target);

    // 1. Outside the panel entirely: untouched backdrop (pure red on the left).
    let [r, _, b, _] = pixel(&out, 10, 10);
    assert!(
        r > 200 && b < 40,
        "outside the panel must be untouched backdrop red, got r={r} b={b}"
    );

    // 2. The rounded corner is masked out: [54,54] is inside the panel rect but in the corner
    //    cutoff (radius 24 → arc centre [74,74], distance ≈28 > 24), so it shows the backdrop.
    let [r, _, b, _] = pixel(&out, 54, 54);
    assert!(
        r > 200 && b < 40,
        "the masked rounded corner must show backdrop red, got r={r} b={b}"
    );

    // 3. Panel centre sits on the backdrop's hard red/blue edge. Unblurred it would be pure blue
    //    (x=100 is the first blue column); a real blur bleeds red across, so BOTH channels are
    //    substantial. This is the proof the convolution ran.
    let [r, _, b, _] = pixel(&out, 100, 100);
    assert!(
        r > 30 && b > 30,
        "the panel centre must show a blurred red↔blue mix (both channels present), got r={r} b={b}"
    );

    // 4. Gamma round-trip: an interior, fully-covered pixel well inside the panel and 30px from
    //    the seam samples uniform red. The blur leaves it red (linear 1,0,0); the 15%-black tint
    //    film yields linear 0.85; the Rgba8Unorm target needs the manual linear→sRGB encode, so
    //    the readback is ~237 (0.930·255), NOT ~217 (0.85·255 — what a SKIPPED encode would give).
    //    This is the assertion that can actually fail on a gamma-encode bug. It pins the encode's
    //    mix-branch/clamp, NOT the exponent — the curve is flat at 0.85 (a 2.2-for-2.4
    //    substitution measures 236 vs 237 here); the exponent is pinned at the dark point by
    //    `composite_encodes_the_dark_point_to_pin_the_exponent` below.
    let [r, g, b, _] = pixel(&out, 70, 100);
    assert!(
        (230..=244).contains(&r) && g <= 6 && b <= 6,
        "interior red must round-trip through the linear→sRGB encode to ~237, got r={r} g={g} b={b}"
    );
}

/// Dark-point oracle for the composite's linear→sRGB encode **exponent** — the assertion the
/// near-white check in `frosted_panel_blurs_the_backdrop_inside_the_masked_rect` cannot provide.
/// A flat red backdrop (byte 255 → linear 1.0 under any power law, so the decode contributes
/// nothing) through an 80%-black tint film yields linear 0.2 red, where the encode curve is ~5×
/// steeper than at 0.85: a 2.2-for-2.4 exponent substitution lands ≈115 vs the canonical ≈124
/// (Δ≈9), outside the ±5 band — at 0.85 the same substitution is Δ≈1 and invisible. The
/// near-white assertion pins the encode's mix-branch/clamp instead; together the two operating
/// points cover the encode (mirrors the glow oracle in `blur_tests.rs`
/// `composite_encodes_linear_through_the_glsl_at_two_operating_points`, which likewise probes a
/// flat scene).
#[test]
fn composite_encodes_the_dark_point_to_pin_the_exponent() {
    let (device, queue) = software_device();
    // Flat red scene so the probe has no seam 30px away: on the red/blue backdrop the blur would
    // bleed a little seam blue toward the probe, muddying what must be a pure encode measurement.
    let backdrop = flat_backdrop(&device, &queue, [255, 0, 0]);
    let out = frost_and_read(
        &device,
        &queue,
        &backdrop,
        10.0,
        Tint::new(LinearRgba::new(0.0, 0.0, 0.0, 0.8)),
        1.0,
    );

    // Interior pixel, fully covered and well inside the panel edges: the flat scene leaves it
    // uniform red, so only the tint film and the encode set the byte. Measured 124 on lavapipe
    // (exactly the canonical encode of 0.2); ±5 keeps the 2.2-substitution's ≈115 outside the
    // band. One combined assert so a failure is attributable to a specific channel.
    let [r, g, b, _] = pixel(&out, 70, 100);
    assert!(
        (119..=129).contains(&r) && g <= 6 && b <= 6,
        "encode of linear 0.2 must read ≈124 (the exponent oracle: 2.2 lands ≈115) with no \
         green/blue from the black film, got r={r} g={g} b={b}"
    );
}

#[test]
fn dual_kawase_preserves_energy_on_a_flat_backdrop() {
    // radius 30 (≥ the 16px threshold) takes the dual-Kawase path. Over a FLAT mid-gray backdrop
    // with no tint, an energy-preserving down/up filter (5-tap ÷8, 8-tap ÷12) must leave the gray
    // unchanged — a wrong-weight kernel that doesn't sum to 1 shifts the brightness and fails here.
    // This is the real guard the "non-trivial output" readback cannot give (IMPL §2b′).
    let (device, queue) = software_device();
    let gray = flat_backdrop(&device, &queue, [128, 128, 128]);
    let out = frost_and_read(
        &device,
        &queue,
        &gray,
        30.0,
        Tint::new(LinearRgba::new(0.0, 0.0, 0.0, 0.0)),
        1.0,
    );

    // Panel centre, far from the panel edges: still ~128 (flat in, flat out).
    let [r, g, b, _] = pixel(&out, 100, 100);
    assert!(
        (120..=136).contains(&r) && (120..=136).contains(&g) && (120..=136).contains(&b),
        "dual-Kawase must preserve a flat backdrop's brightness, got r={r} g={g} b={b}"
    );
}

#[test]
fn dual_kawase_blurs_a_large_radius_edge() {
    // The hard red/blue edge under a large (dual-Kawase) blur: the panel centre, on the seam, is a
    // strong mix — proving the multi-level pyramid actually reaches across the seam (a zero/wrong
    // half-pixel offset would leave the seam sharp and the centre near-pure-blue).
    // Radius 18 (N=4 dual-Kawase) — small enough vs the 100px panel that the red↔blue transition
    // is a measurable band, not a saturated wash (radius 30 mixes the whole panel).
    let (device, queue) = software_device();
    let edge = backdrop_texture(&device, &queue);
    let out = frost_and_read(
        &device,
        &queue,
        &edge,
        18.0,
        Tint::new(LinearRgba::new(0.0, 0.0, 0.0, 0.1)),
        1.0,
    );

    let [r, _, b, _] = pixel(&out, 100, 100);
    assert!(
        r > 40 && b > 40,
        "a large dual-Kawase blur must mix red and blue across the seam, got r={r} b={b}"
    );
    // Transition-width guard: count the mixed (both-channel) pixels along the seam row inside the
    // panel. This bounds the effective blur radius, so a half-pixel offset scaled 2× (transition
    // ~doubles) or 0.5× (~halves) falls outside the band — the energy test and the binary mix
    // check above are offset-magnitude-invariant and cannot see that.
    let mixed = (50u32..150)
        .filter(|&x| {
            let [r, _, b, _] = pixel(&out, x, 100);
            r > 50 && b > 50
        })
        .count();
    assert!(
        (30..=72).contains(&mixed),
        "dual-Kawase transition width must match radius 30 (a 2×/0.5× half-pixel error escapes this band), got {mixed} mixed px"
    );
    // Outside the panel stays untouched backdrop red.
    let [r, _, b, _] = pixel(&out, 10, 100);
    assert!(
        r > 200 && b < 40,
        "outside the panel must stay backdrop red, got r={r} b={b}"
    );
}

/// The analytic edge-halo oracle: along the panel's straight left edge (x≈50, y=100, away from the
/// rounded corners), every pixel must lie within the `[exterior, interior]` luminance envelope plus
/// a tolerance — a premultiplied/gamma halo would overshoot (a fringe brighter or darker than both
/// sides). The high-contrast setup makes the envelope wide, so an overshoot is a real, detectable
/// signal. (AA *presence* is covered by the corner-mask and edge tests; this oracle is only the
/// halo gate, and a 1px-wide AA band aligned to the pixel grid has no integer-center midtone.)
fn assert_no_edge_halo(out: &[u8], exterior_x: u32, interior_x: u32) {
    const TOL: i32 = 6;
    let exterior = i32::from(pixel(out, exterior_x, 100)[0]);
    let interior = i32::from(pixel(out, interior_x, 100)[0]);
    let (lo, hi) = (exterior.min(interior), exterior.max(interior));
    assert!(
        hi - lo > 100,
        "the halo oracle needs a high-contrast envelope, got [{lo},{hi}]"
    );
    for x in 40..62 {
        let v = i32::from(pixel(out, x, 100)[0]);
        assert!(
            v >= lo - TOL && v <= hi + TOL,
            "panel edge at x={x} overshoots the [{lo},{hi}] envelope (halo): v={v}"
        );
    }
}

#[test]
fn translucent_panel_edge_has_no_halo() {
    // IMPL §2d — freeze the premultiplied-vs-straight edge-alpha convention with an analytic oracle
    // (not an eyeball). The straight-alpha composite uses analytic coverage and a constant edge
    // color, so its AA edge blends monotonically — no halo. This pins that decision: a future
    // premultiplied/filtering regression would overshoot the envelope and fail here.
    let (device, queue) = software_device();

    // Bright frost over black — the white-halo (overshoot) direction.
    let black = flat_backdrop(&device, &queue, [0, 0, 0]);
    let out = frost_and_read(
        &device,
        &queue,
        &black,
        8.0,
        Tint::from_srgb_unmultiplied([240, 240, 240, 204]),
        1.0,
    );
    assert_no_edge_halo(&out, 30, 90);

    // Dark frost over white — the dark-fringe (undershoot) direction.
    let white = flat_backdrop(&device, &queue, [255, 255, 255]);
    let out = frost_and_read(
        &device,
        &queue,
        &white,
        8.0,
        Tint::from_srgb_unmultiplied([20, 20, 20, 204]),
        1.0,
    );
    assert_no_edge_halo(&out, 30, 90);
}

/// The surface-global fade (`Presence`) is a real linear blend toward the untouched destination:
/// `out(presence) == lerp(D, F, presence)` where `D` is the destination (the seeded backdrop) and
/// `F` is the fully-present (presence=1) composite. Sampled at the panel interior (coverage=1), so
/// the per-pixel coverage is out of it and the only variable is the master presence — the corrected
/// oracle (the draft's `0.5*coverage` double-applied coverage). `presence=0` must leave `D` untouched.
#[test]
fn presence_fades_the_surface_linearly_toward_the_destination() {
    let (device, queue) = software_device();
    let backdrop = backdrop_texture(&device, &queue);
    let tint = Tint::new(LinearRgba::new(0.0, 0.0, 0.0, 0.2)); // a darkening film, so F != D
    let blur_radius = 16.0;

    let out0 = frost_and_read(&device, &queue, &backdrop, blur_radius, tint, 0.0);
    let half = frost_and_read(&device, &queue, &backdrop, blur_radius, tint, 0.5);
    let full = frost_and_read(&device, &queue, &backdrop, blur_radius, tint, 1.0);
    let dest = read_back(&device, &queue, &backdrop); // D: the untouched destination

    // presence = 0 leaves the destination untouched (the surface is absent).
    let (cx, cy) = (100, 100); // panel interior (panel = [50,50]+100x100), coverage = 1
    for c in 0..4 {
        let d = dest[((cy * DIM + cx) * 4) as usize + c];
        let o0 = pixel(&out0, cx, cy)[c];
        assert!(
            (i32::from(d) - i32::from(o0)).abs() <= 1,
            "presence=0 must equal the destination at the interior (channel {c}: D={d} out0={o0})"
        );
    }

    // presence = 0.5 is the linear midpoint between D (out0) and F (full): out_half ≈ lerp(D, F, 0.5).
    for c in 0..4 {
        let d = i32::from(pixel(&out0, cx, cy)[c]);
        let f = i32::from(pixel(&full, cx, cy)[c]);
        let expected = (d + f + 1) / 2; // round-to-nearest midpoint
        let got = i32::from(pixel(&half, cx, cy)[c]);
        assert!(
            (got - expected).abs() <= 2,
            "presence=0.5 must be lerp(D,F,0.5) at the interior (channel {c}: D={d} F={f} \
             expected≈{expected} got={got})"
        );
    }

    // Sanity: the fade actually moves the pixel (F differs from D), or the oracle is vacuous.
    let moved = (0..3).any(|c| pixel(&full, cx, cy)[c] != pixel(&out0, cx, cy)[c]);
    assert!(
        moved,
        "the frost must change the interior pixel, else the oracle proves nothing"
    );
}

/// The dragged/resized-surface leak guard: frosting a surface whose size changes every frame must
/// NOT accumulate one scratch chain per distinct size. Drive `prepare` across many distinct sizes
/// over far more than `RETENTION_FRAMES` frames and assert the cache stays bounded near the
/// retention window rather than growing with the frame count. `prepare` alone both populates the
/// cache (`ensure_*`) and runs the eviction (`begin_frame`), so no record/target is needed.
#[test]
fn scratch_cache_evicts_old_sizes_instead_of_leaking() {
    let (device, queue) = software_device();
    let backdrop = flat_backdrop(&device, &queue, [128, 128, 128]);
    let source = SourceView {
        view: backdrop.create_view(&wgpu::TextureViewDescriptor::default()),
        size: [DIM, DIM],
        color_space: SourceColorSpace::GammaSrgb,
    };
    let mut blur = WgpuBlur::new(&device).expect("create WgpuBlur");

    // 40 frames, each a DISTINCT panel size → 40 distinct PingPongKeys if nothing ever evicts.
    let frames = 40u32;
    for i in 0..frames {
        let panel = region([0, 0], [40 + i, 40]);
        let request = BlurRequest {
            source_region: panel,
            target_rect: panel,
            blur_radius: BlurRadius::new(6.0),
            tint: Tint::new(LinearRgba::new(1.0, 1.0, 1.0, 0.1)),
            corner_radius: CornerRadius::new(8.0),
            presence: Presence::new(1.0),
        };
        let _prepared = blur
            .prepare(
                &device,
                &queue,
                &source,
                wgpu::TextureFormat::Rgba8Unorm,
                &request,
            )
            .expect("prepare");
    }

    // Retention is 8 frames; with one distinct size touched per frame, frame 40 keeps only the last
    // ~8 chains. Without eviction this would be `frames` (40). The generous bound still catches a
    // regression to the old insert-only cache decisively.
    let cached = blur.cached_chain_count();
    assert!(
        cached <= 12,
        "scratch cache grew to {cached} chains across {frames} resizes — eviction is not bounding it"
    );
}

/// The target-side half of the no-op contract: a zero-area `target_rect` over a NORMAL in-bounds
/// source region (mismatched regions — the path no adapter exercises) must be `Ok(None)`. The
/// composite shader divides by the target size (composite.wgsl:57), so without the guard a zero
/// dimension yields NaN/Inf UVs blended along a visible line instead of nothing.
#[test]
fn prepare_is_a_no_op_for_a_zero_area_target_rect() {
    let (device, queue) = software_device();
    let backdrop = flat_backdrop(&device, &queue, [128, 128, 128]);
    let source = SourceView {
        view: backdrop.create_view(&wgpu::TextureViewDescriptor::default()),
        size: [DIM, DIM],
        color_space: SourceColorSpace::GammaSrgb,
    };
    let mut blur = WgpuBlur::new(&device).expect("create WgpuBlur");

    let request = BlurRequest {
        source_region: region([50, 50], [100, 100]), // normal, fully in-bounds source
        target_rect: region([50, 50], [0, 100]),     // zero width → empty target
        blur_radius: BlurRadius::new(8.0),
        tint: Tint::new(LinearRgba::new(0.0, 0.0, 0.0, 0.15)),
        corner_radius: CornerRadius::new(8.0),
        presence: Presence::default(),
    };

    let prepared = blur
        .prepare(
            &device,
            &queue,
            &source,
            wgpu::TextureFormat::Rgba8Unorm,
            &request,
        )
        .expect("a zero-area target is a no-op, not an error");
    assert!(
        prepared.is_none(),
        "a zero-area target_rect must be a no-op (Ok(None))"
    );
}
