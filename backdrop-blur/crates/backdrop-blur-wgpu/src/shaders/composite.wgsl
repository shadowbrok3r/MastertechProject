// Composite the frosted surface. Drawn over the WHOLE target attachment (not a per-rect
// viewport) so the rounded-rect coverage — not a hard scissor — forms every edge, giving real
// anti-aliasing on the straight sides as well as the corners. Each fragment derives its target
// pixel from @builtin(position); coverage is 0 outside the panel, so LoadOp::Load leaves the
// rest of the target untouched.
//
// The blur ran in linear light; `encode_srgb` re-encodes linear→sRGB for gamma targets
// (Rgba8Unorm/Bgra8Unorm, the egui case). For `*Srgb` targets the hardware encodes on write,
// and for float targets no encode is wanted — both set `encode_srgb = 0`.

struct CompositeParams {
    rect_origin_px: vec2<f32>,    // target rect top-left, in framebuffer px
    rect_size_px: vec2<f32>,      // target rect size, in framebuffer px
    tint: vec4<f32>,              // linear, straight alpha (alpha = film opacity)
    // Map target-rect uv [0,1] onto the blurred scratch, which holds the CLIPPED source region
    // (identity when the source region was fully in-bounds; an inset when it was clipped at an edge).
    backdrop_uv_offset: vec2<f32>,
    backdrop_uv_scale: vec2<f32>,
    corner_radius_px: f32,        // clamped, in framebuffer px
    encode_srgb: u32,             // 1 = manually linear→sRGB encode the output
    opacity: f32,                 // surface-global fade in [0,1]; scales the final blend weight
    _pad: f32,
};

@group(0) @binding(0) var blurred_tex: texture_2d<f32>;
@group(0) @binding(1) var blurred_samp: sampler;
@group(0) @binding(2) var<uniform> params: CompositeParams;

// A single oversized triangle covering the whole attachment. The fragment uses
// @builtin(position) for its pixel coordinate, so no interpolated uv is needed.
@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> @builtin(position) vec4<f32> {
    let x = f32((vi << 1u) & 2u);
    let y = f32(vi & 2u);
    return vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}

// Signed distance to a rounded rectangle centered at the origin (negative inside).
fn sd_rounded_rect(p: vec2<f32>, half: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - half + vec2<f32>(r);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0))) - r;
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let cutoff = c <= vec3<f32>(0.0031308);
    let low = c * 12.92;
    let high = 1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(high, low, cutoff);
}

@fragment
fn fs_main(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let px = frag.xy; // framebuffer pixel center

    // Sample the blurred backdrop. rect-uv maps the target rect to [0,1]; the backdrop remap
    // then accounts for any source-region clip. ClampToEdge handles out-of-range (offscreen).
    let rect_uv = (px - params.rect_origin_px) / params.rect_size_px;
    let sample_uv = params.backdrop_uv_offset + rect_uv * params.backdrop_uv_scale;
    let blurred = textureSampleLevel(blurred_tex, blurred_samp, sample_uv, 0.0);

    // Rounded-rect coverage in pixel space, centered in the rect, with a boundary-centered 1px
    // AA band (50% coverage sits exactly on the geometric edge).
    let half = params.rect_size_px * 0.5;
    let p = px - (params.rect_origin_px + half);
    let d = sd_rounded_rect(p, half, params.corner_radius_px);
    let coverage = 1.0 - smoothstep(-0.5, 0.5, d);

    // Tint film over the blurred backdrop: a linear-light mix by film opacity. The surface's own
    // coverage (the rounded-rect AA) is straight alpha, blended "over" the target by the pipeline.
    // Because coverage is analytic and this edge color is constant, that blend is monotonic — no
    // premultiplied/gamma halo. IMPL §2d's analytic oracle (tests/snapshot.rs) freezes that.
    var rgb = mix(blurred.rgb, params.tint.rgb, params.tint.a);
    if (params.encode_srgb == 1u) {
        rgb = linear_to_srgb(rgb);
    }
    // Surface-global fade: scale the straight-alpha blend weight. The edge color `rgb` is
    // unchanged, so the §2d monotonic no-halo property holds at any opacity.
    return vec4<f32>(rgb, coverage * params.opacity);
}
