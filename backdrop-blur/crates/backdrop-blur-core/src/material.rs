//! The material — *what kind of glass*. The knobs a caller turns to describe a frosted
//! surface: how much blur, what tint film, how round the corners. All logical (in points);
//! the backend resolves them to physical pixels against a region's [`Scale`].

use crate::geometry::Scale;

/// Blur radius in **logical points**.
///
/// Core resolves this to a physical-pixel radius — the one algorithm-*agnostic* step. The
/// **GPU application** of the resulting parameters (allocating the pyramid textures, binding the
/// per-pass offset uniforms, the draws) lives in the backend; the GPU-free *policy* that
/// produces them — the Gaussian sigma/taps and the dual-Kawase level/half-pixel math — lives in
/// the crate's `algorithm` module (DESIGN §4.2, §15). **Reversal noted (glow IMPL §0c):** an earlier
/// version of this paragraph said core "has no notion of levels — that is the wgpu crate's".
/// That held while wgpu was the only backend; the glow backend needs the *same* level policy, so
/// it was hoisted to core to keep the two backends from drifting. Core now owns the level math;
/// only the GPU resources it keys stay backend-specific.
///
/// The resolution is exposed as [`BlurRequest::physical_blur_radius`], **not** a free
/// `blur_radius × scale` call: a [`BlurRequest`] carries two independent scales (source vs
/// target), and the blur convolution happens in the *source* texture's pixel space, so the
/// radius must resolve against `source_region.scale`. Pinning that scale inside the request
/// makes the wrong one impossible to pass (the same guardrail [`ResolvedMask::from_target`]
/// gives the corner radius).
///
/// Non-negative by construction: a negative radius is meaningless, so [`Self::new`] clamps to
/// `0` (which the backend reads as "no blur"), keeping the type total.
///
/// [`BlurRequest::physical_blur_radius`]: crate::BlurRequest::physical_blur_radius
/// [`BlurRequest`]: crate::BlurRequest
/// [`ResolvedMask::from_target`]: crate::ResolvedMask::from_target
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlurRadius(f32);

impl BlurRadius {
    /// Construct from logical points. Non-finite input (`NaN`/`±∞`) and negatives clamp to `0`,
    /// so a garbage radius becomes "no blur" rather than a degenerate kernel downstream.
    pub fn new(points: f32) -> Self {
        Self(if points.is_finite() {
            points.max(0.0)
        } else {
            0.0
        })
    }

    /// The blur radius in logical points (always `>= 0`).
    pub fn points(self) -> f32 {
        self.0
    }

    /// Logical points × a scale = physical-pixel blur radius. Crate-private on purpose: the
    /// *correct* scale is always the source region's, so callers resolve through
    /// [`BlurRequest::physical_blur_radius`](crate::BlurRequest::physical_blur_radius), which
    /// pins it. Exposing a bare `(scale)` socket would let the wrong region's scale through
    /// on a mismatched-DPI surface with no compile error.
    pub(crate) fn to_physical_radius(self, scale: Scale) -> f32 {
        self.0 * scale.factor()
    }
}

/// A straight-alpha color in **linear light**. RGB are linear (already gamma-decoded) and may
/// exceed `1.0` (HDR over-bright); alpha is coverage in `[0, 1]` (never gamma-encoded). The
/// blur convolution runs in linear light, so a tint authored in sRGB must be decoded first —
/// that is exactly what [`Self::from_srgb_unmultiplied`] does, so callers never hand the
/// backend gamma-encoded tint values (DESIGN §4.2).
///
/// Fields are private so the "already linear" invariant is only ever established through a
/// named constructor (matching the other newtypes), and both constructors are **total**:
/// non-finite channels (`NaN`/`±∞`) are scrubbed to `0.0` so a malformed tint can never reach
/// the GPU as undebuggable garbage.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LinearRgba {
    r: f32,
    g: f32,
    b: f32,
    a: f32,
}

