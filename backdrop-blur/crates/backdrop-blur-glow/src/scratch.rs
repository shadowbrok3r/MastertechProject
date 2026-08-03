//! The ping-pong scratch the blur passes render into: linear (`Rgba16F`, or the `Srgb8Rgba8`
//! fallback) textures + their FBOs, keyed by [`PingPongKey`]. The Gaussian path keys a
//! `levels == 1` chain of **two** same-size slots (horizontal → A, vertical → B); the dual-Kawase
//! path keys a `levels == N` **pyramid** of `N + 1` decreasing-size slots (level 0 = full, level
//! `i` = `base >> i`). One cache holds both — `PingPongKey.levels` disambiguates — so a dragged or
//! DPI-changing panel reuses a slot of its size rather than reallocating every frame.
//!
//! **The two kinds never collide on a key.** A Gaussian chain is always `levels == 1`; a dual-Kawase
//! pyramid is always `levels >= 2`. The disjointness is structural, not a coincidence: the Kawase
//! path is taken only when `use_dual_kawase(radius)` (i.e. `radius >= 16`), and at that radius
//! `resolve_kawase_levels` is `round(log2(16)) == 4` and only grows — so it is never `1`. A same-size
//! Gaussian chain and Kawase pyramid therefore hash to different keys and cannot alias the other's
//! slots. The `ensure_*` `debug_assert`s below pin this invariant.
//!
//! **Eviction is last-frame-used, and the decision is pure.** Each cached chain records the frame
//! it was last touched; [`evict_decision`] (a GPU-free function, Tier-0 tested) decides which keys
//! are stale given the current frame and the retention window. The cache calls it, then deletes the
//! GL objects for the returned keys. As with every GL object in this crate, scratch is freed by an
//! explicit destroy path ([`Self::destroy`] / eviction), **never in `Drop`** (DESIGN §11).

use crate::profile::RenderableFloat;
use backdrop_blur_core::{
    BlurError, BlurStage, PingPongKey, RETENTION_FRAMES, evict_decision, kawase_level_size,
};
use glow::HasContext;
use std::collections::HashMap;

/// One ping-pong slot: the sampled texture and the FBO it is rendered into. `Copy` — these are
/// plain GL handles.
#[derive(Clone, Copy)]
pub(crate) struct GlScratch {
    /// The linear-light texture a pass samples / the composite reads.
    pub(crate) texture: glow::Texture,
    /// `texture` attached as color 0 — the render target for the pass that writes this slot.
    fbo: glow::Framebuffer,
    /// The slot's allocated pixel size — the viewport a pass writing this slot must set (Kawase
    /// levels differ in size, so a pass cannot assume the full chain size).
    size: [u32; 2],
}

/// A keyed scratch chain: the per-level slots plus the frame it was last used (for eviction).
struct ScratchChain {
    /// Gaussian: `[A, B]` (two same-size slots). Dual-Kawase: `N + 1` decreasing-size slots.
    slots: Vec<GlScratch>,
    /// The frame index this chain was last touched by `ensure_*`; drives last-frame-used eviction.
    last_used_frame: u64,
}

/// The scratch cache: every keyed chain plus the running frame counter eviction reads. Owned by
/// [`crate::GlowBlur`]; freed only by [`Self::destroy`] (never `Drop`).
pub(crate) struct ScratchCache {
    chains: HashMap<PingPongKey, ScratchChain>,
    /// Bumped once per `prepare`; the "now" eviction compares `last_used_frame` against.
    frame: u64,
}

// --- Constructors ---

impl ScratchCache {
    pub(crate) fn new() -> Self {
        Self {
            chains: HashMap::new(),
            frame: 0,
        }
    }
}

// --- Frame lifecycle + lookup ---

impl ScratchCache {
    /// Advance to the next frame and evict chains untouched for [`RETENTION_FRAMES`]. Called once
    /// at the top of each `prepare`, before `ensure_*`, so the just-touched chain is never evicted.
    pub(crate) fn begin_frame(&mut self, gl: &glow::Context) {
        self.frame = self.frame.wrapping_add(1);
        let stale = evict_decision(
            self.chains.iter().map(|(k, c)| (*k, c.last_used_frame)),
            self.frame,
            RETENTION_FRAMES,
        );
        for key in stale {
            if let Some(chain) = self.chains.remove(&key) {
                delete_chain(gl, &chain.slots);
            }
        }
    }

