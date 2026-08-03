// Dual-Kawase (dual-filter) downsample — the production-compositor blur (KWin, picom; Bjorge/ARM
// SIGGRAPH 2015). One pass halves resolution and ~doubles effective radius. 5 taps: center*4 + 4
// diagonal corners*1, /8 (energy-preserving). Operates on the LINEAR scratch pyramid (the prefilter
// already decoded gamma). Ported from downsample.wgsl. `u_halfpixel = 0.5 / size(sampled texture)`
// (KWin's convention). The `#version` header is prepended at compile time.
uniform sampler2D u_src;
uniform vec2 u_halfpixel;

in vec2 v_uv;
out vec4 frag;

void main() {
    vec2 uv = v_uv;
    vec2 hp = u_halfpixel;
    vec4 sum = textureLod(u_src, uv, 0.0) * 4.0;
    sum += textureLod(u_src, uv + vec2(-hp.x, -hp.y), 0.0);
    sum += textureLod(u_src, uv + vec2( hp.x, -hp.y), 0.0);
    sum += textureLod(u_src, uv + vec2(-hp.x,  hp.y), 0.0);
    sum += textureLod(u_src, uv + vec2( hp.x,  hp.y), 0.0);
    frag = sum / 8.0;
}
