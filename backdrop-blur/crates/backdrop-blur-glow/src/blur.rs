//! The blur itself: `prepare` resolves the request into an owned [`GlPrepared`] and ensures the
//! scratch; `record` replays the blur passes (separable Gaussian or dual-Kawase) into that scratch
//! and then composites the frosted surface into the captured target — leaving every GL binding it
//! touched as it found it (the [`SavedGlState`] save/restore, DESIGN §11).
//!
//! # Coordinate model (load-bearing — DESIGN §5)
//!
//! Everything is GL **bottom-left**, no flip. The grab texture's row 0 is the framebuffer's bottom
//! row (a `copyTexSubImage2D` from a bottom-left FB); the blur-pass vertex shader is a Y-identity;
//! the composite reads `gl_FragCoord` (GL bottom-origin) against bottom-left `rect_origin`. No
//! `height − y` appears anywhere.
//!
//! # Divergence from the wgpu pass structure (recorded)
//!
//! wgpu's `source` is the host's **full** intermediate, so its Gaussian horizontal pass / Kawase
//! prefilter remap a full-scratch `[0,1]` onto the clipped sub-rect *within* that full source. The
//! glow grab (2c) instead extracts **only the clipped region** into a texture sized to it, so the
//! first pass samples it with the **identity** uv remap and `texel_size = 1 / clipped.size`. The
//! `backdrop_uv_remap` machinery is kept (it is the structural clip guard and is identity when the
//! adapter already clipped before grabbing) so the composite still registers a screen-edge-clipped
//! panel 1:1.

use crate::composite::{self, CompositeParams};
use crate::scratch::GlScratch;
use crate::{FramebufferSize, GlowBlur, GrabSource};
use backdrop_blur_core::{
    BackdropBlur, BlurError, BlurRequest, FrostEffect, GlRegion, GrabPass, PingPongKey,
    ResolvedMask, backdrop_uv_remap, kawase_halfpixel, kawase_level_size, resolve_gaussian,
    resolve_kawase_levels, use_dual_kawase,
};
use glow::HasContext;

/// The owned per-call payload `prepare` resolves and `record` consumes. Borrows nothing from
/// [`GlowBlur`] — it holds `Copy` GL handles (the grab texture, the scratch slots) and resolved,
/// GPU-free parameters. Mirrors wgpu's `WgpuPrepared`.
pub struct GlPrepared {
    /// The grabbed backdrop the first blur pass samples (gamma RGBA8, sized to the clipped region).
    source: glow::Texture,
    /// The blur algorithm + its resolved per-pass parameters and the scratch slots to replay over.
    pass: GlBlurPass,
    /// The resolved composite inputs (bottom-left rect, tint, remap, mask, framebuffer size).
    composite: CompositeParams,
    /// The blurrer's generation counter at `prepare` time. `record` `debug_assert`s it still matches
    /// the blurrer's, catching a `Prepared` invalidated by a later `prepare` against the shared
    /// scratch (the K1 single-surface serial contract — seam.rs).
    generation: u64,
}

/// The resolved blur, mirroring wgpu's `PreparedBlur` enum (a typed pass plan, **not** a bare
/// `Vec<f32>`): either a separable Gaussian or a dual-Kawase pyramid. Each variant carries the
/// scratch slots its passes render through and the resolved sampling parameters.
enum GlBlurPass {
    /// Horizontal then vertical Gaussian: H samples the grab (decode) into `scratch[0]` (A); V
    /// samples A into `scratch[1]` (B); the composite reads B.
    Gaussian {
        /// `[A, B]` — two same-size linear slots.
        scratch: [GlScratch; 2],
        /// `1 / clipped.size` — the per-tap texel step (both passes sample a clipped-size texture).
        texel_size: [f32; 2],
        /// The Gaussian standard deviation.
        sigma: f32,
        /// Taps each side of center (clamped backend-side to `MAX_GAUSSIAN_RADIUS`).
        taps: i32,
    },
    /// Prefilter (decode + copy the grab into mip 0) → `N` downsamples → `N` upsamples back to mip
    /// 0; the composite reads mip 0. `halfpixels` carries one offset per down then up pass, the
    /// **size of the level being SAMPLED** (KWin's convention), so `record` only iterates.
    DualKawase {
        /// `N + 1` decreasing-size linear slots (level 0 = full, level `i` = halved).
        pyramid: Vec<GlScratch>,
        /// `N` downsample half-pixels: pass `i` samples level `i` (the larger), writes level `i+1`.
        down_halfpixels: Vec<[f32; 2]>,
        /// `N` upsample half-pixels: pass `j` samples level `N-j` (the smaller), writes `N-1-j`.
        up_halfpixels: Vec<[f32; 2]>,
    },
}

