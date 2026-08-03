//! `backdrop-blur-wgpu` — the wgpu backend for [`backdrop_blur_core`]'s frosted-glass seam.
//!
//! It implements [`BackdropBlur`] with a safe, WGSL pipeline: a **separable Gaussian** blur for
//! small radii and **dual-Kawase** (down/up-sample, the production-compositor algorithm) for large
//! radii, selected by a radius threshold, followed by a tinted, rounded-rect-masked composite. The
//! crate is `#![forbid(unsafe_code)]`; the only place that *could* want `unsafe` — the GPU-uniform
//! `Pod` impls — uses bytemuck derives.
//!
//! # What the host provides
//!
//! The own-loop host renders its UI into an offscreen intermediate and hands it to [`prepare`]
//! as a [`SourceView`] — the texture view **plus its size and color space**, because a
//! `wgpu::TextureView` exposes neither and the backend needs both (the size to clip/scale, the
//! color space to know whether to sRGB-decode on sample — egui renders gamma-encoded, egui#3168).
//! The host owns the final [`Target`](BackdropBlur::Target); the backend owns only the internal
//! ping-pong scratch.
//!
//! # On error handling
//!
//! wgpu resource creation (textures, buffers, pipelines, bind groups) does **not** return `Result`;
//! a fault is reported out-of-band through the device's error handler. This backend wraps every
//! creation it performs in a per-call [`OutOfMemory`](wgpu::ErrorFilter::OutOfMemory) error scope
//! (the [`OomScope`] seam) and maps a captured allocation failure to
//! [`BlurError::DeviceOutOfMemory`] (non-fatal kinds: the sampler, and the primary allocations of
//! buffers/textures) or [`BlurError::DeviceLost`] (native fatal kinds: layouts, shader modules,
//! pipelines, bind groups — wgpu-core `device.lose()`s on their rejected allocation, so the device
//! is already gone when the error is returned). What differs by dispatch is *when the fault comes
//! back*:
//!
//! - **Native:** the fault is recorded synchronously during the create call, so the scope is read
//!   without blocking, and the result is checked (`?`) before the handle is consumed — an
//!   out-of-memory handle never cascades into an uncatchable `Validation` error.
//! - **Web (the browser's WebGPU dispatch):** the scope's `pop()` resolves as a deferred promise.
//!   Construction awaits every scope (the async `WgpuBlur::new` and `prewarm_composite`), so
//!   construction faults are still returned in-band, checked before use. On the frame path each
//!   creation pushes its scope, creates, and *calls* pop synchronously — only the await is
//!   deferred to a spawned task — so a frame never blocks and never panics. A captured fault is
//!   parked generation-stamped in the backend's fault log; the next frame's drain evicts the
//!   poisoned cache entry (so it is rebuilt and re-verified) and folds the fault into the report
//!   the host reads via `WgpuBlur::take_fault` — an every-frame host obligation. The web path
//!   never produces `DeviceLost`; the host's device-lost callback remains the loss signal.
//!
//! Genuine validation/internal faults are crate bugs (this crate builds its own descriptors) and are
//! deliberately left to wgpu's default panic handler. The other error returned synchronously is
//! [`BlurError::UnsupportedTarget`], checked against an allowlist before any GPU call.
//!
//! [`prepare`]: BackdropBlur::prepare
//! [`backdrop_blur_core`]: backdrop_blur_core
#![forbid(unsafe_code)]

use std::collections::HashMap;

use backdrop_blur_core::{BackdropBlur, BlurError, BlurRequest, BlurStage, ResolvedMask};

mod cache;
mod fault;
mod uniforms;

pub use fault::{FaultReport, FaultSlot};

use fault::SlotKey;

use cache::{
    PingPongKey, RETENTION_FRAMES, SCRATCH_FORMAT, TargetEncoding, backdrop_uv_remap,
    composite_encode_srgb, evict_decision, kawase_halfpixel, kawase_level_size, resolve_gaussian,
    resolve_kawase_levels, use_dual_kawase,
};
use uniforms::{CompositeParams, GaussianParams, KawaseParams};

/// The color space of the backdrop the host hands in. egui renders **gamma-encoded** regardless
/// of texture format (egui#3168), so its intermediate is [`GammaSrgb`](Self::GammaSrgb) and must
/// be decoded before the linear-light convolution. A host that renders linear uses [`Linear`].
///
/// [`Linear`]: Self::Linear
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceColorSpace {
    /// sRGB gamma-encoded values (egui). Decoded to linear on sample.
    GammaSrgb,
    /// Already linear-light values. Sampled as-is.
    Linear,
}

/// The backdrop source: a sampleable view plus the two things a `wgpu::TextureView` cannot tell
/// the backend on its own — the texture's pixel size and its color space. The host constructs
/// one per frame from its offscreen intermediate; it owns the view for the call's duration.
pub struct SourceView {
    /// A sampleable view of the host's intermediate texture.
    pub view: wgpu::TextureView,
    /// The intermediate's `[width, height]` in physical pixels.
    pub size: [u32; 2],
    /// Whether the intermediate holds gamma-encoded or linear values.
    pub color_space: SourceColorSpace,
}

/// The owned, per-call handle from `prepare` to `record`: the resolved blur (which algorithm +
/// its bind groups), the composite bind group, and `generation` (so `record` can debug-assert the
/// serial prepare→record contract — a stale handle would alias clobbered scratch, K1). The bind
/// groups already hold their textures/uniforms via wgpu's internal refcounting.
pub struct WgpuPrepared {
    target_format: wgpu::TextureFormat,
    generation: u64,
    blur: PreparedBlur,
    composite_bind: wgpu::BindGroup,
}

/// The resolved blur for one surface: either the separable Gaussian (small radius) or dual-Kawase
/// (large radius). Each variant carries the bind groups its `record` passes replay; the keyed
/// scratch they target lives in [`WgpuBlur`].
enum PreparedBlur {
    /// Horizontal then vertical Gaussian into the 2-texture ping-pong; composite samples B.
    Gaussian {
        key: PingPongKey,
        horizontal_bind: wgpu::BindGroup,
        vertical_bind: wgpu::BindGroup,
    },
    /// Prefilter (decode+remap into mip 0) → `N` downsamples → `N` upsamples back to mip 0;
    /// composite samples mip 0.
    DualKawase {
        key: PingPongKey,
        prefilter_bind: wgpu::BindGroup,
        down_binds: Vec<wgpu::BindGroup>,
        up_binds: Vec<wgpu::BindGroup>,
    },
}

/// The two same-size `Rgba16Float` ping-pong views for the Gaussian path: the horizontal pass
/// writes A (`views[0]`), the vertical pass writes B (`views[1]`), the composite samples B. Only
/// the views are stored — a `wgpu::TextureView` keeps its parent texture alive by refcount.
struct ScratchChain {
    views: [wgpu::TextureView; 2],
    /// The frame this chain was last touched by `ensure_scratch`; drives last-frame-used eviction.
    last_used_frame: u64,
    /// The backend generation active when this chain was created; the wasm32 fault drain compares
    /// it against a deferred fault's stamp to tell a live fault from a stale one.
    #[cfg_attr(
        not(target_arch = "wasm32"),
        expect(dead_code, reason = "read only by the wasm32 fault drain")
    )]
    created_generation: u64,
}

