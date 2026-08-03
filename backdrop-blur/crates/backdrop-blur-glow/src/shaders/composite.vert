// Bare fullscreen-triangle vertex shader for the composite. The fragment uses gl_FragCoord for its
// pixel coordinate (no interpolated uv), so this only positions the triangle. Bottom-left clip-Y
// (`y*2-1`) keeps gl_FragCoord.y — GL's default bottom-origin window coordinate — consistent with
// the bottom-left composite uniforms (DESIGN §5). The `#version` header is prepended at compile time.
void main() {
    float x = float((gl_VertexID << 1) & 2);
    float y = float(gl_VertexID & 2);
    gl_Position = vec4(x * 2.0 - 1.0, y * 2.0 - 1.0, 0.0, 1.0);
}