// --- The seam: BackdropBlur ---

impl BackdropBlur for GlowBlur {
    type Device = glow::Context;
    type Queue = ();
    type CommandSink = glow::Context;
    type SourceTexture = GrabSource;
    type Target = Option<glow::Framebuffer>;
    type TargetSpec = FramebufferSize;
    type Prepared = GlPrepared;

    fn prepare(
        &mut self,
        device: &Self::Device,
        _queue: &Self::Queue,
        source: &Self::SourceTexture,
        target_spec: Self::TargetSpec,
        request: &BlurRequest,
    ) -> Result<Option<Self::Prepared>, BlurError> {
        // The grab is already the clipped region; clip_to against the framebuffer is the identity
        // in the normal path and the no-op guard when the request region was fully offscreen.
        let Some(clipped) = request.source_region.clip_to(target_spec.0) else {
            return Ok(None);
        };
        // The target-side half of the same no-op: the composite divides by the target size
        // (composite.frag:45 — `rect_uv = (px - u_rect_origin_px) / u_rect_size_px`), so a zero
        // dimension would yield NaN/Inf UVs blended at ~50% alpha along a visible line. An empty
        // target is the same valid no-op as an empty source, not an error (DESIGN §9); through
        // `frost_region` it reports `FrostEffect::ClippedEmpty` — consistent with its meaning.
        if request.target_rect.is_empty() {
            return Ok(None);
        }

        let gl = device;
        // Advance the scratch frame + evict stale chains before (re)allocating this frame's chain.
        self.scratch.begin_frame(gl);
        // Bump the generation: this `prepare` invalidates any outstanding `GlPrepared` (the shared
        // scratch is now keyed to this call). `record` debug-asserts the match (K1).
        self.generation = self.generation.wrapping_add(1);

        let [clip_w, clip_h] = [clipped.size[0] as f32, clipped.size[1] as f32];
        let radius = request.physical_blur_radius();
        let format = self.profile.renderable_float;

        let pass = if use_dual_kawase(radius) {
            let levels = resolve_kawase_levels(radius);
            let key = PingPongKey {
                size: clipped.size,
                levels,
            };
            let pyramid = self.scratch.ensure_pyramid(gl, key, format)?;
            let n = levels as usize;
            // Downsample i samples level i (the larger of the pair); upsample j samples level N-j
            // (the smaller). Half-pixel is always 0.5 / size(sampled level) — KWin's convention.
            let down_halfpixels = (0..n)
                .map(|i| kawase_halfpixel(kawase_level_size(clipped.size, i as u32)))
                .collect();
            let up_halfpixels = (0..n)
                .map(|j| kawase_halfpixel(kawase_level_size(clipped.size, (n - j) as u32)))
                .collect();
            GlBlurPass::DualKawase {
                pyramid,
                down_halfpixels,
                up_halfpixels,
            }
        } else {
            let kernel = resolve_gaussian(radius);
            let key = PingPongKey {
                size: clipped.size,
                levels: 1,
            };
            let scratch = self.scratch.ensure_gaussian(gl, key, format)?;
            GlBlurPass::Gaussian {
                scratch,
                texel_size: [1.0 / clip_w, 1.0 / clip_h],
                sigma: kernel.sigma,
                taps: kernel.tap_radius,
            }
        };

        let (backdrop_uv_offset, backdrop_uv_scale) =
            backdrop_uv_remap(&request.source_region, &clipped);
        let mask = ResolvedMask::from_target(&request.target_rect, request.corner_radius);
        let composite = CompositeParams::new(
            [
                request.target_rect.origin[0] as f32,
                request.target_rect.origin[1] as f32,
            ],
            [
                request.target_rect.size[0] as f32,
                request.target_rect.size[1] as f32,
            ],
            request.tint.color(),
            backdrop_uv_offset,
            backdrop_uv_scale,
            mask,
            target_spec.0,
            request.presence.value(),
        );

        Ok(Some(GlPrepared {
            source: source.texture,
            pass,
            composite,
            generation: self.generation,
        }))
    }

