//! `backdrop-blur-glow` — the **glow** (OpenGL 3.3 / GLES 3.0 / WebGL2) grab-pass backend for
//! [`backdrop-blur`]. It is the path the own-loop/wgpu backend never served: real frosted glass
//! for an `eframe`-on-glow app and the `cage` Wayland kiosk, where the host owns the GL loop and
//! the blur must **grab** a region of the live framebuffer rather than receive a ready-made
//! intermediate. The version triple is a construction-time contract: [`GlowBlur::new`] refuses an
//! older context with [`BlurError::UnsupportedContext`].
//!
//! # The one `unsafe` crate
//!
//! Every other crate in the workspace is `#![forbid(unsafe_code)]`. This one cannot be: glow's
//! API is `unsafe` end to end (raw GL is unsynchronized global state). The `unsafe` is
//! **quarantined here** and held to two rules Abdu signed off (DESIGN §11):
//!
//! - `#![deny(unsafe_op_in_unsafe_fn)]` — an `unsafe fn` body gets no free pass; every GL call
//!   still needs an explicit `unsafe` block with a `// SAFETY:` justification.
//! - `#![deny(clippy::undocumented_unsafe_blocks)]` — every `unsafe` block must carry that
//!   comment, so a missing justification fails the build rather than slips through review.
//! - **No GL in `Drop`.** GL objects are freed only by an explicit [`GlowBlur::destroy`] the host
//!   calls from `eframe::App::on_exit` (where the context is still current). `Drop` issues no GL —
//!   a dropped-without-destroy blurrer `log::warn!`s and leaks rather than calling GL on a
//!   possibly-gone context (undefined behavior).
//!
//! # Portability
//!
//! glow is build-script-free (runtime-loaded function pointers), so this crate **compiles on any
//! runner with no GL present** and is a normal workspace member. Everything that needs a live
//! context — the EGL-surfaceless native harness and every readback test — sits behind the
//! `gl-snapshots` feature, so plain `cargo test --workspace` runs only this crate's Tier-0 (pure)
//! tests and stays GPU-free.
//!
//! [`backdrop-blur`]: https://github.com/abdu-benayad/backdrop-blur
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(clippy::undocumented_unsafe_blocks)]

mod blur;
mod composite;
mod grab;
mod profile;
mod program;
mod scratch;

// The EGL-surfaceless headless-GL harness — shared test scaffolding for this crate's `gl_tests`
// AND the egui adapter's gated tests. Doc-hidden `pub` because it is test scaffolding, not library
// API. Native-only (no EGL on wasm) and `gl-snapshots`-gated, so it is compiled only under the
// gated feature and never in a default build.
#[cfg(all(feature = "gl-snapshots", not(target_arch = "wasm32")))]
#[doc(hidden)]
pub mod gl_harness;

use backdrop_blur_core::{BlurError, BlurStage};
use glow::HasContext;
use program::Programs;
use scratch::ScratchCache;

pub use blur::GlPrepared;
pub use profile::{GlProfile, RenderableFloat, ShaderClass};

/// Capture the host's currently-bound **draw** framebuffer — the live target a grab-pass adapter
/// reads the backdrop from (what the host just rendered) and composites the frosted surface into.
/// `None` is the default framebuffer (0). A safe wrapper so a `#![forbid(unsafe_code)]` adapter (the
/// `backdrop-blur-egui` grab-pass path) can capture `GL_DRAW_FRAMEBUFFER_BINDING` without writing its
/// own `unsafe`; pass the result to both [`GrabPass::grab_source`] and [`GlowBlur::frost_region`].
///
/// [`GrabPass::grab_source`]: backdrop_blur_core::GrabPass::grab_source
pub fn current_draw_framebuffer(gl: &glow::Context) -> Option<glow::Framebuffer> {
    // SAFETY: a read-only query of GL_DRAW_FRAMEBUFFER_BINDING on the current context (the crate's
    // context-is-current contract). It takes no caller pointer and mutates no GL state.
    unsafe { gl.get_parameter_framebuffer(glow::DRAW_FRAMEBUFFER_BINDING) }
}

