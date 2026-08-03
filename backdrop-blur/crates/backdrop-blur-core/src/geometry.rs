//! Geometry and the request bundle. The seam speaks **physical pixels**: a [`Region`] is a
//! physical-pixel rectangle that carries its own logical→physical [`Scale`], so the source
//! intermediate and the swapchain target can differ in DPI without a single global factor
//! (DESIGN §4.1). [`ResolvedMask`] is the one shader input core computes; [`BlurRequest`] is
//! the bundle that crosses the seam.

use crate::material::{BlurRadius, CornerRadius, Presence, Tint};

/// A logical→physical scale factor (DPI) for one region. Strictly positive by construction:
/// a zero or negative factor would make the resolution math degenerate (every resolved radius
/// collapses to `0`), so [`Self::new`] floors it at the smallest positive `f32`.
///
/// This guards only against zero/negative. A *near-zero* (sub-pixel) factor is almost always
/// an uninitialized-DPI caller bug that this type does **not** catch, so a backend should not
/// read `factor() > 0` as "meaningfully large".
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scale(f32);

impl Scale {
    /// Construct from a factor, flooring at the smallest positive `f32`.
    pub fn new(factor: f32) -> Self {
        Self(factor.max(f32::MIN_POSITIVE))
    }

    /// The scale factor (always `> 0`).
    pub fn factor(self) -> f32 {
        self.0
    }
}

impl Default for Scale {
    /// 1 physical pixel per logical point.
    fn default() -> Self {
        Self(1.0)
    }
}

/// A physical-pixel rectangle carrying its own logical→physical [`Scale`]. Construct with a
/// struct literal so the two `[u32; 2]` fields are named at the call site (no positional
/// origin/size swap is possible).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Region {
    /// Top-left corner in physical pixels.
    pub origin: [u32; 2],
    /// Width and height in physical pixels.
    pub size: [u32; 2],
    /// This region's logical→physical scale.
    pub scale: Scale,
}

impl Region {
    /// Whether this region has zero area — i.e. its width or height is `0`.
    ///
    /// An empty region cannot be composited into. This is the *target-side* half of the blur
    /// **no-op** contract (`prepare` returns `Ok(None)`, DESIGN §9), mirroring how
    /// [`Self::clip_to`] returning `None` is the source-side half. The composite shaders divide
    /// by the target rect's size, so an empty target must never reach them.
    pub fn is_empty(&self) -> bool {
        self.size[0] == 0 || self.size[1] == 0
    }

    /// Clip this region to a source texture of size `source_extent`, returning the in-bounds
    /// intersection (its scale preserved).
    ///
    /// Returns `None` when the intersection is empty — i.e. the region is zero-area or lies
    /// fully outside the texture. That `None` is the blur **no-op**: `prepare` returns
    /// `Ok(None)` and `record` is never called (DESIGN §4.4/§4.5). A *partially* offscreen
    /// region is clipped to the in-bounds sub-rect rather than dropped, so the backend never
    /// samples outside the source (the "Region clipping" core operation, DESIGN §11). All
    /// arithmetic saturates, so an `origin + size` past `u32::MAX` cannot overflow.
    pub fn clip_to(&self, source_extent: [u32; 2]) -> Option<Region> {
        let [ox, oy] = self.origin;
        let [w, h] = self.size;
        let [ex, ey] = source_extent;

        let x0 = ox.min(ex);
        let y0 = oy.min(ey);
        let x1 = ox.saturating_add(w).min(ex);
        let y1 = oy.saturating_add(h).min(ey);

        let clipped_w = x1.saturating_sub(x0);
        let clipped_h = y1.saturating_sub(y0);

        if clipped_w == 0 || clipped_h == 0 {
            None
        } else {
            Some(Region {
                origin: [x0, y0],
                size: [clipped_w, clipped_h],
                scale: self.scale,
            })
        }
    }
}

impl std::fmt::Display for Region {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let [x, y] = self.origin;
        let [w, h] = self.size;
        let scale = self.scale.factor();
        write!(f, "origin ({x}, {y}), size {w}×{h}, scale {scale}")
    }
}