    fn record(
        &self,
        sink: &mut Self::CommandSink,
        target: &Self::Target,
        prepared: Self::Prepared,
    ) -> Result<(), BlurError> {
        // The seam hands the sink by `&mut` (wgpu's owned CommandEncoder shape); glow draws
        // through the context immediately, so the real work is in `record_shared` (which an
        // eframe adapter holding an `Arc<glow::Context>` can also reach — see [`Self::frost_region`]).
        self.record_shared(sink, target, &prepared)
    }
}

// --- GrabPass: glow produces the source from the live framebuffer ---

impl GrabPass for GlowBlur {
    type Framebuffer = Option<glow::Framebuffer>;

    fn grab_source(
        &mut self,
        device: &Self::Device,
        _queue: &Self::Queue,
        framebuffer: &Self::Framebuffer,
        region: GlRegion,
    ) -> Result<Self::SourceTexture, BlurError> {
        // The grab (2c) consumes the bottom-left region directly — no flip (DESIGN §5). It returns
        // only the grabbed texture: the composite's *full* framebuffer size is the adapter's to know
        // (it holds the true screen size), and it passes that to `prepare` as the `TargetSpec`
        // ([`FramebufferSize`]). So `grab_source` never fabricates a size from the region.
        let texture = self.grab(device, *framebuffer, region)?;
        Ok(GrabSource { texture })
    }
}

// --- Shared-context entry (the eframe-on-glow adapter path) ---

impl GlowBlur {
    /// The shared-context grab-pass entry an `eframe`-on-glow adapter uses. eframe holds the GL
    /// context in an `Arc<glow::Context>`, so it cannot produce the `&mut glow::Context` the seam's
    /// [`record`](BackdropBlur::record) wants (a wgpu-shaped wart — glow draws immediate-mode on a
    /// shared context, never needing exclusivity). This runs grab → prepare → record for one surface
    /// in a paint callback, taking the context by shared `&`.
    ///
    /// `target` is the live draw framebuffer ([`current_draw_framebuffer`](crate::current_draw_framebuffer)):
    /// both the grab read source (what the host just rendered) and the composite destination.
    /// `framebuffer_size` is the **true** screen size in physical px (the composite viewport); the
    /// adapter holds it. Returns [`FrostEffect::ClippedEmpty`] doing nothing when the region clips
    /// to nothing — or the request's `target_rect` is zero-area (`Region::is_empty`), refused here
    /// before the grab runs (the seam's `prepare → Ok(None)` no-op, surfaced instead of swallowed).
    pub fn frost_region(
        &mut self,
        gl: &glow::Context,
        target: Option<glow::Framebuffer>,
        region: GlRegion,
        framebuffer_size: crate::FramebufferSize,
        request: &BlurRequest,
    ) -> Result<FrostEffect, BlurError> {
        // A zero-area target is a guaranteed no-op: `prepare` would refuse it anyway (its own
        // guard stays — the defense is total), but the grab — a real glCopyTexSubImage plus a
        // possible grab-texture realloc — should not run for it. Only a caller assembling its own
        // `BlurRequest` can reach this: the egui adapter derives `region` and `target_rect` from
        // one rect, so its empty targets never arrive here.
        if request.target_rect.is_empty() {
            return Ok(FrostEffect::ClippedEmpty);
        }
        let source = self.grab_source(gl, &(), &target, region)?;
        match self.prepare(gl, &(), &source, framebuffer_size, request)? {
            Some(prepared) => {
                self.record_shared(gl, &target, &prepared)?;
                Ok(FrostEffect::Composited)
            }
            None => Ok(FrostEffect::ClippedEmpty),
        }
    }

