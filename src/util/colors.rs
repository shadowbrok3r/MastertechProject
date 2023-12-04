use egui::{
    epaint, style, Color32, epaint::Shadow,
    style::{Interaction, Margin, Selection, Spacing, WidgetVisuals, Widgets},
    Rounding, Stroke, Style, Vec2, Visuals};


/// Apply the given theme to a [`Context`](egui::Context).
pub fn set_theme(ctx: &egui::Context, theme: Theme) {
    let old = ctx.style().visuals.clone();
    ctx.set_visuals(theme.visuals(old));
}

/// Apply the given theme to a [`Style`](egui::Style).
///
/// # Example
///
/// ```rust
/// # use egui::__run_test_ctx;
/// # __run_test_ctx(|ctx| {
/// let mut style = (*ctx.style()).clone();
/// catppuccin_egui::set_style_theme(&mut style, catppuccin_egui::MACCHIATO);
/// ctx.set_style(style);
/// # });
/// ```
pub fn set_style_theme(style: &mut egui::Style, theme: Theme) {
    let old = style.visuals.clone();
    style.visuals = theme.visuals(old);
}

fn make_widget_visual(
    old: style::WidgetVisuals,
    theme: &Theme,
    bg_fill: egui::Color32,
) -> style::WidgetVisuals {
    style::WidgetVisuals {
        bg_fill,
        weak_bg_fill: bg_fill,
        bg_stroke: egui::Stroke {
            color: theme.overlay1,
            ..old.bg_stroke
        },
        fg_stroke: egui::Stroke {
            color: theme.text,
            ..old.fg_stroke
        },
        ..old
    }
}

/// The colors for a theme variant.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub struct Theme {
    pub rosewater: Color32,
    pub flamingo: Color32,
    pub pink: Color32,
    pub mauve: Color32,
    pub red: Color32,
    pub maroon: Color32,
    pub peach: Color32,
    pub yellow: Color32,
    pub green: Color32,
    pub teal: Color32,
    pub sky: Color32,
    pub sapphire: Color32,
    pub blue: Color32,
    pub lavender: Color32,
    pub text: Color32,
    pub subtext1: Color32,
    pub subtext0: Color32,
    pub overlay2: Color32,
    pub overlay1: Color32,
    pub overlay0: Color32,
    pub surface2: Color32,
    pub surface1: Color32,
    pub surface0: Color32,
    pub base: Color32,
    pub mantle: Color32,
    pub crust: Color32,
}

impl Theme {
    fn visuals(&self, old: egui::Visuals) -> egui::Visuals {
        let is_latte = *self == LATTE;
        egui::Visuals {
            override_text_color: Some(self.text),
            hyperlink_color: self.rosewater,
            faint_bg_color: self.surface0,
            extreme_bg_color: self.crust,
            code_bg_color: self.mantle,
            warn_fg_color: self.peach,
            error_fg_color: self.maroon,
            window_fill: self.base,
            panel_fill: self.base,
            window_stroke: egui::Stroke {
                color: self.overlay1,
                ..old.window_stroke
            },
            widgets: style::Widgets {
                noninteractive: make_widget_visual(old.widgets.noninteractive, self, self.base),
                inactive: make_widget_visual(old.widgets.inactive, self, self.surface0),
                hovered: make_widget_visual(old.widgets.hovered, self, self.surface2),
                active: make_widget_visual(old.widgets.active, self, self.surface1),
                open: make_widget_visual(old.widgets.open, self, self.surface0),
            },
            selection: style::Selection {
                bg_fill: self.blue.linear_multiply(if is_latte { 0.4 } else { 0.2 }),
                stroke: egui::Stroke {
                    color: self.overlay1,
                    ..old.selection.stroke
                },
            },
            window_shadow: epaint::Shadow {
                color: self.base,
                ..old.window_shadow
            },
            popup_shadow: epaint::Shadow {
                color: self.base,
                ..old.popup_shadow
            },
            dark_mode: !is_latte,
            ..old
        }
    }
}

