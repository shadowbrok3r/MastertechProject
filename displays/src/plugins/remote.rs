//! Egui-to-egui remote viewing types and plugin infrastructure.
//!
//! Analogous to the existing `remote_viewer` module (which handles ratatui Buffer serialization
//! for terminal-mode remote viewing), this module defines the types for capturing and replaying
//! egui frames across a network.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────┐          WebSocket           ┌─────────────────────┐
//! │   Host Mastertech   │  ──── EguiFrameMessage ───>  │  Viewer Mastertech  │
//! │                     │                              │                     │
//! │  EguiFrameCapture   │  <─── EguiInputEvent ─────  │  EguiRemoteViewer   │
//! │  (output_hook)      │          (input fwd)         │  (renders frames)   │
//! └─────────────────────┘                              └─────────────────────┘
//! ```
//!
//! Both `EguiFrameCapture` and `EguiRemoteViewer` implement `MastertechPlugin`,
//! so they integrate into the plugin manager lifecycle automatically.
//!
//! ## Serialization Strategy
//!
//! The capture side **tessellates** shapes into `ClippedPrimitive` (meshes), then
//! converts to wire-safe `WireMesh` types and compresses with zstd. The viewer
//! reconstructs `Mesh` objects and paints them directly. This avoids needing to
//! serialize complex types like `Galley` or `Shape::Callback`.

use crossbeam::channel::{Receiver, Sender};
use eframe::egui;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::Mutex;

use super::{MastertechPlugin, PluginHost};

// ─── Wire types ────────────────────────────────────────────────────────────────

/// Named rectangle in **host / capture screen space** (same coordinates as [`EguiInputEvent`] pointer).
/// Registered during UI via [`push_widget_anchor`], attached to the next [`EguiFrameMessage`].
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WidgetAnchor {
    pub key: String,
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
}

impl WidgetAnchor {
    pub fn center(&self) -> (f32, f32) {
        (
            (self.min_x + self.max_x) * 0.5,
            (self.min_y + self.max_y) * 0.5,
        )
    }

    pub fn top_left(&self) -> (f32, f32) {
        (self.min_x, self.min_y)
    }
}

/// Buffer filled during UI; drained into the outgoing frame in [`EguiFrameCapture::output_hook`].
static WIDGET_ANCHORS_BUF: Mutex<Vec<WidgetAnchor>> = Mutex::new(Vec::new());

/// Record a widget rectangle for the current frame (call right after building a `TextEdit`, `Button`, etc.).
/// Keys should be stable dotted paths, e.g. `tur.service_number`, for MCP `remote_egui_click_anchor`.
pub fn push_widget_anchor(key: impl Into<String>, rect: egui::Rect) {
    let mut g = WIDGET_ANCHORS_BUF.lock().unwrap_or_else(|e| e.into_inner());
    g.push(WidgetAnchor {
        key: key.into(),
        min_x: rect.min.x,
        min_y: rect.min.y,
        max_x: rect.max.x,
        max_y: rect.max.y,
    });
}

/// A captured egui frame for transmission to a remote viewer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EguiFrameMessage {
    pub frame_count: u64,
    pub timestamp_ms: u128,
    /// Serialized + zstd-compressed mesh data.
    pub meshes_data: Vec<u8>,
    /// Serialized + zstd-compressed texture delta.
    pub textures_data: Vec<u8>,
    pub width: f32,
    pub height: f32,
    pub pixels_per_point: f32,
    /// `Context::screen_rect().min` on the capture side (for coordinate remap).
    pub screen_min_x: f32,
    pub screen_min_y: f32,
    /// Widgets that called [`push_widget_anchor`] during this frame (may be empty).
    #[serde(default)]
    pub widget_anchors: Vec<WidgetAnchor>,
}

/// A tessellated, clipped mesh ready for wire transmission.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireClippedMesh {
    pub clip_rect: [f32; 4],
    pub mesh: WireMesh,
}