/// A dual-Kawase mip pyramid plus the frame it was last used: `N + 1` decreasing-size views (level 0
/// = full clipped size). The `last_used_frame` drives last-frame-used eviction, exactly as
/// [`ScratchChain`] does for the Gaussian path.
struct PyramidChain {
    views: Vec<wgpu::TextureView>,
    /// The frame this chain was last touched by `ensure_pyramid`; drives last-frame-used eviction.
    last_used_frame: u64,
    /// The backend generation active when this chain was created; the wasm32 fault drain compares
    /// it against a deferred fault's stamp to tell a live fault from a stale one.
    #[cfg_attr(
        not(target_arch = "wasm32"),
        expect(dead_code, reason = "read only by the wasm32 fault drain")
    )]
    created_generation: u64,
}

/// A cached per-target-format composite pipeline plus the generation it was created in (the
/// wasm32 fault drain's staleness stamp, mirroring the chains' `created_generation`).
struct CompositeEntry {
    pipeline: wgpu::RenderPipeline,
    /// The backend generation active when this pipeline was built.
    #[cfg_attr(
        not(target_arch = "wasm32"),
        expect(dead_code, reason = "read only by the wasm32 fault drain")
    )]
    created_generation: u64,
}

/// The wgpu implementation of [`BackdropBlur`]. Holds the fixed pipeline machinery (bind-group
/// layout, sampler, Gaussian/downsample/upsample pipelines) and the per-`(size)` scratch
/// (Gaussian ping-pong + dual-Kawase pyramid) + per-target-format composite caches, so repeated
/// frosted surfaces reuse them.
pub struct WgpuBlur {
    pipeline_layout: wgpu::PipelineLayout,
    bind_group_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    gaussian_pipeline: wgpu::RenderPipeline,
    downsample_pipeline: wgpu::RenderPipeline,
    upsample_pipeline: wgpu::RenderPipeline,
    composite_shader: wgpu::ShaderModule,
    composite_pipelines: HashMap<wgpu::TextureFormat, CompositeEntry>,
    scratch: HashMap<PingPongKey, ScratchChain>,
    /// Dual-Kawase mip pyramids: `N + 1` decreasing-size views, level 0 = full clipped size.
    pyramids: HashMap<PingPongKey, PyramidChain>,
    /// Advanced once per `prepare` ([`Self::begin_frame`]); the "now" last-frame-used eviction
    /// compares each chain's `last_used_frame` against, so a resized/moved surface's old-size chains
    /// are dropped instead of accumulating one per distinct size forever.
    frame: u64,
    /// Bumped once per frosted `prepare` (in [`Self::begin_frame`], so the generation active
    /// *during* a prepare's creations is the stamped one); stamped into [`WgpuPrepared`] so
    /// `record` can detect a stale handle, and into each cache entry so the wasm32 fault drain
    /// can tell a live fault from a stale one.
    generation: u64,
    /// The shared deferred-fault collector for the web dispatch: each frame-path creation's
    /// spawned pop-awaiting task records its outcome here; the frame path drains it and folds
    /// reportable faults into the host report.
    #[cfg(target_arch = "wasm32")]
    faults: fault::SharedFaultLog,
}

// --- Constructors ---

impl WgpuBlur {
    /// Build the fixed pipeline machinery. The Gaussian pipeline (always writing the internal
    /// scratch format) is built now; composite pipelines are built lazily per target format.
    ///
    /// Every creation is wrapped in a per-call [`OutOfMemory`](wgpu::ErrorFilter::OutOfMemory) error
    /// scope, so a device out-of-memory at construction returns an error instead of panicking
    /// (this is the native constructor; the web twin awaits the same scopes — see the module-level
    /// error-handling note): [`BlurError::DeviceLost`] for
    /// the layout/shader/pipeline creations (fatal arms — wgpu has already invalidated the device),
    /// or [`BlurError::DeviceOutOfMemory`] for the sampler (the device survives). A genuine
    /// validation fault (a malformed shader or layout descriptor) is a crate bug and still panics via
    /// wgpu's default handler.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(device: &wgpu::Device) -> Result<Self, BlurError> {
        // wgpu-core 29.0.3: create_bind_group_layout routes solely through fatal handle_hal_error -> device.lose()
        let bind_group_layout = scoped_oom(device, OomOutcome::DeviceLost, || {
            make_bind_group_layout(device)
        })?;

        // wgpu-core 29.0.3: create_pipeline_layout routes solely through fatal handle_hal_error -> device.lose()
        let pipeline_layout = scoped_oom(device, OomOutcome::DeviceLost, || {
            make_pipeline_layout(device, &bind_group_layout)
        })?;

        // wgpu-core 29.0.3: create_sampler non-fatal handler (resource.rs:2244)
        let sampler = scoped_oom(device, OomOutcome::Recoverable, || make_sampler(device))?;

        // wgpu-core 29.0.3: create_shader_module routes solely through fatal handle_hal_error -> device.lose()
        let gaussian_shader = scoped_oom(device, OomOutcome::DeviceLost, || {
            make_gaussian_shader(device)
        })?;
        let downsample_shader = scoped_oom(device, OomOutcome::DeviceLost, || {
            make_downsample_shader(device)
        })?;
        let upsample_shader = scoped_oom(device, OomOutcome::DeviceLost, || {
            make_upsample_shader(device)
        })?;
        let composite_shader = scoped_oom(device, OomOutcome::DeviceLost, || {
            make_composite_shader(device)
        })?;

        // All blur passes write the internal scratch format with no blend; only the composite
        // matches the caller's format and blends.
        // wgpu-core 29.0.3: create_render_pipeline routes solely through fatal handle_hal_error -> device.lose()
        let gaussian_pipeline = scoped_oom(device, OomOutcome::DeviceLost, || {
            make_pipeline(
                device,
                &pipeline_layout,
                &gaussian_shader,
                SCRATCH_FORMAT,
                None,
            )
        })?;
        let downsample_pipeline = scoped_oom(device, OomOutcome::DeviceLost, || {
            make_pipeline(
                device,
                &pipeline_layout,
                &downsample_shader,
                SCRATCH_FORMAT,
                None,
            )
        })?;
        let upsample_pipeline = scoped_oom(device, OomOutcome::DeviceLost, || {
            make_pipeline(
                device,
                &pipeline_layout,
                &upsample_shader,
                SCRATCH_FORMAT,
                None,
            )
        })?;

