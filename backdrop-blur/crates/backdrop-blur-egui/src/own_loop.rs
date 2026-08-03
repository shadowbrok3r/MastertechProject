//! The own-loop adapter: drive `egui-winit` + `egui-wgpu` directly (not eframe), render the UI
//! into an offscreen intermediate, then blur a region and composite the frosted surface into the
//! target — in the one order that does not panic (DESIGN §6, M4-corrected).

use crate::Surface;
use backdrop_blur_core::{BackdropBlur, BlurError, BlurRequest, Region, RepaintPolicy, Scale};
use backdrop_blur_wgpu::{OomScope, SourceColorSpace, SourceView, WgpuBlur};

/// Own-loop-only resolution of a [`Surface`]. This `impl` lives in the `own-loop`-gated module on
/// purpose: it builds a **top-left** [`BlurRequest`] (the egui-wgpu sampling convention), which is
/// wrong for the grab-pass path, so gating the module makes `request` *uncallable* from a
/// grab-pass build — the relocated-flip bug is unrepresentable rather than merely discouraged.
impl Surface {
    /// Resolve to a physical-pixel [`BlurRequest`]. The egui rect (points) scales by
    /// `pixels_per_point`; the backdrop behind the surface is the same screen area.
    fn request(&self, pixels_per_point: f32) -> BlurRequest {
        let origin = [
            (self.rect.min.x * pixels_per_point).round().max(0.0) as u32,
            (self.rect.min.y * pixels_per_point).round().max(0.0) as u32,
        ];
        let size = [
            (self.rect.width() * pixels_per_point).round().max(0.0) as u32,
            (self.rect.height() * pixels_per_point).round().max(0.0) as u32,
        ];
        let region = Region {
            origin,
            size,
            scale: Scale::new(pixels_per_point),
        };
        BlurRequest {
            source_region: region,
            target_rect: region,
            blur_radius: self.blur_radius,
            tint: self.tint,
            corner_radius: self.corner_radius,
            presence: self.presence,
        }
    }
}

/// The strongest repaint obligation across a set of surfaces: `Live` wins, then the shortest
/// `Bounded` interval, else `Static`. [`OwnLoopRenderer::render_frame`] applies this to the egui
/// `Context` itself; this is exposed for hosts that want to inspect the obligation directly.
pub fn strongest_repaint(surfaces: &[Surface]) -> RepaintPolicy {
    surfaces
        .iter()
        .fold(RepaintPolicy::Static, |acc, s| match (acc, s.repaint) {
            (RepaintPolicy::Live, _) | (_, RepaintPolicy::Live) => RepaintPolicy::Live,
            (RepaintPolicy::Bounded(a), RepaintPolicy::Bounded(b)) => {
                RepaintPolicy::Bounded(a.min(b))
            }
            (RepaintPolicy::Bounded(d), RepaintPolicy::Static)
            | (RepaintPolicy::Static, RepaintPolicy::Bounded(d)) => RepaintPolicy::Bounded(d),
            (RepaintPolicy::Static, RepaintPolicy::Static) => RepaintPolicy::Static,
        })
}

/// The per-frame GPU handles a blur needs, bundled so the surface loop stays legible. Generic
/// over the backend `B`, so a test can build one entirely from `()` and run headlessly.
pub(crate) struct SeamContext<'a, B: BackdropBlur> {
    pub device: &'a B::Device,
    pub queue: &'a B::Queue,
    pub sink: &'a mut B::CommandSink,
    pub source: &'a B::SourceTexture,
    pub target: &'a B::Target,
    pub target_spec: B::TargetSpec,
}

/// The backend-agnostic core of the adapter: for each surface, `prepare` the blur and `record` it
/// when the region is non-empty. Generic over the backend so it is exercised headlessly by a
/// recording fake in tests (the real egui rendering around it needs a GPU; this mapping does not).
///
/// Returns the number of surfaces that recorded a blur (a clipped-to-nothing surface prepares to
/// `Ok(None)` and records nothing).
pub(crate) fn composite_surfaces<B>(
    blur: &mut B,
    ctx: SeamContext<'_, B>,
    surfaces: &[Surface],
    pixels_per_point: f32,
) -> Result<usize, BlurError>
where
    B: BackdropBlur,
    B::TargetSpec: Copy,
{
    let mut recorded = 0;
    for surface in surfaces {
        let request = surface.request(pixels_per_point);
        if let Some(prepared) =
            blur.prepare(ctx.device, ctx.queue, ctx.source, ctx.target_spec, &request)?
        {
            blur.record(ctx.sink, ctx.target, prepared)?;
            recorded += 1;
        }
    }
    Ok(recorded)
}

