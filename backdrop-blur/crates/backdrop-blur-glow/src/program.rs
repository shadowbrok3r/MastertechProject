//! Shader compilation and the linked program set. One GLSL source per stage (in `shaders/`) is
//! compiled under a per-target `#version` header ([`version_header`]) so the same source serves
//! desktop GL, GLES3, and WebGL2 (DESIGN §8). Compile/link failures map to
//! [`BlurStage::ShaderCompile`]/[`BlurStage::ProgramLink`] carrying the driver's info log.

use crate::profile::{GlProfile, ShaderClass};
use backdrop_blur_core::{BlurError, BlurStage};
use glow::HasContext;

const FULLSCREEN_VERT: &str = include_str!("shaders/fullscreen.vert");
const GAUSSIAN_FRAG: &str = include_str!("shaders/gaussian.frag");
const DOWNSAMPLE_FRAG: &str = include_str!("shaders/downsample.frag");
const UPSAMPLE_FRAG: &str = include_str!("shaders/upsample.frag");
const COMPOSITE_VERT: &str = include_str!("shaders/composite.vert");
const COMPOSITE_FRAG: &str = include_str!("shaders/composite.frag");

/// The `#version` + precision header prepended to every shader body for `class` (DESIGN §8). ES
/// needs the `precision` qualifiers; desktop GL 1.40 does not use them. Mirrors `egui_glow`'s
/// `ShaderVersion` split (the pattern, not its output): desktop is `#version 140`, not `330 core`.
pub(crate) fn version_header(class: ShaderClass) -> &'static str {
    match class {
        ShaderClass::GlDesktop => "#version 140\n",
        ShaderClass::Es300 => "#version 300 es\nprecision highp float;\nprecision highp int;\n",
    }
}

/// The linked GL programs, one per blur stage. Owned by [`crate::GlowBlur`]; freed by
/// [`Programs::destroy`] (never in `Drop`).
pub(crate) struct Programs {
    pub(crate) gaussian: glow::Program,
    pub(crate) downsample: glow::Program,
    pub(crate) upsample: glow::Program,
    pub(crate) composite: glow::Program,
}

impl Programs {
    /// Compile + link all four programs under `profile`'s shader dialect. On any failure, the
    /// programs already built this call are deleted before returning, so a partial build leaks no
    /// GL objects.
    pub(crate) fn new(gl: &glow::Context, profile: &GlProfile) -> Result<Self, BlurError> {
        let header = version_header(profile.shader_class);
        // Build in order; clean up the predecessors if a later one fails.
        let gaussian = build_program(gl, header, FULLSCREEN_VERT, GAUSSIAN_FRAG)?;
        let downsample = match build_program(gl, header, FULLSCREEN_VERT, DOWNSAMPLE_FRAG) {
            Ok(p) => p,
            Err(e) => {
                delete_programs(gl, &[gaussian]);
                return Err(e);
            }
        };
        let upsample = match build_program(gl, header, FULLSCREEN_VERT, UPSAMPLE_FRAG) {
            Ok(p) => p,
            Err(e) => {
                delete_programs(gl, &[gaussian, downsample]);
                return Err(e);
            }
        };
        let composite = match build_program(gl, header, COMPOSITE_VERT, COMPOSITE_FRAG) {
            Ok(p) => p,
            Err(e) => {
                delete_programs(gl, &[gaussian, downsample, upsample]);
                return Err(e);
            }
        };
        Ok(Self {
            gaussian,
            downsample,
            upsample,
            composite,
        })
    }

    /// Delete the four programs. Caller holds a current context (DESIGN §11 — called from the
    /// host's explicit `destroy`, never `Drop`).
    pub(crate) fn destroy(&self, gl: &glow::Context) {
        delete_programs(
            gl,
            &[
                self.gaussian,
                self.downsample,
                self.upsample,
                self.composite,
            ],
        );
    }
}

/// Delete a set of programs. Used both by [`Programs::destroy`] and the partial-build cleanup.
fn delete_programs(gl: &glow::Context, programs: &[glow::Program]) {
    for &program in programs {
        // SAFETY: each `program` was created on this current context by `build_program` and is not
        // deleted twice (callers pass disjoint, live handles). `delete_program` on a valid program
        // is sound; on the GL default 0 it is a documented no-op.
        unsafe { gl.delete_program(program) };
    }
}

