//! App-owned geometry and type scale, overlaid on whichever theme `Style` is installed.

use std::sync::Arc;

use eframe::egui::{
    Context, FontFamily, FontId, Id, Margin, Style, TextStyle, Vec2,
    style::{HandleShape, ScrollStyle},
    vec2,
};

use crate::{fonts::SEMIBOLD, theme, tokens::Tokens};

/// How tightly controls and rows are packed.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum Density {
    Compact,
    #[default]
    Regular,
    Comfortable,
}

impl Density {
    pub const ALL: [Self; 3] = [Self::Compact, Self::Regular, Self::Comfortable];

    pub fn label(self) -> &'static str {
        match self {
            Self::Compact => "Compact",
            Self::Regular => "Regular",
            Self::Comfortable => "Comfortable",
        }
    }
}

/// Sizes every control, row and bar is laid out with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    /// Height of fields, combos and standard buttons.
    pub control_h: f32,
    /// Height of prominent (primary / dialog) buttons.
    pub button_h: f32,
    /// Button padding.
    pub pad: Vec2,
    /// Item spacing.
    pub gap: Vec2,
    pub window_margin: i8,
    pub menu_margin: i8,
    pub radius_sm: u8,
    pub radius_md: u8,
    pub radius_lg: u8,
    pub combo_w: f32,
    pub slider_w: f32,
    pub tooltip_w: f32,
    pub icon_sm: f32,
    pub icon_md: f32,
    pub icon_lg: f32,
    /// Height of a list or table row.
    pub row_h: f32,
    /// Height of a section or card header.
    pub header_h: f32,
    /// Height of a toolbar or title bar.
    pub bar_h: f32,
    /// Height of a dialog or panel footer.
    pub footer_h: f32,
}

impl Metrics {
    pub fn for_density(density: Density) -> Self {
        let regular = Self {
            control_h: 24.0,
            button_h: 28.0,
            pad: vec2(10.0, 4.0),
            gap: vec2(8.0, 6.0),
            window_margin: 12,
            menu_margin: 6,
            radius_sm: 6,
            radius_md: 8,
            radius_lg: 12,
            combo_w: 120.0,
            slider_w: 150.0,
            tooltip_w: 280.0,
            icon_sm: 14.0,
            icon_md: 16.0,
            icon_lg: 20.0,
            row_h: 24.0,
            header_h: 28.0,
            bar_h: 36.0,
            footer_h: 32.0,
        };
        match density {
            Density::Regular => regular,
            Density::Compact => Self {
                control_h: 22.0,
                button_h: 26.0,
                pad: vec2(8.0, 3.0),
                gap: vec2(6.0, 4.0),
                window_margin: 10,
                menu_margin: 4,
                combo_w: 110.0,
                slider_w: 130.0,
                icon_sm: 12.0,
                icon_md: 14.0,
                icon_lg: 18.0,
                row_h: 22.0,
                header_h: 26.0,
                bar_h: 32.0,
                footer_h: 28.0,
                ..regular
            },
            Density::Comfortable => Self {
                control_h: 28.0,
                button_h: 32.0,
                pad: vec2(12.0, 6.0),
                gap: vec2(10.0, 8.0),
                window_margin: 16,
                menu_margin: 8,
                combo_w: 140.0,
                slider_w: 170.0,
                tooltip_w: 320.0,
                icon_sm: 16.0,
                icon_md: 18.0,
                icon_lg: 22.0,
                row_h: 28.0,
                header_h: 32.0,
                bar_h: 40.0,
                footer_h: 36.0,
                ..regular
            },
        }
    }

    /// The metrics last published on `ctx`, or those of its density.
    pub fn get(ctx: &Context) -> Self {
        ctx.data(|d| d.get_temp::<Self>(metrics_id()))
            .unwrap_or_else(|| Self::for_density(density(ctx)))
    }

    pub fn publish(ctx: &Context, metrics: Self) {
        ctx.data_mut(|d| d.insert_temp(metrics_id(), metrics));
    }
}