/// Mesh data in a serde-friendly format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireMesh {
    pub indices: Vec<u32>,
    pub vertices: Vec<WireVertex>,
    pub texture_id: WireTextureId,
}

/// Vertex data matching `egui::epaint::Vertex`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct WireVertex {
    pub pos: [f32; 2],
    pub uv: [f32; 2],
    pub color: [u8; 4],
}

/// Texture identifier matching `egui::TextureId`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Hash, PartialEq, Eq)]
pub enum WireTextureId {
    Managed(u64),
    User(u64),
}

/// Texture updates for the viewer to apply.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireTexturesDelta {
    pub set: Vec<(WireTextureId, WireImageDelta)>,
    pub free: Vec<WireTextureId>,
}

/// Image data for a texture upload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireImageDelta {
    pub pixels_rgba: Vec<u8>,
    pub width: usize,
    pub height: usize,
    pub pos: Option<[usize; 2]>,
}

// ─── Conversion helpers ────────────────────────────────────────────────────────

impl From<egui::TextureId> for WireTextureId {
    fn from(id: egui::TextureId) -> Self {
        match id {
            egui::TextureId::Managed(n) => WireTextureId::Managed(n),
            egui::TextureId::User(n) => WireTextureId::User(n),
        }
    }
}

impl From<WireTextureId> for egui::TextureId {
    fn from(id: WireTextureId) -> Self {
        match id {
            WireTextureId::Managed(n) => egui::TextureId::Managed(n),
            WireTextureId::User(n) => egui::TextureId::User(n),
        }
    }
}

/// Apply a remote `TexturesDelta` on the **viewer** using [`TextureManager::alloc`] / [`TextureManager::set`].
///
/// egui 0.34+ only allows `set`/`free` on textures that were created with `alloc` in this manager.
/// Arbitrary `TextureId::User` values are not tracked and will panic. We map each `WireTextureId`
/// to a freshly allocated local `TextureId::Managed`.
pub fn apply_wire_textures_delta_for_viewer(
    ctx: &egui::Context,
    delta: &WireTexturesDelta,
    map: &mut HashMap<WireTextureId, egui::TextureId>,
) {
    let tex_mgr = ctx.tex_manager();
    let mut tex_write = tex_mgr.write();

    for wire_id in &delta.free {
        if let Some(id) = map.remove(wire_id) {
            tex_write.free(id);
        }
    }

    for (wire_id, wire_img) in &delta.set {
        let pixels: Vec<egui::Color32> = wire_img
            .pixels_rgba
            .chunks_exact(4)
            .map(|c| egui::Color32::from_rgba_premultiplied(c[0], c[1], c[2], c[3]))
            .collect();

        let color_image = egui::ColorImage {
            size: [wire_img.width, wire_img.height],
            source_size: egui::Vec2::new(wire_img.width as f32, wire_img.height as f32),
            pixels,
        };
        let image_data = egui::ImageData::Color(std::sync::Arc::new(color_image));
        let options = egui::TextureOptions::default();
        let image_delta = if let Some(pos) = wire_img.pos {
            egui::epaint::ImageDelta::partial(pos, image_data, options)
        } else {
            egui::epaint::ImageDelta::full(image_data, options)
        };

        match map.entry(*wire_id) {
            Entry::Occupied(entry) => {
                tex_write.set(*entry.get(), image_delta);
            }
            Entry::Vacant(vacant) => {
                if wire_img.pos.is_some() {
                    log::debug!(
                        "Remote viewer: partial texture update before first full alloc for {wire_id:?}"
                    );
                    continue;
                }
                let id = tex_write.alloc(
                    format!("mastertech_remote_{wire_id:?}").into(),
                    image_delta.image.clone(),
                    image_delta.options,
                );
                vacant.insert(id);
            }
        }
    }
}

