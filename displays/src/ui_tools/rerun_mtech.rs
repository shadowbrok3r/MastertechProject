//! Rerun-inspired palettes with optional flat widget chrome.

use eframe::egui;
use eframe::egui::style::{Selection, WidgetVisuals, Widgets};
use eframe::egui::{CornerRadius, Stroke, Visuals};

use super::carl_dark::Aesthetix;

pub struct RerunMtech;
pub struct RerunMtechOled;

macro_rules! rerun_palette {
    ($ty:ty, $name:expr, $primary:expr, $secondary:expr, $bg0:expr, $bg1:expr, $bg2:expr, $bg3:expr, $bg4:expr, $text:expr) => {
        impl Aesthetix for $ty {
            fn name(&self) -> &'static str {
                $name
            }

            fn primary_accent_color_visuals(&self) -> egui::Color32 {
                $primary
            }

            fn secondary_accent_color_visuals(&self) -> egui::Color32 {
                $secondary
            }

            fn bg_primary_color_visuals(&self) -> egui::Color32 {
                $bg0
            }

            fn bg_secondary_color_visuals(&self) -> egui::Color32 {
                $bg1
            }

            fn bg_triage_color_visuals(&self) -> egui::Color32 {
                $bg2
            }

            fn bg_auxiliary_color_visuals(&self) -> egui::Color32 {
                $bg3
            }

            fn bg_contrast_color_visuals(&self) -> egui::Color32 {
                $bg4
            }

            fn fg_primary_text_color_visuals(&self) -> Option<egui::Color32> {
                Some($text)
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
                16
            }

            fn button_padding(&self) -> egui::Vec2 {
                egui::Vec2 { x: 10.0, y: 6.0 }
            }

            fn item_spacing_style(&self) -> f32 {
                6.0
            }

            fn scroll_bar_width_style(&self) -> f32 {
                6.0
            }

            fn rounding_visuals(&self) -> u8 {
                6
            }

            fn custom_style(&self) -> egui::Style {
                rerun_flat_style(self)
            }
        }
    };
}

rerun_palette!(
    RerunMtech,
    "Rerun MTech",
    egui::Color32::from_rgb(255, 56, 130),
    egui::Color32::from_rgb(220, 38, 102),
    egui::Color32::from_rgb(13, 13, 13),
    egui::Color32::from_rgb(20, 20, 20),
    egui::Color32::from_rgb(28, 28, 28),
    egui::Color32::from_rgb(24, 24, 24),
    egui::Color32::from_rgb(47, 47, 47),
    egui::Color32::from_rgb(220, 220, 220)
);

rerun_palette!(
    RerunMtechOled,
    "Rerun MTech OLED",
    egui::Color32::from_rgb(255, 56, 130),
    egui::Color32::from_rgb(220, 38, 102),
    egui::Color32::from_rgb(0, 0, 0),
    egui::Color32::from_rgb(3, 3, 3),
    egui::Color32::from_rgb(10, 10, 10),
    egui::Color32::from_rgb(6, 6, 6),
    egui::Color32::from_rgb(36, 36, 36),
    egui::Color32::from_rgb(232, 232, 232)
);

pub fn rerun_flat_style(theme: &dyn Aesthetix) -> egui::Style {
    let text_color = theme.fg_primary_text_color_visuals();
    let corner = corner_radius(theme.rounding_visuals());
    egui::style::Style {
        override_text_style: None,
        override_font_id: None,
        text_styles: rerun_text_styles(),
        spacing: rerun_spacing(theme),
        interaction: rerun_interaction(),
        visuals: Visuals {
            dark_mode: theme.dark_mode_visuals(),
            override_text_color: text_color,
            widgets: Widgets {
                noninteractive: flat_noninteractive(theme, corner),
                inactive: flat_inactive(theme, corner),
                hovered: flat_hovered(theme, corner),
                active: flat_active(theme, corner),
                open: flat_open(theme, corner),
            },
            selection: flat_selection(theme),
            hyperlink_color: text_color.unwrap_or_default(),
            panel_fill: theme.bg_primary_color_visuals(),
            faint_bg_color: theme.bg_secondary_color_visuals(),
            extreme_bg_color: theme.bg_triage_color_visuals(),
            code_bg_color: theme.bg_auxiliary_color_visuals(),
            warn_fg_color: theme.fg_warn_text_color_visuals(),
            error_fg_color: theme.fg_error_text_color_visuals(),
            window_corner_radius: corner,
            window_shadow: egui::epaint::Shadow {
                spread: 12,
                color: egui::Color32::from_black_alpha(77),
                ..Default::default()
            },
            window_fill: theme.bg_primary_color_visuals(),
            window_stroke: Stroke::NONE,
            menu_corner_radius: corner,
            popup_shadow: egui::epaint::Shadow {
                spread: 8,
                color: egui::Color32::from_black_alpha(60),
                ..Default::default()
            },
            resize_corner_size: 10.0,
            button_frame: false,
            collapsing_header_frame: false,
            indent_has_left_vline: false,
            striped: false,
            slider_trailing_fill: true,
            image_loading_spinners: false,
            ..Default::default()
        },
        animation_time: 0.083_333_336,
        explanation_tooltips: true,
        ..Default::default()
    }
}

