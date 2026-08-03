// Shared fullscreen-triangle vertex shader for the blur passes (Gaussian, dual-Kawase down/up).
// One oversized triangle covers the viewport; `v_uv` runs [0,1] across it. No vertex attributes —
// positions come from gl_VertexID, so an empty (but bound) VAO is enough.
//
// GL-origin re-derivation (DESIGN §5 — NOT a copy of the WGSL): clip-space Y is `y*2-1`, where the
// WGSL source uses `1-y*2`. GL framebuffers and textures put v=0 at the BOTTOM, so `y*2-1` makes
// v_uv.y and clip.y increase together: the FBO row written at v_uv.y==0 is the same row sampled at
// v==0. Every blur pass is therefore a Y-identity — no per-pass flip to accumulate across the
// odd-length Kawase chain (prefilter + N down + N up = 2N+1 passes). The `#version` header is
// prepended at compile time (profile::version_header).
out vec2 v_uv;

void main() {
    float x = float((gl_VertexID << 1) & 2);
    float y = float(gl_VertexID & 2);
    v_uv = vec2(x, y);
    gl_Position = vec4(x * 2.0 - 1.0, y * 2.0 - 1.0, 0.0, 1.0);
}
