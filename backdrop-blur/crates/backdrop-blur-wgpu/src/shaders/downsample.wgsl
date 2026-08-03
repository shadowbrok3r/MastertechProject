// Dual-Kawase (dual-filter) downsample — the production-compositor blur (KWin, picom; Bjørge/ARM
// SIGGRAPH 2015). One pass halves resolution and ~doubles effective radius, so radius is the
// iteration count, not the kernel size. 5 taps: center×4 + 4 diagonal corners×1, ÷8 (energy-
// preserving). Operates on the LINEAR scratch pyramid — the prefilter already decoded gamma.
//
// `halfpixel = 0.5 / size(sampled texture)` (KWin's convention; do NOT mix picom's). The diagonal
// corners sit one half-texel out, sampling the four quadrant centers of the larger source.
//
// The per-pass spread is fixed at one half-texel — no `offset` scalar like KWin's continuous
// strength dial — so radius granularity is log2-quantized by the level count (see
// `cache::resolve_kawase_levels`). A deliberate v1 divergence (discrete design-token radii).

struct KawaseParams {
    halfpixel: vec2<f32>,
    _pad: vec2<f32>,
};

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var<uniform> params: KawaseParams;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VsOut {
    var out: VsOut;
    let x = f32((vi << 1u) & 2u);
    let y = f32(vi & 2u);
    out.uv = vec2<f32>(x, y);
    out.pos = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let hp = params.halfpixel;
    var sum = textureSampleLevel(src, samp, uv, 0.0) * 4.0;
    sum += textureSampleLevel(src, samp, uv + vec2<f32>(-hp.x, -hp.y), 0.0);
    sum += textureSampleLevel(src, samp, uv + vec2<f32>(hp.x, -hp.y), 0.0);
    sum += textureSampleLevel(src, samp, uv + vec2<f32>(-hp.x, hp.y), 0.0);
    sum += textureSampleLevel(src, samp, uv + vec2<f32>(hp.x, hp.y), 0.0);
    return sum / 8.0;
}
