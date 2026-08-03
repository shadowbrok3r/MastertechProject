//! Tier-1 readback tests for the blur + composite against a **real** GL 3.3 context (the
//! `gl_harness`). These prove correctness, not just compilation: a banded/flat/known backdrop is
//! grabbed, blurred, composited, and read back, and the pixels are asserted to 8-bit-rounding
//! tolerance. If a readback is wrong, the GL is wrong.
//!
//! All coordinates are GL **bottom-left** (DESIGN §5): the backdrop FBO is painted with scissored
//! clears in bottom-origin coords, the panel rect is bottom-left, and readback `y` is bottom-origin.

use super::*;
use crate::GlowBlur;
use crate::gl_harness::{headless_gl, read_texture_rgba8};
use backdrop_blur_core::{
    BlurRadius, CornerRadius, LinearRgba, Presence, Region, Scale, TargetEncoding, Tint,
};
use glow::HasContext;

const DIM: u32 = 128;

/// A backdrop FBO + its color texture. The grab reads this as the live framebuffer.
struct Scene {
    fbo: glow::Framebuffer,
    tex: glow::Texture,
}

/// A target FBO + its color texture (the composite destination). Read back via the texture.
struct Target {
    fbo: glow::Framebuffer,
    tex: glow::Texture,
}

/// Create an empty `DIM×DIM` FBO with the given colour internal format. Caller paints it, then frees
/// via [`free_fbo`]. `SRGB8_ALPHA8` yields an sRGB-capable target (the hardware encodes linear→sRGB on
/// write when `GL_FRAMEBUFFER_SRGB` is enabled); `RGBA8` a plain linear/`Unorm` target.
fn make_fbo_with_format(
    gl: &glow::Context,
    internal_format: i32,
) -> (glow::Framebuffer, glow::Texture) {
    // SAFETY: standard FBO setup on the current harness context; handles are returned for the test
    // to free. `Slice(None)` allocates without uploading.
    unsafe {
        let tex = gl.create_texture().expect("tex");
        gl.bind_texture(glow::TEXTURE_2D, Some(tex));
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            internal_format,
            DIM as i32,
            DIM as i32,
            0,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
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
        let fbo = gl.create_framebuffer().expect("fbo");
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
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
            "FBO incomplete"
        );
        gl.bind_framebuffer(glow::FRAMEBUFFER, None);
        (fbo, tex)
    }
}

/// Create an empty `DIM×DIM` RGBA8 FBO. Caller paints it, then frees via [`free_fbo`].
fn make_fbo(gl: &glow::Context) -> (glow::Framebuffer, glow::Texture) {
    make_fbo_with_format(gl, glow::RGBA8 as i32)
}

fn free_fbo(gl: &glow::Context, fbo: glow::Framebuffer, tex: glow::Texture) {
    // SAFETY: both handles came from `make_fbo` on this context; freed once.
    unsafe {
        gl.delete_framebuffer(fbo);
        gl.delete_texture(tex);
    }
}

/// Fill a whole FBO with one sRGB-byte color.
fn clear_fbo(gl: &glow::Context, fbo: glow::Framebuffer, rgba: [f32; 4]) {
    // SAFETY: bind the FBO, set a full viewport, clear it. Current context; the FBO is live.
    unsafe {
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
        gl.disable(glow::SCISSOR_TEST);
        gl.viewport(0, 0, DIM as i32, DIM as i32);
        gl.clear_color(rgba[0], rgba[1], rgba[2], rgba[3]);
        gl.clear(glow::COLOR_BUFFER_BIT);
        gl.bind_framebuffer(glow::FRAMEBUFFER, None);
    }
}

/// A backdrop split **left/right** at `edge_x` (bottom-left coords): `left` color for x < edge_x,
/// `right` for x >= edge_x. A vertical seam, so a horizontal (x) blur visibly softens it.
fn split_backdrop(gl: &glow::Context, edge_x: u32, left: [f32; 4], right: [f32; 4]) -> Scene {
    let (fbo, tex) = make_fbo(gl);
    // SAFETY: scissored clears paint the two columns; current context, live FBO.
    unsafe {
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
        gl.viewport(0, 0, DIM as i32, DIM as i32);
        gl.disable(glow::SCISSOR_TEST);
        gl.clear_color(left[0], left[1], left[2], left[3]);
        gl.clear(glow::COLOR_BUFFER_BIT);
        gl.enable(glow::SCISSOR_TEST);
        gl.scissor(edge_x as i32, 0, (DIM - edge_x) as i32, DIM as i32);
        gl.clear_color(right[0], right[1], right[2], right[3]);
        gl.clear(glow::COLOR_BUFFER_BIT);
        gl.disable(glow::SCISSOR_TEST);
        gl.bind_framebuffer(glow::FRAMEBUFFER, None);
    }
    Scene { fbo, tex }
}

/// A backdrop split **bottom/top** at y = `DIM/2` (GL bottom-left coords): `bottom` color for the
/// low-y half, `top` for the high-y half. A horizontal seam, so vertical orientation through the
/// whole grab→blur→composite seam is checkable (it stays bottom-color-low, top-color-high). Mirrors
/// `grab.rs`'s `banded_source`.
fn banded_backdrop(gl: &glow::Context, bottom: [f32; 4], top: [f32; 4]) -> Scene {
    let (fbo, tex) = make_fbo(gl);
    // SAFETY: scissored clears paint the two horizontal bands; current context, live FBO.
    unsafe {
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
        gl.viewport(0, 0, DIM as i32, DIM as i32);
        gl.disable(glow::SCISSOR_TEST);
        gl.clear_color(bottom[0], bottom[1], bottom[2], bottom[3]);
        gl.clear(glow::COLOR_BUFFER_BIT);
        gl.enable(glow::SCISSOR_TEST);
        gl.scissor(0, (DIM / 2) as i32, DIM as i32, (DIM - DIM / 2) as i32); // the TOP half — high y
        gl.clear_color(top[0], top[1], top[2], top[3]);
        gl.clear(glow::COLOR_BUFFER_BIT);
        gl.disable(glow::SCISSOR_TEST);
        gl.bind_framebuffer(glow::FRAMEBUFFER, None);
    }
    Scene { fbo, tex }
}

/// A flat one-color backdrop.
fn flat_backdrop(gl: &glow::Context, rgba: [f32; 4]) -> Scene {
    let (fbo, tex) = make_fbo(gl);
    clear_fbo(gl, fbo, rgba);
    Scene { fbo, tex }
}

/// A bottom-left panel region in physical px, scale 1.
fn panel(origin: [u32; 2], size: [u32; 2]) -> Region {
    Region {
        origin,
        size,
        scale: Scale::new(1.0),
    }
}

