//! The feature-neutral spine: [`Surface`], the description of a frosted surface that **both**
//! adapter paths share. The own-loop path resolves it to a top-left [`BlurRequest`]
//! (`Surface::request`, gated `own-loop`); the grab-pass path reads its material fields directly
//! and builds a bottom-left request from a `GlRegion` at the egui callback. Keeping the type here
//! — not in either feature-gated module — is what lets a grab-pass-only (kiosk) build name a
//! `Surface` without compiling the wgpu stack.

use backdrop_blur_core::{BlurRadius, CornerRadius, Presence, RepaintPolicy, Tint};

/// A frosted surface to composite this frame: an egui-space rectangle (logical points) plus the
/// glass parameters and a liveness policy. v1 treats the backdrop directly behind the rect as the
/// blur source (`source_region == target_rect`).
///
/// Fields are public so the grab-pass adapter can read the material (`blur_radius`, `tint`,
/// `corner_radius`) straight off the surface inside the paint callback while deriving geometry
/// from the GL-origin region — the two paths share the *what* (this type) and differ only in the
/// *where* (top-left request vs bottom-left `GlRegion`).
#[derive(Clone, Copy, Debug)]
pub struct Surface {
    /// Where the surface sits, in egui logical points.
    pub rect: egui::Rect,
    /// Blur radius in logical points.
    pub blur_radius: BlurRadius,
    /// The glass film.
    pub tint: Tint,
    /// How round the corners are.
    pub corner_radius: CornerRadius,
    /// Surface-global fade `[0, 1]` — how present the whole surface is (default `1.0`). Drive this
    /// per frame to dissolve the frost in/out (e.g. a modal scrim fading with its dialog).
    pub presence: Presence,
    /// How often the backdrop must be refreshed (drives `request_repaint`).
    pub repaint: RepaintPolicy,
}
