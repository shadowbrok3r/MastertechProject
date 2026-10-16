use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::{Duration, Instant};

use anyhow::Context as _;
use egui_skia::EguiSkiaWinit;
use skia_safe::{AlphaType, Color, ColorType, ImageInfo, Surface, surfaces};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use crate::app_state::MasterTechApp;
use crate::tabs::minidump_args::MinidumpArgs;

// Longest gap between egui passes; keeps logic_inner sampling while idle.
const TICK: Duration = Duration::from_millis(100);
// Shortest gap before a pass egui schedules itself; input still redraws at once.
const MIN_FRAME: Duration = Duration::from_millis(33);

type SoftSurface = softbuffer::Surface<Rc<Window>, Rc<Window>>;

struct SoftwareApp {
    window: Option<Rc<Window>>,
    softbuffer_surface: Option<SoftSurface>,
    skia_surface: Option<Surface>,
    egui_skia: Option<EguiSkiaWinit>,
    mt: Option<MasterTechApp>,
    minidump: MinidumpArgs,
    // Left button held; suppresses CursorLeft so egui keeps the pointer pos
    // (and the release) when a drag crosses the window edge.
    left_down: bool,
    // When the next egui pass is due.
    next_frame: Instant,
    // Shapes and scale last painted onto the skia surface; `None` forces a repaint.
    painted: Option<(Vec<egui::epaint::ClippedShape>, f32)>,
    // Set while presents fail; only the first failure in a run is logged.
    present_failing: bool,
    init_error: Option<anyhow::Error>,
}

impl SoftwareApp {
    fn new(minidump: MinidumpArgs) -> Self {
        Self {
            window: None,
            softbuffer_surface: None,
            skia_surface: None,
            egui_skia: None,
            mt: None,
            minidump,
            left_down: false,
            next_frame: Instant::now(),
            painted: None,
            present_failing: false,
            init_error: None,
        }
    }

    fn recreate_skia_surface(&mut self, width: i32, height: i32) {
        let info = ImageInfo::new(
            (width.max(1), height.max(1)),
            ColorType::BGRA8888,
            AlphaType::Premul,
            None,
        );
        self.skia_surface = surfaces::raster(&info, None, None);
        self.painted = None;
    }

    fn init(&mut self, event_loop: &ActiveEventLoop) -> anyhow::Result<()> {
        let attrs = Window::default_attributes()
            .with_title(format!("Mastertech-{}", database::version_with_build!()))
            .with_inner_size(LogicalSize::new(1000.0, 750.0));
        let window = Rc::new(event_loop.create_window(attrs).context("create window")?);

        let context = softbuffer::Context::new(window.clone())
            .map_err(|e| anyhow::anyhow!("softbuffer context: {e}"))?;
        let softbuffer_surface = softbuffer::Surface::new(&context, window.clone())
            .map_err(|e| anyhow::anyhow!("softbuffer surface: {e}"))?;

        let egui_skia = EguiSkiaWinit::new(window.as_ref(), Some(window.scale_factor() as f32));
        egui_extras::install_image_loaders(&egui_skia.egui_skia.egui_ctx);
        // Construct once; rebuilding would reset the one-time MCP/plugin bootstrap
        // in logic_inner and re-bind the :9003/:9004 servers.
        if self.mt.is_none() {
            let minidump = std::mem::take(&mut self.minidump);
            self.mt = Some(MasterTechApp::new_software(
                &egui_skia.egui_skia.egui_ctx,
                minidump,
            ));
        }

        let size = window.inner_size();
        self.recreate_skia_surface(size.width as i32, size.height as i32);

        window.request_redraw();
        self.softbuffer_surface = Some(softbuffer_surface);
        self.egui_skia = Some(egui_skia);
        self.window = Some(window);
        Ok(())
    }

    /// Runs one egui pass, repaints the skia surface if the output changed, and presents it.
    /// Returns the delay egui asked for before the next pass.
    fn frame(&mut self, event_loop: &ActiveEventLoop, window: &Window) -> Duration {
        let (Some(skia_surface), Some(egui_skia), Some(softbuffer_surface), Some(mt)) = (
            self.skia_surface.as_mut(),
            self.egui_skia.as_mut(),
            self.softbuffer_surface.as_mut(),
            self.mt.as_mut(),
        ) else {
            return TICK;
        };

        let ctx = egui_skia.egui_skia.egui_ctx.clone();
        let raw_input = egui_skia.egui_winit.take_egui_input(window);
        let mut output = ctx.run_ui(raw_input, |ui| {
            mt.logic_inner(ui.ctx());
            mt.ui_inner(ui);
        });
        egui_skia
            .egui_winit
            .handle_platform_output(window, std::mem::take(&mut output.platform_output));

        // The app's ViewportCommand::Close, which egui_skia never applies.
        let close_requested = output
            .viewport_output
            .values()
            .any(|viewport| viewport.commands.contains(&egui::ViewportCommand::Close));
        if close_requested {
            event_loop.exit();
            output.drop_without_applying_deltas();
            return TICK;
        }

        let repaint_after = output
            .viewport_output
            .values()
            .map(|viewport| viewport.repaint_delay)
            .min()
            .unwrap_or(Duration::ZERO);

        let ppp = output.pixels_per_point;
        let changed = !output.textures_delta.is_empty()
            || self.painted.as_ref().is_none_or(|(shapes, painted_ppp)| {
                *painted_ppp != ppp || *shapes != output.shapes
            });
        if changed {
            let primitives = ctx.tessellate(output.shapes.clone(), ppp);
            let canvas = skia_surface.canvas();
            canvas.clear(Color::BLACK);
            egui_skia.egui_skia.painter.paint_and_update_textures(
                canvas,
                ppp,
                primitives,
                std::mem::take(&mut output.textures_delta),
            );
            self.painted = Some((std::mem::take(&mut output.shapes), ppp));
        }

        match present(skia_surface, softbuffer_surface, changed) {
            Ok(()) => self.present_failing = false,
            Err(e) => {
                if !self.present_failing {
                    log::warn!("software renderer: present failed: {e:#}");
                }
                self.present_failing = true;
                self.painted = None;
            }
        }
        repaint_after
    }

