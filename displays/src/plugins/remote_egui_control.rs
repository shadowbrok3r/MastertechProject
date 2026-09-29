//! Routes MCP-issued remote egui input into the admin WebSocket client for a given `connection_string`.
//!
//! When an operator connects to a remote Mastertech instance from the Web Console, the
//! [`WebSocketClient`](crate::tabs::admin_console::client_interface::WebSocketClient) registers
//! here so tools can inject [`EguiInputEvent`](super::remote::EguiInputEvent) over the same binary
//! path as the inline/pop-out viewer (`EGUI_INPUT_TAG` + bincode).

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crossbeam::channel::{Receiver, Sender};
use once_cell::sync::Lazy;
use serde::Serialize;

use super::remote::{EguiFrameMessage, EguiInputEvent, WidgetAnchor};
use crate::EGUI_INPUT_TAG;

/// Latest remote frame dimensions and timing (no mesh/pixel payload) for MCP grounding.
#[derive(Clone, Debug, Serialize)]
pub struct LastFrameMeta {
    pub frame_count: u64,
    pub timestamp_ms: u128,
    pub width: f32,
    pub height: f32,
    pub pixels_per_point: f32,
    pub screen_min_x: f32,
    pub screen_min_y: f32,
    pub meshes_compressed_bytes: usize,
    pub textures_compressed_bytes: usize,
}

impl LastFrameMeta {
    pub fn from_frame(f: &EguiFrameMessage) -> Self {
        Self {
            frame_count: f.frame_count,
            timestamp_ms: f.timestamp_ms,
            width: f.width,
            height: f.height,
            pixels_per_point: f.pixels_per_point,
            screen_min_x: f.screen_min_x,
            screen_min_y: f.screen_min_y,
            meshes_compressed_bytes: f.meshes_data.len(),
            textures_compressed_bytes: f.textures_data.len(),
        }
    }
}

struct HubInner {
    targets: HashMap<String, Sender<Vec<u8>>>,
    last_frame: HashMap<String, LastFrameMeta>,
    /// Latest `widget_anchors` from the last decoded frame per admin session.
    last_anchors: HashMap<String, Vec<WidgetAnchor>>,
    /// Last MCP-injected pointer position in host screen space (for admin overlay).
    last_injected_pointer: HashMap<String, (f32, f32)>,
    /// Session state a session engine reports; a registered target without an entry counts as open.
    open: HashMap<String, bool>,
    /// Connection strings a tool asked the session engine to dial.
    requested: HashSet<String>,
}

/// Shared registry: MCP tools enqueue; admin [`WebSocketClient`](crate::tabs::admin_console::client_interface::WebSocketClient) drains each frame.
#[derive(Clone)]
pub struct RemoteEguiControlHub {
    inner: Arc<Mutex<HubInner>>,
    /// Set when this process runs a session engine that dials requested sessions.
    dialer: Arc<AtomicBool>,
}

impl Default for RemoteEguiControlHub {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HubInner {
                targets: HashMap::new(),
                last_frame: HashMap::new(),
                last_anchors: HashMap::new(),
                last_injected_pointer: HashMap::new(),
                open: HashMap::new(),
                requested: HashSet::new(),
            })),
            dialer: Arc::new(AtomicBool::new(false)),
        }
    }
}

static HUB: Lazy<RemoteEguiControlHub> = Lazy::new(RemoteEguiControlHub::default);

/// Process-wide hub used by MCP and Web Console clients.
pub fn hub() -> RemoteEguiControlHub {
    HUB.clone()
}

impl RemoteEguiControlHub {
    /// Register an admin session; returns a receiver to drain in the UI loop.
    /// Replaces any existing registration for the same `connection_string`.
    pub fn register(&self, connection_string: String) -> Receiver<Vec<u8>> {
        let (tx, rx) = crossbeam::channel::bounded::<Vec<u8>>(256);
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.targets.insert(connection_string, tx);
        rx
    }