/// The offscreen intermediate egui renders into (the blur source). Cached and resized to the
/// screen; its format matches the target so one `egui-wgpu::Renderer` serves both.
struct Intermediate {
    texture: wgpu::Texture,
    size: [u32; 2],
}

/// Whether the own-loop adapter supports compositing into `format`. The adapter renders egui's
/// **gamma-encoded** output (egui#3168) into an intermediate of the *same* format and decodes it
/// in the blur shader; that model is only correct for **non-sRGB `Unorm`** targets. An `*Srgb`
/// target would make the sampler decode once and the shader decode again (washed-out frost), so it
/// is rejected at construction rather than silently mis-rendered.
pub fn is_supported_target(format: wgpu::TextureFormat) -> bool {
    matches!(
        format,
        wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Bgra8Unorm
    )
}

/// Drives one own-loop frame for an `egui-winit` + `egui-wgpu` host: it renders the egui UI into
/// the intermediate (the blur source) and the target (the display), then blurs and composites the
/// frosted surfaces over the target — all on one encoder with a single submit.
pub struct OwnLoopRenderer {
    renderer: egui_wgpu::Renderer,
    target_format: wgpu::TextureFormat,
    intermediate: Option<Intermediate>,
}

impl OwnLoopRenderer {
    /// Build the adapter for a host whose target (swapchain) has `target_format`.
    ///
    /// Returns [`BlurError::UnsupportedTarget`] unless `target_format` is a non-sRGB `Unorm`
    /// format ([`is_supported_target`]) — the adapter pins the decode-in-shader gamma model, which
    /// only matches non-sRGB targets (egui#3168). This makes the documented format assumption a
    /// checked contract rather than prose.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
    ) -> Result<Self, BlurError> {
        if !is_supported_target(target_format) {
            return Err(BlurError::UnsupportedTarget {
                format: format!("{target_format:?} (own-loop needs a non-sRGB Unorm target)"),
            });
        }
        let renderer =
            egui_wgpu::Renderer::new(device, target_format, egui_wgpu::RendererOptions::default());
        Ok(Self {
            renderer,
            target_format,
            intermediate: None,
        })
    }

    /// The web (WebGPU-dispatch) twin of the native constructor. Same allowlist check, plus one
    /// honest difference: it takes the blur backend and awaits
    /// [`WgpuBlur::prewarm_composite`] for `target_format` — construction verifies the composite
    /// pipeline against the backend this renderer will drive, so the frame path never lazily
    /// builds a pipeline whose deferred fault could only surface after the frame that consumed
    /// it. Allocations *inside* `egui_wgpu::Renderer::new` remain third-party and unscoped — the
    /// same named exclusion as native.
    #[cfg(target_arch = "wasm32")]
    pub async fn new(
        device: &wgpu::Device,
        target_format: wgpu::TextureFormat,
        blur: &mut WgpuBlur,
    ) -> Result<Self, BlurError> {
        if !is_supported_target(target_format) {
            return Err(BlurError::UnsupportedTarget {
                format: format!("{target_format:?} (own-loop needs a non-sRGB Unorm target)"),
            });
        }
        blur.prewarm_composite(device, target_format).await?;
        let renderer =
            egui_wgpu::Renderer::new(device, target_format, egui_wgpu::RendererOptions::default());
        Ok(Self {
            renderer,
            target_format,
            intermediate: None,
        })
    }

    /// The intermediate sized to `size`, recreated only on a size change. Total — no panic path: a
    /// stale-size intermediate is dropped, a matching one reused, or a fresh one created through
    /// the backend's [`OomScope`] (native: a device out-of-memory is returned as
    /// [`BlurError::DeviceOutOfMemory`] before the texture is used; web: the deferred fault is
    /// parked and surfaced through the backend's fault report). [`Option::insert`] then stores
    /// and returns it without any `unwrap`.
    fn intermediate(
        &mut self,
        device: &wgpu::Device,
        scope: &OomScope<'_>,
        size: [u32; 2],
    ) -> Result<&Intermediate, BlurError> {
        // Take the cached intermediate, keeping it only if its size still matches. A stale one is
        // dropped here (freeing its texture) *before* the new allocation is attempted.
        let reuse = self.intermediate.take().filter(|i| i.size == size);
        let intermediate = match reuse {
            Some(existing) => existing,
            None => {
                let format = self.target_format;
                let texture = scope.scoped_intermediate(|| {
                    device.create_texture(&wgpu::TextureDescriptor {
                        label: Some("backdrop-blur egui intermediate"),
                        size: wgpu::Extent3d {
                            width: size[0].max(1),
                            height: size[1].max(1),
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format,
                        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                            | wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    })
                })?;
                Intermediate { texture, size }
            }
        };
        Ok(self.intermediate.insert(intermediate))
    }

    /// Render one frosted frame. `ctx` is the host's egui context: the adapter applies the
    /// surfaces' [`RepaintPolicy`] to it (`request_repaint` for `Live`, `request_repaint_after`
    /// for `Bounded`) so a stale backdrop cannot be silently forgotten (§4.6 — the adapter, not
    /// the host, drives the repaint). `frame` carries the tessellated egui output + the target.
    ///
    /// **Out-of-memory contract.** `backdrop-blur`'s own resource creation never reaches wgpu's
    /// default (panicking) handler on a device out-of-memory; what differs by dispatch is how the
    /// fault reaches the host.
    ///
    /// **Native** — the fault is this call's `Err`, split by whether the device survives. The
    /// intermediate texture here, plus the blur backend's scratch **textures** and uniform
    /// **buffers**, return [`BlurError::DeviceOutOfMemory`] — recoverable in the common case: on
    /// that `Err` nothing has been submitted (the `?` returns before `queue.submit`), so do not
    /// present the frame, re-request a repaint, and retry unfrosted or shed surfaces (see that
    /// variant's mixed-site caveat for the narrow window where the device was lost anyway). The
    /// backend's **pipelines and bind groups** — including the per-frame bind groups — return
    /// [`BlurError::DeviceLost`]: wgpu has already invalidated the device, so tear it down and do
    /// **not** retry on it.
    ///
    /// **Web (WebGPU dispatch)** — the same creations run inside deferred scopes: a creation
    /// fault cannot fail this call, and instead surfaces through the backend's
    /// `WgpuBlur::take_fault`, which the host must read **every frame, including frames where
    /// frosting is shed or skipped** (that read keeps fault delivery live). On a report: do not
    /// trust the presented frost — re-request a repaint, retry unfrosted or shed surfaces. The
    /// web path never reports `DeviceLost`; the host's device-lost callback is the loss signal.
    ///
    /// Either way this is **not** a blanket "never panics" — allocations *inside*
    /// `egui_wgpu::Renderer` (font-atlas growth in `update_texture`, vertex/index buffer growth in
    /// `update_buffers`/`render`) are third-party and cannot be scoped by this crate; an
    /// out-of-memory there still reaches wgpu's default (panicking) handler.
    pub fn render_frame(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        ctx: &egui::Context,
        blur: &mut WgpuBlur,
        frame: FrameInput<'_>,
        surfaces: &[Surface],
    ) -> Result<(), BlurError> {
        // 1. Texture deltas first.
        for (id, delta) in &frame.textures_delta.set {
            self.renderer.update_texture(device, queue, *id, delta);
        }

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("backdrop-blur own-loop frame"),
        });

        // 2. Upload vertex/index/uniform buffers; keep the returned command buffers to submit
        //    BEFORE the main encoder (egui-wgpu does not auto-submit them).
        let egui_buffers = self.renderer.update_buffers(
            device,
            queue,
            &mut encoder,
            frame.paint_jobs,
            &frame.screen,
        );

        // Web: a deferred fault on a previous frame's intermediate must drop the cached texture
        // before it can be served again — it is recreated just below. (Size-keyed with no stamp:
        // at worst a late stale report drops one healthy intermediate for one recreation.)
        #[cfg(target_arch = "wasm32")]
        if blur.drain_intermediate_faults() {
            self.intermediate = None;
        }

        // One owned view of the intermediate, used by reference for the egui→intermediate pass
        // (the pass clones it via forget_lifetime) and then moved into the blur `SourceView`.
        // The creation runs through the backend's OomScope (borrowing only `device`, so `blur`
        // stays free for the composite below).
        let size = frame.screen.size_in_pixels;
        let scope = blur.creation_scope(device);
        let intermediate_view = self
            .intermediate(device, &scope, size)?
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // 3 + 4. Render egui into the intermediate (blur source) and into the target (display).
        //        Each render pass is scoped and dropped before the encoder is touched again — a
        //        live `forget_lifetime` pass plus an encoder op is a runtime panic (M4).
        {
            let mut pass = begin_clear_pass(
                &mut encoder,
                &intermediate_view,
                "backdrop-blur egui→intermediate",
            );
            self.renderer
                .render(&mut pass, frame.paint_jobs, &frame.screen);
        }
        {
            let mut pass =
                begin_clear_pass(&mut encoder, frame.target, "backdrop-blur egui→target");
            self.renderer
                .render(&mut pass, frame.paint_jobs, &frame.screen);
        }

        // 5. Blur + composite each surface, sampling the intermediate, writing the target.
        let source = SourceView {
            view: intermediate_view,
            size,
            color_space: SourceColorSpace::GammaSrgb,
        };
        composite_surfaces(
            blur,
            SeamContext {
                device,
                queue,
                sink: &mut encoder,
                source: &source,
                target: frame.target,
                target_spec: self.target_format,
            },
            surfaces,
            frame.screen.pixels_per_point,
        )?;

        // 6. One submit: egui's upload buffers, then the main encoder.
        let main = encoder.finish();
        queue.submit(egui_buffers.into_iter().chain(std::iter::once(main)));

        // Free textures egui dropped this frame.
        for id in &frame.textures_delta.free {
            self.renderer.free_texture(id);
        }

        // The adapter drives liveness: keep the backdrop fresh for Live/Bounded surfaces.
        match strongest_repaint(surfaces) {
            RepaintPolicy::Live => ctx.request_repaint(),
            RepaintPolicy::Bounded(after) => ctx.request_repaint_after(after),
            RepaintPolicy::Static => {}
        }
        Ok(())
    }
}

