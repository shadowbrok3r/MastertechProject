// Separable Gaussian blur — one axis per pass (horizontal then vertical). Ported from
// gaussian.wgsl; all convolution is in linear light. Pass 1 (horizontal) samples the gamma-encoded
// grab and decodes sRGB->linear on sample; pass 2 (vertical) samples the already-linear scratch and
// does not (`u_decode_srgb`). Also serves as the dual-Kawase prefilter at radius 0 (decode + remap
// into the linear mip-0 scratch). The `#version` header is prepended at compile time.
uniform sampler2D u_src;
uniform vec2 u_uv_offset;   // map this pass's output uv [0,1] into the sampled sub-rect
uniform vec2 u_uv_scale;
uniform vec2 u_texel_size;  // 1 / sampled dimensions, so a +/-i tap is direction * texel * i
uniform vec2 u_direction;   // (1,0) horizontal or (0,1) vertical
uniform float u_sigma;
uniform int u_radius;
uniform int u_decode_srgb;  // 1 => decode sRGB->linear on sample (pass 1 / prefilter only)

in vec2 v_uv;
out vec4 frag;

// sRGB EOTF (gamma -> linear). GLSL-ES has no `select`, so the WGSL `select(high, low, c<=cutoff)`
// becomes mix(high, low, vec3(lessThanEqual(...))): vec3(bvec3) is 1.0 where true, picking `low`.
vec3 srgb_to_linear(vec3 c) {
    vec3 low = c / 12.92;
    vec3 high = pow((c + vec3(0.055)) / 1.055, vec3(2.4));
    return mix(high, low, vec3(lessThanEqual(c, vec3(0.04045))));
}

void main() {
    vec2 base_uv = u_uv_offset + v_uv * u_uv_scale;
    vec4 accum = vec4(0.0);
    float weight_sum = 0.0;
    // u_radius is uniform and clamped to MAX_GAUSSIAN_RADIUS backend-side; ES 3.00 allows the
    // uniform-bounded loop.
    for (int i = -u_radius; i <= u_radius; i = i + 1) {
        float fi = float(i);
        float w = exp(-0.5 * (fi / u_sigma) * (fi / u_sigma));
        vec2 uv = base_uv + u_direction * u_texel_size * fi;
        vec4 s = textureLod(u_src, uv, 0.0);
        if (u_decode_srgb == 1) {
            s = vec4(srgb_to_linear(s.rgb), s.a); // alpha is never gamma-encoded
        }
        accum += s * w;
        weight_sum += w;
    }
    frag = accum / weight_sum;
}
