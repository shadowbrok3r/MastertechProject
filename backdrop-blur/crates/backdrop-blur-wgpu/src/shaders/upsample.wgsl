// Dual-Kawase (dual-filter) upsample — the second half of the production blur (KWin, picom;
// Bjørge/ARM SIGGRAPH 2015). One pass doubles resolution. 8 taps: 4 cardinals×1 (at ±2·halfpixel)
// + 4 diagonals×2 (at ±halfpixel), ÷12 (energy-preserving). Linear scratch only.
//
// `halfpixel = 0.5 / size(sampled texture)` — the smaller level being read this pass (KWin's
// convention). Matches the downsample so the round trip is symmetric. The per-pass spread is fixed
// at one half-texel (no continuous `offset` scalar), so radius is log2-quantized — see the
// downsample header and `cache::resolve_kawase_levels`.

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
    var sum = vec4<f32>(0.0);
    // 4 cardinals, weight 1, at ±2·halfpixel.
    sum += textureSampleLevel(src, samp, uv + vec2<f32>(-hp.x * 2.0, 0.0), 0.0);
    sum += textureSampleLevel(src, samp, uv + vec2<f32>(hp.x * 2.0, 0.0), 0.0);
    sum += textureSampleLevel(src, samp, uv + vec2<f32>(0.0, -hp.y * 2.0), 0.0);
    sum += textureSampleLevel(src, samp, uv + vec2<f32>(0.0, hp.y * 2.0), 0.0);
    // 4 diagonals, weight 2, at ±halfpixel.
    sum += textureSampleLevel(src, samp, uv + vec2<f32>(-hp.x, -hp.y), 0.0) * 2.0;
    sum += textureSampleLevel(src, samp, uv + vec2<f32>(hp.x, -hp.y), 0.0) * 2.0;
    sum += textureSampleLevel(src, samp, uv + vec2<f32>(-hp.x, hp.y), 0.0) * 2.0;
    sum += textureSampleLevel(src, samp, uv + vec2<f32>(hp.x, hp.y), 0.0) * 2.0;
    return sum / 12.0;
}