/// Compile a vertex + fragment body under `header`, link them, and free the shader objects (the
/// linked program retains them). The vertex shader is freed even if the fragment compile fails.
fn build_program(
    gl: &glow::Context,
    header: &str,
    vert_body: &str,
    frag_body: &str,
) -> Result<glow::Program, BlurError> {
    let vert = compile(gl, glow::VERTEX_SHADER, header, vert_body)?;
    let frag = match compile(gl, glow::FRAGMENT_SHADER, header, frag_body) {
        Ok(frag) => frag,
        Err(e) => {
            // SAFETY: `vert` was just created on this context and is otherwise unreferenced.
            unsafe { gl.delete_shader(vert) };
            return Err(e);
        }
    };
    let program = link(gl, vert, frag);
    // The program keeps a reference to its shaders after link; delete our handles regardless of the
    // link result so they never leak.
    // SAFETY: both shaders were created on this context and are deleted exactly once here.
    unsafe {
        gl.delete_shader(vert);
        gl.delete_shader(frag);
    }
    program
}

/// Compile one shader stage. `kind` is `glow::VERTEX_SHADER`/`FRAGMENT_SHADER`. Maps a compile
/// failure to [`BlurStage::ShaderCompile`] carrying the driver info log; deletes the failed shader.
fn compile(
    gl: &glow::Context,
    kind: u32,
    header: &str,
    body: &str,
) -> Result<glow::Shader, BlurError> {
    // SAFETY: `gl` is current; `kind` is a valid shader-stage enum. `create_shader` returns a fresh
    // shader handle or an error string, taking no caller pointers.
    let shader = unsafe { gl.create_shader(kind) }.map_err(|e| BlurError::ResourceCreation {
        stage: BlurStage::ShaderCompile,
        source: e.into(),
    })?;
    let source = format!("{header}{body}");
    // SAFETY: `shader` was just created on this context; `shader_source` copies `source`'s bytes
    // (no retained borrow) and `compile_shader` operates on the handle in place.
    unsafe {
        gl.shader_source(shader, &source);
        gl.compile_shader(shader);
    }
    // SAFETY: status/info-log queries on the just-compiled shader handle.
    let ok = unsafe { gl.get_shader_compile_status(shader) };
    if ok {
        Ok(shader)
    } else {
        // SAFETY: read the info log off the failed shader, then delete it (otherwise
        // unreferenced) — both operate on the just-created handle on the current context.
        let log = unsafe {
            let log = gl.get_shader_info_log(shader);
            gl.delete_shader(shader);
            log
        };
        Err(BlurError::ResourceCreation {
            stage: BlurStage::ShaderCompile,
            source: log.into(),
        })
    }
}