    /// The real `record` body, taking the context by shared `&` (glow's reality). The seam's
    /// `&mut`-sink [`record`](BackdropBlur::record) and [`frost_region`](Self::frost_region) both
    /// delegate here. Saves every GL binding it perturbs, runs the passes + composite, then restores
    /// (DESIGN §11).
    fn record_shared(
        &self,
        gl: &glow::Context,
        target: &Option<glow::Framebuffer>,
        prepared: &GlPrepared,
    ) -> Result<(), BlurError> {
        debug_assert_eq!(
            prepared.generation, self.generation,
            "GlPrepared is stale: a later prepare clobbered the shared scratch before this handle \
             was recorded (K1 single-surface serial contract)"
        );
        // Save every binding the blur perturbs, run the passes + composite, then restore.
        let saved = SavedGlState::capture(gl);
        self.run_blur(gl, prepared);
        // Composite into the captured target (the live draw FBO). The composite samples the final
        // linear scratch: Gaussian B, or Kawase mip 0.
        // SAFETY: `*target` is the caller's captured draw framebuffer (None = default FB 0); binding
        // it on the current context is sound. This bind is now **load-bearing for the encode
        // decision**: `resolve_target_encoding` queries the just-bound draw FBO's colour-attachment
        // encoding, so it must run after this bind — it does.
        unsafe { gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, *target) };
        let target_encoding = composite::resolve_target_encoding(gl, &self.profile, *target);
        composite::draw(
            gl,
            &self.programs,
            self.vao,
            prepared.final_scratch(),
            &prepared.composite,
            target_encoding,
        );
        saved.restore(gl);
        Ok(())
    }
}

// --- Pass replay ---

impl GlowBlur {
    /// Replay a prepared blur's passes into its scratch (Gaussian H/V, or Kawase prefilter →
    /// downsamples → upsamples). All passes draw the shared fullscreen triangle with no blend; the
    /// composite (separate) is the only blended draw.
    fn run_blur(&self, gl: &glow::Context, prepared: &GlPrepared) {
        // SAFETY: the current context (record's contract); `vao`/`programs` are live handles created
        // by `new`; `prepared`'s scratch slots and source texture are live `Copy` handles resolved
        // by the matching `prepare`. Blend is disabled for the blur passes (each pass overwrites its
        // target); the caller's save/restore re-establishes the host's blend state afterwards.
        unsafe {
            gl.disable(glow::BLEND);
            gl.disable(glow::SCISSOR_TEST);
            gl.bind_vertex_array(Some(self.vao));
        }
        match &prepared.pass {
            GlBlurPass::Gaussian {
                scratch,
                texel_size,
                sigma,
                taps,
            } => {
                let [a, b] = *scratch;
                // Pass 1 (horizontal): sample the grab (gamma) with decode, blur along x → A.
                self.gaussian_pass(
                    gl,
                    a,
                    prepared.source,
                    *texel_size,
                    [1.0, 0.0],
                    *sigma,
                    *taps,
                    true,
                );
                // Pass 2 (vertical): sample A (linear), blur along y → B.
                self.gaussian_pass(
                    gl,
                    b,
                    a.texture,
                    *texel_size,
                    [0.0, 1.0],
                    *sigma,
                    *taps,
                    false,
                );
            }
            GlBlurPass::DualKawase {
                pyramid,
                down_halfpixels,
                up_halfpixels,
            } => {
                let n = down_halfpixels.len();
                // Prefilter: decode + copy the grab into mip 0 (the Gaussian shader at radius 0).
                // texel_size is unused at radius 0 (no taps), so pass a dummy.
                let mip0 = pyramid[0];
                self.gaussian_pass(
                    gl,
                    mip0,
                    prepared.source,
                    [0.0, 0.0],
                    [1.0, 0.0],
                    0.5,
                    0,
                    true,
                );
                // Downsample i: level i (sampled) → level i+1.
                for (i, &hp) in down_halfpixels.iter().enumerate() {
                    self.kawase_pass(
                        gl,
                        pyramid[i + 1],
                        pyramid[i].texture,
                        self.programs.downsample,
                        hp,
                    );
                }
                // Upsample j: level n-j (sampled) → level n-1-j, ending at mip 0.
                for (j, &hp) in up_halfpixels.iter().enumerate() {
                    self.kawase_pass(
                        gl,
                        pyramid[n - 1 - j],
                        pyramid[n - j].texture,
                        self.programs.upsample,
                        hp,
                    );
                }
            }
        }
    }

