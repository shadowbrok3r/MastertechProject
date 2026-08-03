//! The seam — the traits each GPU backend implements.
//!
//! [`BackdropBlur`] is the universal seam: **two-phase** (`prepare` then `record`) because the
//! backends demand it. wgpu uploads uniforms/textures through its **Queue** (not its
//! `CommandEncoder`), so the upload phase needs the queue and the record phase needs only the
//! command sink; glow is immediate-mode (`prepare` uploads via the context, `record` draws).
//! Every backend implements this trait, and it is **total** — it contains no method a given
//! backend cannot perform.
//!
//! [`GrabPass`] is a **separate, additive** trait for the grab-pass family only. The own-loop
//! path (wgpu) hands the host's already-sampleable intermediate straight to `prepare` and never
//! grabs, so forcing a `grab_source` onto every backend would make wgpu stub an impossible
//! method (it cannot fabricate a source from nothing). Instead the grab-pass (glow) backend
//! *additionally* implements `GrabPass` to blit a sampleable source out of a live framebuffer.
//! This is the socket that keeps glow additive (DESIGN §4.4).
//!
//! The associated types are each backend's resource universe — distinct per backend, which is
//! exactly why these traits are **not object-safe** and backends are **separate crates** (the
//! `wgpu-types` → `wgpu-hal` model). Dispatch is static, monomorphized.
//!
//! # Gate verdict (IMPL §1d): the seam is kept — now proven by the real backend
//!
//! Before committing to this seam, the divergent backend was first sketched against it in a
//! compile-only `examples/glow-gate` crate (now retired). That gate has been **superseded by the
//! real implementation**: [`backdrop-blur-glow`] implements both `BackdropBlur` and `GrabPass`
//! with live, `unsafe` GL and a full Tier-1 readback suite, so the seam is proven by working code
//! rather than an `unimplemented!()` sketch. Each associated type binds to a real glow type
//! (`Device`/`CommandSink` → `glow::Context`, `SourceTexture` → the grab source, `Target` →
//! `Option<glow::Framebuffer>` — the live draw FBO, `None` = the default framebuffer); the one `()`
//! (`Queue`) is honest because glow uploads through its context rather than a queue, and
//! `TargetSpec` is the **framebuffer size** (the composite viewport): the composite needs the full
//! draw-target size for its full-framebuffer `glViewport`, and the encode bit is resolved at record
//! time from the captured target's colour-attachment encoding (consulting `GL_FRAMEBUFFER_SRGB` only
//! where it is a valid capability) rather than from a format token.
//! v1 therefore ships **with** these traits, not a concrete one-backend pair.
//!
//! [`backdrop-blur-glow`]: https://github.com/abdu-benayad/backdrop-blur

use crate::{BlurError, BlurRequest, GlRegion};

/// Implemented once per GPU backend (`WgpuBlur` in v1; `GlowBlur` later). The implementor holds
/// the backend's cached resources — per-`(size, levels)` ping-pong chains and the pipelines —
/// across frames, so repeated frosted surfaces do not rebuild them.
///
/// # Lifecycle contract (v1)
///
/// - **Serial `prepare` → `record` per surface.** v1 scope is a single frosted surface over a
///   once-rendered backdrop; because the ping-pong scratch is shared, two surfaces are not
///   prepared-then-both-recorded — each is prepared and recorded before the next. The
///   [`Prepared`](Self::Prepared) handle is **owned** (it borrows nothing from the blurrer) and
///   **consumed by `record`**, so recording the same handle twice is a compile error, not a
///   contract violation. The one hazard the types still permit — preparing a *second* surface
///   against the shared scratch before recording the first (its scratch is then clobbered) —
///   remains a documented contract, guarded by the backends' generation `debug_assert` (K1);
///   genuine multi-surface batching is deferred future work.
/// - **Single-threaded, frame-serial.** `prepare` takes `&mut self`, `record` takes `&self`.
///   The blurrer is **not** required to be `Send`/`Sync` in v1; a multi-threaded render loop
///   owns one per render thread.
pub trait BackdropBlur {
    /// The GPU device (`wgpu::Device`; `glow::Context`).
    type Device;
    /// The upload queue (`wgpu::Queue`; `()` for glow, which uploads via the context).
    type Queue;
    /// The handle `record` issues GPU work into (`wgpu::CommandEncoder`, deferred;
    /// `glow::Context`, immediate). "Sink" deliberately implies nothing about deferral: the wgpu
    /// backend records, the GL backend executes on the spot.
    type CommandSink;
    /// A sampleable backdrop (`wgpu::TextureView`; `glow::Texture`). Own-loop backends receive
    /// it from the host; grab-pass backends produce it via [`GrabPass::grab_source`].
    type SourceTexture;
    /// The composite destination (`wgpu::TextureView`; a glow framebuffer).
    type Target;
    /// The static facts about the composite target that `prepare` needs ahead of `record`
    /// (`wgpu::TextureFormat` — the composite pipeline is baked and cached per fragment-target
    /// format; `FramebufferSize` for glow — the composite's full-framebuffer viewport).
    type TargetSpec;
    /// An **owned**, opaque per-call handle carrying the resolved payload (kernel offsets,
    /// tint, [`ResolvedMask`](crate::ResolvedMask), target rect, and the resource keys) from
    /// `prepare` to `record`. Owned — it borrows nothing from the blurrer — and consumed by
    /// `record`, so a handle cannot be replayed.
    type Prepared;