        Ok(Self {
            pipeline_layout,
            bind_group_layout,
            sampler,
            gaussian_pipeline,
            downsample_pipeline,
            upsample_pipeline,
            composite_shader,
            composite_pipelines: HashMap::new(),
            scratch: HashMap::new(),
            pyramids: HashMap::new(),
            frame: 0,
            generation: 0,
        })
    }

    /// The web (WebGPU-dispatch) twin of the native constructor: the same creations in the same
    /// order, each awaited through its own `OutOfMemory` error scope — so every fault is known
    /// before the next creation consumes anything (check-before-consume), and a construction
    /// out-of-memory is this call's `Err` instead of a deferred panic. On this dispatch a
    /// creation out-of-memory does not kill the device (measured), so every fault maps to
    /// [`BlurError::DeviceOutOfMemory`]; [`BlurError::DeviceLost`] is never produced by the web
    /// path — the host's own device-lost callback remains the loss signal. A genuine validation
    /// fault (a malformed shader or layout descriptor) is a crate bug and still panics via
    /// wgpu's default handler.
    #[cfg(target_arch = "wasm32")]
    pub async fn new(device: &wgpu::Device) -> Result<Self, BlurError> {
        let bind_group_layout =
            scoped_oom_awaited(device, || make_bind_group_layout(device)).await?;
        let pipeline_layout =
            scoped_oom_awaited(device, || make_pipeline_layout(device, &bind_group_layout)).await?;
        let sampler = scoped_oom_awaited(device, || make_sampler(device)).await?;
        let gaussian_shader = scoped_oom_awaited(device, || make_gaussian_shader(device)).await?;
        let downsample_shader =
            scoped_oom_awaited(device, || make_downsample_shader(device)).await?;
        let upsample_shader = scoped_oom_awaited(device, || make_upsample_shader(device)).await?;
        let composite_shader = scoped_oom_awaited(device, || make_composite_shader(device)).await?;

        // All blur passes write the internal scratch format with no blend; only the composite
        // matches the caller's format and blends.
        let gaussian_pipeline = scoped_oom_awaited(device, || {
            make_pipeline(
                device,
                &pipeline_layout,
                &gaussian_shader,
                SCRATCH_FORMAT,
                None,
            )
        })
        .await?;
        let downsample_pipeline = scoped_oom_awaited(device, || {
            make_pipeline(
                device,
                &pipeline_layout,
                &downsample_shader,
                SCRATCH_FORMAT,
                None,
            )
        })
        .await?;
        let upsample_pipeline = scoped_oom_awaited(device, || {
            make_pipeline(
                device,
                &pipeline_layout,
                &upsample_shader,
                SCRATCH_FORMAT,
                None,
            )
        })
        .await?;

        Ok(Self {
            pipeline_layout,
            bind_group_layout,
            sampler,
            gaussian_pipeline,
            downsample_pipeline,
            upsample_pipeline,
            composite_shader,
            composite_pipelines: HashMap::new(),
            scratch: HashMap::new(),
            pyramids: HashMap::new(),
            frame: 0,
            generation: 0,
            faults: fault::SharedFaultLog::default(),
        })
    }

    /// Build and cache the composite pipeline for `format` now, awaited and checked — the
    /// construction-time home of the one heavyweight frame-path build. Call it once per target
    /// format before the first frame (the egui adapter's web constructor does), so the frame
    /// path never lazily builds a pipeline whose deferred fault could only be reported after the
    /// frame that consumed it. The guarantee is **per-format**: a direct seam user driving other
    /// target formats goes through the frame path's deferred lazy build instead.
    #[cfg(target_arch = "wasm32")]
    pub async fn prewarm_composite(
        &mut self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
    ) -> Result<(), BlurError> {
        if self.composite_pipelines.contains_key(&format) {
            return Ok(());
        }
        let pipeline = scoped_oom_awaited(device, || {
            make_pipeline(
                device,
                &self.pipeline_layout,
                &self.composite_shader,
                format,
                Some(over_blend()),
            )
        })
        .await?;
        self.composite_pipelines.insert(
            format,
            CompositeEntry {
                pipeline,
                created_generation: self.generation,
            },
        );
        Ok(())
    }
}

// --- Internal resource management ---

impl WgpuBlur {
    /// Advance to the next frame — bumping the generation, so every creation this prepare makes
    /// is stamped with the generation its `WgpuPrepared` carries — and evict every scratch/pyramid
    /// chain untouched for
    /// [`RETENTION_FRAMES`]. Called once at the top of [`prepare`](BackdropBlur::prepare), before any
    /// `ensure_*`, so the chain a surface is about to use this frame is never evicted. Dropping a
    /// `HashMap` entry drops its `wgpu::TextureView`s, releasing the underlying textures by refcount
    /// — no explicit GPU free. The stale-set decision is the pure, core-shared [`evict_decision`].
    fn begin_frame(&mut self) {
        // Web: invalidate poisoned cache entries BEFORE the generation bump and the retention
        // eviction, so nothing this frame's `ensure_*` serves can be a faulted resource.
        #[cfg(target_arch = "wasm32")]
        self.absorb_faults();
        self.frame = self.frame.wrapping_add(1);
        self.generation += 1;
        let stale = evict_decision(
            self.scratch.iter().map(|(k, c)| (*k, c.last_used_frame)),
            self.frame,
            RETENTION_FRAMES,
        );
        for key in stale {
            self.scratch.remove(&key);
        }
        let stale = evict_decision(
            self.pyramids.iter().map(|(k, c)| (*k, c.last_used_frame)),
            self.frame,
            RETENTION_FRAMES,
        );
        for key in stale {
            self.pyramids.remove(&key);
        }
    }

    /// Create the two Gaussian ping-pong textures for `key` if not already cached, and mark the chain
    /// used this frame so eviction keeps it.
    fn ensure_scratch(&mut self, scope: &OomScope<'_>, key: PingPongKey) -> Result<(), BlurError> {
        if let Some(chain) = self.scratch.get_mut(&key) {
            chain.last_used_frame = self.frame;
            return Ok(());
        }
        let view_a = scratch_view(
            scope,
            key.size,
            SlotKey::Scratch(key),
            "backdrop-blur scratch A",
        )?;
        let view_b = scratch_view(
            scope,
            key.size,
            SlotKey::Scratch(key),
            "backdrop-blur scratch B",
        )?;
        self.scratch.insert(
            key,
            ScratchChain {
                views: [view_a, view_b],
                last_used_frame: self.frame,
                created_generation: self.generation,
            },
        );
        Ok(())
    }

    /// Create the dual-Kawase mip pyramid for `key` (`key.levels` = `N` → `N + 1` views, level 0
    /// at the full clipped size, level `i` halved) if not already cached.
    fn ensure_pyramid(&mut self, scope: &OomScope<'_>, key: PingPongKey) -> Result<(), BlurError> {
        if let Some(chain) = self.pyramids.get_mut(&key) {
            chain.last_used_frame = self.frame;
            return Ok(());
        }
        let views = (0..=key.levels)
            .map(|level| {
                let size = kawase_level_size(key.size, level);
                scratch_view(
                    scope,
                    size,
                    SlotKey::Pyramid(key),
                    "backdrop-blur kawase mip",
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.pyramids.insert(
            key,
            PyramidChain {
                views,
                last_used_frame: self.frame,
                created_generation: self.generation,
            },
        );
        Ok(())
    }

    /// Build and cache the composite pipeline for `format` if not already present.
    fn ensure_composite_pipeline(
        &mut self,
        scope: &OomScope<'_>,
        format: wgpu::TextureFormat,
    ) -> Result<(), BlurError> {
        if self.composite_pipelines.contains_key(&format) {
            return Ok(());
        }
        // wgpu-core 29.0.3: create_render_pipeline routes solely through fatal handle_hal_error -> device.lose()
        let pipeline = scope.scoped(OomOutcome::DeviceLost, SlotKey::Composite(format), || {
            make_pipeline(
                scope.device,
                &self.pipeline_layout,
                &self.composite_shader,
                format,
                Some(over_blend()),
            )
        })?;
        self.composite_pipelines.insert(
            format,
            CompositeEntry {
                pipeline,
                created_generation: self.generation,
            },
        );
        Ok(())
    }

    /// One bind group: a sampled texture view + the shared sampler + a uniform buffer.
    fn bind(
        &self,
        scope: &OomScope<'_>,
        view: &wgpu::TextureView,
        uniform: &wgpu::Buffer,
        label: &str,
    ) -> Result<wgpu::BindGroup, BlurError> {
        // wgpu-core 29.0.3: create_bind_group routes solely through fatal handle_hal_error -> device.lose()
        scope.scoped(OomOutcome::DeviceLost, SlotKey::BindGroup, || {
            scope.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &self.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: uniform.as_entire_binding(),
                    },
                ],
            })
        })
    }
}

