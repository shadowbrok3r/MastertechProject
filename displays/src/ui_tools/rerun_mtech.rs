//! Rerun-inspired dark theme with MTech pink accents.
//!
//! Neutral near-black surfaces and borderless widgets modeled on
//! <https://github.com/rerun-io/rerun>'s `re_ui` design tokens.

use eframe::egui;
use eframe::egui::style::{Selection, WidgetVisuals, Widgets};
use eframe::egui::{CornerRadius, Stroke, Visuals};

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
        egui::Color32::from_rgb(47, 47, 47)
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

    fn custom_text_styles(
        &self,
    ) -> std::collections::BTreeMap<egui::TextStyle, egui::FontId> {
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

    fn custom_noninteractive_widget_visuals(&self) -> WidgetVisuals {
        WidgetVisuals {
            bg_fill: self.bg_primary_color_visuals(),
            weak_bg_fill: self.bg_secondary_color_visuals(),
            bg_stroke: Stroke::NONE,
            corner_radius: self.corner_radius(),
            fg_stroke: Stroke::new(1.0, self.fg_primary_text_color_visuals().unwrap_or_default()),
            expansion: 0.0,
        }
    }

    fn widget_inactive_visual(&self) -> WidgetVisuals {
        WidgetVisuals {
            bg_fill: self.bg_secondary_color_visuals(),
            weak_bg_fill: self.bg_secondary_color_visuals(),
            bg_stroke: Stroke::NONE,
            corner_radius: self.corner_radius(),
            fg_stroke: Stroke::new(1.0, self.fg_primary_text_color_visuals().unwrap_or_default()),
            expansion: 0.0,
        }
    }

    fn widget_hovered_visual(&self) -> WidgetVisuals {
        WidgetVisuals {
            bg_fill: self.bg_triage_color_visuals(),
            weak_bg_fill: self.bg_triage_color_visuals(),
            bg_stroke: Stroke::NONE,
            corner_radius: self.corner_radius(),
            fg_stroke: Stroke::new(1.0, self.fg_primary_text_color_visuals().unwrap_or_default()),
            expansion: 0.0,
        }
    }

    fn custom_active_widget_visual(&self) -> WidgetVisuals {
        WidgetVisuals {
            bg_fill: self.bg_auxiliary_color_visuals(),
            weak_bg_fill: self.bg_auxiliary_color_visuals(),
            bg_stroke: Stroke::new(1.0, self.bg_contrast_color_visuals()),
            corner_radius: self.corner_radius(),
            fg_stroke: Stroke::new(1.0, self.fg_primary_text_color_visuals().unwrap_or_default()),
            expansion: 0.0,
        }
    }

    fn custom_open_widget_visual(&self) -> WidgetVisuals {
        WidgetVisuals {
            bg_fill: self.bg_triage_color_visuals(),
            weak_bg_fill: self.bg_triage_color_visuals(),
            bg_stroke: Stroke::NONE,
            corner_radius: self.corner_radius(),
            fg_stroke: Stroke::new(1.0, self.fg_primary_text_color_visuals().unwrap_or_default()),
            expansion: 0.0,
        }
    }

    fn custom_selection_visual(&self) -> Selection {
        Selection {
            bg_fill: self.primary_accent_color_visuals().gamma_multiply(0.22),
            stroke: Stroke::new(1.0, self.primary_accent_color_visuals()),
        }
    }

    fn spacing_style(&self) -> egui::style::Spacing {
        egui::style::Spacing {
            item_spacing: egui::Vec2 {
                x: self.item_spacing_style(),
                y: self.item_spacing_style(),
            },
            window_margin: egui::Margin {
                left: self.margin_style(),
                right: self.margin_style(),
                top: self.margin_style(),
                bottom: self.margin_style(),
            },
            button_padding: self.button_padding(),
            menu_margin: egui::Margin {
                left: self.margin_style(),
                right: self.margin_style(),
                top: self.margin_style(),
                bottom: self.margin_style(),
            },
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
                bar_width: self.scroll_bar_width_style(),
                handle_min_length: 12.0,
                bar_inner_margin: 2.0,
                bar_outer_margin: 2.0,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    fn interaction_style(&self) -> egui::style::Interaction {
        egui::style::Interaction {
            resize_grab_radius_side: 5.0,
            resize_grab_radius_corner: 10.0,
            show_tooltips_only_when_still: true,
            tooltip_delay: 0.35,
            ..Default::default()
        }
    }

    fn custom_style(&self) -> egui::Style {
        let text_color = self.fg_primary_text_color_visuals();
        egui::style::Style {
            override_text_style: None,
            override_font_id: None,
            text_styles: self.custom_text_styles(),
            spacing: self.spacing_style(),
            interaction: self.interaction_style(),
            visuals: Visuals {
                dark_mode: self.dark_mode_visuals(),
                override_text_color: text_color,
                widgets: Widgets {
                    noninteractive: self.custom_noninteractive_widget_visuals(),
                    inactive: self.widget_inactive_visual(),
                    hovered: self.widget_hovered_visual(),
                    active: self.custom_active_widget_visual(),
                    open: self.custom_open_widget_visual(),
                },
                selection: self.custom_selection_visual(),
                hyperlink_color: text_color.unwrap_or_default(),
                panel_fill: self.bg_primary_color_visuals(),
                faint_bg_color: self.bg_secondary_color_visuals(),
                extreme_bg_color: self.bg_triage_color_visuals(),
                code_bg_color: self.bg_auxiliary_color_visuals(),
                warn_fg_color: self.fg_warn_text_color_visuals(),
                error_fg_color: self.fg_error_text_color_visuals(),
                window_corner_radius: self.corner_radius(),
                window_shadow: egui::epaint::Shadow {
                    spread: 12,
                    color: egui::Color32::from_black_alpha(77),
                    ..Default::default()
                },
                window_fill: self.bg_primary_color_visuals(),
                window_stroke: Stroke::NONE,
                menu_corner_radius: self.corner_radius(),
                popup_shadow: egui::epaint::Shadow {
                    spread: 8,
                    color: egui::Color32::from_black_alpha(60),
                    ..Default::default()
                },
                resize_corner_size: 10.0,
                clip_rect_margin: 4.0,
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
}

impl RerunMtech {
    fn corner_radius(&self) -> CornerRadius {
        let r = self.rounding_visuals();
        CornerRadius {
            nw: r,
            ne: r,
            sw: r,
            se: r,
        }
    }
}