    /// The Gaussian ping-pong for `key` (`key.levels == 1`): two same-size slots `[A, B]`, created
    /// on first use and reused after. Marks the chain used this frame.
    pub(crate) fn ensure_gaussian(
        &mut self,
        gl: &glow::Context,
        key: PingPongKey,
        format: RenderableFloat,
    ) -> Result<[GlScratch; 2], BlurError> {
        debug_assert_eq!(
            key.levels, 1,
            "a Gaussian chain keys levels == 1 (never collides with a Kawase pyramid)"
        );
        let slots = self.ensure_chain(gl, key, format, || vec![key.size, key.size])?;
        Ok([slots[0], slots[1]])
    }

    /// The dual-Kawase pyramid for `key` (`key.levels == N`): `N + 1` decreasing-size slots (level
    /// 0 = full `key.size`, level `i` = halved). Created on first use and reused after. Marks the
    /// chain used this frame.
    pub(crate) fn ensure_pyramid(
        &mut self,
        gl: &glow::Context,
        key: PingPongKey,
        format: RenderableFloat,
    ) -> Result<Vec<GlScratch>, BlurError> {
        debug_assert!(
            key.levels >= 2,
            "a dual-Kawase pyramid keys levels >= 2 (never collides with a Gaussian chain)"
        );
        self.ensure_chain(gl, key, format, || {
            (0..=key.levels)
                .map(|l| kawase_level_size(key.size, l))
                .collect()
        })
        .map(<[GlScratch]>::to_vec)
    }

    /// Shared core: return the cached chain for `key` (touching its frame), or build one whose slot
    /// sizes `sizes()` produces. On a partial build failure the slots already created are deleted.
    fn ensure_chain(
        &mut self,
        gl: &glow::Context,
        key: PingPongKey,
        format: RenderableFloat,
        sizes: impl FnOnce() -> Vec<[u32; 2]>,
    ) -> Result<&[GlScratch], BlurError> {
        // Two probes: returning the borrow from an early get_mut conflicts with the later insert
        // under NLL (E0499), so the single-probe shape the wgpu twin uses is unavailable here.
        if !self.chains.contains_key(&key) {
            let slots = build_slots(gl, &sizes(), format)?;
            self.chains.insert(
                key,
                ScratchChain {
                    slots,
                    last_used_frame: self.frame,
                },
            );
        }
        let chain = self
            .chains
            .get_mut(&key)
            .ok_or_else(|| BlurError::ResourceCreation {
                stage: BlurStage::PingPongTexture,
                source: "scratch chain missing immediately after insert".into(),
            })?;
        chain.last_used_frame = self.frame;
        Ok(&chain.slots)
    }

    /// Free every cached chain. Called from [`crate::GlowBlur::destroy`] while the context is
    /// current (DESIGN §11). Leaves the cache empty.
    pub(crate) fn destroy(&mut self, gl: &glow::Context) {
        for (_, chain) in self.chains.drain() {
            delete_chain(gl, &chain.slots);
        }
    }
}

// --- GL object construction / teardown (free functions, current-context contract) ---

/// Build one slot per size in `sizes`. On any failure, the slots already built this call are
/// deleted so a partial build leaks nothing.
fn build_slots(
    gl: &glow::Context,
    sizes: &[[u32; 2]],
    format: RenderableFloat,
) -> Result<Vec<GlScratch>, BlurError> {
    let mut built: Vec<GlScratch> = Vec::with_capacity(sizes.len());
    for &size in sizes {
        match create_scratch(gl, size, format) {
            Ok(slot) => built.push(slot),
            Err(e) => {
                delete_chain(gl, &built);
                return Err(e);
            }
        }
    }
    Ok(built)
}

/// Delete a chain's slots (texture + FBO each). Used by eviction, `destroy`, and partial-build
/// cleanup. Caller holds a current context.
fn delete_chain(gl: &glow::Context, slots: &[GlScratch]) {
    for slot in slots {
        // SAFETY: each handle was created by `create_scratch` on this current context and is
        // deleted exactly once (the cache removed/drained the chain, so no alias remains).
        unsafe {
            gl.delete_framebuffer(slot.fbo);
            gl.delete_texture(slot.texture);
        }
    }
}

