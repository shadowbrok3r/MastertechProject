//! Client for egui 0.35's native inspection protocol (egui_inspection::InspectionPlugin).
//!
//! The app serves the protocol on a loopback TCP port; these helpers connect as a client so
//! the in-app MCP tools (PluginToolProvider) can read the AccessKit tree, inject input, and
//! capture screenshots of the local app.

use std::net::TcpStream;
use std::time::Duration;

use eframe::egui::{Event, Key, Modifiers, PointerButton, Pos2};
use egui_inspection::protocol::{read_handshake, read_message, write_message};
use egui_inspection::{Request, Response};

/// Loopback address the InspectionPlugin binds (egui_inspection's DEFAULT_INSPECTION_ADDR).
pub const INSPECT_ADDR: &str = "127.0.0.1:5719";

/// The inspection server is started in debug builds, or when `MTECH_EGUI_INSPECT` is set.
pub fn inspection_enabled() -> bool {
    cfg!(debug_assertions) || std::env::var("MTECH_EGUI_INSPECT").is_ok()
}

fn request_blocking(req: Request) -> anyhow::Result<Response> {
    let mut stream = TcpStream::connect(INSPECT_ADDR).map_err(|e| {
        anyhow::anyhow!(
            "connect {INSPECT_ADDR}: {e} (inspection server not running; use a debug build or set MTECH_EGUI_INSPECT=1)"
        )
    })?;
    stream.set_read_timeout(Some(Duration::from_secs(25)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let _version = read_handshake(&mut stream)?;
    write_message(&mut stream, &req)?;
    Ok(read_message::<_, Response>(&mut stream)?)
}

/// Send one inspection request and await its single response (off the runtime via blocking task).
pub async fn request(req: Request) -> anyhow::Result<Response> {
    tokio::task::spawn_blocking(move || request_blocking(req)).await?
}

pub fn parse_button(s: &str) -> PointerButton {
    match s.to_ascii_lowercase().as_str() {
        "secondary" | "right" => PointerButton::Secondary,
        "middle" => PointerButton::Middle,
        "extra1" => PointerButton::Extra1,
        "extra2" => PointerButton::Extra2,
        _ => PointerButton::Primary,
    }
}

/// Pointer move + button press/release at a logical-point position (one or two clicks).
pub fn click_events(x: f32, y: f32, button: PointerButton, double: bool) -> Vec<Event> {
    let pos = Pos2::new(x, y);
    let m = Modifiers::default();
    let mut ev = vec![Event::PointerMoved(pos)];
    for _ in 0..(if double { 2 } else { 1 }) {
        ev.push(Event::PointerButton { pos, button, pressed: true, modifiers: m });
        ev.push(Event::PointerButton { pos, button, pressed: false, modifiers: m });
    }
    ev
}

/// Key down followed by key up for a single egui key.
pub fn key_events(key: Key) -> Vec<Event> {
    let m = Modifiers::default();
    vec![
        Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: m },
        Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: m },
    ]
}
