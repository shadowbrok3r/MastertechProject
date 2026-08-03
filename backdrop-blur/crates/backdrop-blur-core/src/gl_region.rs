//! The GL-origin region type — a **structural** guard against this design's recurring bug
//! class, the y-flip (DESIGN §5). The flip between egui's top-left sampling space and GL's
//! bottom-left framebuffer space got two design reviews wrong and made the v1 seam doc place
//! the flip in the wrong place; the response, mandated post-review, is to make the orientation
//! a *type* rather than a convention a reader must track.
//!
//! [`GlRegion`] is a physical-pixel rectangle whose origin is **bottom-left** (the GL
//! framebuffer convention). It is *constructed, never flipped*: both egui call sites the glow
//! adapter uses ([`PaintCallbackInfo::viewport_in_pixels`] and `clip_rect_in_pixels`) already
//! expose a bottom-origin `from_bottom_px`, so [`from_bottom_px`](GlRegion::from_bottom_px)
//! takes GL coordinates directly and no `framebuffer_height − y` arithmetic ever appears. The
//! one bridge into the orientation-free [`Region`] the seam speaks is
//! [`into_region`](GlRegion::into_region) — a documented *reinterpret*, no math — which is the
//! single line a reviewer audits. **Any literal `height − y` in this module or its callers is a
//! review red flag.**
//!
//! Why a newtype rather than a phantom `Region<Space>`: lower blast radius. [`GrabPass`] is
//! glow-only, so only the glow-facing surfaces (the grab region, `GlPrepared`'s target rect,
//! the composite uniforms) take `GlRegion`; the frozen wgpu backend keeps the top-left
//! [`Region`] untouched, with no generic churn.
//!
//! [`Region`]: crate::Region
//! [`GrabPass`]: crate::GrabPass
//! [`PaintCallbackInfo::viewport_in_pixels`]: https://docs.rs/egui/latest/egui/struct.PaintCallbackInfo.html

use crate::geometry::{Region, Scale};

/// A physical-pixel rectangle in **GL bottom-left** coordinates (origin at the framebuffer's
/// bottom-left corner, `y` increasing upward). The glow backend's read coordinates, composite
/// `rect_origin`, SDF, and `backdrop_uv_remap` all operate in this one consistent system, so a
/// `copyTexSubImage2D` from the bottom-left framebuffer lines up with `rect_uv.y` increasing
/// upward and nothing is rendered upside-down (DESIGN §5).
///
/// Fields are private: the only way to obtain one is [`from_bottom_px`](Self::from_bottom_px)
/// from already-bottom-left inputs, so the "never flipped" invariant holds by construction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlRegion {
    origin_bl: [u32; 2],
    size: [u32; 2],
    scale: Scale,
}

impl GlRegion {
    // --- Constructors ---

    /// Build from a **bottom-left** physical-pixel origin and size. Named `_bl`/`from_bottom_px`
    /// because the input is required to already be GL-origin (egui's `from_bottom_px` field); the
    /// constructor performs no flip, which is the whole point of the type.
    pub fn from_bottom_px(origin_bl: [u32; 2], size: [u32; 2], scale: Scale) -> Self {
        Self {
            origin_bl,
            size,
            scale,
        }
    }

    // --- Combinators (origin-agnostic: an axis-aligned box ∩ box, valid in either origin) ---

    /// Intersect with another region in the **same** bottom-left space — the `clip_rect ∩
    /// viewport` step the adapter runs before grabbing. Saturating; returns `None` when the two
    /// are disjoint or touch only at an edge (zero area), the blur **no-op**. The result keeps
    /// `self`'s [`Scale`]; both inputs are expected to share it (same frame, same DPI).
    pub fn intersect(&self, other: &GlRegion) -> Option<GlRegion> {
        let [ax, ay] = self.origin_bl;
        let [aw, ah] = self.size;
        let [bx, by] = other.origin_bl;
        let [bw, bh] = other.size;

        let x0 = ax.max(bx);
        let y0 = ay.max(by);
        let x1 = ax.saturating_add(aw).min(bx.saturating_add(bw));
        let y1 = ay.saturating_add(ah).min(by.saturating_add(bh));

        let w = x1.saturating_sub(x0);
        let h = y1.saturating_sub(y0);

        if w == 0 || h == 0 {
            None
        } else {
            Some(GlRegion {
                origin_bl: [x0, y0],
                size: [w, h],
                scale: self.scale,
            })
        }
    }

    /// Clip to a framebuffer of size `extent` (the `framebuffer ∩` step). Equivalent to
    /// [`intersect`](Self::intersect) with the box `origin (0, 0)`, size `extent`; `None` when
    /// the region lies fully outside the framebuffer (the no-op).
    pub fn clip_to(&self, extent: [u32; 2]) -> Option<GlRegion> {
        self.intersect(&GlRegion::from_bottom_px([0, 0], extent, self.scale))
    }

    // --- Getters (bottom-left physical pixels) ---

    /// The bottom-left origin, physical pixels. Used by the grab to set `copyTexSubImage2D` /
    /// `blitFramebuffer` read coordinates directly — GL's read origin is bottom-left, so no flip.
    pub fn origin_bl(self) -> [u32; 2] {
        self.origin_bl
    }

