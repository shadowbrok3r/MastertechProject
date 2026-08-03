//! The success half of a fused frost call's report. The seam already distinguishes "composited"
//! from "clipped to nothing" structurally — [`prepare`] returns `Ok(None)` for a zero-area or
//! fully-offscreen source, or a zero-area `target_rect` (a valid no-op, see `seam.rs`) — but a
//! backend entry that fuses
//! grab → prepare → record into one call (glow's `frost_region`) collapses that `Option` and
//! needs to name the distinction for its own caller. [`FrostEffect`] is that name; without it
//! the fused entry returns a `Result<(), _>` that silently swallows the no-op case.

/// What a single frost call did, when it did not fail.
///
/// The failure half stays [`BlurError`](crate::BlurError); this enum only splits the two
/// success shapes the seam's `prepare` already encodes as `Option<Prepared>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrostEffect {
    /// The blur ran and the frosted surface was composited into the target.
    Composited,
    /// The request clipped to nothing at the backend (`prepare` returned `Ok(None)`): a
    /// zero-area or fully-offscreen source region — or the request's `target_rect` was
    /// zero-area ([`Region::is_empty`](crate::Region::is_empty)). A valid no-op, not an
    /// error — nothing was drawn.
    ClippedEmpty,
}