pub const LATTE: Theme = Theme {
    rosewater: Color32::from_rgb(220, 138, 120),
    flamingo: Color32::from_rgb(221, 120, 120),
    pink: Color32::from_rgb(234, 118, 203),
    mauve: Color32::from_rgb(136, 57, 239),
    red: Color32::from_rgb(210, 15, 57),
    maroon: Color32::from_rgb(230, 69, 83),
    peach: Color32::from_rgb(254, 100, 11),
    yellow: Color32::from_rgb(223, 142, 29),
    green: Color32::from_rgb(64, 160, 43),
    teal: Color32::from_rgb(23, 146, 153),
    sky: Color32::from_rgb(4, 165, 229),
    sapphire: Color32::from_rgb(32, 159, 181),
    blue: Color32::from_rgb(30, 102, 245),
    lavender: Color32::from_rgb(114, 135, 253),
    text: Color32::from_rgb(76, 79, 105),
    subtext1: Color32::from_rgb(92, 95, 119),
    subtext0: Color32::from_rgb(108, 111, 133),
    overlay2: Color32::from_rgb(124, 127, 147),
    overlay1: Color32::from_rgb(140, 143, 161),
    overlay0: Color32::from_rgb(156, 160, 176),
    surface2: Color32::from_rgb(172, 176, 190),
    surface1: Color32::from_rgb(188, 192, 204),
    surface0: Color32::from_rgb(204, 208, 218),
    base: Color32::from_rgb(239, 241, 245),
    mantle: Color32::from_rgb(230, 233, 239),
    crust: Color32::from_rgb(220, 224, 232),
};

pub const FRAPPE: Theme = Theme {
    rosewater: Color32::from_rgb(242, 213, 207),
    flamingo: Color32::from_rgb(238, 190, 190),
    pink: Color32::from_rgb(244, 184, 228),
    mauve: Color32::from_rgb(202, 158, 230),
    red: Color32::from_rgb(231, 130, 132),
    maroon: Color32::from_rgb(234, 153, 156),
    peach: Color32::from_rgb(239, 159, 118),
    yellow: Color32::from_rgb(229, 200, 144),
    green: Color32::from_rgb(166, 209, 137),
    teal: Color32::from_rgb(129, 200, 190),
    sky: Color32::from_rgb(153, 209, 219),
    sapphire: Color32::from_rgb(133, 193, 220),
    blue: Color32::from_rgb(140, 170, 238),
    lavender: Color32::from_rgb(186, 187, 241),
    text: Color32::from_rgb(198, 208, 245),
    subtext1: Color32::from_rgb(181, 191, 226),
    subtext0: Color32::from_rgb(165, 173, 206),
    overlay2: Color32::from_rgb(148, 156, 187),
    overlay1: Color32::from_rgb(131, 139, 167),
    overlay0: Color32::from_rgb(115, 121, 148),
    surface2: Color32::from_rgb(98, 104, 128),
    surface1: Color32::from_rgb(81, 87, 109),
    surface0: Color32::from_rgb(65, 69, 89),
    base: Color32::from_rgb(48, 52, 70),
    mantle: Color32::from_rgb(41, 44, 60),
    crust: Color32::from_rgb(35, 38, 52),
};