fn metrics_id() -> Id {
    Id::unique("mtech.metrics")
}

fn density_id() -> Id {
    Id::unique("mtech.metrics.density")
}

/// The density [`install_overlaid_style`] overlays on `ctx`.
pub fn density(ctx: &Context) -> Density {
    ctx.data(|d| d.get_temp::<Density>(density_id()))
        .unwrap_or_default()
}

/// Records the density the next [`install_overlaid_style`] overlays.
pub fn set_density(ctx: &Context, density: Density) {
    ctx.data_mut(|d| d.insert_temp(density_id(), density));
}

/// Overlays the current density's metrics on `style`, installs it with the kit widget theme, and publishes its metrics and tokens.
pub fn install_overlaid_style(ctx: &Context, mut style: Style) {
    crate::widget_theme::install(ctx);
    let metrics = Metrics::for_density(density(ctx));
    apply_metrics(&mut style, &metrics);
    let tokens = Tokens::derive(
        &style,
        theme::success_ctx(ctx),
        theme::accent_secondary_ctx(ctx),
    );
    ctx.set_global_style(Arc::new(style));
    Metrics::publish(ctx, metrics);
    Tokens::publish(ctx, tokens);
}

/// Type sizes in points, shared by every density.
pub mod type_scale {
    pub const SMALL: f32 = 10.5;
    pub const BODY: f32 = 12.5;
    pub const BUTTON: f32 = 12.5;
    pub const HEADING: f32 = 15.0;
    pub const MONOSPACE: f32 = 12.0;
}

/// Overwrites spacing, type sizes and widget-state geometry; colours are left untouched.
pub fn apply_metrics(style: &mut Style, m: &Metrics) {
    let s = &mut style.spacing;
    s.interact_size = vec2(40.0, m.control_h);
    s.button_padding = m.pad;
    s.item_spacing = m.gap;
    s.window_margin = Margin::same(m.window_margin);
    s.menu_margin = Margin::same(m.menu_margin);
    s.indent = 16.0;
    s.combo_width = m.combo_w;
    s.slider_width = m.slider_w;
    s.icon_width = 14.0;
    s.icon_width_inner = 8.0;
    s.icon_spacing = 6.0;
    s.tooltip_width = m.tooltip_w;
    s.menu_spacing = 2.0;
    s.combo_height = 320.0;
    s.scroll = ScrollStyle::thin();

    apply_type_scale(style);
    style.override_font_id = None;

    let w = &mut style.visuals.widgets;
    let radius = w.inactive.corner_radius;
    for state in [
        &mut w.noninteractive,
        &mut w.inactive,
        &mut w.hovered,
        &mut w.active,
        &mut w.open,
    ] {
        state.bg_stroke.width = whole_px(state.bg_stroke.width);
        state.fg_stroke.width = whole_px(state.fg_stroke.width);
        state.expansion = 0.0;
        state.corner_radius = radius;
    }

    style.interaction.tooltip_delay = 0.35;
    let v = &mut style.visuals;
    v.striped = false;
    v.indent_has_left_vline = false;
    v.handle_shape = HandleShape::Circle;
    v.slider_trailing_fill = true;
}

/// Sets the five built-in text sizes, keeping each style's family.
fn apply_type_scale(style: &mut Style) {
    let body_proportional = style
        .text_styles
        .get(&TextStyle::Body)
        .is_none_or(|f| f.family == FontFamily::Proportional);
    for (text_style, size, family) in [
        (
            TextStyle::Small,
            type_scale::SMALL,
            FontFamily::Proportional,
        ),
        (TextStyle::Body, type_scale::BODY, FontFamily::Proportional),
        (
            TextStyle::Button,
            type_scale::BUTTON,
            FontFamily::Proportional,
        ),
        (
            TextStyle::Heading,
            type_scale::HEADING,
            FontFamily::Proportional,
        ),
        (
            TextStyle::Monospace,
            type_scale::MONOSPACE,
            FontFamily::Monospace,
        ),
    ] {
        style
            .text_styles
            .entry(text_style)
            .or_insert_with(|| FontId::new(size, family))
            .size = size;
    }
    if body_proportional && let Some(heading) = style.text_styles.get_mut(&TextStyle::Heading) {
        heading.family = FontFamily::Name(SEMIBOLD.into());
    }
}