/// Run the full seam — grab_source → prepare → record — over `scene`, compositing into a `target`
/// FBO seeded with `seed`, and return the target's color texture for readback. `blur_radius`/`tint`/
/// `corner_radius` describe the glass. The grab region is the panel itself (the adapter's `viewport ∩
/// clip_rect`, here the whole panel in-bounds).
#[expect(
    clippy::too_many_arguments,
    reason = "test scene builder: each parameter is a distinct, named knob of the frosted-glass \
              scenario; bundling them into a struct would obscure the per-test call sites"
)]
fn frost(
    gl: &mut glow::Context,
    blur: &mut GlowBlur,
    scene: &Scene,
    panel_rect: Region,
    blur_radius: f32,
    corner_radius: f32,
    tint: Tint,
    seed: [f32; 4],
    presence: f32,
) -> Target {
    let (t_fbo, t_tex) = make_fbo(gl);
    clear_fbo(gl, t_fbo, seed);

    let region = GlRegion::from_bottom_px(panel_rect.origin, panel_rect.size, Scale::new(1.0));
    let source = blur
        .grab_source(&*gl, &(), &Some(scene.fbo), region)
        .expect("grab_source");

    let request = BlurRequest {
        source_region: panel_rect,
        target_rect: panel_rect,
        blur_radius: BlurRadius::new(blur_radius),
        tint,
        corner_radius: CornerRadius::new(corner_radius),
        presence: Presence::new(presence),
    };
    // The composite viewport is the true screen size the egui adapter holds, passed as the
    // backend's TargetSpec (a missing size would be a compile error, not a silent AA regression).
    let prepared = blur
        .prepare(&*gl, &(), &source, FramebufferSize([DIM, DIM]), &request)
        .expect("prepare")
        .expect("a non-empty region prepares a blur");
    blur.record(gl, &Some(t_fbo), prepared).expect("record");
    // SAFETY: flush so the readback sees the composite; current context.
    unsafe { gl.finish() };
    Target {
        fbo: t_fbo,
        tex: t_tex,
    }
}

fn no_tint() -> Tint {
    Tint::new(LinearRgba::new(0.0, 0.0, 0.0, 0.0))
}

// --- 2d: Gaussian softens a hard edge ---

#[test]
fn gaussian_softens_a_hard_edge() {
    let mut gl = headless_gl();
    let mut blur = GlowBlur::new(&gl).expect("new");
    // Backdrop: black left, white right, seam at x=64. A Gaussian blur turns the hard step into a
    // smooth ramp — the seam pixel is a midtone, and brightness bleeds a few px into the black side.
    let scene = split_backdrop(&gl, 64, [0.0, 0.0, 0.0, 1.0], [1.0, 1.0, 1.0, 1.0]);
    let p = panel([32, 32], [64, 64]);
    // radius 8 → sigma ≈ 2.67, taps 8: a measurable ramp several px wide.
    let target = frost(
        &mut gl,
        &mut blur,
        &scene,
        p,
        8.0,
        0.0,
        no_tint(),
        [0.5, 0.5, 0.5, 1.0],
        1.0,
    );

    // On the seam (x=64): a midtone, NOT a hard black/white step.
    let seam = read_texture_rgba8(&gl, target.tex, 64, 64)[0];
    assert!(
        (40..=230).contains(&seam),
        "the blurred seam must be a midtone (not a hard edge), got r={seam}"
    );
    // 4px into the black side (x=60): white bled in, so it is clearly brighter than the unblurred
    // black backdrop (which would read 0). This is the proof the convolution spread the edge.
    let near = read_texture_rgba8(&gl, target.tex, 60, 64)[0];
    // 12px into the black side (x=52): far enough that little bleeds through — still dark.
    let far = read_texture_rgba8(&gl, target.tex, 52, 64)[0];
    assert!(
        near > far + 15,
        "blur must brighten the near-seam black side above the far interior, got near={near} far={far}"
    );

    blur.destroy(&gl);
    free_fbo(&gl, scene.fbo, scene.tex);
    free_fbo(&gl, target.fbo, target.tex);
}

// --- 2e: dual-Kawase energy preservation + reach ---

#[test]
fn dual_kawase_preserves_a_flat_backdrop() {
    let mut gl = headless_gl();
    let mut blur = GlowBlur::new(&gl).expect("new");
    // Flat 0.5 gray (the plain RGBA8 scene FBO stores it as byte ≈128, no sRGB encode — see the
    // readback note below). radius 30 (≥16) takes the dual-Kawase path; energy-preserving down
    // (÷8) + up (÷12) must leave the flat gray unchanged — a wrong-weight kernel shifts brightness.
    let mid = 0.5_f32;
    let scene = flat_backdrop(&gl, [mid, mid, mid, 1.0]);
    let p = panel([24, 24], [80, 80]);
    let target = frost(
        &mut gl,
        &mut blur,
        &scene,
        p,
        30.0,
        0.0,
        no_tint(),
        [0.0, 0.0, 0.0, 1.0],
        1.0,
    );

    // Panel center, far from edges: the composite re-encodes linear→sRGB, so the readback must
    // match the backdrop's own sRGB byte (~128, since the FBO stored 0.5 as a plain RGBA8 value).
    let px = read_texture_rgba8(&gl, target.tex, 64, 64);
    for (ch, &v) in px.iter().take(3).enumerate() {
        assert!(
            (120..=136).contains(&v),
            "dual-Kawase must preserve the flat backdrop's brightness, channel {ch} = {v}"
        );
    }
    blur.destroy(&gl);
    free_fbo(&gl, scene.fbo, scene.tex);
    free_fbo(&gl, target.fbo, target.tex);
}

#[test]
fn dual_kawase_reaches_across_a_large_radius_edge() {
    let mut gl = headless_gl();
    let mut blur = GlowBlur::new(&gl).expect("new");
    // Hard red/blue seam at x=64 under a large dual-Kawase blur: the seam pixel becomes a strong
    // red↔blue mix (both channels present), proving the multi-level pyramid reaches across.
    let scene = split_backdrop(&gl, 64, [1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0]);
    let p = panel([24, 24], [80, 80]);
    let target = frost(
        &mut gl,
        &mut blur,
        &scene,
        p,
        20.0,
        0.0,
        no_tint(),
        [0.0, 0.0, 0.0, 1.0],
        1.0,
    );

    let px = read_texture_rgba8(&gl, target.tex, 64, 64);
    assert!(
        px[0] > 40 && px[2] > 40,
        "a large dual-Kawase blur must mix red and blue across the seam, got r={} b={}",
        px[0],
        px[2]
    );
    // Transition-width guard (mirrors the wgpu oracle's `(30..=72)` band, scaled to this 80px panel /
    // radius 20): count the mixed (both-channel-present) pixels along the seam row INSIDE the panel
    // (x ∈ [24, 104), y=64). The binary mix check above is offset-magnitude-invariant — it cannot
    // see a half-pixel offset scaled 2× (transition ~doubles) or 0.5× (~halves); the width band can.
    let mixed = (24u32..104)
        .filter(|&x| {
            let p = read_texture_rgba8(&gl, target.tex, x as i32, 64);
            p[0] > 50 && p[2] > 50
        })
        .count();
    // Centered on the measured ~54 mixed px: a 0.5× half-pixel error halves the transition (~27,
    // below 40) and a 2× error doubles it toward saturating the 80px row (≥68) — both escape the
    // band, while the binary mix check above cannot. (Mirrors the wgpu oracle's centered `(30..=72)`,
    // scaled to this 80px panel / radius 20.)
    assert!(
        (40..=68).contains(&mixed),
        "dual-Kawase transition width must match radius 20 (a 2×/0.5× half-pixel error escapes this \
         band), got {mixed} mixed px"
    );
    blur.destroy(&gl);
    free_fbo(&gl, scene.fbo, scene.tex);
    free_fbo(&gl, target.fbo, target.tex);
}

