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
        ratatui::style::Color::Rgb(..)
        | ratatui::style::Color::Indexed(_)
        | ratatui::style::Color::Reset => None,
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
        for (x, y, cell) in content {
            let mut fg = to_uefi_color(cell.fg).unwrap_or(console::text::Color::White);
            let mut bg = to_uefi_color(cell.bg).unwrap_or(console::text::Color::Black);

            if cell.modifier.contains(ratatui::style::Modifier::REVERSED) {
                // Swap foreground and background colors.
                std::mem::swap(&mut fg, &mut bg);
            }

            // Best-effort, per cell. Real firmware consoles often return a
            // warning/error for glyphs missing from their font (e.g. box
            // drawing, middle dot) which the uefi crate surfaces as an `Err`.
            // Propagating it would abort the entire frame and the app would
            // exit; instead we ignore per-cell failures. Alignment is safe
            // because the cursor is repositioned absolutely for every cell.
            let _ = self.output.set_cursor_position(x as usize, y as usize);
            let _ = self.output.set_color(fg, bg);
            if self.output.write_str(cell.symbol()).is_err() {
                // Substitute a guaranteed-ASCII glyph so layout stays readable.
                let _ = self.output.write_str(" ");
            }
        }

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
        // No-op?
        Ok(())
    }
}
