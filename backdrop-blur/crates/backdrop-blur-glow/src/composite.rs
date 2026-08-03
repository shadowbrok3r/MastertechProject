//! The composite pass: paint the frosted surface over the whole framebuffer. This is a **rewrite,
//! not a port** of wgpu's composite (DESIGN §10/§2f) — same SDF + `backdrop_uv_remap` math, but
//! three load-bearing GL specifics the wgpu render-pass abstraction handled implicitly:
//!
//! 1. **Full-framebuffer viewport.** egui pre-sets the GL viewport to the *panel rect* before the
//!    paint callback. A full-screen NDC triangle under a panel-sized viewport rasterizes only
//!    inside the panel — every fragment `coverage == 1` and the outer half of the analytic AA band
//!    is never generated, silently killing straight-edge AA. So the composite sets
//!    `glViewport(0, 0, fb_w, fb_h)` (overriding egui's), making `gl_FragCoord` carry true
//!    full-framebuffer window coords that match the bottom-left `rect_origin` uniform.
//! 2. **Scissor disabled** for the whole draw, so the AA band is not clipped to egui's per-
//!    primitive scissor box.
//! 3. **Premultiplied blend.** `composite.frag` emits premultiplied alpha (`rgb·a`, `a`, where
//!    `a = coverage · opacity` — the rounded-rect coverage scaled by the surface-global fade),
//!    paired here with `glBlendFunc(ONE, ONE_MINUS_SRC_ALPHA)`.
//!
//! The encode bit is a resolved [`TargetEncoding`] ([`resolve_target_encoding`]), not a
//! `wgpu::TextureFormat` allowlist (glow never sees the `u32` internal format): it is read once from
//! the captured target's colour attachment (`GL_FRAMEBUFFER_ATTACHMENT_COLOR_ENCODING`) plus, where
//! the enable is a valid capability, the `GL_FRAMEBUFFER_SRGB` write-encode state — so the shader
//! encodes iff the hardware will not (DESIGN §10). The resolve must run *after* the target is bound
//! (it queries the bound draw FBO); [`draw`] consumes the already-resolved value and issues no GL
//! query of its own. This dissolves the former per-frame `glIsEnabled(GL_FRAMEBUFFER_SRGB)`, which
//! raised `GL_INVALID_ENUM` on core GLES3/WebGL2.

use crate::profile::GlProfile;
use crate::program::Programs;
use backdrop_blur_core::{LinearRgba, ResolvedMask, TargetEncoding};
use glow::HasContext;

/// The resolved, GPU-free composite inputs `prepare` computes and `record` replays — all in **GL
/// bottom-left framebuffer pixels** (DESIGN §5). Owned (`Copy`); borrows nothing.
#[derive(Clone, Copy)]
pub(crate) struct CompositeParams {
    /// Target rect origin, bottom-left framebuffer px.
    pub(crate) rect_origin_px: [f32; 2],
    /// Target rect size, framebuffer px.
    pub(crate) rect_size_px: [f32; 2],
    /// The glass film: linear RGB, straight alpha (`a` = film opacity).
    pub(crate) tint: LinearRgba,
    /// Map target-rect uv `[0,1]` onto the clipped blurred scratch (`backdrop_uv_remap`).
    pub(crate) backdrop_uv_offset: [f32; 2],
    pub(crate) backdrop_uv_scale: [f32; 2],
    /// The clamped corner radius, framebuffer px (from [`ResolvedMask`]).
    pub(crate) corner_radius_px: f32,
    /// The full framebuffer size in px — the composite viewport (`glViewport(0,0,fb_w,fb_h)`), so
    /// `gl_FragCoord` spans the whole attachment and the outer AA band is generated.
    pub(crate) framebuffer_size: [u32; 2],
    /// Surface-global fade `[0, 1]` — scales the premultiplied output (both rgb and alpha), so the
    /// surface dissolves to the untouched destination as it goes to 0.
    pub(crate) opacity: f32,
}

impl CompositeParams {
    /// Assemble from the resolved mask, tint, and bottom-left target rect/backdrop remap.
    #[expect(
        clippy::too_many_arguments,
        reason = "flat composite inputs; field-per-arg is the point"
    )]
    pub(crate) fn new(
        rect_origin_px: [f32; 2],
        rect_size_px: [f32; 2],
        tint: LinearRgba,
        backdrop_uv_offset: [f32; 2],
        backdrop_uv_scale: [f32; 2],
        mask: ResolvedMask,
        framebuffer_size: [u32; 2],
        opacity: f32,
    ) -> Self {
        Self {
            rect_origin_px,
            rect_size_px,
            tint,
            backdrop_uv_offset,
            backdrop_uv_scale,
            corner_radius_px: mask.corner_radius_px,
            framebuffer_size,
            opacity,
        }
    }
}