impl LinearRgba {
    /// Build from channels that are *already* linear. Non-finite channels are scrubbed to
    /// `0.0` and alpha is clamped to `[0, 1]`; RGB keep their (possibly `> 1`, HDR) magnitude.
    pub fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self {
            r: finite_or_zero(r),
            g: finite_or_zero(g),
            b: finite_or_zero(b),
            a: finite_or_zero(a).clamp(0.0, 1.0),
        }
    }

    /// Decode straight-alpha sRGB bytes to linear light: RGB through the sRGB EOTF, alpha
    /// linearly. This is the gamma decode the linear-space convolution requires. The result is
    /// finite by construction (`u8 / 255` and the EOTF never produce `NaN`/`∞`).
    pub fn from_srgb_unmultiplied(rgba: [u8; 4]) -> Self {
        let [r, g, b, a] = rgba;
        Self {
            r: srgb_to_linear(f32::from(r) / 255.0),
            g: srgb_to_linear(f32::from(g) / 255.0),
            b: srgb_to_linear(f32::from(b) / 255.0),
            a: f32::from(a) / 255.0,
        }
    }

    /// Linear red.
    pub fn r(self) -> f32 {
        self.r
    }

    /// Linear green.
    pub fn g(self) -> f32 {
        self.g
    }

    /// Linear blue.
    pub fn b(self) -> f32 {
        self.b
    }

    /// Film opacity / coverage, in `[0, 1]`.
    pub fn a(self) -> f32 {
        self.a
    }
}

/// The sRGB electro-optical transfer function (gamma → linear) for one normalized channel.
fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.040_45 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Replace a non-finite value (`NaN`/`±∞`) with `0.0`, leaving finite values untouched.
fn finite_or_zero(x: f32) -> f32 {
    if x.is_finite() { x } else { 0.0 }
}

/// The glass film painted over the blurred backdrop. The wrapped color is linear-light; its
/// alpha is the **film opacity** (how much of the tint shows over the blur).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tint(LinearRgba);

impl Tint {
    /// Wrap an already-linear color as the film.
    pub fn new(color: LinearRgba) -> Self {
        Self(color)
    }

    /// Convenience: a film authored as straight-alpha sRGB bytes, decoded to linear.
    pub fn from_srgb_unmultiplied(rgba: [u8; 4]) -> Self {
        Self(LinearRgba::from_srgb_unmultiplied(rgba))
    }

    /// The film color in linear light.
    pub fn color(self) -> LinearRgba {
        self.0
    }
}

/// Corner radius in **logical points**. Resolves (× the target region's [`Scale`]) to a
/// physical-pixel radius, clamped so it can never overshoot the surface (the clamp lives in
/// [`crate::ResolvedMask::from_target`]). Non-negative by construction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CornerRadius(f32);

impl CornerRadius {
    /// Construct from logical points. Non-finite input (`NaN`/`±∞`) and negatives clamp to `0`
    /// (square corners).
    pub fn new(points: f32) -> Self {
        Self(if points.is_finite() {
            points.max(0.0)
        } else {
            0.0
        })
    }

    /// The corner radius in logical points (always `>= 0`).
    pub fn points(self) -> f32 {
        self.0
    }
}

/// How *present* the whole frosted surface is — the surface-global **fade coverage** in
/// `[0, 1]`, distinct from [`Tint`]'s alpha (which is the film *mix*, blur vs tint color) and
/// from [`BlurRadius`] (the blur amount, a length — not a fade). It scales the composite's final blend weight: `1.0` is the
/// surface fully composited (the default — every existing caller and golden is unchanged), `0.0`
/// leaves the destination untouched (the surface absent), and a fractional value blends the
/// frosted result over the destination by that factor. A consumer animating a surface in/out
/// (a modal scrim fading with its dialog) drives this per frame.
///
/// Two-sided clamp `[0, 1]` (unlike [`BlurRadius`]/[`CornerRadius`], which clamp only the
/// lower bound) — the precedent is [`LinearRgba`]'s alpha. Non-finite input falls back to `1.0`
/// (fully present, behavior-preserving), **not** `0.0`: a `NaN` propagates through `f32::clamp`,
/// and a silently-invisible surface is a worse failure than a silently-opaque one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Presence(f32);

impl Presence {
    /// A fully-present surface — the default.
    pub const FULL: Self = Self(1.0);

    /// Construct from a `[0, 1]` factor. Out-of-range clamps; non-finite (`NaN`/`±∞`) falls back
    /// to `1.0` (fully present).
    pub fn new(factor: f32) -> Self {
        Self(if factor.is_finite() {
            factor.clamp(0.0, 1.0)
        } else {
            1.0
        })
    }

    /// The fade coverage in `[0, 1]`.
    pub fn value(self) -> f32 {
        self.0
    }
}