/// One frame's egui output plus where to draw it.
pub struct FrameInput<'a> {
    /// The display target (swapchain view); must have the adapter's `target_format`.
    pub target: &'a wgpu::TextureView,
    /// The tessellated egui primitives for this frame.
    ///
    /// **Backdrop-Root rule (host obligation):** v1 renders this *same* frame into both the blur
    /// source and the display, and the blur samples the surface's own screen area. So the host
    /// must **not** paint a frosted surface's own background/fill into these jobs — otherwise the
    /// blur samples the panel's fill instead of the content behind it. The crate owns only the
    /// background; the surface's foreground is the host's, painted in its own later pass.
    pub paint_jobs: &'a [egui::ClippedPrimitive],
    /// The textures egui created/freed this frame.
    pub textures_delta: &'a egui::TexturesDelta,
    /// Screen size (physical px) + pixels-per-point.
    pub screen: egui_wgpu::ScreenDescriptor,
}

/// Begin a render pass that clears `view`, returning a `'static` pass (egui-wgpu's `render`
/// requires `RenderPass<'static>`). The caller must drop it before reusing the encoder.
fn begin_clear_pass(
    encoder: &mut wgpu::CommandEncoder,
    view: &wgpu::TextureView,
    label: &str,
) -> wgpu::RenderPass<'static> {
    encoder
        .begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
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
        })
        .forget_lifetime()
}