/// Create one `size`-sized linear scratch slot: a `format` texture (ClampToEdge, Linear) plus an
/// FBO with it attached, completeness-checked. `Rgba16F` is the linear-HDR target; `Srgb8Rgba8` is
/// the WebGL2/GLES fallback (DESIGN §9).
fn create_scratch(
    gl: &glow::Context,
    size: [u32; 2],
    format: RenderableFloat,
) -> Result<GlScratch, BlurError> {
    let w = size[0].max(1) as i32;
    let h = size[1].max(1) as i32;
    let (internal, data_format, data_type) = match format {
        // RGBA16F stores HALF_FLOAT texels; the upload format is RGBA, the type HALF_FLOAT.
        RenderableFloat::Rgba16F => (glow::RGBA16F, glow::RGBA, glow::HALF_FLOAT),
        // SRGB8_ALPHA8 is an 8-bit sRGB-encoding target; uploads are RGBA/UNSIGNED_BYTE.
        RenderableFloat::Srgb8Rgba8 => (glow::SRGB8_ALPHA8, glow::RGBA, glow::UNSIGNED_BYTE),
    };

    // SAFETY: `gl` is current; `create_texture` returns a fresh handle or an error string.
    let texture = unsafe { gl.create_texture() }.map_err(|e| BlurError::ResourceCreation {
        stage: BlurStage::PingPongTexture,
        source: e.into(),
    })?;
    // SAFETY: `texture` was just created; allocate storage (no upload — `Slice(None)`) and set the
    // Linear/ClampToEdge sampling the blur passes and composite require. All operate on the bound
    // handle in place.
    unsafe {
        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            internal as i32,
            w,
            h,
            0,
            data_format,
            data_type,
            glow::PixelUnpackData::Slice(None),
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MIN_FILTER,
            glow::LINEAR as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_MAG_FILTER,
            glow::LINEAR as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_S,
            glow::CLAMP_TO_EDGE as i32,
        );
        gl.tex_parameter_i32(
            glow::TEXTURE_2D,
            glow::TEXTURE_WRAP_T,
            glow::CLAMP_TO_EDGE as i32,
        );
    }

    // SAFETY: `gl` is current; `create_framebuffer` returns a fresh FBO or an error string.
    let fbo = match unsafe { gl.create_framebuffer() } {
        Ok(fbo) => fbo,
        Err(e) => {
            // SAFETY: `texture` was just created on this context and is otherwise unreferenced.
            unsafe { gl.delete_texture(texture) };
            return Err(BlurError::ResourceCreation {
                stage: BlurStage::Framebuffer,
                source: e.into(),
            });
        }
    };
    // SAFETY: attach `texture` to `fbo`, check completeness, then unbind. All handles are live on
    // this current context; `bind_framebuffer(None)` restores the default binding the caller's
    // record-level save/restore re-establishes anyway.
    let status = unsafe {
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
        gl.framebuffer_texture_2d(
            glow::FRAMEBUFFER,
            glow::COLOR_ATTACHMENT0,
            glow::TEXTURE_2D,
            Some(texture),
            0,
        );
        let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);
        gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        status
    };
    if status != glow::FRAMEBUFFER_COMPLETE {
        // SAFETY: both handles created above on this context; deleted exactly once on the error path.
        unsafe {
            gl.delete_framebuffer(fbo);
            gl.delete_texture(texture);
        }
        return Err(BlurError::ResourceCreation {
            stage: BlurStage::Framebuffer,
            source: format!("scratch framebuffer incomplete: 0x{status:X}").into(),
        });
    }
    Ok(GlScratch {
        texture,
        fbo,
        size: [w as u32, h as u32],
    })
}

// --- Getters the blur passes use ---

impl GlScratch {
    /// The FBO this slot is rendered into.
    pub(crate) fn fbo(self) -> glow::Framebuffer {
        self.fbo
    }

    /// The slot's pixel size as a GL viewport extent `[w, h]` (`i32`, both `>= 1`).
    pub(crate) fn viewport(self) -> [i32; 2] {
        [self.size[0] as i32, self.size[1] as i32]
    }
}

// The pure eviction *decision* (and its wrap-safety / boundary tests) now lives in
// `backdrop-blur-core::eviction`, shared with the wgpu backend; the GL-backed cache *wiring*
// (`ensure_*` touches the frame, `begin_frame` deletes stale chains) is exercised in `blur_tests.rs`
// under the `gl-snapshots` tier.
