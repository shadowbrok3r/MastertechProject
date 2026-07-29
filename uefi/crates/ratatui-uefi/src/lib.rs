use std::fmt::Write;

use uefi::{boot::ScopedProtocol, proto::console};

/// Implements a backend for the `ratatui` crate suitable for use in a UEFI application
/// or loader.
pub struct UefiOutputBackend {
    output: ScopedProtocol<console::text::Output>,
}

impl UefiOutputBackend {
    pub fn new(output: ScopedProtocol<console::text::Output>) -> Self {
        Self { output }
    }

    /// Writes a run with one reposition, one color change and one string write,
    /// skipping the reposition and the color change when already in effect.
    fn emit_run(&mut self, run: &Run, state: &mut ConsoleState) {
        if run.is_empty() {
            return;
        }

        let mut positioned = state.cursor == Some((run.x, run.y));
        if !positioned {
            positioned = self
                .output
                .set_cursor_position(run.x as usize, run.y as usize)
                .is_ok();
        }

        if !state.holds_color(run.fg, run.bg) {
            state.color = match self.output.set_color(run.fg, run.bg) {
                Ok(()) => Some((run.fg, run.bg)),
                Err(_) => None,
            };
        }

        // Best-effort. Real firmware consoles return a warning/error for glyphs
        // missing from their font (e.g. box drawing, middle dot) which the uefi
        // crate surfaces as an `Err`. Propagating it would abort the entire frame
        // and the app would exit, so a failed run is repainted cell by cell.
        if self.output.write_str(&run.text).is_err() {
            self.emit_run_per_cell(run, state);
        } else if positioned {
            state.cursor = Some((run.end_x(), run.y));
        } else {
            state.cursor = None;
        }
    }

    /// Repaints a failed run one absolutely positioned cell at a time, degrading
    /// a glyph the firmware font lacks to a solid cell in the foreground color.
    fn emit_run_per_cell(&mut self, run: &Run, state: &mut ConsoleState) {
        let mut colored = false;

        for i in 0..run.ends.len() {
            let Some(symbol) = run.symbol(i) else {
                continue;
            };
            let x = run.x.saturating_add(i as u16) as usize;
            let y = run.y as usize;

            if !colored {
                let _ = self.output.set_color(run.fg, run.bg);
                colored = true;
            }
            let _ = self.output.set_cursor_position(x, y);
            if self.output.write_str(symbol).is_err() {
                let _ = self.output.set_cursor_position(x, y);
                let _ = self.output.set_color(run.fg, clamp_bg(run.fg));
                let _ = self.output.write_str(" ");
                colored = false;
            }
        }

        state.cursor = None;
        state.color = None;
    }
}

fn to_uefi_color(color: ratatui::style::Color) -> Option<console::text::Color> {
    match color {
        ratatui::style::Color::Black => Some(console::text::Color::Black),
        ratatui::style::Color::Red => Some(console::text::Color::Red),
        ratatui::style::Color::Green => Some(console::text::Color::Green),
        ratatui::style::Color::Yellow => Some(console::text::Color::Yellow),
        ratatui::style::Color::Blue => Some(console::text::Color::Blue),
        ratatui::style::Color::Magenta => Some(console::text::Color::Magenta),
        ratatui::style::Color::Cyan => Some(console::text::Color::Cyan),
        ratatui::style::Color::Gray => Some(console::text::Color::LightGray),
        ratatui::style::Color::DarkGray => Some(console::text::Color::DarkGray),
        ratatui::style::Color::LightRed => Some(console::text::Color::LightRed),
        ratatui::style::Color::LightGreen => Some(console::text::Color::LightGreen),
        ratatui::style::Color::LightYellow => Some(console::text::Color::Yellow),
        ratatui::style::Color::LightBlue => Some(console::text::Color::LightBlue),
        ratatui::style::Color::LightMagenta => Some(console::text::Color::LightMagenta),
        ratatui::style::Color::LightCyan => Some(console::text::Color::LightCyan),
        ratatui::style::Color::White => Some(console::text::Color::White),
        ratatui::style::Color::Rgb(r, g, b) => Some(quantize_rgb(r, g, b)),
        ratatui::style::Color::Indexed(i) => {
            let (r, g, b) = xterm_to_rgb(i);
            Some(quantize_rgb(r, g, b))
        }
        ratatui::style::Color::Reset => None,
    }
}