    /// One separable-Gaussian pass: render the fullscreen triangle into `dst`, sampling `src` along
    /// `direction` with `taps` taps of stddev `sigma`. `decode` decodes sRGB→linear on sample (pass
    /// 1 / prefilter only). The uv remap is identity — the glow grab is already the clipped region.
    #[expect(
        clippy::too_many_arguments,
        reason = "a GL draw call's parameters are inherently many; bundling them into a struct would \
                  add an indirection that obscures the one-pass-per-call shape"
    )]
    fn gaussian_pass(
        &self,
        gl: &glow::Context,
        dst: GlScratch,
        src: glow::Texture,
        texel_size: [f32; 2],
        direction: [f32; 2],
        sigma: f32,
        taps: i32,
        decode: bool,
    ) {
        let program = self.programs.gaussian;
        // SAFETY: current context; `dst.fbo`/`src`/`program`/`vao` are live handles. The draw target
        // is `dst.fbo`; uniforms come from `program`; a `None` uniform location is a documented
        // no-op (an unused uniform may be optimized out). The viewport is set to the destination
        // size so the triangle covers exactly the slot.
        unsafe {
            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(dst.fbo()));
            let [dw, dh] = dst.viewport();
            gl.viewport(0, 0, dw, dh);
            gl.use_program(Some(program));
            let loc = |name: &str| gl.get_uniform_location(program, name);
            // Identity uv remap (the grab IS the clipped region — divergence from wgpu, module doc).
            gl.uniform_2_f32(loc("u_uv_offset").as_ref(), 0.0, 0.0);
            gl.uniform_2_f32(loc("u_uv_scale").as_ref(), 1.0, 1.0);
            gl.uniform_2_f32(loc("u_texel_size").as_ref(), texel_size[0], texel_size[1]);
            gl.uniform_2_f32(loc("u_direction").as_ref(), direction[0], direction[1]);
            gl.uniform_1_f32(loc("u_sigma").as_ref(), sigma);
            gl.uniform_1_i32(loc("u_radius").as_ref(), taps);
            gl.uniform_1_i32(loc("u_decode_srgb").as_ref(), i32::from(decode));
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(src));
            gl.uniform_1_i32(loc("u_src").as_ref(), 0);
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
        }
    }

    /// One dual-Kawase pass (down or up): render the fullscreen triangle into `dst`, sampling `src`
    /// with `program` (downsample ÷8 or upsample ÷12) at half-pixel `halfpixel`. The viewport is the
    /// destination level's size.
    fn kawase_pass(
        &self,
        gl: &glow::Context,
        dst: GlScratch,
        src: glow::Texture,
        program: glow::Program,
        halfpixel: [f32; 2],
    ) {
        // SAFETY: current context; `dst.fbo`/`src`/`program`/`vao` are live handles. Draw target is
        // `dst.fbo`; uniforms come from `program`; a `None` location is a documented no-op.
        unsafe {
            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(dst.fbo()));
            let [dw, dh] = dst.viewport();
            gl.viewport(0, 0, dw, dh);
            gl.use_program(Some(program));
            let loc = |name: &str| gl.get_uniform_location(program, name);
            gl.uniform_2_f32(loc("u_halfpixel").as_ref(), halfpixel[0], halfpixel[1]);
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, Some(src));
            gl.uniform_1_i32(loc("u_src").as_ref(), 0);
            gl.draw_arrays(glow::TRIANGLES, 0, 3);
        }
    }
}

impl GlPrepared {
    /// The final blurred scratch the composite samples: Gaussian B (slot 1), or Kawase mip 0.
    fn final_scratch(&self) -> glow::Texture {
        match &self.pass {
            GlBlurPass::Gaussian { scratch, .. } => scratch[1].texture,
            GlBlurPass::DualKawase { pyramid, .. } => pyramid[0].texture,
        }
    }
}

/// The full GL state the `record` save/restore covers (DESIGN §11): the bound draw + read FBOs,
/// viewport, scissor box + enable, blend enable + func/equation, the active texture unit + the
/// `TEXTURE_2D` binding on unit 0, the bound program, and the bound VAO. Captured at `record`
/// entry, restored at exit, so the host's state is left exactly as found.
struct SavedGlState {
    draw_fbo: Option<glow::Framebuffer>,
    read_fbo: Option<glow::Framebuffer>,
    viewport: [i32; 4],
    scissor_box: [i32; 4],
    scissor_enabled: bool,
    blend_enabled: bool,
    blend_src_rgb: i32,
    blend_dst_rgb: i32,
    blend_src_alpha: i32,
    blend_dst_alpha: i32,
    blend_equation_rgb: i32,
    blend_equation_alpha: i32,
    active_texture: i32,
    texture_2d_unit0: Option<glow::Texture>,
    program: Option<glow::Program>,
    vao: Option<glow::VertexArray>,
}