// --- The web fault state (deferred reports) ---

#[cfg(target_arch = "wasm32")]
impl WgpuBlur {
    /// Drain every parked backend fault (all but the adapter's `Intermediate` records, which
    /// [`Self::drain_intermediate_faults`] owns), execute each verdict — evict the named cache
    /// entry on a stamp match, fold reportable faults into the host report, drop stale ones —
    /// and leave the log empty of backend records. Idempotent, and called from **two sites**:
    /// `begin_frame`, so a poisoned entry is invalidated before this frame's `ensure_*` can
    /// serve it; and [`Self::take_fault`], so fault delivery stays live even when the host sheds
    /// all frosting and `prepare` never runs. The staleness rule compares stamps, not the
    /// current generation, so the call order is immaterial.
    fn absorb_faults(&mut self) {
        let drained = self
            .faults
            .borrow_mut()
            .drain_where(|fault| fault.slot != SlotKey::Intermediate);
        for fault in drained {
            let entry_stamp = match fault.slot {
                SlotKey::Scratch(key) => self.scratch.get(&key).map(|c| c.created_generation),
                SlotKey::Pyramid(key) => self.pyramids.get(&key).map(|c| c.created_generation),
                SlotKey::Composite(format) => self
                    .composite_pipelines
                    .get(&format)
                    .map(|e| e.created_generation),
                SlotKey::Uniform | SlotKey::BindGroup | SlotKey::Intermediate => None,
            };
            match fault::DrainAction::decide(fault.slot, fault.generation, entry_stamp) {
                fault::DrainAction::EvictAndReport => {
                    self.evict(fault.slot);
                    self.faults
                        .borrow_mut()
                        .fold_report(fault.slot.kind(), fault.message);
                }
                fault::DrainAction::ReportOnly => {
                    self.faults
                        .borrow_mut()
                        .fold_report(fault.slot.kind(), fault.message);
                }
                fault::DrainAction::StaleDrop => {}
            }
        }
    }

    /// Remove the cache entry a keyed slot names; a transient slot has nothing cached (total —
    /// no panic path).
    fn evict(&mut self, slot: SlotKey) {
        match slot {
            SlotKey::Scratch(key) => {
                self.scratch.remove(&key);
            }
            SlotKey::Pyramid(key) => {
                self.pyramids.remove(&key);
            }
            SlotKey::Composite(format) => {
                self.composite_pipelines.remove(&format);
            }
            SlotKey::Uniform | SlotKey::BindGroup | SlotKey::Intermediate => {}
        }
    }

    /// The host's once-per-frame read of the deferred fault state: drain and absorb everything
    /// pending, then hand over (and clear) the folded [`FaultReport`].
    ///
    /// **Host contract — call this every frame, including frames where frosting is shed or
    /// skipped.** This call is what keeps fault delivery live: `prepare` also absorbs, but a
    /// host that responds to a report by shedding surfaces stops calling `prepare`, and without
    /// this read the state could never progress back to "clean". On `Some`: do not trust the
    /// presented frost — re-request a repaint, and retry unfrosted or shed surfaces. Delivery is
    /// eventual: a fault normally surfaces on the next frame's read, later under pressure.
    ///
    /// Sharp edge: an error-scope read still in flight when the device dies resolves as "no
    /// fault", so `None` is **not** a device-liveness signal — the host's own
    /// `set_device_lost_callback` is. (Similarly, if the host abandons the render loop entirely,
    /// pending records simply sit undrained — acceptable, since nothing is being presented.)
    pub fn take_fault(&mut self) -> Option<FaultReport> {
        self.absorb_faults();
        self.faults.borrow_mut().take_report()
    }

    /// Drain any parked faults for the adapter's offscreen intermediate texture, fold them into
    /// the host report, and return whether any were drained — `true` means the adapter must drop
    /// its cached intermediate so the faulted texture is never served again (it is recreated the
    /// same frame). The intermediate is size-keyed with no generation stamp, so a late stale
    /// report can at worst drop one healthy intermediate for one recreation — correctness is
    /// preserved, one texture is rebuilt unnecessarily.
    pub fn drain_intermediate_faults(&mut self) -> bool {
        let drained = self
            .faults
            .borrow_mut()
            .drain_where(|fault| fault.slot == SlotKey::Intermediate);
        let any_drained = !drained.is_empty();
        for fault in drained {
            self.faults
                .borrow_mut()
                .fold_report(FaultSlot::Intermediate, fault.message);
        }
        any_drained
    }
}

// --- Test-support (gated) ---

#[cfg(feature = "image-snapshots")]
impl WgpuBlur {
    /// The number of cached scratch + pyramid chains. Exposed only under the `image-snapshots` test
    /// feature so the gated GPU tier can assert eviction actually bounds the cache as a surface is
    /// dragged/resized (the leak guard). Not part of the public API.
    pub fn cached_chain_count(&self) -> usize {
        self.scratch.len() + self.pyramids.len()
    }
}

// --- The seam ---

impl BackdropBlur for WgpuBlur {
    type Device = wgpu::Device;
    type Queue = wgpu::Queue;
    type CommandSink = wgpu::CommandEncoder;
    type SourceTexture = SourceView;
    type Target = wgpu::TextureView;
    type TargetSpec = wgpu::TextureFormat;
    type Prepared = WgpuPrepared;