/// Nearest EFI text color for an RGB value, hue-first so pastel palettes
/// (e.g. Catppuccin) keep their identity instead of collapsing to gray.
fn quantize_rgb(r: u8, g: u8, b: u8) -> console::text::Color {
    use console::text::Color as C;
    let (r, g, b) = (r as f32, g as f32, b as f32);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let sat = if max == 0.0 { 0.0 } else { (max - min) / max };

    // Visually black regardless of hue (e.g. the 0x06060A app background:
    // 40% mathematical saturation, but black on any real panel).
    if max < 48.0 {
        return C::Black;
    }

    // Low saturation: gray ramp by brightness.
    if sat < 0.25 {
        return match max as u16 {
            0..=63 => C::Black,
            64..=143 => C::DarkGray,
            144..=207 => C::LightGray,
            _ => C::White,
        };
    }

    // Hue in degrees [0, 360).
    let delta = max - min;
    let hue = if max == r {
        60.0 * (((g - b) / delta).rem_euclid(6.0))
    } else if max == g {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };

    let bright = max > 170.0;
    match hue as u16 {
        0..=29 | 330..=360 => if bright { C::LightRed } else { C::Red },
        30..=89 => if bright { C::Yellow } else { C::Brown },
        90..=149 => if bright { C::LightGreen } else { C::Green },
        150..=209 => if bright { C::LightCyan } else { C::Cyan },
        210..=269 => if bright { C::LightBlue } else { C::Blue },
        _ => if bright { C::LightMagenta } else { C::Magenta },
    }
}

/// RGB for an xterm-256 palette index.
fn xterm_to_rgb(i: u8) -> (u8, u8, u8) {
    match i {
        // Standard + bright ANSI colors.
        0 => (0, 0, 0),
        1 => (170, 0, 0),
        2 => (0, 170, 0),
        3 => (170, 85, 0),
        4 => (0, 0, 170),
        5 => (170, 0, 170),
        6 => (0, 170, 170),
        7 => (170, 170, 170),
        8 => (85, 85, 85),
        9 => (255, 85, 85),
        10 => (85, 255, 85),
        11 => (255, 255, 85),
        12 => (85, 85, 255),
        13 => (255, 85, 255),
        14 => (85, 255, 255),
        15 => (255, 255, 255),
        // 6x6x6 color cube.
        16..=231 => {
            let i = i - 16;
            let step = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            (step(i / 36), step((i / 6) % 6), step(i % 6))
        }
        // Grayscale ramp.
        232..=255 => {
            let v = 8 + (i - 232) * 10;
            (v, v, v)
        }
    }
}

/// Compares discriminants; `console::text::Color` does not implement `PartialEq`.
fn same_color(a: console::text::Color, b: console::text::Color) -> bool {
    a as u32 == b as u32
}

/// Cursor position and color attribute the console was last left in; `None` is unknown.
struct ConsoleState {
    cursor: Option<(u16, u16)>,
    color: Option<(console::text::Color, console::text::Color)>,
}

impl ConsoleState {
    const fn unknown() -> Self {
        Self {
            cursor: None,
            color: None,
        }
    }

    fn holds_color(&self, fg: console::text::Color, bg: console::text::Color) -> bool {
        matches!(self.color, Some((f, b)) if same_color(f, fg) && same_color(b, bg))
    }
}

/// Consecutive cells on one row sharing a resolved color pair, written as one string.
struct Run {
    x: u16,
    y: u16,
    fg: console::text::Color,
    bg: console::text::Color,
    text: String,
    /// Byte offset one past each cell's symbol in `text`.
    ends: Vec<usize>,
}

impl Run {
    fn new() -> Self {
        Self {
            x: 0,
            y: 0,
            fg: console::text::Color::White,
            bg: console::text::Color::Black,
            text: String::new(),
            ends: Vec::new(),
        }
    }

    fn is_empty(&self) -> bool {
        self.ends.is_empty()
    }

    fn restart(&mut self, x: u16, y: u16, fg: console::text::Color, bg: console::text::Color) {
        self.x = x;
        self.y = y;
        self.fg = fg;
        self.bg = bg;
        self.text.clear();
        self.ends.clear();
    }

    fn push(&mut self, symbol: &str) {
        self.text.push_str(symbol);
        self.ends.push(self.text.len());
    }

    /// Column one past the run's last cell; one cell counts as one column.
    fn end_x(&self) -> u16 {
        self.x.saturating_add(self.ends.len() as u16)
    }

    fn extends(&self, x: u16, y: u16, fg: console::text::Color, bg: console::text::Color) -> bool {
        !self.is_empty()
            && y == self.y
            && x == self.end_x()
            && same_color(fg, self.fg)
            && same_color(bg, self.bg)
    }

    /// Symbol of the nth cell in the run.
    fn symbol(&self, i: usize) -> Option<&str> {
        let end = *self.ends.get(i)?;
        let start = match i.checked_sub(1) {
            Some(prev) => *self.ends.get(prev)?,
            None => 0,
        };
        self.text.get(start..end)
    }
}

