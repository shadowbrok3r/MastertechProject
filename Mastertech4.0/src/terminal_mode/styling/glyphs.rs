//! Single-cell glyphs for the TUI grid.
//! Every glyph must exist in CascadiaMono at 0.586 em, or the row layout and
//! the backend's `floor(dx / char_width)` mouse mapping drift.

/// Checklist marker for the given state.
pub const fn checkbox(checked: bool) -> &'static str {
    if checked {
        "\u{2713}" // ✓
    } else {
        "\u{25cb}" // ○
    }
}

/// Status and file markers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    Ok,
    Warning,
    Close,
    Private,
    Folder,
    File,
}

impl Glyph {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "\u{2713}",      // ✓
            Self::Warning => "\u{25b2}", // ▲
            Self::Close => "\u{2573}",   // ╳
            Self::Private => "\u{00a7}", // §
            Self::Folder => "\u{25b8}",  // ▸
            Self::File => "\u{25aa}",    // ▪
        }
    }
}

pub const SCROLL_UP: &str = "\u{25b2}"; // ▲
pub const SCROLL_DOWN: &str = "\u{25bc}"; // ▼
pub const SCROLL_LEFT: &str = "\u{25c0}"; // ◀
pub const SCROLL_RIGHT: &str = "\u{25b6}"; // ▶
pub const SCROLL_TRACK_V: &str = "\u{2502}"; // │
pub const SCROLL_TRACK_H: &str = "\u{2500}"; // ─
pub const SCROLL_THUMB: &str = "\u{2588}"; // █

// Coverage of these glyphs is asserted by displays/tests/tui_font_coverage.rs.