// --- 2f: composite — no-halo envelope both directions, straight-edge AA, clip bleed ---

/// Assert every pixel along a horizontal slice (the panel's straight left edge `x ∈ band`, at row
/// `y`) lies within the `[lo, hi]` luminance envelope of the two reference pixels (`exterior_x`
/// outside the panel, `interior_x` well inside). A premultiplied/gamma halo overshoots the envelope.
fn assert_no_edge_halo(
    gl: &glow::Context,
    tex: glow::Texture,
    y: i32,
    exterior_x: i32,
    interior_x: i32,
) {
    const TOL: i32 = 8;
    let exterior = i32::from(read_texture_rgba8(gl, tex, exterior_x, y)[0]);
    let interior = i32::from(read_texture_rgba8(gl, tex, interior_x, y)[0]);
    let (lo, hi) = (exterior.min(interior), exterior.max(interior));
    assert!(
        hi - lo > 80,
        "the halo oracle needs a high-contrast envelope, got [{lo},{hi}]"
    );
    // The panel left edge sits at x=32; scan the AA band around it.
    for x in 26..40 {
        let v = i32::from(read_texture_rgba8(gl, tex, x, y)[0]);
        assert!(
            v >= lo - TOL && v <= hi + TOL,
            "panel edge at x={x} overshoots the [{lo},{hi}] envelope (halo): v={v}"
        );
    }
}

#[test]
fn composite_has_no_edge_halo_both_directions() {
    let mut gl = headless_gl();
    let mut blur = GlowBlur::new(&gl).expect("new");
    let p = panel([32, 32], [64, 64]); // left edge at x=32, y mid-panel = 64

    // Bright frost over black — the white-halo (overshoot) direction.
    let black = flat_backdrop(&gl, [0.0, 0.0, 0.0, 1.0]);
    let bright = Tint::from_srgb_unmultiplied([240, 240, 240, 204]);
    let t1 = frost(
        &mut gl,
        &mut blur,
        &black,
        p,
        8.0,
        0.0,
        bright,
        [0.0, 0.0, 0.0, 1.0],
        1.0,
    );
    assert_no_edge_halo(&gl, t1.tex, 64, 16, 80);

    // Dark frost over white — the dark-fringe (undershoot) direction.
    let white = flat_backdrop(&gl, [1.0, 1.0, 1.0, 1.0]);
    let dark = Tint::from_srgb_unmultiplied([20, 20, 20, 204]);
    let t2 = frost(
        &mut gl,
        &mut blur,
        &white,
        p,
        8.0,
        0.0,
        dark,
        [1.0, 1.0, 1.0, 1.0],
        1.0,
    );
    assert_no_edge_halo(&gl, t2.tex, 64, 16, 80);

    blur.destroy(&gl);
    free_fbo(&gl, black.fbo, black.tex);
    free_fbo(&gl, white.fbo, white.tex);
    free_fbo(&gl, t1.fbo, t1.tex);
    free_fbo(&gl, t2.fbo, t2.tex);
}

#[test]
fn composite_edge_is_analytic_aa_not_a_hard_scissor_cut() {
    let mut gl = headless_gl();
    let mut blur = GlowBlur::new(&gl).expect("new");
    // Bright opaque frost over black, seeded black target. The composite's coverage is the analytic
    // rounded-rect SDF, so the boundary is an AA ramp — NOT a hard rectangular scissor cut. The
    // rounded corner's arc crosses pixel centers (unlike an axis-aligned straight edge, whose ~1px
    // AA band falls between integer centers), so it yields true partial-coverage midtones. A
    // hard-scissor regression (or a coverage that is binary 0/1) shows no midtone anywhere on the
    // arc. This guards corner-AA *presence* only — it does NOT guard the full-framebuffer-viewport
    // requirement (the corner midtones all fall inside the panel rect, so they render under either
    // viewport) and it does NOT prove bottom-left edge registration. Those are pinned by
    // `panel_inset_keeps_vertical_orientation_and_registered_edge` below.
    let black = flat_backdrop(&gl, [0.0, 0.0, 0.0, 1.0]);
    let bright = Tint::from_srgb_unmultiplied([255, 255, 255, 255]);
    // A generous corner radius so the arc spans many pixels.
    let p = panel([32, 32], [64, 64]);
    let target = frost(
        &mut gl,
        &mut blur,
        &black,
        p,
        4.0,
        24.0,
        bright,
        [0.0, 0.0, 0.0, 1.0],
        1.0,
    );

    // Panel interior, away from every edge: fully covered → bright.
    let inside = read_texture_rgba8(&gl, target.tex, 64, 64)[0];
    assert!(
        inside > 215,
        "panel interior must be ~white (full coverage), got {inside}"
    );
    // Far outside the panel: zero coverage → the black seed.
    let outside = read_texture_rgba8(&gl, target.tex, 8, 8)[0];
    assert!(
        outside < 40,
        "far outside must be the black seed, got {outside}"
    );
    // Scan the bottom-left rounded corner's arc (arc centre at [32+24, 32+24] = [56,56], radius 24)
    // for partial-coverage midtones — the analytic AA ramp. A 2D scan (not a 1px-thin diagonal walk)
    // so the ~1px AA band is reliably sampled where the arc crosses pixel centers. A hard scissor cut
    // (binary 0/1 coverage) would show no midtone anywhere in this region.
    let midtones = (28..60)
        .flat_map(|y| (28..60).map(move |x| (x, y)))
        .filter(|&(x, y)| (50..=205).contains(&read_texture_rgba8(&gl, target.tex, x, y)[0]))
        .count();
    assert!(
        midtones >= 8,
        "the rounded corner must show an analytic AA midtone ramp (not a hard scissor cut), \
         found {midtones} midtone px on the arc"
    );

    blur.destroy(&gl);
    free_fbo(&gl, black.fbo, black.tex);
    free_fbo(&gl, target.fbo, target.tex);
}

