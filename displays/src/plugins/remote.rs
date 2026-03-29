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

use crossbeam::channel::{Receiver, Sender};
use eframe::egui;
use serde::{Deserialize, Serialize};

use super::{MastertechPlugin, PluginHost, PluginToolDescriptor};

// ─── Wire types ────────────────────────────────────────────────────────────────

/// A captured egui frame for transmission to a remote viewer.
///
/// Contains the minimal data needed to reconstruct the visual output.
/// In a full implementation, `shapes_data` would be the serialized
/// `Vec<egui::epaint::ClippedShape>` and `textures_delta_data` the serialized
/// `egui::TexturesDelta`. For the scaffold, these are opaque byte blobs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EguiFrameMessage {
    pub frame_count: u64,
    pub timestamp_ms: u128,
    /// Serialized shapes (bincode + zstd compressed).
    pub shapes_data: Vec<u8>,
    /// Serialized texture delta (bincode + zstd compressed).
    pub textures_delta_data: Vec<u8>,
    /// Screen rect width at capture time.
    pub width: f32,
    /// Screen rect height at capture time.
    pub height: f32,
    /// Pixels per point at capture time.
    pub pixels_per_point: f32,
}

/// Input events forwarded from the viewer to the host for injection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EguiInputEvent {
    PointerMoved {
        x: f32,
        y: f32,
    },
    PointerButton {
        x: f32,
        y: f32,
        button: u8,
        pressed: bool,
    },
    Key {
        key_name: String,
        pressed: bool,
        modifiers: EguiModifiers,
    },
    Text(String),
    Scroll {
        delta_x: f32,
        delta_y: f32,
    },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EguiModifiers {
    pub alt: bool,
    pub ctrl: bool,
    pub shift: bool,
    pub command: bool,
}

// ─── Frame Capture Plugin ──────────────────────────────────────────────────────

/// A plugin that captures egui output for remote transmission.
///
/// Hooks into `output_hook` to grab the `FullOutput` each frame,
/// serializes it into an `EguiFrameMessage`, and sends it via a channel.
///
/// The transport layer (WebSocket) is external -- the consumer of `frame_rx`
/// is responsible for compression and transmission.
pub struct EguiFrameCapture {
    enabled: bool,
    frame_count: u64,
    pub frame_tx: Sender<EguiFrameMessage>,
    pub frame_rx: Receiver<EguiFrameMessage>,
    pub input_tx: Sender<EguiInputEvent>,
    pub input_rx: Receiver<EguiInputEvent>,
}

impl EguiFrameCapture {
    pub fn new() -> Self {
        let (frame_tx, frame_rx) = crossbeam::channel::bounded(2);
        let (input_tx, input_rx) = crossbeam::channel::unbounded();
        Self {
            enabled: false,
            frame_count: 0,
            frame_tx,
            frame_rx,
            input_tx,
            input_rx,
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

    fn input_hook(&mut self, input: &mut egui::RawInput) {
        while let Ok(event) = self.input_rx.try_recv() {
            match event {
                EguiInputEvent::PointerMoved { x, y } => {
                    input.events.push(egui::Event::PointerMoved(egui::pos2(x, y)));
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
                _ => {
                    // TODO: implement full PointerButton and Key event translation
                }
            }
        }
    }

    fn output_hook(&mut self, _output: &mut egui::FullOutput) {
        self.frame_count += 1;

        // Stub: in a full implementation, serialize the shapes and textures_delta
        // from output and send via frame_tx. For now, send an empty marker frame.
        let msg = EguiFrameMessage {
            frame_count: self.frame_count,
            timestamp_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis(),
            shapes_data: Vec::new(),
            textures_delta_data: Vec::new(),
            width: 0.0,
            height: 0.0,
            pixels_per_point: 1.0,
        };

        let _ = self.frame_tx.try_send(msg);
    }
}

// ─── Remote Viewer Plugin ──────────────────────────────────────────────────────

/// A plugin that receives remote egui frames and displays them.
///
/// Receives `EguiFrameMessage` frames over a channel (fed by WebSocket),
/// deserializes, and replays the paint commands during `ui()`.
pub struct EguiRemoteViewer {
    enabled: bool,
    pub frame_tx: Sender<EguiFrameMessage>,
    pub frame_rx: Receiver<EguiFrameMessage>,
    pub input_tx: Sender<EguiInputEvent>,
    pub input_rx: Receiver<EguiInputEvent>,
    latest_frame: Option<EguiFrameMessage>,
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

    fn logic(&mut self, _host: &PluginHost) {
        while let Ok(frame) = self.frame_rx.try_recv() {
            self.latest_frame = Some(frame);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _host: &PluginHost) {
        if let Some(frame) = &self.latest_frame {
            egui::Window::new("Remote Egui Viewer")
                .default_size([800.0, 600.0])
                .show(ui.ctx(), |ui| {
                    ui.label(format!(
                        "Remote frame #{} (stub -- shapes replay not yet implemented)",
                        frame.frame_count
                    ));
                    // TODO: deserialize shapes_data and textures_delta_data,
                    // tessellate, and paint using ui.painter()
                });
        }
    }
}