    fn prepare(
        &mut self,
        device: &Self::Device,
        queue: &Self::Queue,
        source: &Self::SourceTexture,
        target_spec: Self::TargetSpec,
        request: &BlurRequest,
    ) -> Result<Option<Self::Prepared>, BlurError> {
        let Some(clipped) = request.source_region.clip_to(source.size) else {
            return Ok(None); // zero-area or fully-offscreen region → no-op
        };
        // The target-side half of the same no-op: the composite shader divides by the target size
        // (composite.wgsl:57 — `rect_uv = (px - rect_origin_px) / rect_size_px`), so a zero dimension
        // would yield NaN/Inf UVs blended at ~50% alpha along a visible line. An empty target is the
        // same valid no-op as an empty source, not an error (DESIGN §9).
        if request.target_rect.is_empty() {
            return Ok(None);
        }

        // Advance the eviction clock and drop scratch chains untouched for RETENTION_FRAMES, before
        // ensuring this frame's chain — so a resized/moved surface's old-size chains are freed rather
        // than accumulating. Placed *after* the no-op guards, mirroring the glow backend (blur.rs:99):
        // the counter counts frosted frames, so a surface that clips to nothing does not age out a
        // chain it is about to reuse when it returns on-screen.
        self.begin_frame();
        let scope = self.creation_scope(device);

        let encode_srgb = matches!(
            composite_encode_srgb(target_spec).ok_or_else(|| BlurError::UnsupportedTarget {
                format: format!("{target_spec:?}"),
            })?,
            TargetEncoding::Srgb
        );
        let decode_srgb = matches!(source.color_space, SourceColorSpace::GammaSrgb);
        self.ensure_composite_pipeline(&scope, target_spec)?;

        let radius = request.physical_blur_radius();
        let [source_w, source_h] = [source.size[0] as f32, source.size[1] as f32];
        let [clip_x, clip_y] = [clipped.origin[0] as f32, clipped.origin[1] as f32];
        let [clip_w, clip_h] = [clipped.size[0] as f32, clipped.size[1] as f32];
        // Maps a full-scratch [0,1] onto the gamma source sub-rect (shared by the Gaussian
        // horizontal pass and the dual-Kawase prefilter, both of which sample the source).
        let remap_offset = [clip_x / source_w, clip_y / source_h];
        let remap_scale = [clip_w / source_w, clip_h / source_h];

        // The composite is identical for both algorithms; only the texture it samples differs
        // (Gaussian scratch B vs Kawase mip 0). `backdrop_uv_*` keeps a clipped source registered
        // 1:1 with the content behind the glass.
        let (backdrop_uv_offset, backdrop_uv_scale) =
            backdrop_uv_remap(&request.source_region, &clipped);
        let mask = ResolvedMask::from_target(&request.target_rect, request.corner_radius);
        let tint = request.tint.color();
        let composite = CompositeParams::new(
            [
                request.target_rect.origin[0] as f32,
                request.target_rect.origin[1] as f32,
            ],
            [
                request.target_rect.size[0] as f32,
                request.target_rect.size[1] as f32,
            ],
            [tint.r(), tint.g(), tint.b(), tint.a()],
            backdrop_uv_offset,
            backdrop_uv_scale,
            mask.corner_radius_px,
            encode_srgb,
            request.presence.value(),
        );
        let composite_buf = uniform_buffer(&scope, queue, &composite, "backdrop-blur composite")?;

        let (blur, composite_bind) = if use_dual_kawase(radius) {
            let levels = resolve_kawase_levels(radius);
            let key = PingPongKey {
                size: clipped.size,
                levels,
            };
            self.ensure_pyramid(&scope, key)?;
            let n = levels as usize;

            // Prefilter: source (gamma, sub-rect) → mip 0 (linear), via the Gaussian pipeline at
            // radius 0 — a pure decode + remap, no blur.
            let prefilter = GaussianParams::new(
                remap_offset,
                remap_scale,
                [1.0 / source_w, 1.0 / source_h],
                [1.0, 0.0],
                0.5,
                0,
                decode_srgb,
            );
            let prefilter_buf =
                uniform_buffer(&scope, queue, &prefilter, "backdrop-blur kawase-prefilter")?;
            // Per-pass half-pixel offsets: each pass samples a known mip level.
            let down_bufs = (0..n)
                .map(|i| {
                    let hp = kawase_halfpixel(kawase_level_size(clipped.size, i as u32));
                    uniform_buffer(
                        &scope,
                        queue,
                        &KawaseParams::new(hp),
                        "backdrop-blur kawase-down",
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            let up_bufs = (0..n)
                .map(|j| {
                    let hp = kawase_halfpixel(kawase_level_size(clipped.size, (n - j) as u32));
                    uniform_buffer(
                        &scope,
                        queue,
                        &KawaseParams::new(hp),
                        "backdrop-blur kawase-up",
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;

            let pyramid = self
                .pyramids
                .get(&key)
                .ok_or_else(|| BlurError::ResourceCreation {
                    stage: BlurStage::PingPongTexture,
                    source: "kawase pyramid missing immediately after ensure_pyramid".into(),
                })?;
            let pyramid = &pyramid.views;
            let prefilter_bind = self.bind(
                &scope,
                &source.view,
                &prefilter_buf,
                "backdrop-blur prefilter-bind",
            )?;
            let down_binds = down_bufs
                .iter()
                .enumerate()
                .map(|(i, buf)| self.bind(&scope, &pyramid[i], buf, "backdrop-blur down-bind"))
                .collect::<Result<Vec<_>, _>>()?;
            let up_binds = up_bufs
                .iter()
                .enumerate()
                .map(|(j, buf)| self.bind(&scope, &pyramid[n - j], buf, "backdrop-blur up-bind"))
                .collect::<Result<Vec<_>, _>>()?;
            let composite_bind = self.bind(
                &scope,
                &pyramid[0],
                &composite_buf,
                "backdrop-blur composite-bind",
            )?;

            (
                PreparedBlur::DualKawase {
                    key,
                    prefilter_bind,
                    down_binds,
                    up_binds,
                },
                composite_bind,
            )
        } else {
            let kernel = resolve_gaussian(radius);
            let key = PingPongKey {
                size: clipped.size,
                levels: 1,
            };
            self.ensure_scratch(&scope, key)?;

            // Pass 1 maps the scratch onto the source sub-rect and decodes; pass 2 samples the
            // full (linear) scratch A.
            let horizontal = GaussianParams::new(
                remap_offset,
                remap_scale,
                [1.0 / source_w, 1.0 / source_h],
                [1.0, 0.0],
                kernel.sigma,
                kernel.tap_radius,
                decode_srgb,
            );
            let vertical = GaussianParams::new(
                [0.0, 0.0],
                [1.0, 1.0],
                [1.0 / clip_w, 1.0 / clip_h],
                [0.0, 1.0],
                kernel.sigma,
                kernel.tap_radius,
                false,
            );
            let horizontal_buf =
                uniform_buffer(&scope, queue, &horizontal, "backdrop-blur gaussian-h")?;
            let vertical_buf =
                uniform_buffer(&scope, queue, &vertical, "backdrop-blur gaussian-v")?;

            let chain = self
                .scratch
                .get(&key)
                .ok_or_else(|| BlurError::ResourceCreation {
                    stage: BlurStage::PingPongTexture,
                    source: "scratch chain missing immediately after ensure_scratch".into(),
                })?;
            let horizontal_bind = self.bind(
                &scope,
                &source.view,
                &horizontal_buf,
                "backdrop-blur h-bind",
            )?;
            let vertical_bind = self.bind(
                &scope,
                &chain.views[0],
                &vertical_buf,
                "backdrop-blur v-bind",
            )?;
            let composite_bind = self.bind(
                &scope,
                &chain.views[1],
                &composite_buf,
                "backdrop-blur composite-bind",
            )?;

            (
                PreparedBlur::Gaussian {
                    key,
                    horizontal_bind,
                    vertical_bind,
                },
                composite_bind,
            )
        };

        Ok(Some(WgpuPrepared {
            target_format: target_spec,
            generation: self.generation,
            blur,
            composite_bind,
        }))
    }

    fn record(
        &self,
        sink: &mut Self::CommandSink,
        target: &Self::Target,
        prepared: Self::Prepared,
    ) -> Result<(), BlurError> {
        // `record` consumes the handle, so double-record is a compile error. The one hazard left
        // to guard: a newer prepare clobbered the shared scratch before this handle was recorded
        // (K1 serial contract). Debug-only — release builds trust the contract.
        debug_assert_eq!(
            prepared.generation, self.generation,
            "Prepared is stale: a newer prepare clobbered the shared scratch before this handle \
             was recorded; v1 requires serial prepare→record per surface (K1)"
        );
        let composite_pipeline = &self
            .composite_pipelines
            .get(&prepared.target_format)
            .ok_or_else(|| BlurError::ResourceCreation {
                stage: BlurStage::CompositePipeline,
                source: "composite pipeline missing at record".into(),
            })?
            .pipeline;

        // Blur into the scratch (Gaussian ping-pong, or the dual-Kawase pyramid).
        self.record_blur(sink, &prepared.blur)?;

        // Composite: the final blurred texture → target, over the WHOLE attachment (default
        // viewport). The
        // rounded-rect coverage forms every edge, so straight sides are anti-aliased and an
        // off-target rect cannot trip scissor validation; coverage 0 outside the panel keeps
        // LoadOp::Load content untouched. (A scissor to the panel + AA margin is a future perf
        // optimization once the host threads the target size in.)
        let mut pass = sink.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("backdrop-blur composite-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(composite_pipeline);
        pass.set_bind_group(0, &prepared.composite_bind, &[]);
        pass.draw(0..3, 0..1);
        Ok(())
    }
}

// --- Error-scope + buffer helpers ---

/// The route every creation's out-of-memory report takes, obtained from
/// [`WgpuBlur::creation_scope`]. Every backend (and adapter-intermediate) resource creation runs
/// inside a per-call [`OutOfMemory`](wgpu::ErrorFilter::OutOfMemory) error scope through this
/// type; push, create, and the pop *call* run on one thread with nothing yielding between (a
/// [`wgpu::ErrorScopeGuard`] is thread-local, `!Send`). What differs by dispatch is *when the
/// fault comes back*: on native the scope resolves synchronously, so the fault is this call's
/// `Err` — returned before the handle is consumed; on the web's WebGPU dispatch the pop resolves
/// as a deferred promise, so the twin parks the outcome in the backend's fault log and the host
/// reads it after the fact via the backend's fault report (same [`BlurError`] variant, same
/// recovery meaning, later delivery).
pub struct OomScope<'a> {
    device: &'a wgpu::Device,
    /// The backend's shared fault log, cloned in so the spawned pop-awaiting tasks outlive the
    /// scope (which borrows nothing of the backend).
    #[cfg(target_arch = "wasm32")]
    faults: fault::SharedFaultLog,
    /// The backend generation the scope was created under; stamped into every parked fault so
    /// the drain can tell a live fault from a stale one.
    #[cfg(target_arch = "wasm32")]
    generation: u64,
}

impl OomScope<'_> {
    /// Run `create` inside an `OutOfMemory` error scope, routing a captured fault per `outcome`
    /// and attributing it to `slot`. Native body: the synchronous [`scoped_oom`] — the fault
    /// comes back as this call's `Err`, the attribution is unused (`_slot`), and the result MUST
    /// be checked (`?`) before the handle is consumed by any later call (an out-of-memory handle
    /// consumed downstream raises an uncatchable `Validation` error — wgpu's contagious
    /// invalidity). The web twin inverts the unused half: `outcome` is ignored (the web path has
    /// no device-fatal creation arm) and the parked fault is attributed per `slot`.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn scoped<T>(
        &self,
        outcome: OomOutcome,
        _slot: SlotKey,
        create: impl FnOnce() -> T,
    ) -> Result<T, BlurError> {
        scoped_oom(self.device, outcome, create)
    }

    /// The web twin of [`scoped`](Self::scoped): push → create → **call pop synchronously** (the
    /// web backend's LIFO scope counter decrements at the pop call, independent of the await) →
    /// spawn a task that awaits the deferred resolution and parks any fault in the backend's log,
    /// attributed per `slot` and stamped with the scope's generation → return the (possibly
    /// invalid) resource as `Ok`. The frame path never awaits and never panics; the fault
    /// surfaces through the backend's fault report on a later frame. `outcome` is ignored: on
    /// this dispatch a creation out-of-memory does not kill the device (measured), so there is
    /// no device-fatal arm to route.
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn scoped<T>(
        &self,
        _outcome: OomOutcome,
        slot: SlotKey,
        create: impl FnOnce() -> T,
    ) -> Result<T, BlurError> {
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let resource = create();
        // `pop` consumes the (!Send) guard immediately; only the owned 'static future is moved
        // into the spawned task, so the push/pop pairing stays on this thread and in LIFO order.
        let pending = scope.pop();
        let faults = std::rc::Rc::clone(&self.faults);
        let generation = self.generation;
        wasm_bindgen_futures::spawn_local(async move {
            if let Some(err) = pending.await {
                faults.borrow_mut().record(fault::PendingFault {
                    slot,
                    generation,
                    message: describe(&err),
                });
            }
        });
        Ok(resource)
    }

    /// The one public creation route, for the egui adapter's offscreen intermediate texture:
    /// `scoped` with the fixed non-fatal arm and intermediate attribution, so the keyed
    /// `SlotKey`/`OomOutcome` internals stay private. Native: a device out-of-memory is this
    /// call's `Err` ([`BlurError::DeviceOutOfMemory`]), returned before the texture is consumed.
    /// Web (WebGPU dispatch): the deferred fault is parked and later surfaced through the
    /// backend's fault report, attributed to [`FaultSlot::Intermediate`].
    pub fn scoped_intermediate<T>(&self, create: impl FnOnce() -> T) -> Result<T, BlurError> {
        // wgpu-core 29.0.3: create_texture primary alloc non-fatal; MIXED (internal clear-view
        // fatal) tagged Recoverable per decision (d).
        self.scoped(OomOutcome::Recoverable, SlotKey::Intermediate, create)
    }
}

impl WgpuBlur {
    /// The [`OomScope`] every creation for this backend must run through. The scope borrows only
    /// `device` (the `&self` borrow ends at return — on the web the fault log is cloned in, not
    /// borrowed), so it can be held across the backend's `&mut self` frame-path calls.
    pub fn creation_scope<'a>(&self, device: &'a wgpu::Device) -> OomScope<'a> {
        OomScope {
            device,
            #[cfg(target_arch = "wasm32")]
            faults: std::rc::Rc::clone(&self.faults),
            #[cfg(target_arch = "wasm32")]
            generation: self.generation,
        }
    }
}

/// The awaited construction-path scope (wasm32): push scope → create → pop → **await** the
/// deferred resolution; a caught fault maps to [`BlurError::DeviceOutOfMemory`] (no web
/// classification — a creation out-of-memory does not kill the device on this dispatch, so
/// [`BlurError::DeviceLost`] is never produced here). Used only where awaiting is allowed
/// (constructors / prewarm), never on the frame path.
///
/// Callers MUST await each creation **strictly sequentially — never `join!`/`select`**: the
/// device's error-scope stack is LIFO, and concurrent push/pop interleaving would corrupt it
/// (pairing a pop with another creation's scope).
#[cfg(target_arch = "wasm32")]
async fn scoped_oom_awaited<T>(
    device: &wgpu::Device,
    create: impl FnOnce() -> T,
) -> Result<T, BlurError> {
    let scope = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let resource = create();
    match scope.pop().await {
        None => Ok(resource),
        Some(err) => Err(BlurError::DeviceOutOfMemory {
            source: describe(&err).into(),
        }),
    }
}

/// Poll a future exactly once and return its output if it is already ready. On the native wgpu
/// backend an error scope's `pop()` future is always already-resolved (the fault was recorded
/// synchronously during the create call), so a single poll suffices — no executor, no blocking. A
/// `Pending` result means the non-native (deferred-promise) path, which this crate does not support.
#[cfg(not(target_arch = "wasm32"))]
fn poll_once<F: std::future::Future>(fut: F) -> Option<F::Output> {
    let mut fut = std::pin::pin!(fut);
    let waker = std::task::Waker::noop();
    let mut cx = std::task::Context::from_waker(waker);
    match fut.as_mut().poll(&mut cx) {
        std::task::Poll::Ready(v) => Some(v),
        std::task::Poll::Pending => None,
    }
}

/// Whether a rejected allocation at a `scoped_oom` site leaves the device alive. wgpu-core routes
/// each creation's fault through one of two handlers: non-fatal (the allocation fails, the device
/// survives) or fatal (`device.lose()` before the error is returned — the device is permanently
/// invalid). Which handler fires is a wgpu-core internal invisible in the returned error, so every
/// call site must state its arm explicitly; the mapping is a static claim about wgpu-core 29.0.3,
/// guarded by the `wgpu_core_version_pin` tripwire test.
enum OomOutcome {
    /// The allocation's handler skips `lose()` on out-of-memory: report
    /// [`BlurError::DeviceOutOfMemory`], the device survives, the host may retry.
    Recoverable,
    /// The allocation's handler calls `lose()` before returning: report [`BlurError::DeviceLost`],
    /// the device is already gone at return, the host must tear down.
    DeviceLost,
}

/// Fold an error and its `source()` chain into one `": "`-joined string. `wgpu::Error`'s `Display`
/// is a bare constant (`"Out of Memory"`); the resource that faulted lives one level down, in
/// wgpu-core's `ContextError` source (the API call + descriptor label). A plain `String` boxed as
/// the backend-error source is chain-terminal, so the chain is flattened into the message here —
/// keeping that diagnostic while staying `Send + Sync` on wasm, where the live `wgpu::Error` is not.
fn describe(err: &(dyn std::error::Error + 'static)) -> String {
    let mut parts = vec![err.to_string()];
    let mut cause = err.source();
    while let Some(c) = cause {
        parts.push(c.to_string());
        cause = c.source();
    }
    parts.join(": ")
}

/// Run `create` inside an [`OutOfMemory`](wgpu::ErrorFilter::OutOfMemory) error scope; route a
/// captured allocation failure per `outcome` — [`BlurError::DeviceOutOfMemory`] where the device
/// survives the rejection, [`BlurError::DeviceLost`] where wgpu-core has already marked the device
/// invalid (`device.lose()`) inside the create call. On the `DeviceLost` arm the device is gone at
/// return; that is the capture instant, not a re-checked liveness status. Only out-of-memory is
/// scoped; validation and internal faults are crate bugs left to wgpu's default panic handler.
/// Compiled only for the native dispatch, whose scopes resolve synchronously (see
/// [`poll_once`]); the wasm32 build's creations go through the deferred twins instead. The result
/// MUST be checked (`?`) before it is consumed by any
/// later call — an out-of-memory handle consumed downstream raises an uncatchable `Validation`
/// error (wgpu's contagious invalidity). Push, create, and pop run on one thread with nothing
/// yielding between, because a [`wgpu::ErrorScopeGuard`] is thread-local (`!Send`).
#[cfg(not(target_arch = "wasm32"))]
fn scoped_oom<T>(
    device: &wgpu::Device,
    outcome: OomOutcome,
    create: impl FnOnce() -> T,
) -> Result<T, BlurError> {
    let scope = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let resource = create();
    match poll_once(scope.pop()) {
        Some(None) => Ok(resource),
        Some(Some(err)) => Err(match outcome {
            OomOutcome::Recoverable => BlurError::DeviceOutOfMemory {
                source: describe(&err).into(),
            },
            OomOutcome::DeviceLost => BlurError::DeviceLost {
                source: describe(&err).into(),
            },
        }),
        None => panic!(
            "backdrop-blur: internal contract violation — the native dispatch resolved an \
             out-of-memory error scope asynchronously (the wasm32 build contains no synchronous \
             scope read, so this can only be a native wgpu backend change)"
        ),
    }
}

/// Create a UNIFORM buffer initialized with `value`'s bytes, inside an `OutOfMemory` error scope.
///
/// Deliberately does **not** use `create_buffer_init`: that helper maps-at-creation and calls
/// `get_mapped_range_mut` on the returned buffer, which on an out-of-memory-invalidated buffer hits a
/// fatal panic path that bypasses the error scope entirely (before the scope is ever read). Instead
/// the mapped buffer is created inside the scope and checked (`?`), and only mapped/written on
/// success — so the `?` short-circuits before the fatal map on out-of-memory. The padding matches
/// `create_buffer_init` exactly (round up to `COPY_BUFFER_ALIGNMENT`, min one alignment).
///
/// The native form does not need the queue (`_queue`); the wasm32 twin uploads via
/// `queue.write_buffer` instead of mapping, so the shared signature carries it.
#[cfg(not(target_arch = "wasm32"))]
fn uniform_buffer<T: bytemuck::Pod>(
    scope: &OomScope<'_>,
    _queue: &wgpu::Queue,
    value: &T,
    label: &str,
) -> Result<wgpu::Buffer, BlurError> {
    let contents = bytemuck::bytes_of(value);
    let unpadded_size = contents.len() as wgpu::BufferAddress;
    let align_mask = wgpu::COPY_BUFFER_ALIGNMENT - 1;
    let padded_size = ((unpadded_size + align_mask) & !align_mask).max(wgpu::COPY_BUFFER_ALIGNMENT);
    // MIXED: primary alloc non-fatal, but the mapped_at_creation path's internal StagingBuffer::new
    // is fatal on OOM; tagged Recoverable per approval decision (d), backstopped by the host's
    // device-lost callback — see the DeviceOutOfMemory rustdoc.
    let buffer = scope.scoped(OomOutcome::Recoverable, SlotKey::Uniform, || {
        scope.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: padded_size,
            usage: wgpu::BufferUsages::UNIFORM,
            mapped_at_creation: true,
        })
    })?;
    buffer
        .get_mapped_range_mut(..)
        .slice(..unpadded_size as usize)
        .copy_from_slice(contents);
    buffer.unmap();
    Ok(buffer)
}

/// The web twin of [`uniform_buffer`]: same signature, **no mapped-consume call in the fault
/// window**. On this dispatch a creation fault resolves only after the frame, so the native
/// mapped-at-creation body would `get_mapped_range_mut` a possibly-invalid buffer before the
/// fault is knowable — a fatal panic path. Instead the buffer is created **unmapped** (with
/// `COPY_DST`) inside the deferred scope and uploaded via `queue.write_buffer`; on an invalid
/// buffer that raises an out-of-band validation error the browser contains, never a panic. The
/// contents are zero-padded to `COPY_BUFFER_ALIGNMENT`, matching the native padding maths (a
/// no-op for this crate's 16-byte-multiple Pod uniforms).
#[cfg(target_arch = "wasm32")]
fn uniform_buffer<T: bytemuck::Pod>(
    scope: &OomScope<'_>,
    queue: &wgpu::Queue,
    value: &T,
    label: &str,
) -> Result<wgpu::Buffer, BlurError> {
    let contents = bytemuck::bytes_of(value);
    let unpadded_size = contents.len() as wgpu::BufferAddress;
    let align_mask = wgpu::COPY_BUFFER_ALIGNMENT - 1;
    let padded_size = ((unpadded_size + align_mask) & !align_mask).max(wgpu::COPY_BUFFER_ALIGNMENT);
    let buffer = scope.scoped(OomOutcome::Recoverable, SlotKey::Uniform, || {
        scope.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: padded_size,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    })?;
    let mut padded = contents.to_vec();
    padded.resize(padded_size as usize, 0);
    queue.write_buffer(&buffer, 0, &padded);
    Ok(buffer)
}

/// A `[width, height]` `SCRATCH_FORMAT` texture usable as both a sampled source and a render
/// target, returned as a view (the view keeps the texture alive by refcount). The texture creation
/// is scoped for out-of-memory; the view creation is not a memory allocation and is left unscoped
/// (a documented low-risk residual).
fn scratch_view(
    scope: &OomScope<'_>,
    size: [u32; 2],
    slot: SlotKey,
    label: &str,
) -> Result<wgpu::TextureView, BlurError> {
    // MIXED: primary alloc non-fatal, but RENDER_ATTACHMENT's internal clear-view creation is
    // fatal on OOM; tagged Recoverable per approval decision (d), backstopped by the host's
    // device-lost callback — see the DeviceOutOfMemory rustdoc.
    let texture = scope.scoped(OomOutcome::Recoverable, slot, || {
        scope.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: SCRATCH_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
    })?;
    Ok(texture.create_view(&wgpu::TextureViewDescriptor::default()))
}

impl WgpuBlur {
    /// Replay a prepared blur's passes into its scratch: the Gaussian horizontal/vertical, or the
    /// dual-Kawase prefilter → downsamples → upsamples back to mip 0.
    fn record_blur(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        blur: &PreparedBlur,
    ) -> Result<(), BlurError> {
        let missing = || BlurError::ResourceCreation {
            stage: BlurStage::PingPongTexture,
            source: "scratch missing at record (prepare not called, or evicted)".into(),
        };
        match blur {
            PreparedBlur::Gaussian {
                key,
                horizontal_bind,
                vertical_bind,
            } => {
                let chain = self.scratch.get(key).ok_or_else(missing)?;
                self.blur_pass(
                    encoder,
                    &chain.views[0],
                    horizontal_bind,
                    &self.gaussian_pipeline,
                    "backdrop-blur h-pass",
                );
                self.blur_pass(
                    encoder,
                    &chain.views[1],
                    vertical_bind,
                    &self.gaussian_pipeline,
                    "backdrop-blur v-pass",
                );
            }
            PreparedBlur::DualKawase {
                key,
                prefilter_bind,
                down_binds,
                up_binds,
            } => {
                let pyramid = self.pyramids.get(key).ok_or_else(missing)?;
                let pyramid = &pyramid.views;
                let n = key.levels as usize;
                // Prefilter: source (gamma, sub-rect) → mip 0 (linear), via the Gaussian pipeline
                // at radius 0 (a pure decode + remap).
                self.blur_pass(
                    encoder,
                    &pyramid[0],
                    prefilter_bind,
                    &self.gaussian_pipeline,
                    "backdrop-blur kawase-prefilter",
                );
                // Downsample i: mip[i] → mip[i+1].
                for (i, bind) in down_binds.iter().enumerate() {
                    self.blur_pass(
                        encoder,
                        &pyramid[i + 1],
                        bind,
                        &self.downsample_pipeline,
                        "backdrop-blur kawase-down",
                    );
                }
                // Upsample j: mip[n-j] → mip[n-1-j], ending at mip 0.
                for (j, bind) in up_binds.iter().enumerate() {
                    self.blur_pass(
                        encoder,
                        &pyramid[n - 1 - j],
                        bind,
                        &self.upsample_pipeline,
                        "backdrop-blur kawase-up",
                    );
                }
            }
        }
        Ok(())
    }

