use super::carl_dark::Aesthetix;

/// Tokyo Night theme.
pub struct TokyoNight;

impl Aesthetix for TokyoNight {
    fn name(&self) -> &str {
        "Tokyo Night"
    }

    fn primary_accent_color_visuals(&self) -> eframe::egui::Color32 {
        eframe::egui::Color32::from_rgb(113, 189, 251)
    }

    fn secondary_accent_color_visuals(&self) -> eframe::egui::Color32 {
        eframe::egui::Color32::from_rgb(215, 135, 255)
    }

    fn bg_primary_color_visuals(&self) -> eframe::egui::Color32 {
        eframe::egui::Color32::from_rgb(34, 35, 39)
    }

    fn bg_secondary_color_visuals(&self) -> eframe::egui::Color32 {
        eframe::egui::Color32::from_rgb(28, 29, 33)
    }

    fn bg_triage_color_visuals(&self) -> eframe::egui::Color32 {
        eframe::egui::Color32::from_rgb(41, 42, 46)
    }

    fn bg_auxiliary_color_visuals(&self) -> eframe::egui::Color32 {
        eframe::egui::Color32::from_rgb(31, 32, 36)
    }

    fn bg_contrast_color_visuals(&self) -> eframe::egui::Color32 {
        eframe::egui::Color32::from_rgb(49, 50, 54)
    }

    fn fg_primary_text_color_visuals(&self) -> Option<eframe::egui::Color32> {
        Some(eframe::egui::Color32::from_rgb(196, 200, 213))
    }

    fn fg_success_text_color_visuals(&self) -> eframe::egui::Color32 {
        eframe::egui::Color32::from_rgb(80, 250, 123)
    }

    fn fg_warn_text_color_visuals(&self) -> eframe::egui::Color32 {
        eframe::egui::Color32::from_rgb(255, 215, 64)
    }

    fn fg_error_text_color_visuals(&self) -> eframe::egui::Color32 {
        eframe::egui::Color32::from_rgb(229, 46, 47)
    }

    fn fg_info_color_visuals(&self) -> eframe::egui::Color32 {
        eframe::egui::Color32::from_rgb(113, 189, 251)
    }

    fn dark_mode_visuals(&self) -> bool {
        true
    }

    fn margin_style(&self) -> i8 {
        12
    }

    fn button_padding(&self) -> eframe::egui::Vec2 {
        eframe::egui::Vec2 { x: 5.0, y: 3.0 }
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
}

/// Tokyo Night Storm.
pub struct TokyoNightStorm;


impl Aesthetix for TokyoNightStorm {
    fn name(&self) -> &'static str {
        "Tokyo Night Storm"
    }

    fn primary_accent_color_visuals(&self) -> eframe::egui::Color32 {
        eframe::egui::Color32::from_rgba_premultiplied(138, 171, 244, 255)
    }

    fn secondary_accent_color_visuals(&self) -> eframe::egui::Color32 {
        eframe::egui::Color32::from_rgba_premultiplied(97, 175, 239, 255)
    }

    fn bg_primary_color_visuals(&self) -> eframe::egui::Color32 {
        eframe::egui::Color32::from_rgba_premultiplied(23, 24, 38, 255)
    }

    fn bg_secondary_color_visuals(&self) -> eframe::egui::Color32 {
        eframe::egui::Color32::from_rgba_premultiplied(31, 31, 51, 255)
    }

    fn bg_triage_color_visuals(&self) -> eframe::egui::Color32 {
        eframe::egui::Color32::from_rgba_premultiplied(33, 35, 53, 255)
    }

    fn bg_auxiliary_color_visuals(&self) -> eframe::egui::Color32 {
        eframe::egui::Color32::from_rgba_premultiplied(27, 29, 45, 255)
    }

    fn bg_contrast_color_visuals(&self) -> eframe::egui::Color32 {
        eframe::egui::Color32::from_rgba_premultiplied(42, 42, 68, 255)
    }

    fn fg_primary_text_color_visuals(&self) -> Option<eframe::egui::Color32> {
        Some(eframe::egui::Color32::from_rgba_premultiplied(204, 204, 204, 255))
    }

    fn fg_success_text_color_visuals(&self) -> eframe::egui::Color32 {
        eframe::egui::Color32::from_rgba_premultiplied(86, 209, 123, 255)
    }

    fn fg_warn_text_color_visuals(&self) -> eframe::egui::Color32 {
        eframe::egui::Color32::from_rgba_premultiplied(255, 161, 90, 255)
    }

    fn fg_error_text_color_visuals(&self) -> eframe::egui::Color32 {
        eframe::egui::Color32::from_rgba_premultiplied(255, 121, 121, 255)
    }

    fn dark_mode_visuals(&self) -> bool {
        true
    }

    fn margin_style(&self) -> i8 {
        12
    }

    fn button_padding(&self) -> eframe::egui::Vec2 {
        eframe::egui::Vec2 { x: 5.0, y: 3.0 }
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
}