/// Link a compiled vertex + fragment shader into a program. On failure, deletes the program and
/// returns [`BlurStage::ProgramLink`] with the link log. On success, detaches the shaders (so the
/// caller can free them) and returns the program.
fn link(
    gl: &glow::Context,
    vert: glow::Shader,
    frag: glow::Shader,
) -> Result<glow::Program, BlurError> {
    // SAFETY: `gl` is current; `create_program` returns a fresh program or an error string.
    let program = unsafe { gl.create_program() }.map_err(|e| BlurError::ResourceCreation {
        stage: BlurStage::ProgramLink,
        source: e.into(),
    })?;
    // SAFETY: `program`, `vert`, `frag` are live handles on this context; attach/link operate on
    // them in place.
    unsafe {
        gl.attach_shader(program, vert);
        gl.attach_shader(program, frag);
        gl.link_program(program);
    }
    // SAFETY: link-status query on the just-linked program.
    if unsafe { gl.get_program_link_status(program) } {
        // SAFETY: detach the shaders so the caller's `delete_shader` actually frees them; the
        // linked program keeps its own copy.
        unsafe {
            gl.detach_shader(program, vert);
            gl.detach_shader(program, frag);
        }
        Ok(program)
    } else {
        // SAFETY: read the link log off the failed program, then delete it — both operate on the
        // just-created program handle on the current context.
        let log = unsafe {
            let log = gl.get_program_info_log(program);
            gl.delete_program(program);
            log
        };
        Err(BlurError::ResourceCreation {
            stage: BlurStage::ProgramLink,
            source: log.into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Host-side mirrors of the GLSL transfer functions, used to pin the gamma constants the shaders
    // carry against a wrong WGSL->GLSL port (e.g. `select`/`mix` inverted, a mistyped cutoff). The
    // encode call site is additionally pinned by the two-operating-point readback oracle
    // (`composite_encodes_linear_through_the_glsl_at_two_operating_points` in blur_tests.rs); the
    // decode-site exponent remains guarded only here (IMPL §2b).
    fn host_srgb_to_linear(c: f32) -> f32 {
        if c <= 0.040_45 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }

    fn host_linear_to_srgb(c: f32) -> f32 {
        if c <= 0.003_130_8 {
            c * 12.92
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        }
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn version_header_is_140_on_desktop_and_300es_with_precision() {
        assert_eq!(version_header(ShaderClass::GlDesktop), "#version 140\n");
        let es = version_header(ShaderClass::Es300);
        assert!(es.starts_with("#version 300 es\n"));
        assert!(es.contains("precision highp float;"));
        assert!(es.contains("precision highp int;"));
    }

    #[test]
    fn decode_matches_canonical_srgb_endpoints_and_midtone() {
        assert!(close(host_srgb_to_linear(0.0), 0.0));
        assert!(close(host_srgb_to_linear(1.0), 1.0));
        // sRGB 188/255 ≈ 0.737 -> ≈ 0.502886 linear (the same midtone core::material pins).
        assert!(close(host_srgb_to_linear(188.0 / 255.0), 0.502_886_5));
        // Below the knee, the linear segment c/12.92.
        assert!(close(
            host_srgb_to_linear(2.0 / 255.0),
            (2.0 / 255.0) / 12.92
        ));
    }

    #[test]
    fn encode_is_the_inverse_of_decode() {
        for &x in &[0.0_f32, 0.001, 0.05, 0.25, 0.5, 0.75, 1.0] {
            let round_trip = host_srgb_to_linear(host_linear_to_srgb(x));
            assert!(close(round_trip, x), "round-trip failed at {x}");
        }
    }

    #[test]
    fn shader_sources_carry_the_canonical_gamma_constants() {
        // A textual guard tying the Tier-0 numeric mirror above to the actual shader strings: if a
        // port edits a constant, this fails alongside the numeric test.
        assert!(GAUSSIAN_FRAG.contains("0.04045"));
        assert!(GAUSSIAN_FRAG.contains("12.92"));
        assert!(GAUSSIAN_FRAG.contains("1.055"));
        assert!(GAUSSIAN_FRAG.contains("2.4"));
        assert!(COMPOSITE_FRAG.contains("0.0031308"));
        assert!(COMPOSITE_FRAG.contains("12.92"));
        // The premultiplied output contract (DESIGN §2f): encode-then-cover, out_a == the effective
        // coverage. With the surface-global fade (PRESENCE.md), that weight is `coverage * u_opacity`
        // folded into both rgb and alpha — `vec4(rgb * a, a)` with `a = coverage * u_opacity`.
        assert!(COMPOSITE_FRAG.contains("coverage * u_opacity"));
        assert!(COMPOSITE_FRAG.contains("rgb * a, a"));
    }

    #[test]
    fn blur_passes_share_the_bottom_left_fullscreen_vertex_shader() {
        // The GL-origin re-derivation (DESIGN §5): clip-Y is y*2-1, NOT the WGSL 1-y*2. A reviewer
        // (and this test) treats any `1.0 - y` here as the y-flip regression.
        assert!(FULLSCREEN_VERT.contains("y * 2.0 - 1.0"));
        assert!(!FULLSCREEN_VERT.contains("1.0 - y"));
        assert!(COMPOSITE_VERT.contains("y * 2.0 - 1.0"));
        assert!(!COMPOSITE_VERT.contains("1.0 - y"));
    }
}