/// Whether the target's hardware sRGB encode-on-write is active — the second half of the encode
/// decision (the first is whether the target attachment is sRGB-capable). Modelled as a typed state
/// rather than a bare `bool` so the meaningless "the enable was queried on a context where the query
/// is invalid" combination cannot be constructed: on core GLES 3.0 / WebGL2 there is no
/// `GL_FRAMEBUFFER_SRGB` to read, so the only representable state there is [`AlwaysOn`].
///
/// [`AlwaysOn`]: WriteEncodeState::AlwaysOn
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WriteEncodeState {
    /// The context has no valid `GL_FRAMEBUFFER_SRGB` enable (core GLES 3.0 / WebGL2): the hardware
    /// sRGB encode-on-write to an sRGB attachment is always on and cannot be gated off.
    AlwaysOn,
    /// The enable is a valid capability (desktop GL, or GLES with `EXT_sRGB_write_control`): this
    /// carries the live `glIsEnabled(GL_FRAMEBUFFER_SRGB)` read that gates hardware encode.
    Explicit(bool),
}

/// The composite's **encode contract**, pure so it is fully Tier-0 testable. Returns the
/// [`TargetEncoding`] the composite shader must honour: [`Srgb`](TargetEncoding::Srgb) means the
/// shader manually applies the sRGB OETF (`u_encode_srgb = 1`, `composite.frag`), taken exactly when
/// the hardware will **not** encode; [`Linear`](TargetEncoding::Linear) means the hardware encodes on
/// write (an sRGB attachment with write-encode active) or the target is a float target, so the shader
/// writes linear light directly. Hardware encodes iff the attachment is sRGB-capable **and**
/// write-encode is active.
pub(crate) fn encode_decision(
    attachment_is_srgb: bool,
    write_encode: WriteEncodeState,
) -> TargetEncoding {
    let write_encode_active = matches!(
        write_encode,
        WriteEncodeState::AlwaysOn | WriteEncodeState::Explicit(true)
    );
    if attachment_is_srgb && write_encode_active {
        TargetEncoding::Linear
    } else {
        TargetEncoding::Srgb
    }
}

/// Resolve the [`TargetEncoding`] for the **currently-bound** draw framebuffer (the captured target).
/// Must be called with `target` bound as `DRAW_FRAMEBUFFER` — the caller does this immediately prior —
/// because it queries that bound FBO's colour attachment. This is the glow analogue of wgpu's
/// format-based `composite_encode_srgb`: glow resolves at *record* time from the live attachment
/// (its seam hands the framebuffer only to `record`, never `prepare`), whereas wgpu resolves at
/// *prepare* time from the static `TextureFormat` (DESIGN §10).
///
/// The default-framebuffer colour attachment is named per dialect — `GL_BACK_LEFT` on desktop GL,
/// `GL_BACK` on GLES3/WebGL2 — while a user FBO always uses `GL_COLOR_ATTACHMENT0`.
pub(crate) fn resolve_target_encoding(
    gl: &glow::Context,
    profile: &GlProfile,
    target: Option<glow::Framebuffer>,
) -> TargetEncoding {
    let attachment = if target.is_some() {
        glow::COLOR_ATTACHMENT0
    } else if profile.embedded {
        glow::BACK
    } else {
        glow::BACK_LEFT
    };
    // SAFETY: called on the current context (record's contract) with `target` bound as
    // DRAW_FRAMEBUFFER. `get_framebuffer_attachment_parameter_i32` reads the bound draw FBO's
    // attachment encoding — it takes no caller pointer and mutates no GL state. `is_enabled` is read
    // only in the `Explicit` arm, gated by `srgb_enable_is_queryable`, so on core GLES3/WebGL2 (where
    // `GL_FRAMEBUFFER_SRGB` is not a valid enum) it is never queried — this is what removes the
    // per-frame `GL_INVALID_ENUM`. A driver that rejects the encoding query returns 0 (glow's getters
    // surface no GL error), i.e. LINEAR → the manual-encode fallback, which is the correct answer for
    // every non-sRGB target.
    unsafe {
        let attachment_is_srgb = gl.get_framebuffer_attachment_parameter_i32(
            glow::DRAW_FRAMEBUFFER,
            attachment,
            glow::FRAMEBUFFER_ATTACHMENT_COLOR_ENCODING,
        ) == glow::SRGB as i32;
        let write_encode = if profile.srgb_enable_is_queryable {
            WriteEncodeState::Explicit(gl.is_enabled(glow::FRAMEBUFFER_SRGB))
        } else {
            WriteEncodeState::AlwaysOn
        };
        encode_decision(attachment_is_srgb, write_encode)
    }
}