/// EFI text backgrounds only support the first 8 (dark) colors; clamp the
/// bright variants down so set_color never gets an invalid attribute.
fn clamp_bg(color: console::text::Color) -> console::text::Color {
    use console::text::Color as C;
    match color {
        C::DarkGray => C::Black,
        C::LightRed => C::Red,
        C::LightGreen => C::Green,
        C::Yellow => C::Brown,
        C::LightBlue => C::Blue,
        C::LightMagenta => C::Magenta,
        C::LightCyan => C::Cyan,
        C::White => C::LightGray,
        other => other,
    }
}

impl ratatui::backend::Backend for UefiOutputBackend {
    // ratatui 0.30 moved the backend error behind an associated type. Keeping
    // `std::io::Error` means every `std::io::Result` signature below still
    // satisfies `Result<_, Self::Error>`.
    type Error = std::io::Error;

    fn draw<'a, I>(&mut self, content: I) -> std::io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a ratatui::buffer::Cell)>,
    {
        let mut state = ConsoleState::unknown();
        let mut run = Run::new();

        for (x, y, cell) in content {
            let symbol = cell.symbol();
            // A zero-width symbol would desynchronize the run's column arithmetic.
            if symbol.is_empty() {
                continue;
            }

            let mut fg = to_uefi_color(cell.fg).unwrap_or(console::text::Color::White);
            let mut bg = to_uefi_color(cell.bg).unwrap_or(console::text::Color::Black);

            if cell.modifier.contains(ratatui::style::Modifier::REVERSED) {
                // Swap foreground and background colors.
                std::mem::swap(&mut fg, &mut bg);
            }
            let bg = clamp_bg(bg);

            if !run.extends(x, y, fg, bg) {
                self.emit_run(&run, &mut state);
                run.restart(x, y, fg, bg);
            }
            run.push(symbol);
        }

        self.emit_run(&run, &mut state);

        Ok(())
    }

    fn hide_cursor(&mut self) -> std::io::Result<()> {
        // Not supported on all platforms.
        let _ = self.output.enable_cursor(false);

        Ok(())
    }

    fn show_cursor(&mut self) -> std::io::Result<()> {
        // Not supported on all platforms.
        let _ = self.output.enable_cursor(true);

        Ok(())
    }

    fn get_cursor_position(&mut self) -> std::io::Result<ratatui::prelude::Position> {
        let (col, row) = self.output.cursor_position();

        Ok(ratatui::prelude::Position {
            x: col as u16,
            y: row as u16,
        })
    }

    fn set_cursor_position<P: Into<ratatui::prelude::Position>>(
        &mut self,
        position: P,
    ) -> std::io::Result<()> {
        let pos = position.into();

        self.output
            .set_cursor_position(pos.x as usize, pos.y as usize)
            .map_err(|_| std::io::Error::other("Failed to set cursor position"))
    }

    fn clear(&mut self) -> std::io::Result<()> {
        // Best-effort: some firmware consoles return a warning/error from
        // ClearScreen. Don't let that abort the render — the draw loop paints
        // every cell anyway.
        let _ = self.output.clear();
        Ok(())
    }

    fn clear_region(
        &mut self,
        clear_type: ratatui::backend::ClearType,
    ) -> std::io::Result<()> {
        // The UEFI text console only exposes a full-screen clear. Honor that for
        // `All`; treat partial clears as a no-op so the normal draw loop (which
        // diffs cells and overwrites in place) is unaffected.
        match clear_type {
            ratatui::backend::ClearType::All => self.clear(),
            _ => Ok(()),
        }
    }

    fn size(&self) -> std::io::Result<ratatui::prelude::Size> {
        // Defensive: a console may fail to report a mode, or report 0x0 before
        // SetMode. Fall back to the UEFI-standard 80x25 and use saturating math
        // so we never underflow `rows - 2` into a giant (panicking) buffer size.
        let (cols, rows) = match self.output.current_mode() {
            Ok(Some(mode)) => (mode.columns(), mode.rows()),
            _ => (80, 25),
        };
        let cols = if cols == 0 { 80 } else { cols };
        let rows = if rows < 3 { 25 } else { rows };

        Ok(ratatui::prelude::Size {
            width: cols as u16,
            height: (rows - 2) as u16,
        })
    }

    fn window_size(&mut self) -> std::io::Result<ratatui::backend::WindowSize> {
        let size = self.size()?;

        // TODO: Fill out pixel dimensions?
        Ok(ratatui::backend::WindowSize {
            columns_rows: size,
            pixels: ratatui::prelude::Size {
                width: 0,
                height: 0,
            },
        })
    }

    fn flush(&mut self) -> std::io::Result<()> {
        // No-op: draw writes every run before returning, nothing is buffered here.
        Ok(())
    }
}
