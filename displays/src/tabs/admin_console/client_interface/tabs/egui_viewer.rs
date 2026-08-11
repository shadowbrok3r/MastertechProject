use crate::plugins::remote::{
    apply_wire_textures_delta_for_viewer, decompress, wire_to_clipped_primitive_for_viewer,
    EguiFrameMessage, EguiInputEvent, EguiModifiers, WireClippedMesh, WireTextureId,
    WireTexturesDelta,
};
use crate::remote_viewer::input_focus::RemoteViewFocus;
use crossbeam::channel::{Receiver, Sender};
use eframe::egui::{self, Color32, Event, RichText, Stroke, Ui};
use std::collections::HashMap;
use crate::ui_tools::theme;

/// `RUST_LOG=egui_remote=debug` for pointer-move spam; `=error` for milestones only.
const EGUI_REMOTE_LOG: &str = "egui_remote";

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
    /// Owns egui focus while the canvas is armed, so Tab/Enter never reach the host UI.
    focus: RemoteViewFocus,
    /// Throttle `PointerMoved` error logs (still use debug each move when log level allows).
    remote_diag_tick: u32,
    /// Last host-space position we actually sent to the remote, for de-duplication.
    last_sent_host_pos: Option<(f32, f32)>,
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
            focus: RemoteViewFocus::new(),
            remote_diag_tick: 0,
            last_sent_host_pos: None,
        }
    }

    /// Remote content size in **points** from the last frame (`EguiFrameMessage::width` / `height`).
    pub fn remote_canvas_points(&self) -> Option<(f32, f32)> {
        self.latest_frame
            .as_ref()
            .map(|f| (f.width, f.height))
    }

    /// Returns `true` when a new frame was decoded (caller should request repaint).
    pub fn poll_frames(&mut self) -> bool {
        let mut newest: Option<EguiFrameMessage> = None;
        let mut skipped = 0u32;
        while let Ok(frame) = self.frame_rx.try_recv() {
            if newest.is_some() {
                skipped += 1;
            }
            newest = Some(frame);
        }
        if let Some(frame) = newest {
            if skipped > 0 {
                log::debug!(
                    target: EGUI_REMOTE_LOG,
                    "[admin_inline] poll_frames: decoded 1 frame, skipped {skipped} stale"
                );
            }
            self.decode_frame(&frame);
            self.latest_frame = Some(frame);
            self.has_received_frame = true;
            true
        } else {
            false
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
    /// When `mcp_pointer_session` is `Some(connection_string)`, draws the last MCP-injected pointer on the canvas (yellow reticle).
    pub fn ui(
        &mut self,
        ui: &mut Ui,
        mut send_input: impl FnMut(EguiInputEvent),
        mcp_pointer_session: Option<&str>,
    ) {
        let got_new_frame = self.poll_frames();

        let Some(frame) = &self.latest_frame else {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.label(
                    RichText::new("Waiting for egui frames...")
                        .color(theme::weak_text(ui))
                        .size(14.0),
                );
                ui.add_space(8.0);
                ui.label(
                    RichText::new(
                        "Enable Egui Frame Capture on the machine being viewed; keep Remote Viewer off there.",
                    )
                    .color(theme::faint_text(ui))
                    .small(),
                );
                ui.spinner();
            });
            return;
        };


        let width = frame.width.max(1.0);
        let height = frame.height.max(1.0);
        let remote_origin = egui::pos2(frame.screen_min_x, frame.screen_min_y);
        let screen_min_x = frame.screen_min_x;
        let screen_min_y = frame.screen_min_y;

        self.apply_pending_textures(ui.ctx());
        let inp = ui.ctx().input(|i| i.clone());
        self.remote_diag_tick = self.remote_diag_tick.wrapping_add(1);

        // let frame_count = frame.frame_count;

        // let ppp = frame.pixels_per_point;
        // let mesh_count = self.cached_meshes.len();
        // ui.horizontal(|ui| {
        //     ui.label(
        //         RichText::new(format!(
        //             "Frame #{frame_count} | {}x{} @{ppp:.1}x | {mesh_count} meshes",
        //             width as u32,
        //             height as u32,
        //         ))
        //         .color(Color32::from_rgb(140, 180, 140))
        //         .small(),
        //     );
        // });
        // ui.separator();

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

        // Focus before any forwarding: a Tab or Enter bound for the remote must not also move focus
        // or activate a widget in the host console behind this canvas.
        self.remote_kb_focus = self.focus.update(&response);

        let hovered = response.hovered();
        if self.remote_canvas_was_hovered && !hovered {
            log::debug!(
                target: EGUI_REMOTE_LOG,
                "[admin_inline] PointerLeave (was hovered, now not)"
            );
            send_input(EguiInputEvent::PointerLeave);
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

        if let Some(session) = mcp_pointer_session {
            if let Some((hx, hy)) =
                crate::plugins::remote_egui_control::hub().get_last_injected_pointer(session)
            {
                let hp = egui::pos2(hx, hy);
                let local =
                    canvas_rect.min + (hp - egui::pos2(screen_min_x, screen_min_y)) * scale;
                if canvas_rect.contains(local) {
                    let marker = theme::warn(ui);
                    painter.circle_filled(local, 5.0, marker.gamma_multiply(0.8));
                    // White ring: the crosshair sits over arbitrary remote pixels, so it needs one
                    // contrast edge the theme cannot take away.
                    painter.circle_stroke(local, 9.0, Stroke::new(1.2_f32, Color32::WHITE));
                    let arm = 12.0f32;
                    painter.line_segment(
                        [
                            local - egui::vec2(arm, 0.0),
                            local + egui::vec2(arm, 0.0),
                        ],
                        Stroke::new(1.0_f32, marker),
                    );
                    painter.line_segment(
                        [
                            local - egui::vec2(0.0, arm),
                            local + egui::vec2(0.0, arm),
                        ],
                        Stroke::new(1.0_f32, marker),
                    );
                }
            }
        }

        let canvas_min = canvas_rect.min;
        let to_host =
            |p: egui::Pos2| egui::pos2(screen_min_x, screen_min_y) + (p - canvas_min) / scale;

        if response.hovered() {
            match inp.pointer.hover_pos() {
                Some(pos) if canvas_rect.contains(pos) => {
                    let r = to_host(pos);
                    let moved_enough = match self.last_sent_host_pos {
                        Some((lx, ly)) => (r.x - lx).abs() >= 3.0 || (r.y - ly).abs() >= 3.0,
                        None => true,
                    };
                    if moved_enough {
                        self.last_sent_host_pos = Some((r.x, r.y));
                        log::debug!(
                            target: EGUI_REMOTE_LOG,
                            "[admin_inline] PointerMoved host=({:.1},{:.1})",
                            r.x,
                            r.y,
                        );
                        send_input(EguiInputEvent::PointerMoved { x: r.x, y: r.y });
                    }
                }
                Some(pos) => {
                    if self.remote_diag_tick % 60 == 0 {
                        log::debug!(
                            target: EGUI_REMOTE_LOG,
                            "[admin_inline] hovered but hover_pos outside canvas: pos={pos:?} canvas={canvas_rect:?}"
                        );
                    }
                }
                None => {}
            }
        }

        if inp.pointer.primary_pressed() {
            let ip = inp.pointer.interact_pos();
            let inside = ip.is_some_and(|p| canvas_rect.contains(p));
            log::debug!(
                target: EGUI_REMOTE_LOG,
                "[admin_inline] primary_pressed interact_pos={ip:?} canvas_contains={inside} canvas={canvas_rect:?}"
            );
            if let Some(pos) = ip {
                if canvas_rect.contains(pos) {
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
            let ip = inp.pointer.interact_pos();
            log::debug!(
                target: EGUI_REMOTE_LOG,
                "[admin_inline] primary_released interact_pos={ip:?}"
            );
            if let Some(pos) = ip {
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
            log::debug!(
                target: EGUI_REMOTE_LOG,
                "[admin_inline] Scroll delta=({:.2},{:.2}) (scaled to host points)",
                scroll.x / scale,
                scroll.y / scale
            );
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
                        log::debug!(
                            target: EGUI_REMOTE_LOG,
                            "[admin_inline] Key {:?} pressed={pressed} -> forwarding",
                            key.name()
                        );
                        send_input(EguiInputEvent::Key {
                            key_name: key.name().to_string(),
                            pressed: *pressed,
                            modifiers: EguiModifiers::from(*modifiers),
                        });
                    }
                    Event::Text(t) => {
                        log::debug!(
                            target: EGUI_REMOTE_LOG,
                            "[admin_inline] Text len={} -> forwarding",
                            t.len()
                        );
                        send_input(EguiInputEvent::Text(t.clone()));
                    }
                    _ => {}
                }
            }
        }

        self.focus.swallow_keys(ui);

        if got_new_frame {
            ui.ctx().request_repaint();
        } else {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(32));
        }
    }
}
