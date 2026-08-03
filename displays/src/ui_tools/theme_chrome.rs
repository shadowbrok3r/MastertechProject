//! Baseline widget chrome presets for theme picker and startup.

use eframe::egui::{Color32, Shadow, Stroke, Style, Visuals};
use once_cell::sync::Lazy;

use crate::ui_tools::glass_backdrop::GlassParams;
use crate::ui_tools::mtech_glass::glassify;

// style_from_json migrates styles saved under older egui versions instead of failing strict decode.
static SHIPPED_CHROME: Lazy<Style> = Lazy::new(|| {
    crate::ui_tools::style_from_json(crate::STYLE.as_bytes())
        .expect("STYLE JSON must deserialize to egui::Style")
});

static LEGACY_CLASSIC_CHROME: Lazy<Style> = Lazy::new(|| {
    crate::ui_tools::style_from_json(include_bytes!("legacy_classic_style.json"))
        .expect("legacy_classic_style.json must deserialize to egui::Style")
});

static MTECH_NOIR_CHROME: Lazy<Style> = Lazy::new(|| {
    crate::ui_tools::style_from_json(include_bytes!("mtech_noir_style.json"))
        .expect("mtech_noir_style.json must deserialize to egui::Style")
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

/// OLED-black chrome with violet strokes, crimson press, cyan warn and rose error.
pub fn mtech_noir_chrome() -> Style {
    MTECH_NOIR_CHROME.clone()
}

// Floating surfaces of the glass variant sit this opaque over the frosted backdrop: heavy enough
// to read text on unblurred, sheer enough that the blur is the dominant material when it is live.
const NOIR_GLASS_WINDOW_ALPHA: f32 = 0.55;
const NOIR_GLASS_EDGE: Color32 = Color32::from_rgb(136, 128, 219);
const NOIR_GLASS_GLOW: Color32 = Color32::from_rgb(52, 40, 120);

/// [`mtech_noir_chrome`] as tinted glass over a real blurred backdrop: translucent widget panes
/// from [`glassify`], and floating surfaces sheer enough for [`mtech_noir_glass_params`]'s frost to
/// read through them.
pub fn mtech_noir_glass_chrome() -> Style {
    let mut style = glassify(&mtech_noir_chrome());
    let v = &mut style.visuals;
    // glassify leaves windows near-opaque (0.93); the frost needs a far sheerer pane.
    v.window_fill = Color32::from_rgb(6, 5, 12).gamma_multiply(NOIR_GLASS_WINDOW_ALPHA);
    v.window_stroke = Stroke::new(1.0, NOIR_GLASS_EDGE.gamma_multiply(0.55));
    v.window_shadow = Shadow {
        offset: [0, 0],
        blur: 20,
        spread: 2,
        color: NOIR_GLASS_GLOW.gamma_multiply(0.38),
    };
    v.popup_shadow = Shadow {
        offset: [0, 0],
        blur: 14,
        spread: 1,
        color: NOIR_GLASS_GLOW.gamma_multiply(0.30),
    };
    style
}

/// The glass material [`mtech_noir_glass_chrome`] is drawn against: a wide blur under a violet-black
/// film thin enough that the blur, not the film, is what reads.
pub fn mtech_noir_glass_params() -> GlassParams {
    GlassParams {
        enabled: true,
        blur_radius: 28.0,
        tint: Color32::from_rgba_unmultiplied(14, 12, 26, 72),
        // Matches the menu corner radius the modal window frame uses.
        corner_radius: 6.0,
        presence: 1.0,
    }
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

    // Fails if mtech_noir_style.json stops migrating to the current egui Style schema.
    #[test]
    fn mtech_noir_chrome_decodes_to_its_own_palette() {
        let noir = mtech_noir_chrome();
        assert_ne!(noir, Style::default());
        assert_eq!(noir.visuals.panel_fill, Color32::BLACK);
        assert_eq!(noir.visuals.warn_fg_color, Color32::from_rgb(76, 219, 255));
        assert_eq!(noir.visuals.error_fg_color, Color32::from_rgb(255, 73, 137));
    }

    #[test]
    fn mtech_noir_glass_chrome_keeps_the_palette_but_opens_up_windows() {
        let noir = mtech_noir_chrome();
        let glass = mtech_noir_glass_chrome();

        // Same palette and geometry: only the surfaces change.
        assert_eq!(glass.visuals.panel_fill, noir.visuals.panel_fill);
        assert_eq!(glass.visuals.warn_fg_color, noir.visuals.warn_fg_color);
        assert_eq!(glass.text_styles, noir.text_styles);
        assert_eq!(glass.spacing, noir.spacing);

        // Windows are sheer enough for a frost to read through, and widget panes are translucent.
        assert!(glass.visuals.window_fill.a() < 180);
        assert!(glass.visuals.widgets.hovered.weak_bg_fill.a() < 255);
    }

    // The glass chrome is meaningless without a material that actually draws.
    #[test]
    fn mtech_noir_glass_params_are_visible() {
        assert!(mtech_noir_glass_params().is_visible());
    }
}