#[cfg(test)]
mod tests {
    // Coverage boundary: these default-tier tests cover the backend-agnostic surface→prepare/record
    // mapping (`composite_surfaces`), the repaint fold, and the format guard. `render_frame`'s
    // frame ordering (update_buffers → scoped passes dropped before encoder reuse → single chained
    // submit) needs real egui-wgpu + a GPU, so it is covered only by the gated `own_loop_render`
    // test (`--features image-snapshots`, lavapipe), not the always-on `cargo test`.
    use super::*;
    use backdrop_blur_core::{BlurRadius, CornerRadius, Tint};
    use std::cell::RefCell;

    #[test]
    fn is_supported_target_accepts_only_non_srgb_unorm() {
        assert!(is_supported_target(wgpu::TextureFormat::Rgba8Unorm));
        assert!(is_supported_target(wgpu::TextureFormat::Bgra8Unorm));
        // sRGB targets would double-decode the gamma intermediate — rejected.
        assert!(!is_supported_target(wgpu::TextureFormat::Rgba8UnormSrgb));
        assert!(!is_supported_target(wgpu::TextureFormat::Bgra8UnormSrgb));
        assert!(!is_supported_target(wgpu::TextureFormat::Rgba16Float));
    }

    /// A recording fake backend: all associated types are `()`, so the surface→prepare/record
    /// wiring runs with no GPU. It returns `Ok(None)` for a zero-area region (the no-op), mirroring
    /// the real backend's clip behavior, so the test can assert "prepare always, record only when
    /// the region is non-empty".
    #[derive(Default)]
    struct RecordingBlur {
        events: RefCell<Vec<&'static str>>,
    }