pub fn wire_to_clipped_primitive_for_viewer(
    wire: &WireClippedMesh,
    remote_origin: egui::Pos2,
    canvas_min: egui::Pos2,
    scale: f32,
    tex_map: &HashMap<WireTextureId, egui::TextureId>,
) -> Option<egui::ClippedPrimitive> {
    let tex_id = *tex_map.get(&wire.mesh.texture_id)?;
    let map_pt = |x: f32, y: f32| -> egui::Pos2 {
        let p = egui::pos2(x, y);
        canvas_min + (p - remote_origin) * scale
    };
    let mesh = egui::epaint::Mesh {
        indices: wire.mesh.indices.clone(),
        vertices: wire
            .mesh
            .vertices
            .iter()
            .map(|v| egui::epaint::Vertex {
                pos: map_pt(v.pos[0], v.pos[1]),
                uv: egui::pos2(v.uv[0], v.uv[1]),
                color: egui::Color32::from_rgba_premultiplied(
                    v.color[0], v.color[1], v.color[2], v.color[3],
                ),
            })
            .collect(),
        texture_id: tex_id,
    };
    let cmin = map_pt(wire.clip_rect[0], wire.clip_rect[1]);
    let cmax = map_pt(wire.clip_rect[2], wire.clip_rect[3]);
    Some(egui::ClippedPrimitive {
        clip_rect: egui::Rect::from_min_max(cmin, cmax),
        primitive: egui::epaint::Primitive::Mesh(mesh),
    })
}

impl From<&egui::epaint::Vertex> for WireVertex {
    fn from(v: &egui::epaint::Vertex) -> Self {
        Self {
            pos: [v.pos.x, v.pos.y],
            uv: [v.uv.x, v.uv.y],
            color: v.color.to_array(),
        }
    }
}

impl From<WireVertex> for egui::epaint::Vertex {
    fn from(v: WireVertex) -> Self {
        Self {
            pos: egui::pos2(v.pos[0], v.pos[1]),
            uv: egui::pos2(v.uv[0], v.uv[1]),
            color: egui::Color32::from_rgba_premultiplied(
                v.color[0], v.color[1], v.color[2], v.color[3],
            ),
        }
    }
}

fn clipped_primitive_to_wire(prim: &egui::ClippedPrimitive) -> Option<WireClippedMesh> {
    let egui::epaint::Primitive::Mesh(ref mesh) = prim.primitive else {
        return None;
    };
    Some(WireClippedMesh {
        clip_rect: [
            prim.clip_rect.min.x,
            prim.clip_rect.min.y,
            prim.clip_rect.max.x,
            prim.clip_rect.max.y,
        ],
        mesh: WireMesh {
            indices: mesh.indices.clone(),
            vertices: mesh.vertices.iter().map(WireVertex::from).collect(),
            texture_id: mesh.texture_id.into(),
        },
    })
}

pub fn wire_to_clipped_primitive(wire: &WireClippedMesh) -> egui::ClippedPrimitive {
    let mesh = egui::epaint::Mesh {
        indices: wire.mesh.indices.clone(),
        vertices: wire.mesh.vertices.iter().copied().map(egui::epaint::Vertex::from).collect(),
        texture_id: wire.mesh.texture_id.into(),
    };
    egui::ClippedPrimitive {
        clip_rect: egui::Rect::from_min_max(
            egui::pos2(wire.clip_rect[0], wire.clip_rect[1]),
            egui::pos2(wire.clip_rect[2], wire.clip_rect[3]),
        ),
        primitive: egui::epaint::Primitive::Mesh(mesh),
    }
}

fn textures_delta_to_wire(delta: &egui::TexturesDelta) -> WireTexturesDelta {
    WireTexturesDelta {
        set: delta
            .set
            .iter()
            .map(|(id, img_delta)| {
                let rgba = image_data_to_rgba(&img_delta.image);
                let [w, h] = img_delta.image.size();
                (
                    (*id).into(),
                    WireImageDelta {
                        pixels_rgba: rgba,
                        width: w,
                        height: h,
                        pos: img_delta.pos,
                    },
                )
            })
            .collect(),
        free: delta.free.iter().map(|id| (*id).into()).collect(),
    }
}

