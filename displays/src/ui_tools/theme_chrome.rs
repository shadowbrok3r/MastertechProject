//! Baseline widget chrome presets for theme picker and startup.

use eframe::egui::{Style, Visuals};
use once_cell::sync::Lazy;

static SHIPPED_CHROME: Lazy<Style> = Lazy::new(|| {
    serde_json::from_str(crate::STYLE).expect("STYLE JSON must deserialize to egui::Style")
});

static LEGACY_CLASSIC_CHROME: Lazy<Style> = Lazy::new(|| {
    serde_json::from_str(include_str!("legacy_classic_style.json"))
        .expect("legacy_classic_style.json must deserialize to egui::Style")
});

static DEFAULT_EGUI_CHROME: Lazy<Style> = Lazy::new(|| {
    let mut style = Style::default();
    style.visuals = Visuals::dark();
    style
});

/// Shipped monospace theme from [`crate::STYLE`].
pub fn shipped_chrome() -> Style {
    SHIPPED_CHROME.clone()
}

/// Proportional legacy classic chrome (uploaded preset).
pub fn legacy_classic_chrome() -> Style {
    LEGACY_CLASSIC_CHROME.clone()
}

/// Vanilla egui dark mode defaults.
pub fn default_egui_chrome() -> Style {
    DEFAULT_EGUI_CHROME.clone()
}
