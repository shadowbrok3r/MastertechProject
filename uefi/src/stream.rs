//! Pre-boot TUI streaming: capture the firmware's rendered ratatui buffer, map
//! it to the shared `tcp_protocol::preboot` wire types, and map inbound viewer
//! events back into `terminput` for injection into the run loop.
//!
//! The wire types are shared with the admin console's `RataguiBackend` via the
//! `tcp_protocol` crate (serde + bincode), so firmware and the viewer stay
//! byte-compatible without firmware ever depending on `displays`, zstd, or egui.

use ratatui::buffer::Buffer;
use ratatui::layout::Position;
use ratatui::style::Color;
use tcp_protocol::preboot::{
    self, PbColor, PbKeyCode, PreBootCell, PreBootEvent, PreBootFrame,
};
use terminput::{Event, KeyCode, KeyEvent, KeyModifiers};

fn map_color(c: Color) -> PbColor {
    match c {
        Color::Reset => PbColor::Reset,
        Color::Black => PbColor::Black,
        Color::Red => PbColor::Red,
        Color::Green => PbColor::Green,
        Color::Yellow => PbColor::Yellow,
        Color::Blue => PbColor::Blue,
        Color::Magenta => PbColor::Magenta,
        Color::Cyan => PbColor::Cyan,
        Color::Gray => PbColor::Gray,
        Color::DarkGray => PbColor::DarkGray,
        Color::LightRed => PbColor::LightRed,
        Color::LightGreen => PbColor::LightGreen,
        Color::LightYellow => PbColor::LightYellow,
        Color::LightBlue => PbColor::LightBlue,
        Color::LightMagenta => PbColor::LightMagenta,
        Color::LightCyan => PbColor::LightCyan,
        Color::White => PbColor::White,
        Color::Indexed(i) => PbColor::Indexed(i),
        Color::Rgb(r, g, b) => PbColor::Rgb(r, g, b),
    }
}

/// Snapshot a rendered ratatui buffer into a wire frame (row-major).
pub fn buffer_to_frame(buf: &Buffer, frame: u64) -> PreBootFrame {
    let area = buf.area;
    let mut cells = Vec::with_capacity(area.width as usize * area.height as usize);
    for y in 0..area.height {
        for x in 0..area.width {
            let pos = Position { x: area.x + x, y: area.y + y };
            if let Some(c) = buf.cell(pos) {
                cells.push(PreBootCell {
                    symbol: c.symbol().to_string(),
                    fg: map_color(c.fg),
                    bg: map_color(c.bg),
                    mods: c.modifier.bits(),
                });
            } else {
                cells.push(PreBootCell {
                    symbol: " ".to_string(),
                    fg: PbColor::Reset,
                    bg: PbColor::Reset,
                    mods: 0,
                });
            }
        }
    }
    PreBootFrame { frame, cols: area.width, rows: area.height, cells }
}

/// Map a viewer event to a `terminput` event for injection. Only key events
/// map (the run loop is key-driven); mouse events return None for now.
pub fn event_to_terminput(ev: &PreBootEvent) -> Option<Event> {
    let PreBootEvent::Key(k) = ev else {
        return None;
    };
    let code = match k.code {
        PbKeyCode::Char(c) => KeyCode::Char(c),
        PbKeyCode::Enter => KeyCode::Enter,
        PbKeyCode::Esc => KeyCode::Esc,
        PbKeyCode::Backspace => KeyCode::Backspace,
        PbKeyCode::Tab => KeyCode::Tab,
        PbKeyCode::Up => KeyCode::Up,
        PbKeyCode::Down => KeyCode::Down,
        PbKeyCode::Left => KeyCode::Left,
        PbKeyCode::Right => KeyCode::Right,
        PbKeyCode::Home => KeyCode::Home,
        PbKeyCode::End => KeyCode::End,
        PbKeyCode::PageUp => KeyCode::PageUp,
        PbKeyCode::PageDown => KeyCode::PageDown,
        PbKeyCode::Delete => KeyCode::Delete,
        PbKeyCode::Insert => KeyCode::Insert,
        PbKeyCode::F(n) => KeyCode::F(n),
    };
    let mut m = KeyModifiers::empty();
    if k.ctrl {
        m |= KeyModifiers::CTRL;
    }
    if k.alt {
        m |= KeyModifiers::ALT;
    }
    if k.shift {
        m |= KeyModifiers::SHIFT;
    }
    // normalize_case folds shift into the char (a+SHIFT -> A) so the firmware
    // key match, which distinguishes 'a' from 'A', sees the right code.
    Some(Event::Key(KeyEvent::new(code).modifiers(m).normalize_case()))
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Suppresses re-sending an unchanged screen; only encodes when content moved.
#[derive(Default)]
pub struct Throttle {
    last_hash: u64,
}

impl Throttle {
    pub fn new() -> Self {
        Self { last_hash: 0 }
    }

    /// Encode `frame` to its raw bincode body (for an HTTP POST), or None if
    /// identical to the last frame emitted.
    pub fn body_if_dirty(&mut self, frame: &PreBootFrame) -> Option<Vec<u8>> {
        let body = preboot::encode_frame(frame);
        let h = fnv1a(&body);
        if h == self.last_hash {
            return None;
        }
        self.last_hash = h;
        Some(body)
    }
}