/// Font / default atlas uses `TextureId::default()` (`Managed(0)`). `textures_delta` usually
/// carries only **partial** updates for it; remote viewers drop partials until a full alloc exists,
/// so late joiners never map `Managed(0)` and almost all meshes disappear.
fn merge_full_default_font_texture_for_remote(ctx: &egui::Context, wire_tex: &mut WireTexturesDelta) {
    const ATLAS: WireTextureId = WireTextureId::Managed(0);
    wire_tex.set.retain(|(id, _)| *id != ATLAS);
    let font_img = ctx.fonts(|f| f.image());
    let [w, h] = font_img.size;
    let pixels_rgba: Vec<u8> = font_img.pixels.iter().flat_map(|c| c.to_array()).collect();
    wire_tex.set.push((
        ATLAS,
        WireImageDelta {
            pixels_rgba,
            width: w,
            height: h,
            pos: None,
        },
    ));
}

fn image_data_to_rgba(image: &egui::ImageData) -> Vec<u8> {
    match image {
        egui::ImageData::Color(color_img) => {
            color_img.pixels.iter().flat_map(|c| c.to_array()).collect()
        }
    }
}

pub fn compress(data: &[u8]) -> Vec<u8> {
    zstd::encode_all(data, 3).unwrap_or_else(|_| data.to_vec())
}

pub fn decompress(data: &[u8]) -> Result<Vec<u8>, String> {
    if data.is_empty() {
        return Ok(Vec::new());
    }
    zstd::decode_all(data).map_err(|e| format!("zstd decompress: {e}"))
}

// ─── Input events ──────────────────────────────────────────────────────────────

/// Input events forwarded from the viewer to the host for injection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EguiInputEvent {
    PointerMoved { x: f32, y: f32 },
    PointerButton { x: f32, y: f32, button: u8, pressed: bool },
    /// Cursor left the remote canvas on the viewer; release host pointer override.
    PointerLeave,
    Key { key_name: String, pressed: bool, modifiers: EguiModifiers },
    Text(String),
    Scroll { delta_x: f32, delta_y: f32 },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct EguiModifiers {
    pub alt: bool,
    pub ctrl: bool,
    pub shift: bool,
    pub command: bool,
}

impl From<&EguiModifiers> for egui::Modifiers {
    fn from(m: &EguiModifiers) -> Self {
        Self {
            alt: m.alt,
            ctrl: m.ctrl,
            shift: m.shift,
            mac_cmd: m.command,
            command: m.command,
        }
    }
}

impl From<egui::Modifiers> for EguiModifiers {
    fn from(m: egui::Modifiers) -> Self {
        Self {
            alt: m.alt,
            ctrl: m.ctrl,
            shift: m.shift,
            command: m.command || m.mac_cmd,
        }
    }
}

// ─── Frame Capture Plugin ──────────────────────────────────────────────────────

/// Max remote [`EguiInputEvent`]s applied in a single [`egui::RawInput`] pass.
///
/// If we enqueue an entire MCP sequence (many clicks + `Text` events) into one frame, egui can
/// process all `Event::Text` against the **pre-frame** focused widget (often a multiline field),
/// so only the last-focused field appears to update. One click sequence is hover + press + release
/// (3) plus one `Text` (1) = 4 events per field.
const MAX_REMOTE_EGUI_EVENTS_PER_FRAME: usize = 4;