pub const MACCHIATO: Theme = Theme {
    rosewater: Color32::from_rgb(244, 219, 214),
    flamingo: Color32::from_rgb(240, 198, 198),
    pink: Color32::from_rgb(245, 189, 230),
    mauve: Color32::from_rgb(198, 160, 246),
    red: Color32::from_rgb(237, 135, 150),
    maroon: Color32::from_rgb(238, 153, 160),
    peach: Color32::from_rgb(245, 169, 127),
    yellow: Color32::from_rgb(238, 212, 159),
    green: Color32::from_rgb(166, 218, 149),
    teal: Color32::from_rgb(139, 213, 202),
    sky: Color32::from_rgb(145, 215, 227),
    sapphire: Color32::from_rgb(125, 196, 228),
    blue: Color32::from_rgb(138, 173, 244),
    lavender: Color32::from_rgb(183, 189, 248),
    text: Color32::from_rgb(202, 211, 245),
    subtext1: Color32::from_rgb(184, 192, 224),
    subtext0: Color32::from_rgb(165, 173, 203),
    overlay2: Color32::from_rgb(147, 154, 183),
    overlay1: Color32::from_rgb(128, 135, 162),
    overlay0: Color32::from_rgb(110, 115, 141),
    surface2: Color32::from_rgb(91, 96, 120),
    surface1: Color32::from_rgb(73, 77, 100),
    surface0: Color32::from_rgb(54, 58, 79),
    base: Color32::from_rgb(36, 39, 58),
    mantle: Color32::from_rgb(30, 32, 48),
    crust: Color32::from_rgb(24, 25, 38),
};

pub const MOCHA: Theme = Theme {
    text: Color32::from_rgb(29, 187, 176), // override_text_color
    rosewater: Color32::from_rgb(97, 131, 187), // hyperlink_color
    surface0: Color32::from_rgb(26, 27, 38), // faint_bg_color
    crust: Color32::from_rgb(81, 86, 112), // extreme_bg_color
    mantle: Color32::from_rgb(26, 27, 38), // code_bg_color
    peach: Color32::from_rgb(187, 164, 97), // warn_fg_color
    maroon: Color32::from_rgb(187, 97, 107), // error_fg_color
    base: Color32::from_rgb(26, 27, 38), // panel_fill: RGB(22, 22, 30), // window_fill
    flamingo: Color32::from_rgb(242, 205, 205), // panel_fill
    pink: Color32::from_rgb(245, 194, 231),
    mauve: Color32::from_rgb(203, 166, 247),
    red: Color32::from_rgb(243, 139, 168),
    yellow: Color32::from_rgb(249, 226, 175),
    green: Color32::from_rgb(166, 227, 161),
    teal: Color32::from_rgb(148, 226, 213),
    sky: Color32::from_rgb(137, 220, 235),
    sapphire: Color32::from_rgb(116, 199, 236),
    blue: Color32::from_rgb(137, 180, 250),
    lavender: Color32::from_rgb(180, 190, 254),
    subtext1: Color32::from_rgb(186, 194, 222),
    subtext0: Color32::from_rgb(166, 173, 200),
    overlay2: Color32::from_rgb(147, 153, 178),
    overlay1: Color32::from_rgb(13, 15, 23),
    overlay0: Color32::from_rgb(108, 112, 134),
    surface2: Color32::from_rgb(88, 91, 112),
    surface1: Color32::from_rgb(69, 71, 90),
};
/*
override_text_color: RGB(120, 124, 153)
hyperlink_color: RGB(97, 131, 187)
faint_bg_color: RGB(20, 20, 27)
extreme_bg_color: RGB(81, 86, 112)
code_bg_color: RGB(26, 27, 38)
warn_fg_color: RGB(187, 164, 97)
error_fg_color: RGB(187, 97, 107)
window_fill: RGB(13, 15, 23)
panel_fill: RGB(22, 22, 30)
window_stroke: RGB(13, 15, 23)
widgets:
noninteractive: RGB(20, 20, 27)
inactive: RGB(20, 20, 27)
open: RGB(20, 20, 27)
selection:
bg_fill: The color #515c7e40 includes an alpha channel, and I have retained it in hex format.
stroke: Not available
window_shadow: RGB(13, 15, 23)
popup_shadow: RGB(13, 15, 23)
dark_mode: True
*/

