// Dual-Kawase (dual-filter) upsample — the second half of the production blur. One pass doubles
// resolution. 8 taps: 4 cardinals*1 (at +/-2*halfpixel) + 4 diagonals*2 (at +/-halfpixel), /12
// (energy-preserving). Linear scratch only. Ported from upsample.wgsl. `u_halfpixel = 0.5 /
// size(sampled texture)` — the smaller level read this pass, matching the downsample so the round
// trip is symmetric. The `#version` header is prepended at compile time.
uniform sampler2D u_src;
uniform vec2 u_halfpixel;

in vec2 v_uv;
out vec4 frag;

void main() {
    vec2 uv = v_uv;
    vec2 hp = u_halfpixel;
    vec4 sum = vec4(0.0);
    // 4 cardinals, weight 1, at +/-2*halfpixel.
    sum += textureLod(u_src, uv + vec2(-hp.x * 2.0, 0.0), 0.0);
    sum += textureLod(u_src, uv + vec2( hp.x * 2.0, 0.0), 0.0);
    sum += textureLod(u_src, uv + vec2(0.0, -hp.y * 2.0), 0.0);
    sum += textureLod(u_src, uv + vec2(0.0,  hp.y * 2.0), 0.0);
    // 4 diagonals, weight 2, at +/-halfpixel.
    sum += textureLod(u_src, uv + vec2(-hp.x, -hp.y), 0.0) * 2.0;
    sum += textureLod(u_src, uv + vec2( hp.x, -hp.y), 0.0) * 2.0;
    sum += textureLod(u_src, uv + vec2(-hp.x,  hp.y), 0.0) * 2.0;
    sum += textureLod(u_src, uv + vec2( hp.x,  hp.y), 0.0) * 2.0;
    frag = sum / 12.0;
}