    /// **Phase 1** — holds the device + queue. Allocates and keys the ping-pong chain, lazily
    /// builds and caches pipelines (the fixed-scratch down/up pipelines once; the composite
    /// pipeline per `target_spec`), and resolves the payload into an owned
    /// [`Prepared`](Self::Prepared).
    ///
    /// Returns `Ok(None)` when `request.source_region` clips to nothing against `source` — a
    /// zero-area or fully-offscreen region (see [`Region::clip_to`]) — or when
    /// `request.target_rect` is zero-area (see [`Region::is_empty`]; the composite divides by the
    /// target size, so an empty target must never reach it). Either is a **no-op**, valid input
    /// rather than an error; `record` is then simply not called. Returns `Err` only on a real
    /// GPU fault.
    ///
    /// [`Region::clip_to`]: crate::Region::clip_to
    /// [`Region::is_empty`]: crate::Region::is_empty
    fn prepare(
        &mut self,
        device: &Self::Device,
        queue: &Self::Queue,
        source: &Self::SourceTexture,
        target_spec: Self::TargetSpec,
        request: &BlurRequest,
    ) -> Result<Option<Self::Prepared>, BlurError>;

    /// **Phase 2** — holds only the sink + target. Records downsample → upsample → composite
    /// for a `prepared` produced earlier in the same frame. Consumes the handle: recording the
    /// same surface again requires a fresh `prepare`.
    ///
    /// `target` **must differ from** the `source` the matching `prepare` sampled: wgpu forbids
    /// sampling the texture a pass writes, so read-after-write within one surface is a contract
    /// violation, not a runtime fallback. The immediate-mode backend additionally restores any
    /// GL state it touched (bound FBO, viewport, blend func, texture units), leaving state as
    /// found.
    fn record(
        &self,
        sink: &mut Self::CommandSink,
        target: &Self::Target,
        prepared: Self::Prepared,
    ) -> Result<(), BlurError>;
}

/// The **grab-pass** socket, implemented *in addition to* [`BackdropBlur`] only by backends that
/// must extract a sampleable backdrop from a live framebuffer (glow; the deferred mainstream-egui
/// path). Own-loop backends (wgpu) do **not** implement this — they receive an already-sampleable
/// source — which is why it is a separate trait rather than a method every backend must stub.
pub trait GrabPass: BackdropBlur {
    /// The live framebuffer to grab from (a glow framebuffer). Distinct from
    /// [`BackdropBlur::Target`]; it is the *read* source, not the composite destination.
    type Framebuffer;

    /// Produce a sampleable [`SourceTexture`](BackdropBlur::SourceTexture) by blitting (and
    /// MSAA-resolving) the `region` out of the live `framebuffer` — backend-specific GL the host
    /// cannot do generically.
    ///
    /// `region` is a [`GlRegion`] — **already in GL bottom-left coordinates** — so `grab_source`
    /// performs **no** read-origin flip. This is a deliberate divergence from the v1 seam, where
    /// this doc placed the bottom-left↔top-left flip *inside* `grab_source`: the adapter now
    /// derives the region from egui's bottom-origin `from_bottom_px` and builds the whole
    /// [`BlurRequest`] bottom-left (DESIGN §5), so a flip here would be a *double* flip. The
    /// y-orientation is carried by the type, not by an internal arithmetic step (see [`GlRegion`]).
    fn grab_source(
        &mut self,
        device: &Self::Device,
        queue: &Self::Queue,
        framebuffer: &Self::Framebuffer,
        region: GlRegion,
    ) -> Result<Self::SourceTexture, BlurError>;
}
