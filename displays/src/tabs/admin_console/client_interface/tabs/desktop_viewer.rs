use crate::remote_desktop::{
    DesktopFrameEncoding, DesktopFrameMessage, DesktopInputEvent, DesktopModifiers,
    DesktopMouseButton,
};
use crossbeam::channel::{Receiver, Sender};
use eframe::egui::{
    self, Color32, ColorImage, Event, PointerButton, Rect, RichText, TextureHandle, TextureOptions,
    Ui,
};
use web_time::{Instant, SystemTime};

/// Inline raster viewer for full remote-desktop control. Decodes JPEG frames
/// into an egui texture and forwards pointer/keyboard input as normalized
/// [`DesktopInputEvent`]s (the caller tags and sends them to the client).
pub struct DesktopViewer {
    pub frame_tx: Sender<DesktopFrameMessage>,
    frame_rx: Receiver<DesktopFrameMessage>,
    texture: Option<TextureHandle>,
    frame_size: [usize; 2],
    pub has_received_frame: bool,
    last_sent_pos: Option<(f32, f32)>,
    canvas_hovered: bool,
    kb_focus: bool,
    pub frames_shown: u64,
    pub last_latency_ms: u128,
    pub last_encode_ms: u32,
    pub last_decode_ms: u128,
    pub last_frame_bytes: usize,
}

/// Whether an unmodified press of `key` emits a companion `Event::Text`.
fn key_produces_text(key: egui::Key) -> bool {
    use egui::Key as K;
    !matches!(
        key,
        K::ArrowDown
            | K::ArrowLeft
            | K::ArrowRight
            | K::ArrowUp
            | K::Escape
            | K::Tab
            | K::Backspace
            | K::Enter
            | K::Insert
            | K::Delete
            | K::Home
            | K::End
            | K::PageUp
            | K::PageDown
            | K::F1
            | K::F2
            | K::F3
            | K::F4
            | K::F5
            | K::F6
            | K::F7
            | K::F8
            | K::F9
            | K::F10
            | K::F11
            | K::F12
    )
}

impl DesktopViewer {
    pub fn new() -> Self {
        let (frame_tx, frame_rx) = crossbeam::channel::bounded(4);
        Self {
            frame_tx,
            frame_rx,
            texture: None,
            frame_size: [0, 0],
            has_received_frame: false,
            last_sent_pos: None,
            canvas_hovered: false,
            kb_focus: false,
            frames_shown: 0,
            last_latency_ms: 0,
            last_encode_ms: 0,
            last_decode_ms: 0,
            last_frame_bytes: 0,
        }
    }

    /// Returns `true` when a new frame was uploaded (caller should repaint).
    pub fn poll_frames(&mut self, ctx: &egui::Context) -> bool {
        let mut newest: Option<DesktopFrameMessage> = None;
        while let Ok(frame) = self.frame_rx.try_recv() {
            newest = Some(frame);
        }
        if let Some(frame) = newest {
            self.upload(ctx, &frame);
            self.has_received_frame = true;
            true
        } else {
            false
        }
    }

    fn upload(&mut self, ctx: &egui::Context, frame: &DesktopFrameMessage) {
        let decode_start = Instant::now();
        let color = match frame.encoding {
            DesktopFrameEncoding::Jpeg => {
                match image::load_from_memory_with_format(&frame.data, image::ImageFormat::Jpeg) {
                    Ok(img) => {
                        let rgba = img.to_rgba8();
                        let size = [rgba.width() as usize, rgba.height() as usize];
                        ColorImage::from_rgba_unmultiplied(size, rgba.as_raw())
                    }
                    Err(e) => {
                        log::warn!(target: "remote_desktop", "jpeg decode failed: {e}");
                        return;
                    }
                }
            }
            DesktopFrameEncoding::Rgba => {
                let size = [frame.width as usize, frame.height as usize];
                if frame.data.len() < size[0] * size[1] * 4 {
                    log::warn!(target: "remote_desktop", "rgba frame smaller than declared size");
                    return;
                }
                ColorImage::from_rgba_unmultiplied(size, &frame.data)
            }
        };

        self.frame_size = color.size;
        match &mut self.texture {
            Some(handle) => handle.set(color, TextureOptions::default()),
            None => {
                self.texture =
                    Some(ctx.load_texture("remote_desktop", color, TextureOptions::default()))
            }
        }

        self.frames_shown = self.frames_shown.wrapping_add(1);
        self.last_encode_ms = frame.encode_ms;
        self.last_frame_bytes = frame.data.len();
        self.last_decode_ms = decode_start.elapsed().as_millis();
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        self.last_latency_ms = now.saturating_sub(frame.timestamp_ms);
    }

