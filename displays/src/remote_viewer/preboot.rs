//! Admin-side adapter for the firmware pre-boot TUI stream.
//!
//! The UEFI diagnostic app can't produce the OS client's zstd+egui
//! `BufferMessage` (no zstd C-FFI, no egui in firmware), so it ships the plain
//! `tcp_protocol::preboot` types under dedicated frame tags. This converts an
//! incoming [`PreBootFrame`] into a ratatui [`Buffer`] the existing
//! [`RataguiBackend`] renders unchanged, and maps the viewer's outbound
//! [`TerminalEvent`] into a [`PreBootEvent`] for the return channel.

use eframe::egui::Key;
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect};
use ratatui::style::{Color, Modifier};
use tcp_protocol::preboot::{PbColor, PbKeyCode, PreBootEvent, PreBootFrame, PreBootKey};

use super::ratagui::TerminalEvent;

fn pb_to_color(c: PbColor) -> Color {
    match c {
        PbColor::Reset => Color::Reset,
        PbColor::Black => Color::Black,
        PbColor::Red => Color::Red,
        PbColor::Green => Color::Green,
        PbColor::Yellow => Color::Yellow,
        PbColor::Blue => Color::Blue,
        PbColor::Magenta => Color::Magenta,
        PbColor::Cyan => Color::Cyan,
        PbColor::Gray => Color::Gray,
        PbColor::DarkGray => Color::DarkGray,
        PbColor::LightRed => Color::LightRed,
        PbColor::LightGreen => Color::LightGreen,
        PbColor::LightYellow => Color::LightYellow,
        PbColor::LightBlue => Color::LightBlue,
        PbColor::LightMagenta => Color::LightMagenta,
        PbColor::LightCyan => Color::LightCyan,
        PbColor::White => Color::White,
        PbColor::Indexed(i) => Color::Indexed(i),
        PbColor::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

/// Decode a firmware frame into a ratatui buffer for the viewer widget.
pub fn from_preboot(frame: &PreBootFrame) -> Buffer {
    let mut buf = Buffer::empty(Rect::new(0, 0, frame.cols, frame.rows));
    let mut i = 0usize;
    for y in 0..frame.rows {
        for x in 0..frame.cols {
            if let Some(src) = frame.cells.get(i) {
                if let Some(dst) = buf.cell_mut(Position { x, y }) {
                    dst.set_symbol(&src.symbol);
                    dst.fg = pb_to_color(src.fg);
                    dst.bg = pb_to_color(src.bg);
                    dst.modifier = Modifier::from_bits_truncate(src.mods);
                }
            }
            i += 1;
        }
    }
    buf
}

fn egui_key_to_pb(k: Key) -> Option<PbKeyCode> {
    Some(match k {
        Key::Enter => PbKeyCode::Enter,
        Key::Escape => PbKeyCode::Esc,
        Key::Backspace => PbKeyCode::Backspace,
        Key::Tab => PbKeyCode::Tab,
        Key::Space => PbKeyCode::Char(' '),
        Key::ArrowUp => PbKeyCode::Up,
        Key::ArrowDown => PbKeyCode::Down,
        Key::ArrowLeft => PbKeyCode::Left,
        Key::ArrowRight => PbKeyCode::Right,
        Key::Home => PbKeyCode::Home,
        Key::End => PbKeyCode::End,
        Key::PageUp => PbKeyCode::PageUp,
        Key::PageDown => PbKeyCode::PageDown,
        Key::Delete => PbKeyCode::Delete,
        Key::Insert => PbKeyCode::Insert,
        Key::F1 => PbKeyCode::F(1),
        Key::F2 => PbKeyCode::F(2),
        Key::F3 => PbKeyCode::F(3),
        Key::F4 => PbKeyCode::F(4),
        Key::F5 => PbKeyCode::F(5),
        Key::F6 => PbKeyCode::F(6),
        Key::F7 => PbKeyCode::F(7),
        Key::F8 => PbKeyCode::F(8),
        Key::F9 => PbKeyCode::F(9),
        Key::F10 => PbKeyCode::F(10),
        Key::F11 => PbKeyCode::F(11),
        Key::F12 => PbKeyCode::F(12),
        Key::Num0 => PbKeyCode::Char('0'),
        Key::Num1 => PbKeyCode::Char('1'),
        Key::Num2 => PbKeyCode::Char('2'),
        Key::Num3 => PbKeyCode::Char('3'),
        Key::Num4 => PbKeyCode::Char('4'),
        Key::Num5 => PbKeyCode::Char('5'),
        Key::Num6 => PbKeyCode::Char('6'),
        Key::Num7 => PbKeyCode::Char('7'),
        Key::Num8 => PbKeyCode::Char('8'),
        Key::Num9 => PbKeyCode::Char('9'),
        Key::A => PbKeyCode::Char('a'),
        Key::B => PbKeyCode::Char('b'),
        Key::C => PbKeyCode::Char('c'),
        Key::D => PbKeyCode::Char('d'),
        Key::E => PbKeyCode::Char('e'),
        Key::F => PbKeyCode::Char('f'),
        Key::G => PbKeyCode::Char('g'),
        Key::H => PbKeyCode::Char('h'),
        Key::I => PbKeyCode::Char('i'),
        Key::J => PbKeyCode::Char('j'),
        Key::K => PbKeyCode::Char('k'),
        Key::L => PbKeyCode::Char('l'),
        Key::M => PbKeyCode::Char('m'),
        Key::N => PbKeyCode::Char('n'),
        Key::O => PbKeyCode::Char('o'),
        Key::P => PbKeyCode::Char('p'),
        Key::Q => PbKeyCode::Char('q'),
        Key::R => PbKeyCode::Char('r'),
        Key::S => PbKeyCode::Char('s'),
        Key::T => PbKeyCode::Char('t'),
        Key::U => PbKeyCode::Char('u'),
        Key::V => PbKeyCode::Char('v'),
        Key::W => PbKeyCode::Char('w'),
        Key::X => PbKeyCode::Char('x'),
        Key::Y => PbKeyCode::Char('y'),
        Key::Z => PbKeyCode::Char('z'),
        _ => return None,
    })
}

/// Map an outbound viewer event to the firmware wire type. Returns None for
/// events the firmware loop doesn't consume (e.g. mouse-move, unmapped keys).
pub fn terminal_event_to_preboot(ev: &TerminalEvent) -> Option<PreBootEvent> {
    match ev {
        TerminalEvent::KeyPress { code, modifiers } => Some(PreBootEvent::Key(PreBootKey {
            code: egui_key_to_pb(*code)?,
            ctrl: modifiers.ctrl || modifiers.command,
            alt: modifiers.alt,
            shift: modifiers.shift,
        })),
        TerminalEvent::MouseClick { x, y } => Some(PreBootEvent::MouseClick { x: *x, y: *y }),
        TerminalEvent::MouseScroll { x, y, up } => {
            Some(PreBootEvent::MouseScroll { x: *x, y: *y, up: *up })
        }
        TerminalEvent::MouseMove { .. } => None,
    }
}
