use crate::plugins::remote::{
    apply_wire_textures_delta_for_viewer, decompress, wire_to_clipped_primitive_for_viewer,
    EguiFrameMessage, EguiInputEvent, EguiModifiers, WireClippedMesh, WireTextureId,
    WireTexturesDelta,
};
use crossbeam::channel::{Receiver, Sender};
use eframe::egui::{self, Color32, Event, RichText, Ui};
use std::collections::HashMap;

/// Inline egui frame viewer for the admin console client interface.
///
/// Renders remote frames with **texture id remapping** (avoids clobbering the local font atlas)
/// and optional **input forwarding** via `send_input` (WebSocket to the remote client).
pub struct InlineEguiViewer {
    pub frame_tx: Sender<EguiFrameMessage>,
    frame_rx: Receiver<EguiFrameMessage>,
    latest_frame: Option<EguiFrameMessage>,
    cached_meshes: Vec<WireClippedMesh>,
    pending_textures: Option<WireTexturesDelta>,
    remote_tex_map: HashMap<WireTextureId, egui::TextureId>,
    pub has_received_frame: bool,
    /// For emitting [`EguiInputEvent::PointerLeave`] on hover end.
    remote_canvas_was_hovered: bool,
    /// Keyboard forwarding without calling [`egui::Response::has_focus`] (avoids nested `ctx` locks).
    remote_kb_focus: bool,
}

impl InlineEguiViewer {
    pub fn new() -> Self {
        let (frame_tx, frame_rx) = crossbeam::channel::bounded(4);
        Self {
            frame_tx,
            frame_rx,
            latest_frame: None,
            cached_meshes: Vec::new(),
            pending_textures: None,
            remote_tex_map: HashMap::new(),
            has_received_frame: false,
            remote_canvas_was_hovered: false,
            remote_kb_focus: false,
        }
    }

    /// Remote content size in **points** from the last frame (`EguiFrameMessage::width` / `height`).
    pub fn remote_canvas_points(&self) -> Option<(f32, f32)> {
        self.latest_frame
            .as_ref()
            .map(|f| (f.width, f.height))
    }

    pub fn poll_frames(&mut self) {
        while let Ok(frame) = self.frame_rx.try_recv() {
            self.decode_frame(&frame);
            self.latest_frame = Some(frame);
            self.has_received_frame = true;
        }
    }

    fn decode_frame(&mut self, frame: &EguiFrameMessage) {
        if let Ok(mesh_bytes) = decompress(&frame.meshes_data) {
            if let Ok((meshes, _)) =
                bincode::serde::decode_from_slice::<Vec<WireClippedMesh>, _>(
                    &mesh_bytes,
                    bincode::config::standard(),
                )
            {
                self.cached_meshes = meshes;
            }
        }

        if let Ok(tex_bytes) = decompress(&frame.textures_data) {
            if let Ok((delta, _)) =
                bincode::serde::decode_from_slice::<WireTexturesDelta, _>(
                    &tex_bytes,
                    bincode::config::standard(),
                )
            {
                self.pending_textures = Some(delta);
            }
        }
    }

    fn apply_pending_textures(&mut self, ctx: &egui::Context) {
        let Some(delta) = self.pending_textures.take() else {
            return;
        };
        apply_wire_textures_delta_for_viewer(ctx, &delta, &mut self.remote_tex_map);
    }