/// Captures egui output each frame, tessellates to meshes, serializes + compresses,
/// and sends via a channel. The transport layer (WebSocket) consumes `frame_rx`.
pub struct EguiFrameCapture {
    enabled: bool,
    frame_count: u64,
    ctx: Option<egui::Context>,
    /// Latest pointer in host space from the remote viewer; replayed every `input_hook` so
    /// multipass frames (and native winit moves) do not drop the injected position.
    remote_pointer_pos: Option<egui::Pos2>,
    pub frame_tx: Sender<EguiFrameMessage>,
    pub frame_rx: Receiver<EguiFrameMessage>,
    pub input_tx: Sender<EguiInputEvent>,
    pub input_rx: Receiver<EguiInputEvent>,
    last_capture_time: std::time::Instant,
}

impl EguiFrameCapture {
    pub fn new() -> Self {
        let (frame_tx, frame_rx) = crossbeam::channel::bounded(2);
        let (input_tx, input_rx) = crossbeam::channel::unbounded();
        Self {
            enabled: false,
            frame_count: 0,
            ctx: None,
            remote_pointer_pos: None,
            frame_tx,
            frame_rx,
            input_tx,
            input_rx,
            last_capture_time: std::time::Instant::now(),
        }
    }
}

impl Default for EguiFrameCapture {
    fn default() -> Self {
        Self::new()
    }
}