/// Draw the composite into the currently-bound draw framebuffer (the captured target). The caller
/// (`record`) has already bound `target` as the draw FBO and saved every GL binding this perturbs;
/// this function sets the composite-specific state (viewport, scissor-off, premult blend, program,
/// uniforms, the blurred texture on unit 0, the shared VAO) and issues the triangle. The encode bit
/// is the pre-resolved `target_encoding` ([`resolve_target_encoding`], run by the caller after the
/// target bind), so `draw` issues no GL query of its own.
///
/// `blurred` is the final linear scratch the composite samples (Gaussian B, or Kawase mip 0).
pub(crate) fn draw(
    gl: &glow::Context,
    programs: &Programs,
    vao: glow::VertexArray,
    blurred: glow::Texture,
    params: &CompositeParams,
    target_encoding: TargetEncoding,
) {
    let program = programs.composite;
    let [fb_w, fb_h] = params.framebuffer_size;

    // SAFETY: the uniform locations and the draw all run on the current context (record's contract)
    // with a live, linked `composite` program and the live `blurred` texture + shared `vao`. The
    // encode bit is the pre-resolved `target_encoding` (see `resolve_target_encoding`), so no GL
    // query happens here. Every uniform location comes from this program; a `None` location is a
    // documented no-op (an optimizer may drop an unused uniform), so missing-location handling is not
    // a fault. The viewport/scissor/blend state the caller saved is restored by the caller after this
    // returns.
    unsafe {
        // The shader manually encodes linear→sRGB exactly when the target does not encode in hardware
        // (`TargetEncoding::Srgb`); a `Linear` target (a hardware-sRGB attachment with write-encode
        // active, or a float target) receives linear light directly.
        let encode_srgb = matches!(target_encoding, TargetEncoding::Srgb);

        gl.viewport(0, 0, fb_w as i32, fb_h as i32);
        gl.disable(glow::SCISSOR_TEST);
        gl.enable(glow::BLEND);
        gl.blend_equation(glow::FUNC_ADD);
        gl.blend_func(glow::ONE, glow::ONE_MINUS_SRC_ALPHA);

        gl.use_program(Some(program));

        let loc = |name: &str| gl.get_uniform_location(program, name);
        gl.uniform_2_f32(
            loc("u_rect_origin_px").as_ref(),
            params.rect_origin_px[0],
            params.rect_origin_px[1],
        );
        gl.uniform_2_f32(
            loc("u_rect_size_px").as_ref(),
            params.rect_size_px[0],
            params.rect_size_px[1],
        );
        gl.uniform_4_f32(
            loc("u_tint").as_ref(),
            params.tint.r(),
            params.tint.g(),
            params.tint.b(),
            params.tint.a(),
        );
        gl.uniform_2_f32(
            loc("u_backdrop_uv_offset").as_ref(),
            params.backdrop_uv_offset[0],
            params.backdrop_uv_offset[1],
        );
        gl.uniform_2_f32(
            loc("u_backdrop_uv_scale").as_ref(),
            params.backdrop_uv_scale[0],
            params.backdrop_uv_scale[1],
        );
        gl.uniform_1_f32(loc("u_corner_radius_px").as_ref(), params.corner_radius_px);
        gl.uniform_1_i32(loc("u_encode_srgb").as_ref(), i32::from(encode_srgb));
        gl.uniform_1_f32(loc("u_opacity").as_ref(), params.opacity);

        // The blurred scratch on texture unit 0; the sampler uniform points there.
        gl.active_texture(glow::TEXTURE0);
        gl.bind_texture(glow::TEXTURE_2D, Some(blurred));
        gl.uniform_1_i32(loc("u_blurred").as_ref(), 0);

        gl.bind_vertex_array(Some(vao));
        gl.draw_arrays(glow::TRIANGLES, 0, 3);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The 6 meaningful (attachment × write_encode) rows. The seventh/eighth "(sRGB, not-queryable,
    // enabled=?)" combinations of the old 3-bool signature are unrepresentable: `AlwaysOn` carries no
    // enable bit, so the invalid enable can never be queried where it does not exist.

    #[test]
    fn non_srgb_attachment_always_manually_encodes() {
        // A non-sRGB (linear/Unorm) attachment: the hardware never encodes, so the shader must —
        // regardless of the write-encode state.
        assert_eq!(
            encode_decision(false, WriteEncodeState::AlwaysOn),
            TargetEncoding::Srgb
        );
        assert_eq!(
            encode_decision(false, WriteEncodeState::Explicit(true)),
            TargetEncoding::Srgb
        );
        assert_eq!(
            encode_decision(false, WriteEncodeState::Explicit(false)),
            TargetEncoding::Srgb
        );
    }

    #[test]
    fn srgb_attachment_on_core_gles_or_webgl2_lets_hardware_encode() {
        // sRGB attachment where the enable does not exist (core GLES3/WebGL2): encode-on-write is
        // always on, so the shader writes linear and lets the hardware encode.
        assert_eq!(
            encode_decision(true, WriteEncodeState::AlwaysOn),
            TargetEncoding::Linear
        );
    }

    #[test]
    fn srgb_attachment_follows_the_explicit_enable() {
        // sRGB attachment on a context where the enable is a valid capability: hardware encodes iff
        // the enable is on; otherwise the shader must encode.
        assert_eq!(
            encode_decision(true, WriteEncodeState::Explicit(true)),
            TargetEncoding::Linear
        );
        assert_eq!(
            encode_decision(true, WriteEncodeState::Explicit(false)),
            TargetEncoding::Srgb
        );
    }
}