/// What core computes for the shader: the target surface's half-extents (physical px) and the
/// physical corner radius, **clamped to `min(half_extent)`** so the rounded-rect SDF can never
/// overshoot into a malformed shape. The per-pixel SDF is the shader's job; this is its
/// resolved, GPU-agnostic input — which is exactly what makes the clamp headless-testable
/// (DESIGN §4.3, §11).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedMask {
    /// Half the target rect's width and height, in physical pixels.
    pub half_extents: [f32; 2],
    /// The clamped physical corner radius, in physical pixels.
    pub corner_radius_px: f32,
}

impl ResolvedMask {
    /// Resolve the mask for a target rect: half-extents from its size, and the corner radius
    /// scaled to physical pixels then clamped to `min(half_width, half_height)`. The
    /// half-extents derive from `u32` sizes, so they are non-negative and the clamp's
    /// `min <= max` precondition always holds.
    pub fn from_target(target: &Region, corner_radius: CornerRadius) -> Self {
        let half_extents = [target.size[0] as f32 / 2.0, target.size[1] as f32 / 2.0];
        let max_radius = half_extents[0].min(half_extents[1]);
        let corner_radius_px =
            (corner_radius.points() * target.scale.factor()).clamp(0.0, max_radius);
        Self {
            half_extents,
            corner_radius_px,
        }
    }
}

/// The one backend-agnostic bundle that crosses the seam. `source_region` says where the
/// backdrop lives in the source texture; `target_rect` says where to composite the frosted
/// surface in the target. Both carry independent sizes and scales (DESIGN §4.1/§4.3).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BlurRequest {
    /// Where the backdrop to blur lives in the `source` texture (physical px + its scale).
    pub source_region: Region,
    /// Where to composite the frosted surface in the `target` (physical px + its scale).
    pub target_rect: Region,
    /// How much blur (logical points).
    pub blur_radius: BlurRadius,
    /// The glass film.
    pub tint: Tint,
    /// How round the surface corners are (logical points).
    pub corner_radius: CornerRadius,
    /// Surface-global fade coverage `[0, 1]` — how present the whole surface is (default `1.0`).
    pub presence: Presence,
}