impl MastertechPlugin for EguiFrameCapture {
    fn id(&self) -> &'static str {
        "com.mastertech.egui-frame-capture"
    }

    fn name(&self) -> &str {
        "Egui Frame Capture"
    }

    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

    fn description(&self) -> &str {
        "Captures egui output for remote egui-to-egui viewing"
    }

    fn enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    fn on_load(&mut self, host: &PluginHost) {
        self.ctx = host.ctx.clone();
    }

    fn input_hook(&mut self, input: &mut egui::RawInput) {
        if !self.enabled {
            return;
        }
        let mut drained = 0u32;
        let mut backlog = false;
        for _ in 0..MAX_REMOTE_EGUI_EVENTS_PER_FRAME {
            let event = match self.input_rx.try_recv() {
                Ok(e) => e,
                Err(_) => break,
            };
            drained += 1;
            log::debug!(target: "egui_remote", "[host_capture] inject from channel: {event:?}");
            match event {
                EguiInputEvent::PointerMoved { x, y } => {
                    let p = egui::pos2(x, y);
                    self.remote_pointer_pos = Some(p);
                    input.events.push(egui::Event::PointerMoved(p));
                }
                EguiInputEvent::PointerButton { x, y, button, pressed } => {
                    let btn = match button {
                        0 => egui::PointerButton::Primary,
                        1 => egui::PointerButton::Secondary,
                        2 => egui::PointerButton::Middle,
                        3 => egui::PointerButton::Extra1,
                        _ => egui::PointerButton::Extra2,
                    };
                    input.events.push(egui::Event::PointerButton {
                        pos: egui::pos2(x, y),
                        button: btn,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    });
                }
                EguiInputEvent::PointerLeave => {
                    self.remote_pointer_pos = None;
                    input.events.push(egui::Event::PointerGone);
                }
                EguiInputEvent::Key { key_name, pressed, modifiers } => {
                    if let Some(key) = egui::Key::from_name(&key_name) {
                        input.events.push(egui::Event::Key {
                            key,
                            physical_key: None,
                            pressed,
                            repeat: false,
                            modifiers: (&modifiers).into(),
                        });
                    }
                }
                EguiInputEvent::Text(text) => {
                    input.events.push(egui::Event::Text(text));
                }
                EguiInputEvent::Scroll { delta_x, delta_y } => {
                    input.events.push(egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(delta_x, delta_y),
                        phase: egui::TouchPhase::Move,
                        modifiers: egui::Modifiers::NONE,
                    });
                }
            }
        }
        if !self.input_rx.is_empty() {
            backlog = true;
        }
        // Also drain the process-wide channel (used by the TCP transport path
        // which doesn't have direct access to this plugin's `input_tx`).
        for event in super::drain_egui_inputs() {
            drained += 1;
            // Reuse the same match arm logic — just re-push through the same match.
            match event {
                EguiInputEvent::PointerMoved { x, y } => {
                    let p = egui::pos2(x, y);
                    self.remote_pointer_pos = Some(p);
                    input.events.push(egui::Event::PointerMoved(p));
                }
                EguiInputEvent::PointerButton { x, y, button, pressed } => {
                    let btn = match button {
                        0 => egui::PointerButton::Primary,
                        1 => egui::PointerButton::Secondary,
                        2 => egui::PointerButton::Middle,
                        3 => egui::PointerButton::Extra1,
                        _ => egui::PointerButton::Extra2,
                    };
                    input.events.push(egui::Event::PointerButton {
                        pos: egui::pos2(x, y),
                        button: btn,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    });
                }
                EguiInputEvent::PointerLeave => {
                    self.remote_pointer_pos = None;
                    input.events.push(egui::Event::PointerGone);
                }
                EguiInputEvent::Key { key_name, pressed, modifiers } => {
                    if let Some(key) = egui::Key::from_name(&key_name) {
                        input.events.push(egui::Event::Key {
                            key,
                            physical_key: None,
                            pressed,
                            repeat: false,
                            modifiers: egui::Modifiers {
                                alt: modifiers.alt,
                                ctrl: modifiers.ctrl,
                                shift: modifiers.shift,
                                mac_cmd: false,
                                command: modifiers.ctrl,
                            },
                        });
                    }
                }
                EguiInputEvent::Text(text) => {
                    input.events.push(egui::Event::Text(text));
                }
                EguiInputEvent::Scroll { delta_x, delta_y } => {
                    input.events.push(egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(delta_x, delta_y),
                        phase: egui::TouchPhase::Move,
                        modifiers: egui::Modifiers::NONE,
                    });
                }
            }
        }
        if drained > 0 {
            log::debug!(
                target: "egui_remote",
                "[host_capture] input_hook drained {drained} remote event(s) from channel (cap {} per frame; backlog={backlog})",
                MAX_REMOTE_EGUI_EVENTS_PER_FRAME
            );
        }
        if backlog {
            if let Some(ctx) = &self.ctx {
                ctx.request_repaint();
            }
        }
        if let Some(p) = self.remote_pointer_pos {
            log::debug!(
                target: "egui_remote",
                "[host_capture] replay PointerMoved after channel drain: ({:.1},{:.1})",
                p.x,
                p.y
            );
            input.events.push(egui::Event::PointerMoved(p));
        }
    }

    fn output_hook(&mut self, output: &mut egui::FullOutput) {
        if !self.enabled {
            return;
        }
        let Some(ctx) = &self.ctx else { return };

        const MIN_CAPTURE_INTERVAL: std::time::Duration = std::time::Duration::from_millis(66);
        if self.last_capture_time.elapsed() < MIN_CAPTURE_INTERVAL {
            return;
        }
        self.last_capture_time = std::time::Instant::now();

        self.frame_count += 1;

        let ppp = ctx.pixels_per_point();
        let screen_rect = ctx.screen_rect();

        // Tessellate synchronously — requires `ctx` and happens quickly.
        let shapes = output.shapes.clone();
        let primitives = ctx.tessellate(shapes, ppp);
        let wire_meshes: Vec<WireClippedMesh> = primitives
            .iter()
            .filter_map(clipped_primitive_to_wire)
            .collect();

        let mut wire_tex = textures_delta_to_wire(&output.textures_delta);
        merge_full_default_font_texture_for_remote(ctx, &mut wire_tex);

        let widget_anchors = {
            let mut g = WIDGET_ANCHORS_BUF.lock().unwrap_or_else(|e| e.into_inner());
            std::mem::take(&mut *g)
        };

        let frame_count = self.frame_count;
        let frame_tx = self.frame_tx.clone();
        let width = screen_rect.width();
        let height = screen_rect.height();
        let screen_min_x = screen_rect.min.x;
        let screen_min_y = screen_rect.min.y;
        let timestamp_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();

        let build_and_send = move || {
            let meshes_bytes =
                bincode::serde::encode_to_vec(&wire_meshes, bincode::config::standard())
                    .unwrap_or_default();
            let tex_bytes =
                bincode::serde::encode_to_vec(&wire_tex, bincode::config::standard())
                    .unwrap_or_default();
            let msg = EguiFrameMessage {
                frame_count,
                timestamp_ms,
                meshes_data: compress(&meshes_bytes),
                textures_data: compress(&tex_bytes),
                width,
                height,
                pixels_per_point: ppp,
                screen_min_x,
                screen_min_y,
                widget_anchors,
            };
            let _ = frame_tx.try_send(msg);
        };

        // Offload bincode serialization + zstd compression to a background blocking thread (native).
        // WASM has no `std::thread::spawn`; run inline (briefly blocks `output_hook`).
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::spawn(build_and_send);
        #[cfg(target_arch = "wasm32")]
        build_and_send();
    }
}