/// The full draw-target size in physical pixels — the composite viewport (`glViewport(0,0,fb_w,
/// fb_h)`), passed to [`BackdropBlur::prepare`] as the glow backend's `TargetSpec`. The composite
/// needs it because the AA band outside the panel is only generated under a full-framebuffer
/// `gl_FragCoord` (DESIGN §10), and the grabbed texture alone cannot tell the backend the screen
/// size. Making it a **required** `prepare` input (rather than a field on [`GrabSource`] the adapter
/// must remember to override) turns a missing screen size into a compile error, not a silent AA
/// regression: the egui adapter passes the true screen size it holds.
///
/// [`BackdropBlur::prepare`]: backdrop_blur_core::BackdropBlur::prepare
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FramebufferSize(pub [u32; 2]);

/// The grab-pass source the glow backend's [`BackdropBlur::prepare`] consumes: just the grabbed
/// backdrop texture. The full framebuffer size is **not** carried here — it is a required `prepare`
/// input ([`FramebufferSize`], the backend's `TargetSpec`) the egui adapter supplies from the true
/// screen size, so a missing screen size is a compile error rather than a fabricated-then-overridden
/// field. Mirrors wgpu's `SourceView`, minus the size (which wgpu folds into the view).
///
/// [`BackdropBlur::prepare`]: backdrop_blur_core::BackdropBlur::prepare
#[derive(Clone, Copy)]
pub struct GrabSource {
    /// The grabbed, sampleable backdrop (gamma RGBA8, sized to the clipped region).
    pub texture: glow::Texture,
}

/// The glow backend's cross-frame GL resources: the compiled blur/composite programs and the
/// shared fullscreen-triangle VAO (and, from later steps, the ping-pong scratch and grab
/// textures). Holds **no** `glow::Context` — the context is passed per call, so a `GlowBlur` is a
/// plain bag of GL handles and is `Send` (the host owns one per render thread).
///
/// # Teardown contract (DESIGN §11, Abdu-signed-off)
///
/// GL objects are freed only by [`GlowBlur::destroy`], which the host calls from
/// `eframe::App::on_exit` while the context is still current. [`Drop`] issues **no GL** — a
/// blurrer dropped without `destroy` `log::warn!`s and leaks rather than calling GL on a
/// possibly-destroyed context (undefined behavior). `destroy` is idempotent.
pub struct GlowBlur {
    profile: GlProfile,
    programs: Programs,
    /// The shared, empty VAO bound for every `gl_VertexID` fullscreen-triangle draw.
    vao: glow::VertexArray,
    /// Size-keyed grab target: the live-framebuffer region copy the blur samples. Lazily created
    /// on the first grab and reallocated when the region size changes (see [`grab`]).
    grab: Option<grab::GrabTarget>,
    /// The ping-pong scratch the blur passes render into (Gaussian chains + dual-Kawase pyramids),
    /// keyed by `PingPongKey` with last-frame-used eviction (see [`scratch`]).
    scratch: ScratchCache,
    /// Bumped once per `prepare`; stamped into the returned [`GlPrepared`] so `record` can
    /// `debug_assert` the handle was not invalidated by a later `prepare` against the shared scratch
    /// (the K1 single-surface serial contract — seam.rs). Mirrors wgpu's generation counter.
    generation: u64,
    /// Set by [`Self::destroy`]; gates `Drop`'s leak warning and makes `destroy` idempotent.
    destroyed: bool,
}

impl GlowBlur {
    /// Build the backend against a **live, current** GL context: probe its capabilities, compile +
    /// link the programs under the right shader dialect, and create the shared VAO. Refuses a
    /// context below the documented minimums — desktop GL 3.3, GLES 3.0, or WebGL 2.0 — with
    /// [`BlurError::UnsupportedContext`]. On any failure the partial GL state is cleaned up before
    /// returning. The version gate is best-effort: a driver whose `GL_VERSION` carries no parseable
    /// version number proceeds ungated and may still fail later, at shader compile.
    pub fn new(gl: &glow::Context) -> Result<Self, BlurError> {
        let profile = GlProfile::probe(gl)?;
        let programs = Programs::new(gl, &profile)?;
        // SAFETY: `gl` is current; `create_vertex_array` returns a fresh VAO or an error string,
        // taking no caller pointers.
        let vao = match unsafe { gl.create_vertex_array() } {
            Ok(vao) => vao,
            Err(e) => {
                programs.destroy(gl);
                return Err(BlurError::ResourceCreation {
                    stage: BlurStage::VertexArray,
                    source: e.into(),
                });
            }
        };
        Ok(Self {
            profile,
            programs,
            vao,
            grab: None,
            scratch: ScratchCache::new(),
            generation: 0,
            destroyed: false,
        })
    }