/// FIX-3 — the test that actually guards the full-framebuffer-viewport contract *and* proves
/// vertical orientation end-to-end. The panel is **inset** (origin [16,16], size [80,80] in a
/// 128×128 framebuffer), so its straight edges sit at known interior framebuffer coordinates:
///
/// (a) **Vertical orientation through the composite.** The backdrop is bottom-red / top-blue (GL
///     bottom-left). The composited panel must keep red at low y (inside, bottom) and blue at high
///     y (inside, top) — no other end-to-end test proves orientation survives grab→blur→composite
///     (the blur passes, the composite's `gl_FragCoord` read, and the readback all agree on
///     bottom-origin only if every one of them does).
///
/// (b) **Bottom-left registration guard.** The composite reads `gl_FragCoord` (GL bottom-origin
///     window coords, generated by the full-framebuffer `glViewport(0,0,fb_w,fb_h)`) against the
///     bottom-left `rect_origin` uniform (composite.rs §1, DESIGN §5). The panel's straight top edge
///     is at framebuffer y=96 (origin 16 + size 80), so the SDF coverage ramp must transition there
///     in bottom-left coordinates: row y=95 is the last fully-covered row (blurred blue, b high) and
///     y=96 is the first uncovered row (the black seed, b≈0). A registration bug — a y-flip
///     (top-left vs bottom-left), or a `rect_origin`/`gl_FragCoord` mismatch — moves this transition
///     to the wrong row, so pinning it to y=95→96 guards that the composite registered the panel in
///     bottom-left framebuffer pixels.
///     (Proven non-vacuous: shifting the `u_rect_origin_px` uniform by +8 px in y moves the edge to
///     y=103/104 and this assertion fails — see the FIX-3 report.)
#[test]
fn panel_inset_keeps_vertical_orientation_and_registered_edge() {
    let mut gl = headless_gl();
    let mut blur = GlowBlur::new(&gl).expect("new");
    // Bottom-red / top-blue backdrop (full 128×128), an inset square-cornered panel, a faint dark
    // tint so the blurred backdrop dominates the panel interior (orientation visible), black seed.
    let scene = banded_backdrop(&gl, [1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0]);
    let p = panel([16, 16], [80, 80]); // straight top edge at framebuffer y=96
    let tint = Tint::new(LinearRgba::new(0.0, 0.0, 0.0, 0.12));
    let target = frost(
        &mut gl,
        &mut blur,
        &scene,
        p,
        6.0,
        0.0,
        tint,
        [0.0, 0.0, 0.0, 1.0],
        1.0,
    );

    // (a) Vertical orientation: well inside the panel, low y is red, high y is blue. Sample away
    // from the horizontal seam (y=64) so the blur has not washed the band into a mix.
    let low = read_texture_rgba8(&gl, target.tex, 56, 28); // inside, bottom → red
    assert!(
        low[0] > 120 && low[2] < 80,
        "vertical orientation: low-y inside the panel must be red (bottom band), got {low:?}"
    );
    let high = read_texture_rgba8(&gl, target.tex, 56, 84); // inside, top → blue
    assert!(
        high[2] > 120 && high[0] < 80,
        "vertical orientation: high-y inside the panel must be blue (top band), got {high:?}"
    );

    // (b) Viewport registration: the straight top edge (framebuffer y=96) transitions exactly at
    // y=95→96 only when the composite paints under the full-framebuffer viewport (gl_FragCoord = true
    // window coords). A column well inside the panel's x-span, away from corners.
    let x = 56;
    let last_covered = i32::from(read_texture_rgba8(&gl, target.tex, x, 95)[2]); // last covered row
    let first_clear = i32::from(read_texture_rgba8(&gl, target.tex, x, 96)[2]); // first seed row
    assert!(
        last_covered > 150,
        "the registered top edge requires row y=95 fully covered (blurred blue), got b={last_covered}"
    );
    assert!(
        first_clear < 40,
        "the registered top edge requires row y=96 uncovered (black seed), got b={first_clear}"
    );
    // And the rows just beyond stay seed (the panel did not slide upward into them), while rows
    // just inside stay covered (it did not slide downward) — a registration shift would break one.
    assert!(
        i32::from(read_texture_rgba8(&gl, target.tex, x, 98)[2]) < 40,
        "two rows past the top edge must still be the seed (no upward registration slide)"
    );
    assert!(
        i32::from(read_texture_rgba8(&gl, target.tex, x, 92)[2]) > 150,
        "well inside the top edge must still be covered (no downward registration slide)"
    );

    blur.destroy(&gl);
    free_fbo(&gl, scene.fbo, scene.tex);
    free_fbo(&gl, target.fbo, target.tex);
}

#[test]
fn composite_leaves_content_outside_the_panel_untouched() {
    let mut gl = headless_gl();
    let mut blur = GlowBlur::new(&gl).expect("new");
    // A clip/bleed check: the panel covers the center; the target is seeded a distinct green. Any
    // pixel far outside the panel must remain the seed (coverage 0 there → the premultiplied blend
    // leaves dst untouched). A scissor/viewport bug that drew past the panel would overwrite it.
    let scene = flat_backdrop(&gl, [1.0, 0.0, 0.0, 1.0]); // red backdrop
    let p = panel([48, 48], [32, 32]);
    let seed = [0.0, 1.0, 0.0, 1.0]; // green seed
    let target = frost(
        &mut gl,
        &mut blur,
        &scene,
        p,
        6.0,
        0.0,
        no_tint(),
        seed,
        1.0,
    );

    // Corner of the framebuffer, far from the panel: still the green seed.
    let corner = read_texture_rgba8(&gl, target.tex, 8, 8);
    assert!(
        corner[1] > 200 && corner[0] < 40 && corner[2] < 40,
        "content outside the panel must stay the seed (no bleed), got {corner:?}"
    );
    // Panel center: the frosted red backdrop shows through (red present).
    let center = read_texture_rgba8(&gl, target.tex, 64, 64);
    assert!(
        center[0] > 150,
        "the panel center must show the frosted red backdrop, got {center:?}"
    );

    blur.destroy(&gl);
    free_fbo(&gl, scene.fbo, scene.tex);
    free_fbo(&gl, target.fbo, target.tex);
}