    /// A full-attachment blur pass (replace, no blend): clears then draws the oversized triangle
    /// into `attachment` using `bind` and `pipeline`.
    fn blur_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        attachment: &wgpu::TextureView,
        bind: &wgpu::BindGroup,
        pipeline: &wgpu::RenderPipeline,
        label: &str,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: attachment,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, bind, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// Standard non-premultiplied "over" blend for the composite.
fn over_blend() -> wgpu::BlendState {
    wgpu::BlendState {
        color: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::SrcAlpha,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        },
        alpha: wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        },
    }
}

// --- Raw creations ---
//
// The descriptor bodies of every fixed creation, extracted so the native constructor (wrapping
// each in a synchronous `scoped_oom`) and the wasm32 constructor (awaiting each scope) run the
// *same* descriptor code instead of duplicating it. Raw: no error scope here — the caller owns
// the scope and the outcome arm.

/// The one shared bind-group layout: sampled texture + filtering sampler + uniform buffer.
fn make_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("backdrop-blur bind group layout"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    })
}

/// The one pipeline layout over the shared bind-group layout.
fn make_pipeline_layout(
    device: &wgpu::Device,
    bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::PipelineLayout {
    device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("backdrop-blur pipeline layout"),
        bind_group_layouts: &[Some(bind_group_layout)],
        immediate_size: 0,
    })
}