impl Default for Presence {
    fn default() -> Self {
        Self::FULL
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn blur_radius_new_clamps_negative_to_zero() {
        assert_eq!(BlurRadius::new(-3.0).points(), 0.0);
        assert_eq!(BlurRadius::new(8.0).points(), 8.0);
    }

    #[test]
    fn blur_radius_new_scrubs_non_finite_to_zero() {
        assert_eq!(BlurRadius::new(f32::NAN).points(), 0.0);
        assert_eq!(BlurRadius::new(f32::INFINITY).points(), 0.0);
        assert_eq!(BlurRadius::new(f32::NEG_INFINITY).points(), 0.0);
    }

    #[test]
    fn to_physical_radius_multiplies_by_scale() {
        let r = BlurRadius::new(8.0).to_physical_radius(Scale::new(2.0));
        assert!(close(r, 16.0));
    }

    #[test]
    fn to_physical_radius_of_zero_radius_is_zero() {
        let r = BlurRadius::new(0.0).to_physical_radius(Scale::new(3.0));
        assert!(close(r, 0.0));
    }

    #[test]
    fn corner_radius_new_clamps_negative_to_zero() {
        assert_eq!(CornerRadius::new(-1.0).points(), 0.0);
        assert_eq!(CornerRadius::new(12.0).points(), 12.0);
    }

    #[test]
    fn from_srgb_unmultiplied_maps_endpoints_exactly() {
        let black = LinearRgba::from_srgb_unmultiplied([0, 0, 0, 255]);
        assert!(close(black.r(), 0.0) && close(black.g(), 0.0) && close(black.b(), 0.0));
        assert!(close(black.a(), 1.0));

        let white = LinearRgba::from_srgb_unmultiplied([255, 255, 255, 255]);
        assert!(close(white.r(), 1.0) && close(white.g(), 1.0) && close(white.b(), 1.0));
    }

    #[test]
    fn from_srgb_unmultiplied_decodes_midtone_through_eotf() {
        // sRGB 188/255 ≈ 0.737 gamma → ≈ 0.502 linear (the classic "perceptual half").
        let mid = LinearRgba::from_srgb_unmultiplied([188, 188, 188, 128]);
        assert!(close(mid.r(), 0.502_886_5));
        // Alpha is linear, not gamma-decoded.
        assert!(close(mid.a(), 128.0 / 255.0));
    }

    #[test]
    fn from_srgb_unmultiplied_uses_linear_segment_near_black() {
        // Below the 0.04045 knee the transfer is the linear c/12.92 segment.
        let dark = LinearRgba::from_srgb_unmultiplied([2, 2, 2, 255]);
        let expected = (2.0 / 255.0) / 12.92;
        assert!(close(dark.r(), expected));
    }

    #[test]
    fn new_scrubs_non_finite_channels_to_zero() {
        let scrubbed = LinearRgba::new(f32::NAN, f32::INFINITY, f32::NEG_INFINITY, f32::NAN);
        assert_eq!(scrubbed.r(), 0.0);
        assert_eq!(scrubbed.g(), 0.0);
        assert_eq!(scrubbed.b(), 0.0);
        assert_eq!(scrubbed.a(), 0.0);
    }

    #[test]
    fn new_clamps_alpha_but_keeps_hdr_rgb() {
        let color = LinearRgba::new(4.0, 0.0, 0.0, 1.5);
        assert!(close(color.r(), 4.0)); // HDR over-bright preserved
        assert!(close(color.a(), 1.0)); // alpha clamped into [0, 1]
    }

    #[test]
    fn tint_from_srgb_decodes_its_wrapped_color() {
        let tint = Tint::from_srgb_unmultiplied([255, 255, 255, 64]);
        assert!(close(tint.color().r(), 1.0));
        assert!(close(tint.color().a(), 64.0 / 255.0));
    }

    #[test]
    fn presence_new_clamps_into_unit_range() {
        assert_eq!(Presence::new(-1.0).value(), 0.0);
        assert_eq!(Presence::new(2.0).value(), 1.0);
        assert!(close(Presence::new(0.3).value(), 0.3));
    }

    #[test]
    fn presence_new_scrubs_non_finite_to_full() {
        // Non-finite falls back to 1.0 (fully present), NOT 0.0 — a NaN propagates through
        // f32::clamp, and an invisible surface is the worse silent failure.
        assert_eq!(Presence::new(f32::NAN).value(), 1.0);
        assert_eq!(Presence::new(f32::INFINITY).value(), 1.0);
        assert_eq!(Presence::new(f32::NEG_INFINITY).value(), 1.0);
    }

    #[test]
    fn presence_default_and_full_are_one() {
        assert_eq!(Presence::default().value(), 1.0);
        assert_eq!(Presence::FULL.value(), 1.0);
    }
}
