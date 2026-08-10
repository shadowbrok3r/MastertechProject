//! Wire types for full remote-desktop control (raster screen streaming + OS
//! input injection) between the admin console and a connected client.
//!
//! These mirror the egui-frame streaming types in `plugins::remote` but carry
//! a rasterized desktop capture instead of tessellated egui meshes, and drive
//! OS-level input (`SendInput`/enigo) instead of egui events. Frames travel as
//! binary payloads prefixed with [`crate::DESKTOP_FRAME_TAG`]; input events
//! travel prefixed with [`crate::DESKTOP_INPUT_TAG`].

use serde::{Deserialize, Serialize};
use facet::Facet;

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
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Facet)]
#[repr(u8)]
pub enum DesktopMouseButton {
    Left,
    Right,
    Middle,
}

/// Modifier-key state accompanying a [`DesktopInputEvent::Key`].
#[derive(Serialize, Deserialize, Debug, Clone, Copy, Default, PartialEq, Eq, Facet)]
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
#[derive(Serialize, Deserialize, Debug, Clone, Facet)]
#[repr(u8)]
pub enum DesktopInputEvent {
    MouseMove { x: f32, y: f32 },
    MouseButton { x: f32, y: f32, button: DesktopMouseButton, pressed: bool },
    MouseScroll { delta_x: f32, delta_y: f32 },
    /// `key_name` is an `egui::Key::name()` string, mapped to an enigo key on the client.
    Key { key_name: String, pressed: bool, modifiers: DesktopModifiers },
    Text(String),
    /// Place the admin's clipboard text on the client's clipboard.
    ///
    /// Rides the input stream rather than a `Cmd` so it stays ordered against
    /// the keystrokes around it — a paste chord forwarded in the same frame
    /// must land after the text it is meant to paste.
    ClipboardSet(String),
}

/// One captured frame returned by a gated one-shot capture, plus the geometry a
/// caller needs to turn screenshot pixels back into click coordinates.
#[derive(Serialize, Deserialize, Debug, Clone, Facet)]
pub struct DesktopShot {
    pub monitor_id: u32,
    /// Dimensions of the returned image, after `scale`.
    pub width: u32,
    pub height: u32,
    /// Full monitor resolution before scaling.
    pub monitor_width: u32,
    pub monitor_height: u32,
    pub encode_ms: u32,
    /// Baseline JPEG bytes.
    pub jpeg: Vec<u8>,
}

/// A top-level window on the client.
///
/// Geometry is in the same physical screen pixels a capture uses, so a rect can
/// be compared against screenshot coordinates directly.
#[derive(Serialize, Deserialize, Debug, Clone, Facet)]
pub struct WindowInfo {
    /// Opaque handle for [`crate::Cmd::DesktopActivateWindow`]. Not stable
    /// across a reboot, and stale the moment the window closes.
    pub hwnd: u64,
    pub title: String,
    pub class_name: String,
    pub pid: u32,
    pub process_name: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub is_foreground: bool,
    pub is_minimized: bool,
    pub is_maximized: bool,
}

/// What currently has keyboard focus — the answer to "where would my next
/// keystroke actually go".
#[derive(Serialize, Deserialize, Debug, Clone, Facet)]
pub struct FocusInfo {
    pub foreground: Option<WindowInfo>,
    /// Window class of the focused child control, e.g. `Edit` or `RichEditD2DPT`.
    /// `None` when the foreground thread does not expose one.
    pub focused_control_class: Option<String>,
    /// Text caret in physical screen pixels — literally where typed characters
    /// will appear. `None` when nothing is accepting text.
    pub caret: Option<(i32, i32, u32, u32)>,
    /// Name of the desktop that owns input: `Default` for the normal
    /// interactive desktop, `Screen-saver` while a screensaver runs, `Winlogon`
    /// for the secure desktop. `None` when opening it was denied.
    pub input_desktop: Option<String>,
    /// True when `input_desktop` is the desktop this session runs on, and so
    /// injected input and captures can reach it.
    pub input_reachable: bool,
    /// True only for the Winlogon secure desktop — a UAC prompt or the logon
    /// screen — or when opening the input desktop was denied, which is itself
    /// that signal. A screensaver also blocks input but is not this.
    pub secure_desktop_suspected: bool,
    /// How the client resolved coordinates, for diagnosing DPI mismatches.
    pub dpi_context: String,
}

/// A monitor available on the client, reported in response to
/// [`crate::Cmd::DesktopListMonitors`].
#[derive(Serialize, Deserialize, Debug, Clone, Facet)]
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
