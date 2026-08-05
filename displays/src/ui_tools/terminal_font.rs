//! Terminal font face, shared by the ratatui surfaces and the egui command shell.
//! Sits outside `remote_viewer` so wasm, which drops ratatui, can still resolve it.

use eframe::egui::{FontFamily, FontId};

/// Family every terminal surface renders with, registered as a single face in
/// `app_state::font_definitions`.
pub const TERMINAL_FONT_FAMILY: &str = "CascadiaMono";

/// Point size the terminal surfaces start at before any `fit_font_to_grid`.
pub const TERMINAL_FONT_SIZE: u16 = 12;

/// A `FontId` in the terminal family.
pub fn terminal_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(TERMINAL_FONT_FAMILY.into()))
}