// ─── Remote Viewer Plugin ──────────────────────────────────────────────────────

/// Receives remote egui frames over a channel (fed by WebSocket),
/// deserializes, and replays the paint commands during `ui()`.
pub struct EguiRemoteViewer {
    enabled: bool,
    pub frame_tx: Sender<EguiFrameMessage>,
    pub frame_rx: Receiver<EguiFrameMessage>,
    pub input_tx: Sender<EguiInputEvent>,
    pub input_rx: Receiver<EguiInputEvent>,
    latest_frame: Option<EguiFrameMessage>,
    cached_meshes: Vec<WireClippedMesh>,
    pending_textures: Option<WireTexturesDelta>,
    remote_tex_map: HashMap<WireTextureId, egui::TextureId>,
}

impl EguiRemoteViewer {
    pub fn new() -> Self {
        let (frame_tx, frame_rx) = crossbeam::channel::bounded(2);
        let (input_tx, input_rx) = crossbeam::channel::unbounded();
        Self {
            enabled: false,
            frame_tx,
            frame_rx,
            input_tx,
            input_rx,
            latest_frame: None,
            cached_meshes: Vec::new(),
            pending_textures: None,
            remote_tex_map: HashMap::new(),
        }
    }

    fn decode_frame(&mut self, frame: &EguiFrameMessage) {
        if let Ok(mesh_bytes) = decompress(&frame.meshes_data) {
            if let Ok((meshes, _)) = bincode::serde::decode_from_slice::<Vec<WireClippedMesh>, _>(
                &mesh_bytes,
                bincode::config::standard(),
            ) {
                self.cached_meshes = meshes;
            }
        }

        if let Ok(tex_bytes) = decompress(&frame.textures_data) {
            if let Ok((delta, _)) = bincode::serde::decode_from_slice::<WireTexturesDelta, _>(
                &tex_bytes,
                bincode::config::standard(),
            ) {
                self.pending_textures = Some(delta);
            }
        }
    }

}

impl Default for EguiRemoteViewer {
    fn default() -> Self {
        Self::new()
    }
}

