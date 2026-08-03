// Composite the frosted surface over the WHOLE target (full-framebuffer viewport, GL_SCISSOR_TEST
// disabled) so the rounded-rect coverage — not a hard scissor — forms every edge, giving real AA on
// the straight sides as well as the corners (DESIGN §10/§11). Coverage is 0 outside the panel, so a
// LoadOp::Load-equivalent (no clear) leaves the rest of the target untouched.
//
// DELIBERATE DIVERGENCE from composite.wgsl (DESIGN §10/§2f): this emits **premultiplied** alpha,
// paired with glBlendFunc(ONE, ONE_MINUS_SRC_ALPHA):
//     out_rgb = encode(mix(blurred, tint.rgb, tint.a)) * coverage;   out_a = coverage;
// Two load-bearing clauses: (1) encode happens BEFORE the coverage multiply — the sRGB OETF is
// concave, so cover-then-encode overshoots into a halo; (2) out_a == coverage, so the blend's
// (1 - src_a) factor matches (1 - coverage). Algebraically identical to the WGSL straight-alpha path
// under encode-then-cover, but expressed premultiplied because the web target requires it (the glow
// GLSL is a separate file from the WGSL, so this carries zero wgpu edit). The `#version` header is
// prepended at compile time.
uniform sampler2D u_blurred;
uniform vec2 u_rect_origin_px;        // target rect origin, GL bottom-left framebuffer px
uniform vec2 u_rect_size_px;          // target rect size, framebuffer px
uniform vec4 u_tint;                  // linear, straight alpha (a = film opacity)
uniform vec2 u_backdrop_uv_offset;    // map target-rect uv [0,1] onto the clipped scratch sub-rect
uniform vec2 u_backdrop_uv_scale;
uniform float u_corner_radius_px;     // clamped, framebuffer px
uniform int u_encode_srgb;            // 1 => manually linear->sRGB encode the output
uniform float u_opacity;              // surface-global fade in [0,1]; scales the premultiplied output

out vec4 frag;

// Signed distance to a rounded rectangle centered at the origin (negative inside).
float sd_rounded_rect(vec2 p, vec2 half_size, float r) {
    vec2 q = abs(p) - half_size + vec2(r);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2(0.0))) - r;
}

// Inverse sRGB EOTF (linear -> gamma). Same select->mix translation as the decode.
vec3 linear_to_srgb(vec3 c) {
    vec3 low = c * 12.92;
    vec3 high = 1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055;
    return mix(high, low, vec3(lessThanEqual(c, vec3(0.0031308))));
}

void main() {
    vec2 px = gl_FragCoord.xy; // GL bottom-origin pixel center

    // Sample the blurred backdrop. rect-uv maps the target rect to [0,1]; the backdrop remap then
    // accounts for any source-region clip. ClampToEdge handles out-of-range (offscreen).
    vec2 rect_uv = (px - u_rect_origin_px) / u_rect_size_px;
    vec2 sample_uv = u_backdrop_uv_offset + rect_uv * u_backdrop_uv_scale;
    vec4 blurred = textureLod(u_blurred, sample_uv, 0.0);

    // Rounded-rect coverage in pixel space, centered in the rect, with a boundary-centered 1px AA
    // band (50% coverage on the geometric edge).
    vec2 half_size = u_rect_size_px * 0.5;
    vec2 p = px - (u_rect_origin_px + half_size);
    float d = sd_rounded_rect(p, half_size, u_corner_radius_px);
    float coverage = 1.0 - smoothstep(-0.5, 0.5, d);

    // Tint film over the blurred backdrop: a linear-light mix by film opacity.
    vec3 rgb = mix(blurred.rgb, u_tint.rgb, u_tint.a);
    if (u_encode_srgb == 1) {
        rgb = linear_to_srgb(rgb);
    }
    // Premultiplied output: encode first, THEN fold coverage (and the surface-global opacity fade)
    // into both rgb and alpha. opacity scales the effective coverage, so the surface dissolves to
    // the untouched destination at opacity 0 — and the §2d no-halo property holds (the edge color is
    // unchanged; only the premultiplied weight scales, consistently in rgb and alpha).
    float a = coverage * u_opacity;
    frag = vec4(rgb * a, a);
}