    /// Width and height, physical pixels. The grab texture is sized to this.
    pub fn size(self) -> [u32; 2] {
        self.size
    }

    // --- The bridge (DESIGN §5: the one audited reinterpret) ---

    /// Reinterpret as the orientation-free [`Region`] the seam speaks. **Pure reinterpret — no
    /// arithmetic, no flip:** the bottom-left numbers pass straight through, and every *compute*
    /// consumer on the glow path (`grab_source`, the SDF, `backdrop_uv_remap`, the composite
    /// uniforms) treats the resulting `Region` as bottom-left consistently, so no coordinate is
    /// ever double-interpreted. This is the single line a review checks for a hidden `height − y`.
    ///
    /// The one consumer that must **not** receive an `into_region()`'d value is a human-facing
    /// error: [`Region`]'s `Display` is documented top-left, so a bottom-left number printed
    /// through it would mislead a debugger. That is why [`BlurError::GrabFailed`] carries a
    /// `GlRegion` directly (which prints with an explicit bottom-left marker), not a reinterpreted
    /// `Region`.
    ///
    /// [`BlurError::GrabFailed`]: crate::BlurError::GrabFailed
    pub fn into_region(self) -> Region {
        Region {
            origin: self.origin_bl,
            size: self.size,
            scale: self.scale,
        }
    }
}

impl std::fmt::Display for GlRegion {
    /// Prints with an explicit `bottom-left` marker so an error message can never be mistaken for
    /// the top-left [`Region`] convention.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let [x, y] = self.origin_bl;
        let [w, h] = self.size;
        let scale = self.scale.factor();
        write!(f, "origin-bl ({x}, {y}), size {w}×{h}, scale {scale}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    fn gl(origin_bl: [u32; 2], size: [u32; 2], scale: f32) -> GlRegion {
        GlRegion::from_bottom_px(origin_bl, size, Scale::new(scale))
    }

    #[test]
    fn into_region_is_a_pure_reinterpret() {
        // The numbers pass straight through — no height − y, no swap.
        let r = gl([10, 20], [100, 60], 2.0).into_region();
        assert_eq!(r.origin, [10, 20]);
        assert_eq!(r.size, [100, 60]);
        assert!(close(r.scale.factor(), 2.0));
    }

    #[test]
    fn intersect_overlapping_yields_the_overlap_box() {
        // [10,10]+[40,40] ∩ [30,20]+[40,40] = [30,20]+[20,30].
        let overlap = gl([10, 10], [40, 40], 1.0)
            .intersect(&gl([30, 20], [40, 40], 1.0))
            .expect("the boxes overlap");
        assert_eq!(overlap.origin_bl, [30, 20]);
        assert_eq!(overlap.size, [20, 30]);
    }

    #[test]
    fn intersect_preserves_self_scale() {
        let overlap = gl([0, 0], [50, 50], 2.0)
            .intersect(&gl([10, 10], [50, 50], 1.0))
            .expect("the boxes overlap");
        assert!(close(overlap.scale.factor(), 2.0));
    }

    #[test]
    fn intersect_disjoint_is_none() {
        assert_eq!(
            gl([0, 0], [10, 10], 1.0).intersect(&gl([20, 0], [10, 10], 1.0)),
            None
        );
    }

    #[test]
    fn intersect_edge_touching_is_none() {
        // Share the x=10 edge only → zero width.
        assert_eq!(
            gl([0, 0], [10, 10], 1.0).intersect(&gl([10, 0], [10, 10], 1.0)),
            None
        );
    }

    #[test]
    fn clip_to_leaves_an_in_bounds_region_unchanged() {
        let r = gl([10, 10], [20, 20], 1.0);
        assert_eq!(r.clip_to([100, 100]), Some(r));
    }

    #[test]
    fn clip_to_clamps_a_partially_offscreen_region() {
        // Runs 10px past the top/right of a 100×100 framebuffer.
        let clipped = gl([90, 90], [20, 20], 2.0)
            .clip_to([100, 100])
            .expect("partial overlap is not a no-op");
        assert_eq!(clipped.origin_bl, [90, 90]);
        assert_eq!(clipped.size, [10, 10]);
        assert!(close(clipped.scale.factor(), 2.0));
    }

    #[test]
    fn clip_to_is_none_when_origin_past_extent() {
        assert_eq!(gl([100, 0], [10, 10], 1.0).clip_to([100, 100]), None);
    }

    #[test]
    fn clip_to_is_none_for_zero_area() {
        assert_eq!(gl([0, 0], [0, 10], 1.0).clip_to([100, 100]), None);
    }

    #[test]
    fn intersect_saturates_instead_of_overflowing() {
        // origin + size would overflow u32; saturating arithmetic clips cleanly.
        let r = gl([u32::MAX - 1, 0], [10, 10], 1.0);
        assert_eq!(r.clip_to([100, 100]), None);
    }

    #[test]
    fn display_marks_the_origin_bottom_left() {
        // The "-bl" marker is what keeps an error message from being read as top-left Region.
        assert_eq!(
            gl([4, 8], [100, 60], 2.0).to_string(),
            "origin-bl (4, 8), size 100×60, scale 2"
        );
    }
}