// pub const MOCHA: Theme = Theme {
//     rosewater: Color32::from_rgb(245, 224, 220),
//     flamingo: Color32::from_rgb(242, 205, 205),
//     pink: Color32::from_rgb(245, 194, 231),
//     mauve: Color32::from_rgb(203, 166, 247),
//     red: Color32::from_rgb(243, 139, 168),
//     maroon: Color32::from_rgb(235, 160, 172),
//     peach: Color32::from_rgb(250, 179, 135),
//     yellow: Color32::from_rgb(249, 226, 175),
//     green: Color32::from_rgb(166, 227, 161),
//     teal: Color32::from_rgb(148, 226, 213),
//     sky: Color32::from_rgb(137, 220, 235),
//     sapphire: Color32::from_rgb(116, 199, 236),
//     blue: Color32::from_rgb(137, 180, 250),
//     lavender: Color32::from_rgb(180, 190, 254),
//     text: Color32::from_rgb(205, 214, 244),
//     subtext1: Color32::from_rgb(186, 194, 222),
//     subtext0: Color32::from_rgb(166, 173, 200),
//     overlay2: Color32::from_rgb(147, 153, 178),
//     overlay1: Color32::from_rgb(127, 132, 156),
//     overlay0: Color32::from_rgb(108, 112, 134),
//     surface2: Color32::from_rgb(88, 91, 112),
//     surface1: Color32::from_rgb(69, 71, 90),
//     surface0: Color32::from_rgb(49, 50, 68),
//     base: Color32::from_rgb(30, 30, 46),
//     mantle: Color32::from_rgb(24, 24, 37),
//     crust: Color32::from_rgb(17, 17, 27),
// };

