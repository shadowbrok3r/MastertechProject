//! Rerun-inspired dark theme with MTech pink accents.
//!
//! Neutral near-black surfaces and crisp typography modeled on
//! <https://github.com/rerun-io/rerun>'s `re_ui` design tokens, with
//! the blue accent swapped for the hot pink from `masterlogoV3.png`.

use eframe::egui;

use super::carl_dark::Aesthetix;

pub struct RerunMtech;

impl Aesthetix for RerunMtech {
    fn name(&self) -> &'static str {
        "Rerun MTech"
    }

    fn primary_accent_color_visuals(&self) -> egui::Color32 {
        egui::Color32::from_rgb(255, 56, 130)
    }

    fn secondary_accent_color_visuals(&self) -> egui::Color32 {
        egui::Color32::from_rgb(220, 38, 102)
    }

    fn bg_primary_color_visuals(&self) -> egui::Color32 {
        egui::Color32::from_rgb(13, 13, 13)
    }

    fn bg_secondary_color_visuals(&self) -> egui::Color32 {
        egui::Color32::from_rgb(20, 20, 20)
    }

    fn bg_triage_color_visuals(&self) -> egui::Color32 {
        egui::Color32::from_rgb(28, 28, 28)
    }

    fn bg_auxiliary_color_visuals(&self) -> egui::Color32 {
        egui::Color32::from_rgb(24, 24, 24)
    }

    fn bg_contrast_color_visuals(&self) -> egui::Color32 {
        egui::Color32::from_rgb(60, 60, 60)
    }

    fn fg_primary_text_color_visuals(&self) -> Option<egui::Color32> {
        Some(egui::Color32::from_rgb(220, 220, 220))
    }

    fn fg_success_text_color_visuals(&self) -> egui::Color32 {
        egui::Color32::from_rgb(72, 199, 142)
    }

    fn fg_warn_text_color_visuals(&self) -> egui::Color32 {
        egui::Color32::from_rgb(255, 138, 76)
    }

    fn fg_error_text_color_visuals(&self) -> egui::Color32 {
        egui::Color32::from_rgb(255, 73, 99)
    }

    fn fg_info_color_visuals(&self) -> egui::Color32 {
        egui::Color32::from_rgb(125, 195, 255)
    }

    fn dark_mode_visuals(&self) -> bool {
        true
    }

    fn margin_style(&self) -> i8 {
        12
    }

    fn button_padding(&self) -> egui::Vec2 {
        egui::Vec2 { x: 5.0, y: 3.0 }
    }

    fn item_spacing_style(&self) -> f32 {
        3.0
    }

    fn scroll_bar_width_style(&self) -> f32 {
        6.0
    }

    fn rounding_visuals(&self) -> u8 {
        6
    }

    fn custom_text_styles(
        &self,
    ) -> std::collections::BTreeMap<egui::TextStyle, egui::FontId> {
        use egui::FontFamily::Monospace;
        [
            (egui::TextStyle::Small, egui::FontId::new(10.0, Monospace)),
            (egui::TextStyle::Body, egui::FontId::new(14.0, Monospace)),
            (egui::TextStyle::Button, egui::FontId::new(14.0, Monospace)),
            (egui::TextStyle::Heading, egui::FontId::new(18.0, Monospace)),
            (egui::TextStyle::Monospace, egui::FontId::new(12.0, Monospace)),
        ]
        .into()
    }
}