    /// Ticks the app logic without an egui pass while the window shows nothing.
    fn logic_only(&mut self, event_loop: &ActiveEventLoop, window: &Window) {
        let (Some(egui_skia), Some(mt)) = (self.egui_skia.as_mut(), self.mt.as_mut()) else {
            return;
        };
        let ctx = egui_skia.egui_skia.egui_ctx.clone();
        let input = ctx.input(|i| egui::RawInput {
            viewports: i.raw.viewports.clone(),
            focused: i.raw.focused,
            ..Default::default()
        });
        let output = ctx.run_logic(&input, |ctx| mt.logic_inner(ctx));
        egui_skia
            .egui_winit
            .handle_platform_output(window, output.platform_output);
        let close_requested = output
            .viewport_commands
            .values()
            .flatten()
            .any(|command| *command == egui::ViewportCommand::Close);
        if close_requested {
            event_loop.exit();
        }
    }
}

impl ApplicationHandler for SoftwareApp {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, _cause: StartCause) {
        let now = Instant::now();
        if now < self.next_frame {
            return;
        }
        // Replaced by the pass's own deadline; bounds retries if RedrawRequested never arrives.
        self.next_frame = now + TICK;
        let Some(window) = self.window.clone() else {
            return;
        };
        if window.is_minimized() == Some(true) {
            self.logic_only(event_loop, &window);
        } else {
            window.request_redraw();
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let Err(e) = self.init(event_loop) {
            self.init_error = Some(e);
            event_loop.exit();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(window) = self.window.clone() else {
            return;
        };

        if let WindowEvent::MouseInput {
            state,
            button: MouseButton::Left,
            ..
        } = &event
        {
            self.left_down = *state == ElementState::Pressed;
        }
        // Withholds RedrawRequested (egui-winit always marks it repaint) and CursorLeft mid-drag.
        let forward = !matches!(event, WindowEvent::RedrawRequested)
            && !(self.left_down && matches!(event, WindowEvent::CursorLeft { .. }));
        if forward && let Some(egui_skia) = self.egui_skia.as_mut() {
            let response = egui_skia.on_window_event(&window, &event);
            if response.repaint {
                window.request_redraw();
            }
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                self.recreate_skia_surface(size.width as i32, size.height as i32);
                window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                let started = Instant::now();
                let wait = if window.is_minimized() == Some(true) {
                    self.logic_only(event_loop, &window);
                    TICK
                } else {
                    self.frame(event_loop, &window).clamp(MIN_FRAME, TICK)
                };
                self.next_frame = started + wait;
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(ControlFlow::WaitUntil(self.next_frame));
    }
}

/// Copies the skia surface into the softbuffer surface when `changed` or the buffer is stale,
/// then presents it.
fn present(
    skia_surface: &mut Surface,
    softbuffer_surface: &mut SoftSurface,
    changed: bool,
) -> anyhow::Result<()> {
    let (Some(width), Some(height)) = (
        NonZeroU32::new(skia_surface.width().max(0) as u32),
        NonZeroU32::new(skia_surface.height().max(0) as u32),
    ) else {
        return Ok(());
    };
    softbuffer_surface
        .resize(width, height)
        .map_err(|e| anyhow::anyhow!("resize to {width}x{height}: {e}"))?;
    let mut buffer = softbuffer_surface
        .buffer_mut()
        .map_err(|e| anyhow::anyhow!("buffer: {e}"))?;
    if changed || buffer.age() == 0 {
        let pixmap = skia_surface
            .peek_pixels()
            .context("skia surface has no pixels")?;
        anyhow::ensure!(
            pixmap.color_type() == ColorType::BGRA8888,
            "skia surface is {:?}, not BGRA8888",
            pixmap.color_type()
        );
        let row_bytes = pixmap.row_bytes();
        let bytes = pixmap.bytes().context("skia surface has no pixels")?;
        let rows = buffer.chunks_exact_mut(width.get() as usize);
        for (dst_row, src_row) in rows.zip(bytes.chunks(row_bytes)) {
            for (dst, px) in dst_row.iter_mut().zip(src_row.as_chunks::<4>().0) {
                // BGRA bytes to softbuffer's 0RGB.
                *dst = u32::from_le_bytes([px[0], px[1], px[2], 0]);
            }
        }
    }
    buffer
        .present()
        .map_err(|e| anyhow::anyhow!("present: {e}"))
}

/// Software-rendered (skia raster) host for machines with no working GPU GL stack.
pub fn run(minidump: MinidumpArgs) -> anyhow::Result<()> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + TICK));
    let mut app = SoftwareApp::new(minidump);
    event_loop.run_app(&mut app)?;
    app.init_error.map_or(Ok(()), Err)
}
