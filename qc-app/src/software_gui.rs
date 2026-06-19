use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use egui_skia::EguiSkiaWinit;
use skia_safe::{surfaces, AlphaType, Color, ColorType, ImageInfo, Surface};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use crate::app::QcApp;

// Idle/backend redraw floor. Input redraws immediately; egui animations drive
// faster via repaint_after. 10 fps idle keeps the headless tick alive without
// pegging a core on CPU rasterization.
const TICK: Duration = Duration::from_millis(100);

struct SoftwareApp {
    window: Option<Rc<Window>>,
    softbuffer_surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
    skia_surface: Option<Surface>,
    egui_skia: Option<EguiSkiaWinit>,
    qc: QcApp,
}

impl SoftwareApp {
    fn new() -> Self {
        Self {
            window: None,
            softbuffer_surface: None,
            skia_surface: None,
            egui_skia: None,
            qc: QcApp::default(),
        }
    }

    fn recreate_skia_surface(&mut self, width: i32, height: i32) {
        self.skia_surface = surfaces::raster_n32_premul((width.max(1), height.max(1)));
    }
}

impl ApplicationHandler for SoftwareApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let attrs = Window::default_attributes()
            .with_title(format!("Mastertech QC - {}", database::version_with_build!()))
            .with_inner_size(LogicalSize::new(500.0, 768.0));
        let window = Rc::new(event_loop.create_window(attrs).unwrap());

        let context = softbuffer::Context::new(window.clone()).unwrap();
        let softbuffer_surface = softbuffer::Surface::new(&context, window.clone()).unwrap();

        let egui_skia = EguiSkiaWinit::new(window.as_ref(), Some(window.scale_factor() as f32));
        crate::configure_egui_ctx(&egui_skia.egui_skia.egui_ctx);

        let size = window.inner_size();
        self.recreate_skia_surface(size.width as i32, size.height as i32);

        self.softbuffer_surface = Some(softbuffer_surface);
        self.egui_skia = Some(egui_skia);
        self.window = Some(window);
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

        if let Some(egui_skia) = self.egui_skia.as_mut() {
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
                let (Some(skia_surface), Some(egui_skia), Some(softbuffer_surface)) = (
                    self.skia_surface.as_mut(),
                    self.egui_skia.as_mut(),
                    self.softbuffer_surface.as_mut(),
                ) else {
                    return;
                };
                let width = skia_surface.width();
                let height = skia_surface.height();

                let canvas = skia_surface.canvas();
                canvas.clear(Color::from_argb(255, 0, 0, 0));

                let qc = &mut self.qc;
                let repaint_after = egui_skia.run(&window, |ctx| {
                    qc.logic_inner(ctx);
                    // Top-level central panel against a bare Context; show_inside needs a Ui we don't have here.
                    #[allow(deprecated)]
                    egui::CentralPanel::default().show(ctx, |ui| qc.ui_inner(ui));
                });

                egui_skia.paint(canvas);
                present(skia_surface, softbuffer_surface, width, height);

                if crate::LAUNCH_TERMINAL.load(Ordering::Relaxed) {
                    event_loop.exit();
                    return;
                }

                // Cap the wait at one tick so logic_inner keeps sampling/MCP/heartbeat alive.
                let next = Instant::now() + TICK.min(repaint_after.max(Duration::ZERO));
                event_loop.set_control_flow(ControlFlow::WaitUntil(next));
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if crate::LAUNCH_TERMINAL.load(Ordering::Relaxed) {
            event_loop.exit();
            return;
        }
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }
}

fn present(
    skia_surface: &mut Surface,
    softbuffer_surface: &mut softbuffer::Surface<Rc<Window>, Rc<Window>>,
    width: i32,
    height: i32,
) {
    let (Some(w), Some(h)) = (NonZeroU32::new(width as u32), NonZeroU32::new(height as u32)) else {
        return;
    };
    softbuffer_surface.resize(w, h).unwrap();

    let info = ImageInfo::new((width, height), ColorType::RGBA8888, AlphaType::Premul, None);
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    let ok = skia_surface.read_pixels(&info, &mut rgba, (width * 4) as usize, (0, 0));
    if !ok {
        return;
    }

    let mut buffer = softbuffer_surface.buffer_mut().unwrap();
    for (dst, px) in buffer.iter_mut().zip(rgba.chunks_exact(4)) {
        *dst = ((px[0] as u32) << 16) | ((px[1] as u32) << 8) | (px[2] as u32);
    }
    buffer.present().unwrap();
}

/// Software-rendered (skia raster) fallback host for machines with no working GPU GL stack.
pub fn run() -> anyhow::Result<()> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + TICK));
    let mut app = SoftwareApp::new();
    event_loop.run_app(&mut app)?;
    Ok(())
}
