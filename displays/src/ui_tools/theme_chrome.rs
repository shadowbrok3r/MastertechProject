//! Baseline widget chrome presets for theme picker and startup.

use eframe::egui::{Style, Visuals};
use once_cell::sync::Lazy;

// style_from_json migrates styles saved under older egui versions instead of failing strict decode.
static SHIPPED_CHROME: Lazy<Style> = Lazy::new(|| {
    crate::ui_tools::style_from_json(crate::STYLE.as_bytes())
        .expect("STYLE JSON must deserialize to egui::Style")
});

static LEGACY_CLASSIC_CHROME: Lazy<Style> = Lazy::new(|| {
    crate::ui_tools::style_from_json(include_bytes!("legacy_classic_style.json"))
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

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{FontFamily, TextStyle};

    // Fails if legacy_classic_style.json stops migrating to the current egui Style schema.
    #[test]
    fn legacy_classic_chrome_migrates_across_egui_versions() {
        assert_ne!(legacy_classic_chrome(), Style::default());
    }

    #[test]
    fn shipped_chrome_is_not_default_egui() {
        let shipped = shipped_chrome();
        let default = Style::default();
        assert_eq!(
            shipped.text_styles.get(&TextStyle::Body).map(|f| f.family.clone()),
            Some(FontFamily::Monospace),
        );
        assert_ne!(shipped.spacing.item_spacing, default.spacing.item_spacing);
    }
}