fn rerun_text_styles() -> std::collections::BTreeMap<egui::TextStyle, egui::FontId> {
    use egui::FontFamily::{Monospace, Proportional};
    [
        (
            egui::TextStyle::Small,
            egui::FontId::new(11.0, Proportional),
        ),
        (egui::TextStyle::Body, egui::FontId::new(14.0, Proportional)),
        (
            egui::TextStyle::Button,
            egui::FontId::new(14.0, Proportional),
        ),
        (
            egui::TextStyle::Heading,
            egui::FontId::new(20.0, Proportional),
        ),
        (
            egui::TextStyle::Monospace,
            egui::FontId::new(12.0, Monospace),
        ),
    ]
    .into()
}

fn rerun_spacing(theme: &dyn Aesthetix) -> egui::style::Spacing {
    egui::style::Spacing {
        item_spacing: egui::Vec2::splat(6.0),
        window_margin: egui::Margin::same(16),
        button_padding: egui::Vec2 { x: 10.0, y: 6.0 },
        menu_margin: egui::Margin::same(16),
        indent: 18.0,
        interact_size: egui::Vec2 { x: 40.0, y: 24.0 },
        slider_width: 100.0,
        combo_width: 8.0,
        text_edit_width: 280.0,
        icon_width: 14.0,
        icon_width_inner: 8.0,
        icon_spacing: 6.0,
        tooltip_width: 720.0,
        indent_ends_with_horizontal_line: false,
        combo_height: 200.0,
        scroll: egui::style::ScrollStyle {
            bar_width: theme.scroll_bar_width_style(),
            handle_min_length: 12.0,
            bar_inner_margin: 2.0,
            bar_outer_margin: 2.0,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn rerun_interaction() -> egui::style::Interaction {
    egui::style::Interaction {
        resize_grab_radius_side: 5.0,
        resize_grab_radius_corner: 10.0,
        show_tooltips_only_when_still: true,
        tooltip_delay: 0.35,
        ..Default::default()
    }
}

fn corner_radius(r: u8) -> CornerRadius {
    CornerRadius {
        nw: r,
        ne: r,
        sw: r,
        se: r,
    }
}

fn flat_noninteractive(theme: &dyn Aesthetix, corner: CornerRadius) -> WidgetVisuals {
    WidgetVisuals {
        bg_fill: theme.bg_primary_color_visuals(),
        weak_bg_fill: theme.bg_secondary_color_visuals(),
        bg_stroke: Stroke::NONE,
        corner_radius: corner,
        fg_stroke: Stroke::new(1.0, theme.fg_primary_text_color_visuals().unwrap_or_default()),
        expansion: 0.0,
    }
}

fn flat_inactive(theme: &dyn Aesthetix, corner: CornerRadius) -> WidgetVisuals {
    WidgetVisuals {
        bg_fill: theme.bg_secondary_color_visuals(),
        weak_bg_fill: theme.bg_secondary_color_visuals(),
        bg_stroke: Stroke::NONE,
        corner_radius: corner,
        fg_stroke: Stroke::new(1.0, theme.fg_primary_text_color_visuals().unwrap_or_default()),
        expansion: 0.0,
    }
}

fn flat_hovered(theme: &dyn Aesthetix, corner: CornerRadius) -> WidgetVisuals {
    WidgetVisuals {
        bg_fill: theme.bg_triage_color_visuals(),
        weak_bg_fill: theme.bg_triage_color_visuals(),
        bg_stroke: Stroke::NONE,
        corner_radius: corner,
        fg_stroke: Stroke::new(1.0, theme.fg_primary_text_color_visuals().unwrap_or_default()),
        expansion: 0.0,
    }
}

fn flat_active(theme: &dyn Aesthetix, corner: CornerRadius) -> WidgetVisuals {
    WidgetVisuals {
        bg_fill: theme.bg_auxiliary_color_visuals(),
        weak_bg_fill: theme.bg_auxiliary_color_visuals(),
        bg_stroke: Stroke::new(1.0, theme.bg_contrast_color_visuals()),
        corner_radius: corner,
        fg_stroke: Stroke::new(1.0, theme.fg_primary_text_color_visuals().unwrap_or_default()),
        expansion: 0.0,
    }
}

fn flat_open(theme: &dyn Aesthetix, corner: CornerRadius) -> WidgetVisuals {
    WidgetVisuals {
        bg_fill: theme.bg_triage_color_visuals(),
        weak_bg_fill: theme.bg_triage_color_visuals(),
        bg_stroke: Stroke::NONE,
        corner_radius: corner,
        fg_stroke: Stroke::new(1.0, theme.fg_primary_text_color_visuals().unwrap_or_default()),
        expansion: 0.0,
    }
}

fn flat_selection(theme: &dyn Aesthetix) -> Selection {
    Selection {
        bg_fill: theme.primary_accent_color_visuals().gamma_multiply(0.22),
        stroke: Stroke::new(1.0, theme.primary_accent_color_visuals()),
    }
}