/// Two-operating-point oracle for the composite's **encode** call site (`encode_srgb` in
/// `composite.frag`): a pure-red backdrop (byte 255 → decodes to linear 1.0 under any power law — a
/// fixed point, so the decode site contributes nothing) is attenuated by a black tint film to a
/// known linear value, and the readback byte must match the canonical linear→sRGB encode of it.
///
/// What this proves / does not prove:
/// - **Proves** the encode's mix-branch/clamp at 0.85 (Point A — a swapped mix argument order reads
///   255) and the encode's exponent at 0.2 (Point B — the curve is ~5× steeper there, so a
///   2.2-for-2.4 substitution lands ≈115 vs the canonical ≈124, outside the band; at 0.85 the same
///   substitution measures 236 vs 237, invisible).
/// - **Does not prove** the decode site's exponent: it is structurally near-unobservable through
///   byte round-trips (byte 255 decodes to 1.0 under any power law, and decode∘encode is an
///   inverse pair whose flatness bounds a single-site exponent bug at ~2 bytes at every operating
///   point). The decode BRANCH-SWAP is already caught by the energy test above (a swapped decode
///   turns mid-gray 128 into ~55, far outside its band).
#[test]
fn composite_encodes_linear_through_the_glsl_at_two_operating_points() {
    let mut gl = headless_gl();
    let mut blur = GlowBlur::new(&gl).expect("new");
    // Red backdrop: byte 255 → linear 1.0 exactly, so the tint film alone sets the linear output.
    let scene = flat_backdrop(&gl, [1.0, 0.0, 0.0, 1.0]);
    let p = panel([24, 24], [80, 80]);

    // Point A (near-white): black film, alpha 0.15 → linear 0.85 red → canonical encode byte ≈237.
    // Pins the encode's mix-branch/clamp; does NOT pin the exponent (the curve is flat at 0.85 — a
    // 2.2-for-2.4 substitution measures 236 vs 237 here).
    let ta = frost(
        &mut gl,
        &mut blur,
        &scene,
        p,
        30.0,
        0.0,
        Tint::new(LinearRgba::new(0.0, 0.0, 0.0, 0.15)),
        [0.0, 0.0, 0.0, 1.0],
        1.0,
    );
    let a = read_texture_rgba8(&gl, ta.tex, 64, 64);

    // Point B (dark): black film, alpha 0.8 → linear 0.2 → canonical encode byte ≈124. This is the
    // exponent oracle: the curve is ~5× steeper at 0.2, so a 2.2 substitution lands ≈115 (Δ≈9),
    // outside the band.
    let tb = frost(
        &mut gl,
        &mut blur,
        &scene,
        p,
        30.0,
        0.0,
        Tint::new(LinearRgba::new(0.0, 0.0, 0.0, 0.8)),
        [0.0, 0.0, 0.0, 1.0],
        1.0,
    );
    let b = read_texture_rgba8(&gl, tb.tex, 64, 64);

    // Point A: measured 237 on this harness (exactly the canonical encode of 0.85).
    assert!(
        (230..=244).contains(&a[0]),
        "encode of linear 0.85 must read ≈237 (a swapped mix reads 255), got r={}",
        a[0]
    );
    assert!(
        a[1] <= 6 && a[2] <= 6,
        "the black film cannot introduce green/blue, got {a:?}"
    );
    // Point B: measured 124 on this harness (exactly the canonical encode of 0.2); ±5 keeps the
    // 2.2-substitution's ≈115 outside the band.
    assert!(
        (119..=129).contains(&b[0]),
        "encode of linear 0.2 must read ≈124 (the exponent oracle: 2.2 lands ≈115), got r={}",
        b[0]
    );
    assert!(
        b[1] <= 6 && b[2] <= 6,
        "the black film cannot introduce green/blue, got {b:?}"
    );

    blur.destroy(&gl);
    free_fbo(&gl, scene.fbo, scene.tex);
    free_fbo(&gl, ta.fbo, ta.tex);
    free_fbo(&gl, tb.fbo, tb.tex);
}

// --- 2g: end-to-end frost + GL-state-unchanged across record ---

#[test]
fn end_to_end_frosts_a_panel_over_a_known_backdrop() {
    let mut gl = headless_gl();
    let mut blur = GlowBlur::new(&gl).expect("new");
    // Red/blue seam; a faint dark tint film; readback proves the convolution ran (both channels at
    // the seam) and the corner mask cut the rounded corner (backdrop shows through there).
    let scene = split_backdrop(&gl, 64, [1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0]);
    let p = panel([32, 32], [64, 64]);
    let tint = Tint::new(LinearRgba::new(0.0, 0.0, 0.0, 0.15));
    let target = frost(
        &mut gl,
        &mut blur,
        &scene,
        p,
        6.0,
        16.0,
        tint,
        [0.0, 0.0, 0.0, 1.0],
        1.0,
    );

    // Panel center sits on the seam → a blurred red↔blue mix (both channels substantial).
    let center = read_texture_rgba8(&gl, target.tex, 64, 64);
    assert!(
        center[0] > 25 && center[2] > 25,
        "the frosted panel center must show a blurred red↔blue mix, got {center:?}"
    );
    blur.destroy(&gl);
    free_fbo(&gl, scene.fbo, scene.tex);
    free_fbo(&gl, target.fbo, target.tex);
}

/// A snapshot of the GL bindings the state assertion compares before/after `record`. Covers every
/// entry in the record save/restore list (DESIGN §11), including the **alpha** blend factors, the
/// blend **equation** (FIX 2), and the `TEXTURE_2D` binding on unit 0.
struct StateProbe {
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
    program: Option<glow::Program>,
    vao: Option<glow::VertexArray>,
    active_texture: i32,
    texture_2d_unit0: Option<glow::Texture>,
}

fn probe_state(gl: &glow::Context) -> StateProbe {
    // SAFETY: read-only GL state queries on the current context; the array getters write exactly
    // their 4-element buffers. Reading unit 0's TEXTURE_2D binding makes unit 0 active first and
    // restores the previously-active unit after, so the probe perturbs nothing.
    unsafe {
        let mut viewport = [0_i32; 4];
        gl.get_parameter_i32_slice(glow::VIEWPORT, &mut viewport);
        let mut scissor_box = [0_i32; 4];
        gl.get_parameter_i32_slice(glow::SCISSOR_BOX, &mut scissor_box);
        let active_texture = gl.get_parameter_i32(glow::ACTIVE_TEXTURE);
        gl.active_texture(glow::TEXTURE0);
        let texture_2d_unit0 = gl.get_parameter_texture(glow::TEXTURE_BINDING_2D);
        gl.active_texture(active_texture as u32);
        StateProbe {
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
            program: gl.get_parameter_program(glow::CURRENT_PROGRAM),
            vao: gl.get_parameter_vertex_array(glow::VERTEX_ARRAY_BINDING),
            active_texture,
            texture_2d_unit0,
        }
    }
}

