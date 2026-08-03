//! The GPU-uniform structs, packed to match the WGSL `struct` layouts byte-for-byte. These
//! live here (not in core) because the layout — including the explicit `_pad` that satisfies
//! WGSL's 16-byte rounding — is a wgpu/WGSL concern, and `#[derive(Pod, Zeroable)]` keeps the
//! crate `#![forbid(unsafe_code)]` (no `unsafe impl`).
//!
//! Every field is 4-byte-aligned, `vec2`/`vec4` members sit on their required boundaries, and
//! each struct is a multiple of 16 bytes. The `*_layout_matches_wgsl` tests pin this so a
//! silent offset drift cannot misread the mask or tint.

use bytemuck::{Pod, Zeroable};

/// Mirrors `GaussianParams` in `shaders/gaussian.wgsl` (48 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct GaussianParams {
    pub uv_offset: [f32; 2],
    pub uv_scale: [f32; 2],
    pub texel_size: [f32; 2],
    pub direction: [f32; 2],
    pub sigma: f32,
    pub radius: i32,
    pub decode_srgb: u32,
    pub _pad: u32,
}

impl GaussianParams {
    pub(crate) fn new(
        uv_offset: [f32; 2],
        uv_scale: [f32; 2],
        texel_size: [f32; 2],
        direction: [f32; 2],
        sigma: f32,
        radius: i32,
        decode_srgb: bool,
    ) -> Self {
        Self {
            uv_offset,
            uv_scale,
            texel_size,
            direction,
            sigma,
            radius,
            decode_srgb: u32::from(decode_srgb),
            _pad: 0,
        }
    }
}

/// Mirrors `CompositeParams` in `shaders/composite.wgsl` (64 bytes).
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct CompositeParams {
    pub rect_origin_px: [f32; 2],
    pub rect_size_px: [f32; 2],
    pub tint: [f32; 4],
    pub backdrop_uv_offset: [f32; 2],
    pub backdrop_uv_scale: [f32; 2],
    pub corner_radius_px: f32,
    pub encode_srgb: u32,
    pub opacity: f32,
    pub _pad: f32,
}

impl CompositeParams {
    #[expect(
        clippy::too_many_arguments,
        reason = "flat UBO mirror; field-per-arg is the point"
    )]
    pub(crate) fn new(
        rect_origin_px: [f32; 2],
        rect_size_px: [f32; 2],
        tint: [f32; 4],
        backdrop_uv_offset: [f32; 2],
        backdrop_uv_scale: [f32; 2],
        corner_radius_px: f32,
        encode_srgb: bool,
        opacity: f32,
    ) -> Self {
        Self {
            rect_origin_px,
            rect_size_px,
            tint,
            backdrop_uv_offset,
            backdrop_uv_scale,
            corner_radius_px,
            encode_srgb: u32::from(encode_srgb),
            opacity,
            _pad: 0.0,
        }
    }
}

/// Mirrors `KawaseParams` in `shaders/{downsample,upsample}.wgsl` (16 bytes). The half-texel
/// sampling offset for one dual-Kawase pass.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(crate) struct KawaseParams {
    pub halfpixel: [f32; 2],
    pub _pad: [f32; 2],
}

impl KawaseParams {
    pub(crate) fn new(halfpixel: [f32; 2]) -> Self {
        Self {
            halfpixel,
            _pad: [0.0; 2],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{offset_of, size_of};

    #[test]
    fn gaussian_params_layout_matches_wgsl() {
        assert_eq!(size_of::<GaussianParams>(), 48);
        assert_eq!(offset_of!(GaussianParams, uv_offset), 0);
        assert_eq!(offset_of!(GaussianParams, uv_scale), 8);
        assert_eq!(offset_of!(GaussianParams, texel_size), 16);
        assert_eq!(offset_of!(GaussianParams, direction), 24);
        assert_eq!(offset_of!(GaussianParams, sigma), 32);
        assert_eq!(offset_of!(GaussianParams, radius), 36);
        assert_eq!(offset_of!(GaussianParams, decode_srgb), 40);
    }

    #[test]
    fn composite_params_layout_matches_wgsl() {
        assert_eq!(size_of::<CompositeParams>(), 64);
        assert_eq!(offset_of!(CompositeParams, rect_origin_px), 0);
        assert_eq!(offset_of!(CompositeParams, rect_size_px), 8);
        // `tint` is a vec4 — it MUST land on a 16-byte boundary or the GPU misreads it.
        assert_eq!(offset_of!(CompositeParams, tint), 16);
        assert_eq!(offset_of!(CompositeParams, backdrop_uv_offset), 32);
        assert_eq!(offset_of!(CompositeParams, backdrop_uv_scale), 40);
        assert_eq!(offset_of!(CompositeParams, corner_radius_px), 48);
        assert_eq!(offset_of!(CompositeParams, encode_srgb), 52);
        assert_eq!(offset_of!(CompositeParams, opacity), 56);
    }

    #[test]
    fn kawase_params_layout_matches_wgsl() {
        assert_eq!(size_of::<KawaseParams>(), 16);
        assert_eq!(offset_of!(KawaseParams, halfpixel), 0);
    }

    #[test]
    fn decode_srgb_flag_packs_from_bool() {
        let on = GaussianParams::new([0.0; 2], [1.0; 2], [0.0; 2], [1.0, 0.0], 2.0, 3, true);
        let off = GaussianParams::new([0.0; 2], [1.0; 2], [0.0; 2], [1.0, 0.0], 2.0, 3, false);
        assert_eq!(on.decode_srgb, 1);
        assert_eq!(off.decode_srgb, 0);
    }
}