    impl BackdropBlur for RecordingBlur {
        type Device = ();
        type Queue = ();
        type CommandSink = ();
        type SourceTexture = ();
        type Target = ();
        type TargetSpec = ();
        type Prepared = ();

        fn prepare(
            &mut self,
            _device: &(),
            _queue: &(),
            _source: &(),
            _target_spec: (),
            request: &BlurRequest,
        ) -> Result<Option<()>, BlurError> {
            self.events.borrow_mut().push("prepare");
            if request.source_region.size[0] == 0 || request.source_region.size[1] == 0 {
                Ok(None)
            } else {
                Ok(Some(()))
            }
        }

        fn record(&self, _sink: &mut (), _target: &(), _prepared: ()) -> Result<(), BlurError> {
            self.events.borrow_mut().push("record");
            Ok(())
        }
    }

    fn surface(rect: egui::Rect) -> Surface {
        Surface {
            rect,
            blur_radius: BlurRadius::new(8.0),
            tint: Tint::new(backdrop_blur_core::LinearRgba::new(0.0, 0.0, 0.0, 0.1)),
            corner_radius: CornerRadius::new(12.0),
            presence: backdrop_blur_core::Presence::default(),
            repaint: RepaintPolicy::Static,
        }
    }

    #[test]
    fn composite_surfaces_prepares_each_and_records_only_non_empty() {
        let mut blur = RecordingBlur::default();
        let surfaces = [
            surface(egui::Rect::from_min_size(
                egui::pos2(10.0, 10.0),
                egui::vec2(100.0, 60.0),
            )),
            surface(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(0.0, 0.0),
            )), // empty → no-op
            surface(egui::Rect::from_min_size(
                egui::pos2(50.0, 50.0),
                egui::vec2(80.0, 40.0),
            )),
        ];
        let recorded = composite_surfaces(
            &mut blur,
            SeamContext {
                device: &(),
                queue: &(),
                sink: &mut (),
                source: &(),
                target: &(),
                target_spec: (),
            },
            &surfaces,
            1.0,
        )
        .expect("the fake backend never errors");

        assert_eq!(recorded, 2);
        let events = blur.events.into_inner();
        assert_eq!(
            events.iter().filter(|e| **e == "prepare").count(),
            3,
            "prepare runs for every surface"
        );
        assert_eq!(
            events.iter().filter(|e| **e == "record").count(),
            2,
            "record skips the empty surface"
        );
        // Order proof: the empty surface prepares but does not record between the two real ones.
        assert_eq!(
            events,
            vec!["prepare", "record", "prepare", "prepare", "record"]
        );
    }

    #[test]
    fn strongest_repaint_prefers_live_then_shortest_bounded() {
        use std::time::Duration;
        let live = surface(egui::Rect::ZERO);
        let mut live = live;
        live.repaint = RepaintPolicy::Live;

        let mut bounded_long = surface(egui::Rect::ZERO);
        bounded_long.repaint = RepaintPolicy::Bounded(Duration::from_millis(500));
        let mut bounded_short = surface(egui::Rect::ZERO);
        bounded_short.repaint = RepaintPolicy::Bounded(Duration::from_millis(100));

        assert_eq!(strongest_repaint(&[]), RepaintPolicy::Static);
        assert_eq!(
            strongest_repaint(&[bounded_long, bounded_short]),
            RepaintPolicy::Bounded(Duration::from_millis(100))
        );
        assert_eq!(
            strongest_repaint(&[bounded_long, live]),
            RepaintPolicy::Live
        );
    }
}