    /// Paint the latest frame and forward input via `send_input`.
    pub fn ui(&mut self, ui: &mut Ui, mut send_input: impl FnMut(DesktopInputEvent)) {
        let got_new = self.poll_frames(ui.ctx());

        let Some(texture) = self.texture.clone() else {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.label(
                    RichText::new("Waiting for desktop frames...")
                        .color(Color32::GRAY)
                        .size(14.0),
                );
                ui.add_space(8.0);
                ui.label(
                    RichText::new("Start Remote Desktop from the menu to begin streaming.")
                        .color(Color32::from_rgb(120, 120, 140))
                        .small(),
                );
                ui.spinner();
            });
            return;
        };

        let [pw, ph] = self.frame_size;
        let width = pw.max(1) as f32;
        let height = ph.max(1) as f32;
        let max_w = ui.available_width();
        let max_h = ui.available_height().max(120.0);
        let scale = (max_w / width).min(max_h / height).min(1.0e6).max(1.0e-6);
        let draw = egui::vec2(width * scale, height * scale);
        let canvas_rect = Rect::from_min_size(ui.cursor().min, draw);
        let response = ui.allocate_rect(
            canvas_rect,
            egui::Sense::click_and_drag().union(egui::Sense::hover()),
        );

        ui.painter().image(
            texture.id(),
            canvas_rect,
            Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );

        let to_norm = |p: egui::Pos2| -> (f32, f32) {
            let rel = p - canvas_rect.min;
            ((rel.x / draw.x).clamp(0.0, 1.0), (rel.y / draw.y).clamp(0.0, 1.0))
        };

        let hovered = response.hovered();
        self.canvas_hovered = hovered;
        let inp = ui.ctx().input(|i| i.clone());

        if hovered {
            if let Some(pos) = inp.pointer.hover_pos() {
                if canvas_rect.contains(pos) {
                    let (nx, ny) = to_norm(pos);
                    let moved = match self.last_sent_pos {
                        Some((lx, ly)) => (nx - lx).abs() >= 0.002 || (ny - ly).abs() >= 0.002,
                        None => true,
                    };
                    if moved {
                        self.last_sent_pos = Some((nx, ny));
                        send_input(DesktopInputEvent::MouseMove { x: nx, y: ny });
                    }
                }
            }
        }

        for event in &inp.events {
            match event {
                Event::PointerButton { pos, button, pressed, .. } => {
                    let dm = match button {
                        PointerButton::Primary => DesktopMouseButton::Left,
                        PointerButton::Secondary => DesktopMouseButton::Right,
                        PointerButton::Middle => DesktopMouseButton::Middle,
                        _ => continue,
                    };
                    if *pressed {
                        if !canvas_rect.contains(*pos) {
                            self.kb_focus = false;
                            continue;
                        }
                        self.kb_focus = true;
                    }
                    let (nx, ny) = to_norm(*pos);
                    send_input(DesktopInputEvent::MouseButton {
                        x: nx,
                        y: ny,
                        button: dm,
                        pressed: *pressed,
                    });
                }
                Event::Key { key, pressed, modifiers, .. } if self.kb_focus => {
                    // Printable presses also arrive as `Event::Text`, which carries the
                    // typing; forwarding both injects each character twice. Presses go
                    // through only for chords and non-text keys; releases always go
                    // through so a chorded press can't leave a key held.
                    let chorded = modifiers.ctrl || modifiers.alt || modifiers.command || modifiers.mac_cmd;
                    if !*pressed || chorded || !key_produces_text(*key) {
                        send_input(DesktopInputEvent::Key {
                            key_name: key.name().to_string(),
                            pressed: *pressed,
                            modifiers: DesktopModifiers {
                                ctrl: modifiers.ctrl,
                                shift: modifiers.shift,
                                alt: modifiers.alt,
                                meta: modifiers.mac_cmd,
                            },
                        });
                    }
                }
                Event::Text(t) if self.kb_focus => {
                    send_input(DesktopInputEvent::Text(t.clone()));
                }
                _ => {}
            }
        }

        let scroll = inp.smooth_scroll_delta;
        if scroll != egui::Vec2::ZERO && hovered {
            send_input(DesktopInputEvent::MouseScroll {
                delta_x: scroll.x,
                delta_y: scroll.y,
            });
        }

        if got_new {
            ui.ctx().request_repaint();
        } else {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(32));
        }
    }
}