    pub fn unregister(&self, connection_string: &str) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.targets.remove(connection_string);
        g.last_frame.remove(connection_string);
        g.last_anchors.remove(connection_string);
        g.last_injected_pointer.remove(connection_string);
        g.open.remove(connection_string);
    }

    /// Marks this process as running a session engine that dials requested sessions.
    pub fn set_dialer(&self) {
        self.dialer.store(true, Ordering::Relaxed);
    }

    pub fn has_dialer(&self) -> bool {
        self.dialer.load(Ordering::Relaxed)
    }

    /// Asks the session engine to open a session to `connection_string`.
    pub fn request_session(&self, connection_string: &str) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.requested.insert(connection_string.to_string());
    }

    /// Drains the connection strings tools asked the session engine to dial.
    pub fn take_requested(&self) -> Vec<String> {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.requested.drain().collect()
    }

    /// Records whether the session engine's transport to `connection_string` is connected.
    pub fn set_open(&self, connection_string: &str, open: bool) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.open.insert(connection_string.to_string(), open);
    }

    /// True when a session to `connection_string` is registered and not known to be disconnected.
    pub fn is_ready(&self, connection_string: &str) -> bool {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.targets.contains_key(connection_string) && g.open.get(connection_string) != Some(&false)
    }

    /// Called when a tagged egui frame arrives from the remote client (WebSocket task).
    pub fn record_last_frame(&self, connection_string: &str, frame: &EguiFrameMessage) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.last_frame.insert(
            connection_string.to_string(),
            LastFrameMeta::from_frame(frame),
        );
        g.last_anchors.insert(
            connection_string.to_string(),
            frame.widget_anchors.clone(),
        );
    }

    pub fn get_last_widget_anchors(&self, connection_string: &str) -> Vec<WidgetAnchor> {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.last_anchors
            .get(connection_string)
            .cloned()
            .unwrap_or_default()
    }

    pub fn get_last_injected_pointer(&self, connection_string: &str) -> Option<(f32, f32)> {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.last_injected_pointer.get(connection_string).copied()
    }

    fn note_injected_pointer(&self, connection_string: &str, x: f32, y: f32) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.last_injected_pointer
            .insert(connection_string.to_string(), (x, y));
    }

    pub fn get_last_frame_meta(&self, connection_string: &str) -> Option<LastFrameMeta> {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.last_frame.get(connection_string).cloned()
    }

    pub fn list_targets(&self) -> Vec<String> {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let mut v: Vec<String> = g.targets.keys().cloned().collect();
        v.sort();
        v
    }

    /// Enqueue one input event for the given remote client. Fails if no admin session registered.
    pub fn send_event(
        &self,
        connection_string: &str,
        event: EguiInputEvent,
    ) -> Result<(), String> {
        self.maybe_note_pointer_for_event(connection_string, &event);
        let bin = encode_tagged_input(&event)?;
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let tx = g.targets.get(connection_string).ok_or_else(|| no_session(connection_string))?;
        tx.try_send(bin)
            .map_err(|e| format!("remote egui queue full or disconnected: {e}"))
    }

    fn maybe_note_pointer_for_event(&self, connection_string: &str, event: &EguiInputEvent) {
        match event {
            EguiInputEvent::PointerMoved { x, y } | EguiInputEvent::PointerButton { x, y, .. } => {
                self.note_injected_pointer(connection_string, *x, *y);
            }
            _ => {}
        }
    }

    /// Enqueue several events in order (e.g. click = move, press, release).
    pub fn send_events(
        &self,
        connection_string: &str,
        events: &[EguiInputEvent],
    ) -> Result<(), String> {
        // Update pointer overlay without holding `inner` while locked — `maybe_note_pointer_for_event`
        // calls `note_injected_pointer`, which takes the same mutex (non-reentrant `std::sync::Mutex`).
        for ev in events {
            self.maybe_note_pointer_for_event(connection_string, ev);
        }
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let tx = g.targets.get(connection_string).ok_or_else(|| no_session(connection_string))?;
        for ev in events {
            let bin = encode_tagged_input(ev)?;
            tx.try_send(bin)
                .map_err(|e| format!("remote egui queue full or disconnected: {e}"))?;
        }
        Ok(())
    }
}

impl RemoteEguiControlHub {
    /// Send raw binary (e.g. a bincode-serialized `Cmd`) to the remote client via the admin WebSocket.
    /// Unlike `send_event`, this does NOT prepend `EGUI_INPUT_TAG` — the remote client
    /// will deserialize the bytes directly as a `Cmd`.
    pub fn send_raw_binary(
        &self,
        connection_string: &str,
        data: Vec<u8>,
    ) -> Result<(), String> {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let tx = g.targets.get(connection_string).ok_or_else(|| no_session(connection_string))?;
        tx.try_send(data)
            .map_err(|e| format!("remote command queue full or disconnected: {e}"))
    }
}

/// The error a send to a client without an admin session returns.
pub fn no_session(connection_string: &str) -> String {
    if HUB.has_dialer() {
        format!("no admin session to {connection_string:?} is open; the client is offline, restarting or has crashed")
    } else {
        format!("no admin session to {connection_string:?} is open; open its console in Mastertech, or the client is offline")
    }
}

fn encode_tagged_input(event: &EguiInputEvent) -> Result<Vec<u8>, String> {
    let mut v = vec![EGUI_INPUT_TAG];
    let ser = bincode::serde::encode_to_vec(event, bincode::config::standard())
        .map_err(|e| format!("bincode encode: {e}"))?;
    v.extend(ser);
    Ok(v)
}