#[test]
fn record_leaves_gl_state_unchanged() {
    let mut gl = headless_gl();
    let mut blur = GlowBlur::new(&gl).expect("new");
    let scene = flat_backdrop(&gl, [0.4, 0.4, 0.4, 1.0]);
    let (t_fbo, t_tex) = make_fbo(&gl);
    clear_fbo(&gl, t_fbo, [0.0, 0.0, 0.0, 1.0]);

    let p = panel([32, 32], [64, 64]);
    let region = GlRegion::from_bottom_px(p.origin, p.size, Scale::new(1.0));
    let source = blur
        .grab_source(&gl, &(), &Some(scene.fbo), region)
        .expect("grab");
    let request = BlurRequest {
        source_region: p,
        target_rect: p,
        blur_radius: BlurRadius::new(8.0),
        tint: Tint::new(LinearRgba::new(0.0, 0.0, 0.0, 0.2)),
        corner_radius: CornerRadius::new(8.0),
        presence: Presence::default(),
    };
    let prepared = blur
        .prepare(&gl, &(), &source, FramebufferSize([DIM, DIM]), &request)
        .expect("prepare")
        .expect("non-empty");

    // A distinctive throwaway texture to bind on unit 0 — record must leave unit 0's binding as
    // found, not pointing at its own grab/scratch texture.
    // SAFETY: create a 1×1 RGBA8 texture on the current context; freed at the end of the test.
    let host_tex = unsafe {
        let t = gl.create_texture().expect("host tex");
        gl.bind_texture(glow::TEXTURE_2D, Some(t));
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            glow::RGBA8 as i32,
            1,
            1,
            0,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelUnpackData::Slice(None),
        );
        t
    };

    // Establish a distinctive host GL state, snapshot it, run record, snapshot again — every binding
    // must match. This catches a missing entry in the record save/restore list (DESIGN §11). The
    // blend is set ASYMMETRIC (different RGB vs alpha factors) and to a NON-default equation
    // (FUNC_REVERSE_SUBTRACT), and unit 0 holds a distinctive texture — so the alpha factors, the
    // blend equation (FIX 2), and the unit-0 binding are all genuinely exercised, not trivially
    // already-default.
    // SAFETY: set a known host state on the current context; all handles are live.
    unsafe {
        gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(t_fbo));
        gl.bind_framebuffer(glow::READ_FRAMEBUFFER, Some(scene.fbo));
        gl.viewport(3, 7, 40, 50);
        gl.enable(glow::SCISSOR_TEST);
        gl.scissor(1, 2, 20, 30);
        gl.enable(glow::BLEND);
        gl.blend_func_separate(
            glow::SRC_ALPHA,
            glow::ONE_MINUS_SRC_ALPHA,
            glow::ONE,
            glow::ZERO,
        );
        gl.blend_equation(glow::FUNC_REVERSE_SUBTRACT);
        gl.active_texture(glow::TEXTURE0);
        gl.bind_texture(glow::TEXTURE_2D, Some(host_tex));
        gl.bind_vertex_array(Some(blur.vao));
    }
    let before = probe_state(&gl);
    blur.record(&mut gl, &Some(t_fbo), prepared)
        .expect("record");
    // SAFETY: flush the record's commands before re-probing.
    unsafe { gl.finish() };
    let after = probe_state(&gl);

    assert_eq!(before.draw_fbo, after.draw_fbo, "draw FBO changed");
    assert_eq!(before.read_fbo, after.read_fbo, "read FBO changed");
    assert_eq!(before.viewport, after.viewport, "viewport changed");
    assert_eq!(before.scissor_box, after.scissor_box, "scissor box changed");
    assert_eq!(
        before.scissor_enabled, after.scissor_enabled,
        "scissor enable changed"
    );
    assert_eq!(
        before.blend_enabled, after.blend_enabled,
        "blend enable changed"
    );
    assert_eq!(
        before.blend_src_rgb, after.blend_src_rgb,
        "blend src changed"
    );
    assert_eq!(
        before.blend_dst_rgb, after.blend_dst_rgb,
        "blend dst changed"
    );
    assert_eq!(
        before.blend_src_alpha, after.blend_src_alpha,
        "blend src alpha changed"
    );
    assert_eq!(
        before.blend_dst_alpha, after.blend_dst_alpha,
        "blend dst alpha changed"
    );
    assert_eq!(
        before.blend_equation_rgb, after.blend_equation_rgb,
        "blend equation rgb changed (FIX 2)"
    );
    assert_eq!(
        before.blend_equation_alpha, after.blend_equation_alpha,
        "blend equation alpha changed (FIX 2)"
    );
    assert_eq!(before.program, after.program, "program changed");
    assert_eq!(before.vao, after.vao, "VAO changed");
    assert_eq!(
        before.active_texture, after.active_texture,
        "active texture changed"
    );
    assert_eq!(
        before.texture_2d_unit0, after.texture_2d_unit0,
        "TEXTURE_2D on unit 0 changed"
    );

    blur.destroy(&gl);
    // SAFETY: `host_tex` was created on this context above and is freed once here.
    unsafe { gl.delete_texture(host_tex) };
    free_fbo(&gl, scene.fbo, scene.tex);
    free_fbo(&gl, t_fbo, t_tex);
}

/// A no-op when the source region clips to nothing: `prepare` returns `Ok(None)` (DESIGN §4.4).
#[test]
fn prepare_is_a_no_op_for_a_fully_offscreen_region() {
    let gl = headless_gl();
    let mut blur = GlowBlur::new(&gl).expect("new");
    let scene = flat_backdrop(&gl, [0.5, 0.5, 0.5, 1.0]);
    // A region whose origin is past the framebuffer extent clips to nothing.
    let offscreen = panel([DIM, DIM], [10, 10]);
    let source = GrabSource { texture: scene.tex };
    let request = BlurRequest {
        source_region: offscreen,
        target_rect: offscreen,
        blur_radius: BlurRadius::new(8.0),
        tint: no_tint(),
        corner_radius: CornerRadius::new(0.0),
        presence: Presence::default(),
    };
    let prepared = blur
        .prepare(&gl, &(), &source, FramebufferSize([DIM, DIM]), &request)
        .expect("prepare ok");
    assert!(
        prepared.is_none(),
        "a fully-offscreen region must be a no-op (Ok(None))"
    );
    blur.destroy(&gl);
    free_fbo(&gl, scene.fbo, scene.tex);
}

/// The fused shared-context entry surfaces the seam's `prepare → Ok(None)` no-op as
/// [`FrostEffect::ClippedEmpty`] instead of swallowing it: a valid grab whose request then clips
/// to nothing composites nothing and says so.
#[test]
fn frost_region_reports_clipped_empty_when_the_request_clips_to_nothing() {
    let gl = headless_gl();
    let mut blur = GlowBlur::new(&gl).expect("new");
    let scene = flat_backdrop(&gl, [0.5, 0.5, 0.5, 1.0]);
    // The grab region is in-bounds (the grab succeeds); the request's source region is fully
    // offscreen, so `prepare` inside `frost_region` returns `Ok(None)`.
    let grab = GlRegion::from_bottom_px([0, 0], [16, 16], Scale::new(1.0));
    let offscreen = panel([DIM, DIM], [10, 10]);
    let request = BlurRequest {
        source_region: offscreen,
        target_rect: offscreen,
        blur_radius: BlurRadius::new(8.0),
        tint: no_tint(),
        corner_radius: CornerRadius::new(0.0),
        presence: Presence::default(),
    };
    let effect = blur
        .frost_region(
            &gl,
            Some(scene.fbo),
            grab,
            FramebufferSize([DIM, DIM]),
            &request,
        )
        .expect("frost_region ok");
    assert_eq!(
        effect,
        FrostEffect::ClippedEmpty,
        "a request that clips to nothing must report ClippedEmpty, not a silent no-op"
    );
    blur.destroy(&gl);
    free_fbo(&gl, scene.fbo, scene.tex);
}

/// The target-side half of the no-op: a zero-area `target_rect` over a NORMAL in-bounds source
/// region (mismatched regions — the path no adapter exercises) is `prepare → Ok(None)`, surfaced
/// by the fused entry as [`FrostEffect::ClippedEmpty`]. The composite divides by the target size
/// (composite.frag:45), so without the guard a zero dimension yields NaN/Inf UVs along a visible
/// line instead of nothing.
#[test]
fn frost_region_reports_clipped_empty_for_a_zero_area_target_rect() {
    let gl = headless_gl();
    let mut blur = GlowBlur::new(&gl).expect("new");
    let scene = flat_backdrop(&gl, [0.5, 0.5, 0.5, 1.0]);
    // Both the grab and the source region are normal and in-bounds; only the target is empty.
    let p = panel([32, 32], [64, 64]);
    let grab = GlRegion::from_bottom_px(p.origin, p.size, Scale::new(1.0));
    let request = BlurRequest {
        source_region: p,
        target_rect: panel([32, 32], [0, 64]), // zero width → empty target
        blur_radius: BlurRadius::new(8.0),
        tint: no_tint(),
        corner_radius: CornerRadius::new(0.0),
        presence: Presence::default(),
    };
    let effect = blur
        .frost_region(
            &gl,
            Some(scene.fbo),
            grab,
            FramebufferSize([DIM, DIM]),
            &request,
        )
        .expect("frost_region ok");
    assert_eq!(
        effect,
        FrostEffect::ClippedEmpty,
        "a zero-area target_rect must report ClippedEmpty, not reach the composite"
    );
    blur.destroy(&gl);
    free_fbo(&gl, scene.fbo, scene.tex);
}