impl SavedGlState {
    /// Read every binding `record` will perturb. Pure GL queries; mutates no state.
    fn capture(gl: &glow::Context) -> Self {
        // SAFETY: read-only GL state queries on the current context (record's contract); none take a
        // caller pointer except the array getters, which write exactly their 4-element buffers.
        unsafe {
            let mut viewport = [0_i32; 4];
            gl.get_parameter_i32_slice(glow::VIEWPORT, &mut viewport);
            let mut scissor_box = [0_i32; 4];
            gl.get_parameter_i32_slice(glow::SCISSOR_BOX, &mut scissor_box);
            // Read texture unit 0's binding: make it active first, restore the active unit after.
            let active_texture = gl.get_parameter_i32(glow::ACTIVE_TEXTURE);
            gl.active_texture(glow::TEXTURE0);
            let texture_2d_unit0 = gl.get_parameter_texture(glow::TEXTURE_BINDING_2D);
            // SAFETY note: active_texture is u32 GL enum; restore it as read.
            gl.active_texture(active_texture as u32);
            Self {
                draw_fbo: gl.get_parameter_framebuffer(glow::DRAW_FRAMEBUFFER_BINDING),
                read_fbo: gl.get_parameter_framebuffer(glow::READ_FRAMEBUFFER_BINDING),
                viewport,
                scissor_box,
                scissor_enabled: gl.is_enabled(glow::SCISSOR_TEST),
                blend_enabled: gl.is_enabled(glow::BLEND),
                blend_src_rgb: gl.get_parameter_i32(glow::BLEND_SRC_RGB),
                blend_dst_rgb: gl.get_parameter_i32(glow::BLEND_DST_RGB),
                blend_src_alpha: gl.get_parameter_i32(glow::BLEND_SRC_ALPHA),
                blend_dst_alpha: gl.get_parameter_i32(glow::BLEND_DST_ALPHA),
                blend_equation_rgb: gl.get_parameter_i32(glow::BLEND_EQUATION_RGB),
                blend_equation_alpha: gl.get_parameter_i32(glow::BLEND_EQUATION_ALPHA),
                active_texture,
                texture_2d_unit0,
                program: gl.get_parameter_program(glow::CURRENT_PROGRAM),
                vao: gl.get_parameter_vertex_array(glow::VERTEX_ARRAY_BINDING),
            }
        }
    }

    /// Restore every captured binding, leaving GL state as `capture` found it.
    fn restore(&self, gl: &glow::Context) {
        // SAFETY: every value was read from this current context by `capture`; restoring them is the
        // inverse of the perturbation `record` applied. `blend_func_separate` takes the saved RGB
        // and alpha factors and `blend_equation_separate` the saved RGB/alpha equations, so an
        // asymmetric host blend (factors *and* equation) is restored exactly.
        unsafe {
            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, self.draw_fbo);
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, self.read_fbo);
            gl.viewport(
                self.viewport[0],
                self.viewport[1],
                self.viewport[2],
                self.viewport[3],
            );
            gl.scissor(
                self.scissor_box[0],
                self.scissor_box[1],
                self.scissor_box[2],
                self.scissor_box[3],
            );
            set_enabled(gl, glow::SCISSOR_TEST, self.scissor_enabled);
            set_enabled(gl, glow::BLEND, self.blend_enabled);
            gl.blend_func_separate(
                self.blend_src_rgb as u32,
                self.blend_dst_rgb as u32,
                self.blend_src_alpha as u32,
                self.blend_dst_alpha as u32,
            );
            gl.blend_equation_separate(
                self.blend_equation_rgb as u32,
                self.blend_equation_alpha as u32,
            );
            gl.active_texture(glow::TEXTURE0);
            gl.bind_texture(glow::TEXTURE_2D, self.texture_2d_unit0);
            gl.active_texture(self.active_texture as u32);
            gl.use_program(self.program);
            gl.bind_vertex_array(self.vao);
        }
    }
}

/// Enable or disable a GL capability to match a saved boolean.
fn set_enabled(gl: &glow::Context, capability: u32, enabled: bool) {
    // SAFETY: current context; `enable`/`disable` toggle a documented capability enum in place.
    unsafe {
        if enabled {
            gl.enable(capability);
        } else {
            gl.disable(capability);
        }
    }
}

#[cfg(all(test, feature = "gl-snapshots", not(target_arch = "wasm32")))]
#[path = "blur_tests.rs"]
mod gl_tests;
