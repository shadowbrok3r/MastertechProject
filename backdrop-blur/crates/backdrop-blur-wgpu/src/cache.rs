//! Format-coupled policy for the wgpu backend: the scratch texture format and the
//! composite-target allowlist. These name `wgpu::TextureFormat`, so they cannot live in the
//! GPU-free core; they are the wgpu-specific remainder of what used to be this module.
//!
//! The algorithm-agnostic math — the ping-pong cache key, the Gaussian-kernel resolution, the
//! dual-Kawase level/half-pixel policy, the backdrop UV remap, and the [`TargetEncoding`]
//! vocabulary — is **shared with the glow backend** and now lives in
//! [`backdrop_blur_core::algorithm`] (DESIGN §15). This module re-exports it `pub(crate)` so the
//! wgpu render paths keep their short `cache::name` references unchanged.

use wgpu::TextureFormat;

// Re-exported from core so the existing `cache::name` references in `lib.rs` keep resolving.
// `pub(crate)` (not `pub`) preserves this crate's API surface: these were `pub(crate)` before
// the hoist, and a bare `pub use` would re-export them as public items of `backdrop-blur-wgpu`.
pub(crate) use backdrop_blur_core::{
    PingPongKey, RETENTION_FRAMES, TargetEncoding, backdrop_uv_remap, evict_decision,
    kawase_halfpixel, kawase_level_size, resolve_gaussian, resolve_kawase_levels, use_dual_kawase,
};

/// The internal scratch format — linear HDR, filterable, usable as both a render target and a
/// sampled texture. Fixed for every blur chain; only the composite touches the caller's format.
pub(crate) const SCRATCH_FORMAT: TextureFormat = TextureFormat::Rgba16Float;

/// The [`TargetEncoding`] for a composite `format`, or `None` if `format` is not a supported
/// composite target (→ `BlurError::UnsupportedTarget`). `*Srgb` targets and the float target are
/// [`Linear`](TargetEncoding::Linear) (hardware encodes, or no encode); `Unorm` gamma targets
/// (the egui case) are [`Srgb`](TargetEncoding::Srgb) and need the manual encode. The `None`
/// arm is the load-bearing "format not on the allowlist" signal consumed in `lib.rs`.
pub(crate) fn composite_encode_srgb(format: TextureFormat) -> Option<TargetEncoding> {
    match format {
        TextureFormat::Rgba8UnormSrgb | TextureFormat::Bgra8UnormSrgb => {
            Some(TargetEncoding::Linear)
        }
        TextureFormat::Rgba8Unorm | TextureFormat::Bgra8Unorm => Some(TargetEncoding::Srgb),
        TextureFormat::Rgba16Float => Some(TargetEncoding::Linear),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composite_encode_srgb_allowlist() {
        // Hardware-encoding sRGB targets → linear write, no manual encode.
        assert_eq!(
            composite_encode_srgb(TextureFormat::Rgba8UnormSrgb),
            Some(TargetEncoding::Linear)
        );
        assert_eq!(
            composite_encode_srgb(TextureFormat::Bgra8UnormSrgb),
            Some(TargetEncoding::Linear)
        );
        // Gamma Unorm targets (egui) need the manual encode.
        assert_eq!(
            composite_encode_srgb(TextureFormat::Rgba8Unorm),
            Some(TargetEncoding::Srgb)
        );
        assert_eq!(
            composite_encode_srgb(TextureFormat::Bgra8Unorm),
            Some(TargetEncoding::Srgb)
        );
        // Linear float target.
        assert_eq!(
            composite_encode_srgb(TextureFormat::Rgba16Float),
            Some(TargetEncoding::Linear)
        );
        // Unsupported.
        assert_eq!(composite_encode_srgb(TextureFormat::R8Unorm), None);
    }
}