pub fn style() -> Style {
    Style {
        // override the text styles here:
        // override_text_style: Option<TextStyle>

        // override the font id here:
        // override_font_id: Option<FontId>

        // set your text styles here:
        // text_styles: BTreeMap<TextStyle, FontId>,

        // set your drag value text style:
        // drag_value_text_style: TextStyle,
        spacing: Spacing {
            ..Default::default()
        },
        interaction: Interaction {
            ..Default::default()
        },
        visuals: Visuals {
            dark_mode: true,
            override_text_color: Some(Color32::from_rgb_additive(179, 198, 255)),
            widgets: Widgets {
                noninteractive: WidgetVisuals {
                    bg_fill: Color32::from_rgba_premultiplied(27, 27, 27, 255),
                    weak_bg_fill: Color32::from_rgba_premultiplied(27, 27, 27, 255),
                    bg_stroke: Stroke {
                        width: 1.0,
                        color: Color32::from_rgba_premultiplied(60, 60, 60, 255),
                    },
                    rounding: Rounding {
                        nw: 2.0,
                        ne: 2.0,
                        sw: 2.0,
                        se: 2.0,
                    },
                    fg_stroke: Stroke {
                        width: 1.0,
                        color: Color32::from_rgba_premultiplied(140, 140, 140, 255),
                    },
                    expansion: 0.0,
                },
                inactive: WidgetVisuals {
                    bg_fill: Color32::from_rgba_premultiplied(60, 60, 60, 255),
                    weak_bg_fill: Color32::from_rgba_premultiplied(60, 60, 60, 255),
                    bg_stroke: Stroke {
                        width: 0.0,
                        color: Color32::from_rgba_premultiplied(0, 0, 0, 0),
                    },
                    rounding: Rounding {
                        nw: 2.0,
                        ne: 2.0,
                        sw: 2.0,
                        se: 2.0,
                    },
                    fg_stroke: Stroke {
                        width: 1.0,
                        color: Color32::from_rgba_premultiplied(180, 180, 180, 255),
                    },
                    expansion: 0.0,
                },
                hovered: WidgetVisuals {
                    bg_fill: Color32::from_rgba_premultiplied(70, 70, 70, 255),
                    weak_bg_fill: Color32::from_rgba_premultiplied(70, 70, 70, 255),
                    bg_stroke: Stroke {
                        width: 1.0,
                        color: Color32::from_rgba_premultiplied(150, 150, 150, 255),
                    },
                    rounding: Rounding {
                        nw: 3.0,
                        ne: 3.0,
                        sw: 3.0,
                        se: 3.0,
                    },
                    fg_stroke: Stroke {
                        width: 1.5,
                        color: Color32::from_rgba_premultiplied(240, 240, 240, 255),
                    },
                    expansion: 1.0,
                },
                active: WidgetVisuals {
                    bg_fill: Color32::from_rgba_premultiplied(55, 55, 55, 255),
                    weak_bg_fill: Color32::from_rgba_premultiplied(55, 55, 55, 255),
                    bg_stroke: Stroke {
                        width: 1.0,
                        color: Color32::from_rgba_premultiplied(255, 255, 255, 255),
                    },
                    rounding: Rounding {
                        nw: 2.0,
                        ne: 2.0,
                        sw: 2.0,
                        se: 2.0,
                    },
                    fg_stroke: Stroke {
                        width: 2.0,
                        color: Color32::from_rgba_premultiplied(255, 255, 255, 255),
                    },
                    expansion: 1.0,
                },
                open: WidgetVisuals {
                    bg_fill: Color32::from_rgba_premultiplied(27, 27, 27, 255),
                    weak_bg_fill: Color32::from_rgba_premultiplied(27, 27, 27, 255),
                    bg_stroke: Stroke {
                        width: 1.0,
                        color: Color32::from_rgba_premultiplied(60, 60, 60, 255),
                    },
                    rounding: Rounding {
                        nw: 2.0,
                        ne: 2.0,
                        sw: 2.0,
                        se: 2.0,
                    },
                    fg_stroke: Stroke {
                        width: 1.0,
                        color: Color32::from_rgba_premultiplied(210, 210, 210, 255),
                    },
                    expansion: 0.0,
                },
            },
            selection: Selection {
                bg_fill: Color32::from_rgba_premultiplied(11, 74, 81, 255),
                stroke: Stroke {
                    width: 1.0,
                    color: Color32::from_rgba_premultiplied(198, 109, 202, 151),
                },
            },
            hyperlink_color: Color32::from_rgba_premultiplied(90, 255, 248, 255),
            faint_bg_color: Color32::from_rgba_premultiplied(1, 17, 16, 0),
            extreme_bg_color: Color32::from_rgba_premultiplied(10, 10, 10, 255),
            code_bg_color: Color32::from_rgba_premultiplied(64, 64, 64, 255),
            warn_fg_color: Color32::from_rgba_premultiplied(255, 143, 0, 255),
            error_fg_color: Color32::from_rgba_premultiplied(255, 0, 0, 255),
            window_rounding: Rounding {
                nw: 6.0,
                ne: 6.0,
                sw: 6.0,
                se: 6.0,
            },
            window_shadow: Shadow {
                extrusion: 32.0,
                color: Color32::from_rgba_premultiplied(0, 0, 0, 96),
            },
            window_fill: Color32::from_rgba_premultiplied(11, 13, 17, 128),
            window_stroke: Stroke {
                width: 1.0,
                color: Color32::from_rgba_premultiplied(79, 0, 52, 20),
            },
            menu_rounding: Rounding {
                nw: 6.0,
                ne: 6.0,
                sw: 6.0,
                se: 6.0,
            },
            panel_fill: Color32::from_rgba_premultiplied(15, 16, 19, 255),
            popup_shadow: Shadow {
                extrusion: 16.0,
                color: Color32::from_rgba_premultiplied(0, 0, 0, 96),
            },
            resize_corner_size: 10.399999618530273,
            text_cursor_preview: true,
            clip_rect_margin: 3.0,
            button_frame: true,
            collapsing_header_frame: false,
            indent_has_left_vline: true,
            striped: false,
            slider_trailing_fill: false,
            ..Default::default()
        },
        animation_time: 0.0833333358168602,
        explanation_tooltips: false,
        ..Default::default()
    }
}