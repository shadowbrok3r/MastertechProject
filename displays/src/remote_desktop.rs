//! Wire types for full remote-desktop control (raster screen streaming + OS
//! input injection) between the admin console and a connected client.
//!
//! These mirror the egui-frame streaming types in `plugins::remote` but carry
//! a rasterized desktop capture instead of tessellated egui meshes, and drive
//! OS-level input (`SendInput`/enigo) instead of egui events. Frames travel as
//! binary payloads prefixed with [`crate::DESKTOP_FRAME_TAG`]; input events
//! travel prefixed with [`crate::DESKTOP_INPUT_TAG`].

use serde::{Deserialize, Serialize};

/// Encoding of the pixel data in a [`DesktopFrameMessage`].
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopFrameEncoding {
    /// Baseline JPEG bytes.
    Jpeg,
    /// Raw RGBA8 pixels, row-major, `width * height * 4` bytes.
    Rgba,
}

/// A single captured desktop frame sent client → admin.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DesktopFrameMessage {
    pub frame_count: u64,
    pub timestamp_ms: u128,
    pub monitor_id: u32,
    pub width: u32,
    pub height: u32,
    pub encoding: DesktopFrameEncoding,
    pub data: Vec<u8>,
    pub encode_ms: u32,
    /// Cursor position in monitor-local pixels, or `-1` when unknown.
    pub cursor_x: i32,
    pub cursor_y: i32,
}

/// Mouse button in a [`DesktopInputEvent`].
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopMouseButton {
    Left,
    Right,
    Middle,
}

/// Modifier-key state accompanying a [`DesktopInputEvent::Key`].
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DesktopModifiers {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub meta: bool,
}

/// An input event sent admin → client for injection into the client desktop.
///
/// Pointer coordinates are normalized `0.0..=1.0` within the streamed monitor,
/// so the admin needs no knowledge of the client's resolution; the client maps
/// them to absolute screen pixels using the active monitor geometry.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum DesktopInputEvent {
    MouseMove { x: f32, y: f32 },
    MouseButton { x: f32, y: f32, button: DesktopMouseButton, pressed: bool },
    MouseScroll { delta_x: f32, delta_y: f32 },
    /// `key_name` is an `egui::Key::name()` string, mapped to an enigo key on the client.
    Key { key_name: String, pressed: bool, modifiers: DesktopModifiers },
    Text(String),
}

/// A monitor available on the client, reported in response to
/// [`crate::Cmd::DesktopListMonitors`].
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DesktopMonitorInfo {
    pub id: u32,
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub is_primary: bool,
    pub scale_factor: f32,
}