/// The shared clamp-to-edge bilinear sampler.
fn make_sampler(device: &wgpu::Device) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("backdrop-blur sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..Default::default()
    })
}

/// The separable-Gaussian shader (also the dual-Kawase prefilter at radius 0).
fn make_gaussian_shader(device: &wgpu::Device) -> wgpu::ShaderModule {
    device.create_shader_module(wgpu::include_wgsl!("shaders/gaussian.wgsl"))
}

/// The dual-Kawase downsample shader.
fn make_downsample_shader(device: &wgpu::Device) -> wgpu::ShaderModule {
    device.create_shader_module(wgpu::include_wgsl!("shaders/downsample.wgsl"))
}

/// The dual-Kawase upsample shader.
fn make_upsample_shader(device: &wgpu::Device) -> wgpu::ShaderModule {
    device.create_shader_module(wgpu::include_wgsl!("shaders/upsample.wgsl"))
}

/// The tinted, rounded-rect-masked composite shader.
fn make_composite_shader(device: &wgpu::Device) -> wgpu::ShaderModule {
    device.create_shader_module(wgpu::include_wgsl!("shaders/composite.wgsl"))
}

/// Build a render pipeline from a shader module (a `vs_main`/`fs_main` pair) writing `format`,
/// with optional blend. Used for the Gaussian, the two Kawase, and the composite pipelines.
/// Raw — the caller wraps it in the out-of-memory scope; a genuine validation fault (bad
/// shader/layout) is a crate bug left to panic.
fn make_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
    blend: Option<wgpu::BlendState>,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("backdrop-blur pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            buffers: &[],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: wgpu::PipelineCompilationOptions::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
        cache: None,
    })
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn poll_once_yields_a_ready_future() {
        // The native error-scope case: `pop()` is already resolved, so one poll returns its value.
        assert_eq!(poll_once(std::future::ready(7u32)), Some(7));
    }

    #[test]
    fn poll_once_reports_pending_as_none() {
        // The non-native (deferred) case `scoped_oom` panics on: a never-ready future polls to None.
        assert_eq!(poll_once(std::future::pending::<u32>()), None);
    }
}