    /// Free every GL object this backend owns. The host calls it from `on_exit` while the context
    /// is current (DESIGN §11). Idempotent: a second call is a no-op.
    pub fn destroy(&mut self, gl: &glow::Context) {
        if self.destroyed {
            return;
        }
        self.destroy_grab(gl);
        self.scratch.destroy(gl);
        self.programs.destroy(gl);
        // SAFETY: `self.vao` was created by `new` on a context the caller guarantees is the same,
        // current one; it is deleted exactly once (the `destroyed` guard prevents a second delete).
        unsafe { gl.delete_vertex_array(self.vao) };
        self.destroyed = true;
    }
}

impl Drop for GlowBlur {
    fn drop(&mut self) {
        if !self.destroyed {
            // No GL here by contract — the context may already be gone. Warn (uncatchable) + leak.
            log::warn!(
                "GlowBlur dropped without destroy(&gl): GL objects leaked. Call \
                 GlowBlur::destroy from eframe::App::on_exit while the context is current \
                 (DESIGN §11)."
            );
        }
    }
}

/// Tier-1: the backend against a **real** surfaceless GL context (the `gl_harness`). Gated behind
/// `gl-snapshots` + native, so plain `cargo test --workspace` never compiles or runs it — absent on
/// a non-GL runner, not skipped (IMPL §14).
#[cfg(all(test, feature = "gl-snapshots", not(target_arch = "wasm32")))]
mod gl_tests {
    use crate::GlowBlur;
    use crate::gl_harness::{headless_gl, read_rgba8};
    use glow::HasContext;

    /// Harness self-check: the surfaceless context clears an FBO to a known color and reads it back.
    /// Isolates an EGL/harness failure from a backend-logic failure.
    #[test]
    fn harness_context_clears_and_reads_back() {
        let gl = headless_gl();
        let (w, h) = (64_i32, 64_i32);
        // SAFETY: standard FBO setup on the current context; every handle is created and freed here,
        // and framebuffer completeness is asserted before reading.
        unsafe {
            let fbo = gl.create_framebuffer().expect("create_framebuffer");
            gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
            let tex = gl.create_texture().expect("create_texture");
            gl.bind_texture(glow::TEXTURE_2D, Some(tex));
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::RGBA8 as i32,
                w,
                h,
                0,
                glow::RGBA,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(None),
            );
            gl.framebuffer_texture_2d(
                glow::FRAMEBUFFER,
                glow::COLOR_ATTACHMENT0,
                glow::TEXTURE_2D,
                Some(tex),
                0,
            );
            assert_eq!(
                gl.check_framebuffer_status(glow::FRAMEBUFFER),
                glow::FRAMEBUFFER_COMPLETE,
                "harness FBO incomplete"
            );
            gl.viewport(0, 0, w, h);
            gl.clear_color(0.2, 0.4, 0.6, 1.0);
            gl.clear(glow::COLOR_BUFFER_BIT);

            let px = read_rgba8(&gl, 32, 32); // ~ (51, 102, 153, 255), +/-1 for 8-bit rounding
            assert!(
                (px[0] as i32 - 51).abs() <= 1
                    && (px[1] as i32 - 102).abs() <= 1
                    && (px[2] as i32 - 153).abs() <= 1
                    && px[3] == 255,
                "clear-color readback was {px:?}"
            );

            gl.bind_framebuffer(glow::FRAMEBUFFER, None);
            gl.delete_framebuffer(fbo);
            gl.delete_texture(tex);
        }
    }

    /// 2b: `GlowBlur::new` compiles + links every program and creates the shared VAO on a real GL
    /// context (the GLSL actually compiles under the resolved `#version` header — a syntax error the
    /// Tier-0 gamma test can't catch). `destroy` frees them; a second `destroy` is an idempotent
    /// no-op.
    #[test]
    fn glow_blur_builds_all_programs_on_a_real_context() {
        let gl = headless_gl();
        let mut blur = GlowBlur::new(&gl)
            .expect("GlowBlur::new should compile+link all programs on a GL 3.3 context");
        blur.destroy(&gl);
        blur.destroy(&gl); // idempotent — must not double-free or panic
    }
}
