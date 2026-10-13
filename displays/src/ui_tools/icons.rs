//! Phosphor icon glyphs for egui.
//!
//! The definitions live in `mtech-ui` so the lower UI crates (and the shared
//! stress dashboard) can use the same glyph set; this stays the canonical path
//! every call site imports from.

pub use mtech_ui::icon_pack as svg;
pub use mtech_ui::icons::*;

/// An SVG pack icon, or a font glyph that paints through the pack when the pack has it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconRef<'a> {
    Svg(svg::Icon),
    Glyph(&'a str),
}

impl IconRef<'_> {
    /// The pack icon; `None` for a glyph the pack lacks.
    pub fn icon(self) -> Option<svg::Icon> {
        match self {
            Self::Svg(icon) => Some(icon),
            Self::Glyph(glyph) => svg::Icon::from_glyph(glyph),
        }
    }

    /// The font glyph.
    pub fn glyph(&self) -> &str {
        match self {
            Self::Svg(icon) => icon.glyph(),
            Self::Glyph(glyph) => glyph,
        }
    }
}

impl From<svg::Icon> for IconRef<'_> {
    fn from(icon: svg::Icon) -> Self {
        Self::Svg(icon)
    }
}

impl<'a> From<&'a str> for IconRef<'a> {
    fn from(glyph: &'a str) -> Self {
        Self::Glyph(glyph)
    }
}

/// Icon of every create-task action.
pub const CREATE_TASK: svg::Icon = svg::PLUS;

pub const USER: &str = p::USER;
pub const PAPERCLIP: &str = p::PAPERCLIP;
pub const QUEUE: &str = p::QUEUE;
pub const SEND: &str = p::PAPER_PLANE_RIGHT;
pub const PIN: &str = p::PUSH_PIN;
pub const SIDEBAR: &str = p::SIDEBAR_SIMPLE;
pub const SNOOZE: &str = p::ALARM;
pub const VOICE: &str = p::MICROPHONE;
pub const RECORDS: &str = p::DATABASE;