/// The fused entry reports [`FrostEffect::Composited`] for a normal in-bounds panel — the
/// positive half of the `FrostEffect` contract (pixel-level proofs live in the readback tests).
#[test]
fn frost_region_reports_composited_for_a_normal_panel() {
    let gl = headless_gl();
    let mut blur = GlowBlur::new(&gl).expect("new");
    let scene = split_backdrop(&gl, 64, [1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0]);
    let p = panel([32, 32], [64, 64]);
    let grab = GlRegion::from_bottom_px(p.origin, p.size, Scale::new(1.0));
    let request = BlurRequest {
        source_region: p,
        target_rect: p,
        blur_radius: BlurRadius::new(6.0),
        tint: no_tint(),
        corner_radius: CornerRadius::new(0.0),
        presence: Presence::default(),
    };
    let effect = blur
        .frost_region(
            &gl,
            Some(scene.fbo),
            grab,
            FramebufferSize([DIM, DIM]),
            &request,
        )
        .expect("frost_region ok");
    assert_eq!(
        effect,
        FrostEffect::Composited,
        "an in-bounds panel must report Composited"
    );
    blur.destroy(&gl);
    free_fbo(&gl, scene.fbo, scene.tex);
}

/// The surface-global fade (`Presence`) is a real linear blend toward the untouched destination:
/// `out(presence) == lerp(D, F, presence)` at a panel-interior pixel (coverage = 1, so the per-pixel
/// coverage is out of it and the master presence is the only variable). `presence = 0` leaves the
/// seeded destination untouched; `0.5` is the byte-space midpoint of D and the fully-present F. This
/// is the premultiplied-path counterpart of the wgpu oracle, proving the glow `rgb*a, a` fold is the
/// same linear fade.
#[test]
fn presence_fades_the_surface_linearly_toward_the_destination() {
    let mut gl = headless_gl();
    let mut blur = GlowBlur::new(&gl).expect("new");
    // Destination seed near-black; backdrop bright gray — so the frosted interior F differs from D
    // and the fade D -> F is non-trivial. No tint, so F is the pure blurred backdrop.
    let scene = flat_backdrop(&gl, [0.7, 0.7, 0.7, 1.0]);
    let p = panel([24, 24], [80, 80]);
    let seed = [0.05, 0.05, 0.05, 1.0];

    let t0 = frost(
        &mut gl,
        &mut blur,
        &scene,
        p,
        12.0,
        0.0,
        no_tint(),
        seed,
        0.0,
    );
    let thalf = frost(
        &mut gl,
        &mut blur,
        &scene,
        p,
        12.0,
        0.0,
        no_tint(),
        seed,
        0.5,
    );
    let t1 = frost(
        &mut gl,
        &mut blur,
        &scene,
        p,
        12.0,
        0.0,
        no_tint(),
        seed,
        1.0,
    );

    // Panel interior (panel = [24,24]+80x80), coverage = 1.
    let (cx, cy) = (64, 64);
    let d = read_texture_rgba8(&gl, t0.tex, cx, cy); // presence 0 == the destination D
    let h = read_texture_rgba8(&gl, thalf.tex, cx, cy);
    let f = read_texture_rgba8(&gl, t1.tex, cx, cy); // presence 1 == fully-present F

    // presence = 0 leaves the destination untouched (near-black seed).
    for (ch, &channel) in d.iter().take(3).enumerate() {
        assert!(
            channel <= 16,
            "presence=0 leaves the destination untouched (channel {ch} = {channel})"
        );
    }
    // presence = 0.5 is the linear midpoint between D and F (byte space, where the blend happens).
    for ch in 0..3 {
        let expected = (i32::from(d[ch]) + i32::from(f[ch]) + 1) / 2;
        let got = i32::from(h[ch]);
        assert!(
            (got - expected).abs() <= 3,
            "presence=0.5 == lerp(D,F,0.5) at the interior (channel {ch}: D={} F={} expected≈{expected} got={got})",
            d[ch],
            f[ch]
        );
    }
    // The fade must actually move the pixel, or the oracle is vacuous.
    assert!(
        (0..3).any(|ch| f[ch] != d[ch]),
        "the frost must change the interior pixel, else the oracle proves nothing"
    );

    blur.destroy(&gl);
    free_fbo(&gl, scene.fbo, scene.tex);
    free_fbo(&gl, t0.fbo, t0.tex);
    free_fbo(&gl, thalf.fbo, thalf.tex);
    free_fbo(&gl, t1.fbo, t1.tex);
}

// --- G1/G2: attachment-resolved encode — decision guard (5a) + hardware-encode rendering (5b) ---