impl MastertechPlugin for EguiRemoteViewer {
    fn id(&self) -> &'static str {
        "com.mastertech.egui-remote-viewer"
    }

    fn name(&self) -> &str {
        "Egui Remote Viewer"
    }

    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

    fn description(&self) -> &str {
        "Displays egui frames from a remote Mastertech instance"
    }

    fn enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    fn logic(&mut self, host: &PluginHost) {
        while let Ok(frame) = self.frame_rx.try_recv() {
            self.decode_frame(&frame);
            self.latest_frame = Some(frame);
        }
        if self.enabled {
            if let Some(ctx) = host.ctx.as_ref() {
                if let Some(delta) = self.pending_textures.take() {
                    apply_wire_textures_delta_for_viewer(ctx, &delta, &mut self.remote_tex_map);
                }
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _host: &PluginHost) {
        if !self.enabled || self.latest_frame.is_none() {
            return;
        }

        let inp = ui.ctx().input(|i| i.clone());

        let frame_count = self.latest_frame.as_ref().unwrap().frame_count;
        let width = self.latest_frame.as_ref().unwrap().width;
        let height = self.latest_frame.as_ref().unwrap().height;
        let ppp = self.latest_frame.as_ref().unwrap().pixels_per_point;
        let mesh_count = self.cached_meshes.len();

        let meshes: Vec<WireClippedMesh> = self.cached_meshes.clone();
        let input_tx = self.input_tx.clone();
        let tex_map = self.remote_tex_map.clone();

        let screen_min_x = self.latest_frame.as_ref().unwrap().screen_min_x;
        let screen_min_y = self.latest_frame.as_ref().unwrap().screen_min_y;
        let remote_origin = egui::pos2(screen_min_x, screen_min_y);

        egui::Window::new("Remote Egui Viewer")
            .default_size([width.max(400.0), height.max(300.0)])
            .show(ui.ctx(), move |ui| {
                ui.label(format!(
                    "Frame #{frame_count} | {}×{} @{ppp:.1}x | {mesh_count} meshes",
                    width as u32,
                    height as u32,
                ));
                ui.separator();

                let canvas_rect = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(width, height));
                let response = ui.allocate_rect(
                    canvas_rect,
                    egui::Sense::click_and_drag().union(egui::Sense::hover()),
                );

                let painter = ui.painter();
                for wire_mesh in &meshes {
                    let Some(prim) = wire_to_clipped_primitive_for_viewer(
                        wire_mesh,
                        remote_origin,
                        canvas_rect.min,
                        1.0,
                        &tex_map,
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

                if response.hovered() {
                    if let Some(pos) = inp.pointer.hover_pos() {
                        if canvas_rect.contains(pos) {
                            let r = remote_origin + (pos - canvas_rect.min);
                            if input_tx
                                .try_send(EguiInputEvent::PointerMoved { x: r.x, y: r.y })
                                .is_err()
                            {
                                log::error!(
                                    target: "egui_remote",
                                    "[admin_plugin_window] try_send PointerMoved failed (channel full?)"
                                );
                            }
                        }
                    }
                }

                if inp.pointer.primary_pressed() {
                    let ip = inp.pointer.interact_pos();
                    let inside = ip.is_some_and(|p| canvas_rect.contains(p));
                    log::error!(
                        target: "egui_remote",
                        "[admin_plugin_window] primary_pressed interact_pos={ip:?} inside_canvas={inside}"
                    );
                    if let Some(pos) = ip {
                        if canvas_rect.contains(pos) {
                            let r = remote_origin + (pos - canvas_rect.min);
                            if input_tx
                                .try_send(EguiInputEvent::PointerButton {
                                    x: r.x,
                                    y: r.y,
                                    button: 0,
                                    pressed: true,
                                })
                                .is_err()
                            {
                                log::error!(
                                    target: "egui_remote",
                                    "[admin_plugin_window] try_send PointerButton down failed"
                                );
                            }
                        }
                    }
                }
                if inp.pointer.primary_released() {
                    if let Some(pos) = inp.pointer.interact_pos() {
                        let r = remote_origin + (pos - canvas_rect.min);
                        log::error!(
                            target: "egui_remote",
                            "[admin_plugin_window] primary_released -> send release at ({:.1},{:.1})",
                            r.x,
                            r.y
                        );
                        if input_tx
                            .try_send(EguiInputEvent::PointerButton {
                                x: r.x,
                                y: r.y,
                                button: 0,
                                pressed: false,
                            })
                            .is_err()
                        {
                            log::error!(
                                target: "egui_remote",
                                "[admin_plugin_window] try_send PointerButton up failed"
                            );
                        }
                    }
                }
            });
    }
}
