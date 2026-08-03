// Separable Gaussian blur — one axis per pass (horizontal then vertical). The proven
// first-pixel path (the dual-Kawase down/up filter is a later, gated increment).
//
// Pass 1 (horizontal) samples the host's egui intermediate, which is GAMMA-encoded regardless
// of its texture format (egui#3168), so it decodes sRGB→linear on sample. Pass 2 (vertical)
// samples the scratch written by pass 1, which is already linear (Rgba16Float), so it does not
// decode. `decode_srgb` carries that one-bit difference. All convolution is in linear light.

struct GaussianParams {
    // Map this pass's output uv [0,1] into the SAMPLED texture's uv space. Pass 1 maps the
    // scratch onto the source sub-rect (source_region / source_size); pass 2 is identity.
    uv_offset: vec2<f32>,
    uv_scale: vec2<f32>,
    // 1 / (sampled texture dimensions), so a ±i-pixel tap is `direction * texel_size * i`.
    texel_size: vec2<f32>,
    // (1,0) horizontal or (0,1) vertical.
    direction: vec2<f32>,
    sigma: f32,
    radius: i32,
    decode_srgb: u32,
    _pad: u32,
};

@group(0) @binding(0) var src_tex: texture_2d<f32>;
@group(0) @binding(1) var src_samp: sampler;
@group(0) @binding(2) var<uniform> params: GaussianParams;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

// A single oversized triangle covering the viewport; uv runs [0,1] across it.
@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
    var out: VsOut;
    let x = f32((vi << 1u) & 2u);
    let y = f32(vi & 2u);
    out.uv = vec2<f32>(x, y);
    out.pos = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    return out;
}

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let cutoff = c <= vec3<f32>(0.04045);
    let low = c / 12.92;
    let high = pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(high, low, cutoff);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let base_uv = params.uv_offset + in.uv * params.uv_scale;
    var accum = vec4<f32>(0.0);
    var weight_sum = 0.0;
    let r = params.radius;
    // Bounded by the backend's MAX_GAUSSIAN_RADIUS clamp; loop is uniform (radius is uniform).
    for (var i: i32 = -r; i <= r; i = i + 1) {
        let w = exp(-0.5 * (f32(i) / params.sigma) * (f32(i) / params.sigma));
        let uv = base_uv + params.direction * params.texel_size * f32(i);
        var s = textureSampleLevel(src_tex, src_samp, uv, 0.0);
        if (params.decode_srgb == 1u) {
            s = vec4<f32>(srgb_to_linear(s.rgb), s.a); // alpha is never gamma-encoded
        }
        accum = accum + s * w;
        weight_sum = weight_sum + w;
    }
    return accum / weight_sum;
}