    /// Draw the remote frame. Pass `|_| {}` for `send_input` when not forwarding input.
    pub fn ui(&mut self, ui: &mut Ui, mut send_input: impl FnMut(EguiInputEvent)) {
        self.poll_frames();

        let Some(frame) = &self.latest_frame else {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.label(
                    RichText::new("Waiting for egui frames...")
                        .color(Color32::GRAY)
                        .size(14.0),
                );
                ui.add_space(8.0);
                ui.label(
                    RichText::new(
                        "Enable Egui Frame Capture on the machine being viewed; keep Remote Viewer off there.",
                    )
                    .color(Color32::from_rgb(120, 120, 140))
                    .small(),
                );
                ui.spinner();
            });
            return;
        };

        let frame_count = frame.frame_count;
        let width = frame.width.max(1.0);
        let height = frame.height.max(1.0);
        let ppp = frame.pixels_per_point;
        let mesh_count = self.cached_meshes.len();
        let remote_origin = egui::pos2(frame.screen_min_x, frame.screen_min_y);
        let screen_min_x = frame.screen_min_x;
        let screen_min_y = frame.screen_min_y;

        self.apply_pending_textures(ui.ctx());
        let inp = ui.ctx().input(|i| i.clone());

        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!(
                    "Frame #{frame_count} | {}x{} @{ppp:.1}x | {mesh_count} meshes",
                    width as u32,
                    height as u32,
                ))
                .color(Color32::from_rgb(140, 180, 140))
                .small(),
            );
        });
        ui.separator();

        let max_w = ui.available_width();
        let max_h = ui.available_height().max(120.0);
        let scale = (max_w / width).min(max_h / height).min(1.0e6).max(1.0e-6);
        let draw_w = width * scale;
        let draw_h = height * scale;
        let canvas_rect = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(draw_w, draw_h));
        let response = ui.allocate_rect(
            canvas_rect,
            egui::Sense::click_and_drag().union(egui::Sense::hover()),
        );

        let hovered = response.hovered();
        if self.remote_canvas_was_hovered && !hovered {
            send_input(EguiInputEvent::PointerLeave);
            self.remote_kb_focus = false;
        }
        self.remote_canvas_was_hovered = hovered;

        let meshes = self.cached_meshes.clone();
        let tex_map = &self.remote_tex_map;
        let painter = ui.painter();
        for wire_mesh in &meshes {
            let Some(prim) = wire_to_clipped_primitive_for_viewer(
                wire_mesh,
                remote_origin,
                canvas_rect.min,
                scale,
                tex_map,
            ) else {
                continue;
            };
            if let egui::epaint::Primitive::Mesh(mesh) = prim.primitive {
                let clip = prim.clip_rect.intersect(canvas_rect);
                if clip.width() > 0.0 && clip.height() > 0.0 {
                    painter.with_clip_rect(clip).add(egui::Shape::mesh(mesh));
                }
            }
        }

        let canvas_min = canvas_rect.min;
        let to_host =
            |p: egui::Pos2| egui::pos2(screen_min_x, screen_min_y) + (p - canvas_min) / scale;

        if response.hovered() {
            if let Some(pos) = inp.pointer.hover_pos() {
                if canvas_rect.contains(pos) {
                    let r = to_host(pos);
                    send_input(EguiInputEvent::PointerMoved { x: r.x, y: r.y });
                }
            }
        }

        if inp.pointer.primary_pressed() {
            if let Some(pos) = inp.pointer.interact_pos() {
                if canvas_rect.contains(pos) {
                    self.remote_kb_focus = true;
                    let r = to_host(pos);
                    send_input(EguiInputEvent::PointerButton {
                        x: r.x,
                        y: r.y,
                        button: 0,
                        pressed: true,
                    });
                }
            }
        }
        if inp.pointer.primary_released() {
            if let Some(pos) = inp.pointer.interact_pos() {
                let r = to_host(pos);
                send_input(EguiInputEvent::PointerButton {
                    x: r.x,
                    y: r.y,
                    button: 0,
                    pressed: false,
                });
            }
        }

        let scroll = inp.smooth_scroll_delta;
        if scroll != egui::Vec2::ZERO && response.hovered() {
            send_input(EguiInputEvent::Scroll {
                delta_x: scroll.x / scale,
                delta_y: scroll.y / scale,
            });
        }

        if self.remote_kb_focus {
            for event in &inp.events {
                match event {
                    Event::Key {
                        key,
                        pressed,
                        modifiers,
                        ..
                    } => {
                        send_input(EguiInputEvent::Key {
                            key_name: key.name().to_string(),
                            pressed: *pressed,
                            modifiers: EguiModifiers::from(*modifiers),
                        });
                    }
                    Event::Text(t) => {
                        send_input(EguiInputEvent::Text(t.clone()));
                    }
                    _ => {}
                }
            }
        }

        ui.ctx().request_repaint();
    }
}