/// A stroke width snapped to whole pixels; a visible stroke stays at least 1 px.
fn whole_px(width: f32) -> f32 {
    if width > 0.0 {
        width.round().max(1.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{Color32, CornerRadius, Stroke, Visuals};

    fn fractional_style() -> Style {
        let mut style = Style::default();
        let w = &mut style.visuals.widgets;
        w.noninteractive.bg_stroke = Stroke::new(0.4, Color32::RED);
        w.inactive.bg_stroke = Stroke::new(1.5, Color32::GREEN);
        w.inactive.corner_radius = CornerRadius::same(5);
        w.hovered.bg_stroke = Stroke::new(1.7, Color32::BLUE);
        w.hovered.fg_stroke = Stroke::new(2.4, Color32::WHITE);
        w.hovered.expansion = 2.0;
        w.hovered.corner_radius = CornerRadius::same(9);
        w.active.bg_stroke = Stroke::new(0.0, Color32::BLACK);
        w.active.expansion = 1.0;
        w.open.fg_stroke = Stroke::new(0.6, Color32::GRAY);
        style.override_font_id = Some(FontId::proportional(14.0));
        style
    }

    fn monospace_style() -> Style {
        let mut style = Style::default();
        for font in style.text_styles.values_mut() {
            font.family = FontFamily::Monospace;
        }
        style
    }

    fn light_style() -> Style {
        Style {
            visuals: Visuals::light(),
            ..Style::default()
        }
    }

    fn bases() -> [Style; 4] {
        [
            Style::default(),
            light_style(),
            fractional_style(),
            monospace_style(),
        ]
    }

    fn overlaid(mut style: Style, density: Density) -> Style {
        apply_metrics(&mut style, &Metrics::for_density(density));
        style
    }

    #[test]
    fn applying_the_overlay_twice_changes_nothing() {
        for base in bases() {
            for density in Density::ALL {
                let once = overlaid(base.clone(), density);
                let twice = overlaid(once.clone(), density);
                assert!(once == twice, "{density:?} overlay is not idempotent");
            }
        }
    }

    #[test]
    fn the_overlay_replaces_whatever_density_came_before() {
        for base in bases() {
            for from in Density::ALL {
                for to in Density::ALL {
                    let direct = overlaid(base.clone(), to);
                    let via = overlaid(overlaid(base.clone(), from), to);
                    assert!(direct == via, "{from:?} -> {to:?} leaves a trace");
                }
            }
        }
    }

    #[test]
    fn controls_take_the_density_height() {
        for density in Density::ALL {
            let m = Metrics::for_density(density);
            let style = overlaid(Style::default(), density);
            assert_eq!(style.spacing.interact_size.y, m.control_h);
            assert_eq!(style.spacing.button_padding, m.pad);
            assert_eq!(style.spacing.item_spacing, m.gap);
            assert!(m.button_h > m.control_h);
        }
    }

    #[test]
    fn widget_states_get_whole_pixel_strokes_no_growth_and_one_radius() {
        let style = overlaid(fractional_style(), Density::Regular);
        let w = &style.visuals.widgets;
        let states = [
            &w.noninteractive,
            &w.inactive,
            &w.hovered,
            &w.active,
            &w.open,
        ];
        for state in states {
            for width in [state.bg_stroke.width, state.fg_stroke.width] {
                assert_eq!(width, width.round(), "fractional stroke {width}");
            }
            assert_eq!(state.expansion, 0.0);
            assert_eq!(state.corner_radius, CornerRadius::same(5));
        }
        assert_eq!(w.noninteractive.bg_stroke.width, 1.0);
        assert_eq!(w.inactive.bg_stroke.width, 2.0);
        assert_eq!(w.hovered.fg_stroke.width, 2.0);
        assert_eq!(w.active.bg_stroke.width, 0.0);
        assert_eq!(w.open.fg_stroke.width, 1.0);
    }

    #[test]
    fn the_overlay_never_recolours() {
        let before = fractional_style();
        let after = overlaid(before.clone(), Density::Comfortable);
        let colours = |v: &Visuals| {
            let w = &v.widgets;
            [
                &w.noninteractive,
                &w.inactive,
                &w.hovered,
                &w.active,
                &w.open,
            ]
            .map(|s| {
                (
                    s.bg_fill,
                    s.weak_bg_fill,
                    s.bg_stroke.color,
                    s.fg_stroke.color,
                )
            })
        };
        assert_eq!(colours(&before.visuals), colours(&after.visuals));
        assert_eq!(before.visuals.window_fill, after.visuals.window_fill);
        assert_eq!(before.visuals.panel_fill, after.visuals.panel_fill);
        assert_eq!(before.visuals.selection, after.visuals.selection);
        assert_eq!(
            before.visuals.override_text_color,
            after.visuals.override_text_color
        );
    }

    #[test]
    fn the_type_scale_keeps_each_family_and_bolds_proportional_headings() {
        let proportional = overlaid(Style::default(), Density::Regular);
        let heading = &proportional.text_styles[&TextStyle::Heading];
        assert_eq!(heading.family, FontFamily::Name(SEMIBOLD.into()));
        assert_eq!(heading.size, type_scale::HEADING);
        assert_eq!(
            proportional.text_styles[&TextStyle::Body].size,
            type_scale::BODY
        );
        assert!(proportional.override_font_id.is_none());

        let mono = overlaid(monospace_style(), Density::Regular);
        for (text_style, font) in &mono.text_styles {
            assert_eq!(
                font.family,
                FontFamily::Monospace,
                "{text_style:?} changed family"
            );
        }
        assert_eq!(mono.text_styles[&TextStyle::Small].size, type_scale::SMALL);
    }

    #[test]
    fn missing_text_styles_are_filled_in() {
        let mut style = Style::default();
        style.text_styles.clear();
        let style = overlaid(style, Density::Regular);
        assert_eq!(style.text_styles.len(), 5);
        assert_eq!(
            style.text_styles[&TextStyle::Monospace].family,
            FontFamily::Monospace
        );
    }

    #[test]
    fn install_overlaid_style_overlays_and_publishes_the_context_density() {
        let ctx = Context::default();
        set_density(&ctx, Density::Compact);
        install_overlaid_style(&ctx, fractional_style());

        let compact = Metrics::for_density(Density::Compact);
        assert_eq!(Metrics::get(&ctx), compact);
        let style = ctx.global_style();
        assert_eq!(style.spacing.interact_size.y, compact.control_h);
        assert_eq!(style.visuals.widgets.hovered.expansion, 0.0);
    }

    #[test]
    fn install_overlaid_style_publishes_tokens_of_the_installed_style() {
        let ctx = Context::default();
        let success = Color32::from_rgb(1, 200, 100);
        let secondary = Color32::from_rgb(200, 20, 120);
        theme::set_success_color(&ctx, success);
        theme::set_accent_secondary(&ctx, secondary);
        install_overlaid_style(&ctx, fractional_style());

        let tokens = Tokens::get(&ctx);
        assert_eq!(
            tokens,
            Tokens::derive(&ctx.global_style(), success, secondary)
        );
        assert_eq!(tokens.success, success);
        assert_eq!(tokens.radius_sm, 5);
    }

    #[test]
    fn unpublished_metrics_follow_the_recorded_density() {
        let ctx = Context::default();
        assert_eq!(Metrics::get(&ctx), Metrics::for_density(Density::Regular));
        set_density(&ctx, Density::Comfortable);
        assert_eq!(
            Metrics::get(&ctx),
            Metrics::for_density(Density::Comfortable)
        );
    }
}