/// 5a — [`resolve_target_encoding`](crate::composite::resolve_target_encoding) reads the bound
/// target's colour-attachment encoding and, where the enable is a valid capability (desktop GL here),
/// the `GL_FRAMEBUFFER_SRGB` state. A non-sRGB (RGBA8) attachment always resolves to `Srgb` (the
/// shader must encode); an sRGB (`SRGB8_ALPHA8`) attachment's encode follows the enable. Also pins the
/// **orthogonality** the resolver relies on — the reported attachment encoding does not move with the
/// enable — and that the `COLOR_ENCODING` query is **error-clean** on this dialect (the G1 property).
#[test]
fn resolve_target_encoding_reads_the_attachment_and_enable() {
    let gl = headless_gl();
    let profile = crate::GlProfile::probe(&gl).expect("desktop profile");
    assert!(!profile.embedded, "the surfaceless harness is desktop GL");
    assert!(
        profile.srgb_enable_is_queryable,
        "desktop GL exposes GL_FRAMEBUFFER_SRGB as a core capability"
    );

    let (srgb_fbo, srgb_tex) = make_fbo_with_format(&gl, glow::SRGB8_ALPHA8 as i32);
    let (rgba_fbo, rgba_tex) = make_fbo(&gl);

    // SAFETY: read-only attachment/enable queries + enable toggles on the current context; the
    // FRAMEBUFFER_SRGB enable is restored to its entry state before returning.
    unsafe {
        let entry_enabled = gl.is_enabled(glow::FRAMEBUFFER_SRGB);

        // Orthogonality: the RGBA8 attachment reports LINEAR encoding, unchanged by the enable.
        gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(rgba_fbo));
        gl.disable(glow::FRAMEBUFFER_SRGB);
        let rgba_off = gl.get_framebuffer_attachment_parameter_i32(
            glow::DRAW_FRAMEBUFFER,
            glow::COLOR_ATTACHMENT0,
            glow::FRAMEBUFFER_ATTACHMENT_COLOR_ENCODING,
        );
        gl.enable(glow::FRAMEBUFFER_SRGB);
        let rgba_on = gl.get_framebuffer_attachment_parameter_i32(
            glow::DRAW_FRAMEBUFFER,
            glow::COLOR_ATTACHMENT0,
            glow::FRAMEBUFFER_ATTACHMENT_COLOR_ENCODING,
        );
        assert_eq!(
            rgba_off,
            glow::LINEAR as i32,
            "an RGBA8 attachment is LINEAR"
        );
        assert_eq!(
            rgba_on, rgba_off,
            "attachment encoding is orthogonal to the FRAMEBUFFER_SRGB enable"
        );

        // The SRGB8_ALPHA8 attachment reports SRGB, and the query raises no GL error on desktop GL.
        gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(srgb_fbo));
        let srgb_enc = gl.get_framebuffer_attachment_parameter_i32(
            glow::DRAW_FRAMEBUFFER,
            glow::COLOR_ATTACHMENT0,
            glow::FRAMEBUFFER_ATTACHMENT_COLOR_ENCODING,
        );
        assert_eq!(
            srgb_enc,
            glow::SRGB as i32,
            "an SRGB8_ALPHA8 attachment is SRGB"
        );
        assert_eq!(
            gl.get_error(),
            glow::NO_ERROR,
            "the COLOR_ENCODING query is error-clean on desktop GL (the G1 property)"
        );

        // Resolver: non-sRGB target ⇒ Srgb, with the enable off OR on.
        gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(rgba_fbo));
        gl.disable(glow::FRAMEBUFFER_SRGB);
        assert_eq!(
            crate::composite::resolve_target_encoding(&gl, &profile, Some(rgba_fbo)),
            TargetEncoding::Srgb
        );
        gl.enable(glow::FRAMEBUFFER_SRGB);
        assert_eq!(
            crate::composite::resolve_target_encoding(&gl, &profile, Some(rgba_fbo)),
            TargetEncoding::Srgb,
            "a non-sRGB target never lets the hardware encode, even with the enable on"
        );

        // Resolver: sRGB target ⇒ follows the (valid) enable.
        gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, Some(srgb_fbo));
        gl.disable(glow::FRAMEBUFFER_SRGB);
        assert_eq!(
            crate::composite::resolve_target_encoding(&gl, &profile, Some(srgb_fbo)),
            TargetEncoding::Srgb,
            "sRGB target, write-encode off: the shader must encode"
        );
        gl.enable(glow::FRAMEBUFFER_SRGB);
        assert_eq!(
            crate::composite::resolve_target_encoding(&gl, &profile, Some(srgb_fbo)),
            TargetEncoding::Linear,
            "sRGB target, write-encode on: the hardware encodes"
        );

        if entry_enabled {
            gl.enable(glow::FRAMEBUFFER_SRGB);
        } else {
            gl.disable(glow::FRAMEBUFFER_SRGB);
        }
        gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, None);
    }

    free_fbo(&gl, srgb_fbo, srgb_tex);
    free_fbo(&gl, rgba_fbo, rgba_tex);
}

/// 5b — the `TargetEncoding::Linear` branch, **render-verified**. With an `SRGB8_ALPHA8` target and
/// `GL_FRAMEBUFFER_SRGB` enabled the resolver yields `Linear`, so the composite shader skips its own
/// OETF and the hardware encodes linear→sRGB on write. Compositing the same linear `0.85` the
/// manual-encode oracle uses (`composite_encodes_linear_through_the_glsl_at_two_operating_points`)
/// must read back the SAME canonical byte ≈237 — proving the encode happens exactly **once**. A wrong
/// `Srgb` resolution (the shader *also* encodes) double-encodes to ≈247; the band around 237 excludes
/// it. `0.85` is the existing oracle's proven-separated operating point (near 0/1 the sRGB OETF fixed
/// points would collapse the single/double/raw values together).
///
/// This is the first test to exercise hardware sRGB encode-on-write on this harness. If a driver does
/// not honour it, this test fails distinctively (≈217, the raw-linear value) — at which point the
/// `Linear` branch would need a decision-only guard and the harness limitation recorded here.
#[test]
fn linear_branch_lets_hardware_encode_to_an_srgb_target() {
    let mut gl = headless_gl();
    let mut blur = GlowBlur::new(&gl).expect("new");
    // Red backdrop: byte 255 → linear 1.0 exactly, so the tint film alone sets the linear output.
    let scene = flat_backdrop(&gl, [1.0, 0.0, 0.0, 1.0]);
    let p = panel([24, 24], [80, 80]);

    // An sRGB target; turn on hardware sRGB write-encode (restored at the end).
    let (t_fbo, t_tex) = make_fbo_with_format(&gl, glow::SRGB8_ALPHA8 as i32);
    // SAFETY: read + set the enable on the current context; restored before returning.
    let entry_enabled = unsafe {
        let was = gl.is_enabled(glow::FRAMEBUFFER_SRGB);
        gl.enable(glow::FRAMEBUFFER_SRGB);
        was
    };
    clear_fbo(&gl, t_fbo, [0.0, 0.0, 0.0, 1.0]);

    // Grab → prepare → record into the caller-supplied sRGB target (mirrors `frost`).
    let region = GlRegion::from_bottom_px(p.origin, p.size, Scale::new(1.0));
    let source = blur
        .grab_source(&gl, &(), &Some(scene.fbo), region)
        .expect("grab");
    let request = BlurRequest {
        source_region: p,
        target_rect: p,
        blur_radius: BlurRadius::new(30.0),
        // Black film, alpha 0.15 → linear 0.85 red (the oracle's Point A).
        tint: Tint::new(LinearRgba::new(0.0, 0.0, 0.0, 0.15)),
        corner_radius: CornerRadius::new(0.0),
        presence: Presence::new(1.0),
    };
    let prepared = blur
        .prepare(&gl, &(), &source, FramebufferSize([DIM, DIM]), &request)
        .expect("prepare")
        .expect("non-empty region prepares a blur");
    blur.record(&mut gl, &Some(t_fbo), prepared)
        .expect("record");
    // SAFETY: flush so the readback sees the composite.
    unsafe { gl.finish() };

    let r = i32::from(read_texture_rgba8(&gl, t_tex, 64, 64)[0]);
    const TOL: i32 = 7;
    // The band [230,244] discriminates all three outcomes: correct single-encode 237 (in), the
    // shader-double-encode 247 (`srgb(srgb(0.85))·255`, out by 3), and raw-linear 217 (`0.85·255`,
    // no encode, out by 13). Measured 237 on this harness — exactly the canonical single encode.
    assert!(
        (237 - TOL..=237 + TOL).contains(&r),
        "the Linear branch must let the hardware encode 0.85 exactly once (≈237): got r={r} \
         (≈247 = shader double-encoded; ≈217 = raw linear, no encode)"
    );

    // SAFETY: restore the enable to its entry state.
    unsafe {
        if entry_enabled {
            gl.enable(glow::FRAMEBUFFER_SRGB);
        } else {
            gl.disable(glow::FRAMEBUFFER_SRGB);
        }
    }
    blur.destroy(&gl);
    free_fbo(&gl, scene.fbo, scene.tex);
    free_fbo(&gl, t_fbo, t_tex);
}