impl BlurRequest {
    /// The physical-pixel blur radius, resolved against the **source** region's scale.
    ///
    /// The blur convolution happens in source-texture pixel space, so this is the only correct
    /// scale; pinning it here (rather than exposing a bare `blur_radius.to_physical_radius(scale)`)
    /// means a backend cannot accidentally resolve against `target_rect.scale` on a
    /// mismatched-DPI surface. Mirrors how [`ResolvedMask::from_target`] pins the target scale
    /// for the corner radius.
    pub fn physical_blur_radius(&self) -> f32 {
        self.blur_radius
            .to_physical_radius(self.source_region.scale)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::material::LinearRgba;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    fn region(origin: [u32; 2], size: [u32; 2], scale: f32) -> Region {
        Region {
            origin,
            size,
            scale: Scale::new(scale),
        }
    }

    #[test]
    fn scale_new_floors_nonpositive_to_positive() {
        assert!(Scale::new(0.0).factor() > 0.0);
        assert!(Scale::new(-2.0).factor() > 0.0);
        assert!(close(Scale::new(2.5).factor(), 2.5));
    }

    #[test]
    fn scale_default_is_one() {
        assert!(close(Scale::default().factor(), 1.0));
    }

    #[test]
    fn clip_to_leaves_an_in_bounds_region_unchanged() {
        let r = region([10, 10], [20, 20], 1.0);
        assert_eq!(r.clip_to([100, 100]), Some(r));
    }

    #[test]
    fn clip_to_clamps_a_partially_offscreen_region() {
        // origin in-bounds, extent runs 10px past each edge of a 100×100 source.
        let r = region([90, 90], [20, 20], 2.0);
        let clipped = r
            .clip_to([100, 100])
            .expect("partial overlap is not a no-op");
        assert_eq!(clipped.origin, [90, 90]);
        assert_eq!(clipped.size, [10, 10]);
        // The scale is preserved through the clip.
        assert!(close(clipped.scale.factor(), 2.0));
    }

    #[test]
    fn clip_to_is_none_when_origin_past_extent() {
        let r = region([100, 0], [10, 10], 1.0);
        assert_eq!(r.clip_to([100, 100]), None);
    }

    #[test]
    fn clip_to_is_none_for_zero_area() {
        let r = region([0, 0], [0, 10], 1.0);
        assert_eq!(r.clip_to([100, 100]), None);
    }

    #[test]
    fn clip_to_saturates_instead_of_overflowing() {
        // origin + size would overflow u32; saturating arithmetic clips to the extent.
        let r = region([u32::MAX - 1, 0], [10, 10], 1.0);
        assert_eq!(r.clip_to([100, 100]), None);
    }

    #[test]
    fn is_empty_true_for_zero_width() {
        assert!(region([5, 5], [0, 10], 1.0).is_empty());
        // Both dimensions zero is still empty.
        assert!(region([5, 5], [0, 0], 1.0).is_empty());
    }

    #[test]
    fn is_empty_true_for_zero_height() {
        assert!(region([5, 5], [10, 0], 1.0).is_empty());
    }

    #[test]
    fn is_empty_false_for_positive_area() {
        assert!(!region([5, 5], [1, 1], 1.0).is_empty());
        assert!(!region([0, 0], [100, 60], 2.0).is_empty());
    }

    #[test]
    fn region_display_reads_as_a_sentence_fragment() {
        let r = region([4, 8], [100, 60], 2.0);
        assert_eq!(r.to_string(), "origin (4, 8), size 100×60, scale 2");
    }

    #[test]
    fn resolved_mask_half_extents_are_half_the_size() {
        let mask =
            ResolvedMask::from_target(&region([0, 0], [80, 40], 1.0), CornerRadius::new(8.0));
        assert!(close(mask.half_extents[0], 40.0));
        assert!(close(mask.half_extents[1], 20.0));
        assert!(close(mask.corner_radius_px, 8.0));
    }

    #[test]
    fn resolved_mask_scales_corner_radius_to_physical() {
        // 8 logical points × 2.0 = 16 physical px, well under the 100px half-extent.
        let mask =
            ResolvedMask::from_target(&region([0, 0], [200, 200], 2.0), CornerRadius::new(8.0));
        assert!(close(mask.corner_radius_px, 16.0));
    }

    #[test]
    fn resolved_mask_clamps_radius_to_min_half_extent() {
        // A 999pt radius cannot exceed min(half) = min(20, 50) = 20.
        let mask =
            ResolvedMask::from_target(&region([0, 0], [40, 100], 1.0), CornerRadius::new(999.0));
        assert!(close(mask.corner_radius_px, 20.0));
    }

    #[test]
    fn physical_blur_radius_resolves_against_the_source_scale() {
        // Source DPI 2.0, target DPI 1.0 — the blur must use the SOURCE scale (2.0).
        let request = BlurRequest {
            source_region: region([0, 0], [100, 100], 2.0),
            target_rect: region([0, 0], [80, 60], 1.0),
            blur_radius: BlurRadius::new(8.0),
            tint: Tint::new(LinearRgba::new(0.1, 0.1, 0.12, 0.7)),
            corner_radius: CornerRadius::new(10.0),
            presence: Presence::default(),
        };
        assert!(close(request.physical_blur_radius(), 16.0));
    }

    #[test]
    fn blur_request_constructs_by_named_fields() {
        // Compile-time evidence that the request is assembled by name (swap-safe).
        let r = region([0, 0], [100, 60], 1.0);
        let request = BlurRequest {
            source_region: r,
            target_rect: r,
            blur_radius: BlurRadius::new(12.0),
            tint: Tint::new(LinearRgba::new(0.1, 0.1, 0.12, 0.7)),
            corner_radius: CornerRadius::new(10.0),
            presence: Presence::default(),
        };
        assert_eq!(request.blur_radius.points(), 12.0);
        assert!(close(request.tint.color().a(), 0.7));
    }
}
