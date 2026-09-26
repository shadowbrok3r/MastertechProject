//! Phosphor icon glyphs for egui.
//!
//! The definitions live in `mtech-ui` so the lower UI crates (and the shared
//! stress dashboard) can use the same glyph set; this stays the canonical path
//! every call site imports from.

pub use mtech_ui::icons::*;

pub const USER: &str = p::USER;
pub const PAPERCLIP: &str = p::PAPERCLIP;
pub const QUEUE: &str = p::QUEUE;
pub const COMPACT: &str = p::ARROWS_IN_LINE_VERTICAL;
pub const SEND: &str = p::PAPER_PLANE_RIGHT;
pub const SEND_NOW: &str = p::LIGHTNING;
pub const APPROVE_ALL: &str = p::CHECKS;
pub const UNLOCKED: &str = p::LOCK_OPEN;
pub const PIN: &str = p::PUSH_PIN;
pub const UNPIN: &str = p::PUSH_PIN_SLASH;
