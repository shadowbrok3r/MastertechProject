//! `backdrop-blur-core` — the backend-agnostic heart of [`backdrop-blur`].
//!
//! This crate owns the *vocabulary* of frosted glass and the *seam* every GPU backend
//! implements, and nothing else. It has **no GPU dependency**, is fully headless-testable,
//! and forbids `unsafe`. That makes it the one crate in the workspace that cannot break a
//! backend: the material/geometry types, the [`BlurError`] model, the liveness policy, and
//! the backdrop-blur seam trait all live here, while `wgpu`/`glow` resource types stay out.
//!
//! # The shape of a blur
//!
//! A caller describes a frosted surface with a [`BlurRequest`] — *where* the backdrop lives
//! and the surface goes ([`Region`]s in physical pixels), and *what kind of glass* it is
//! ([`BlurRadius`], [`Tint`], [`CornerRadius`]). Core resolves the algorithm-agnostic parts
//! (a physical blur radius via [`BlurRequest::physical_blur_radius`]; a clamped
//! [`ResolvedMask`]); the backend resolves the algorithm-specific parts (kernel offsets,
//! pipelines) and does the GPU work.
//!
//! See `docs/DESIGN.md` (§4 is the load-bearing type design) and `docs/IMPL.md` for the
//! rationale and build sequence.
//!
//! [`backdrop-blur`]: https://github.com/abdu-benayad/backdrop-blur
#![forbid(unsafe_code)]

mod algorithm;
mod error;
mod eviction;
mod geometry;
mod gl_region;
mod liveness;
mod material;
mod outcome;
mod seam;

pub use algorithm::{
    GaussianKernel, KAWASE_THRESHOLD_PX, MAX_GAUSSIAN_RADIUS, MAX_KAWASE_LEVELS, PingPongKey,
    TargetEncoding, backdrop_uv_remap, kawase_halfpixel, kawase_level_size, resolve_gaussian,
    resolve_kawase_levels, use_dual_kawase,
};
pub use error::{BackendError, BlurError, BlurStage};
pub use eviction::{RETENTION_FRAMES, evict_decision};
pub use geometry::{BlurRequest, Region, ResolvedMask, Scale};
pub use gl_region::GlRegion;
pub use liveness::RepaintPolicy;
pub use material::{BlurRadius, CornerRadius, LinearRgba, Presence, Tint};
pub use outcome::FrostEffect;
pub use seam::{BackdropBlur, GrabPass};
