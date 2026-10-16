//! Opaque colour roles and radii resolved from the installed theme.

use eframe::egui::{Color32, Context, Id, Style, Visuals, style::WidgetVisuals};

use crate::theme;

/// Opacity of a role colour in its `_soft` wash.
const SOFT: f32 = 0.18;
/// Share of `primary` in `primary_border`.
const BORDER_MIX: f32 = 0.65;
/// Share of `text_dim` kept in `text_faint`.
const FAINT_MIX: f32 = 0.65;
/// Lift applied to `base` when a theme has no distinct raised surface.
const RAISE_MIX: f32 = 0.05;
/// Alpha below which a colour's hue is too imprecise to pick an accent from.
const MIN_ACCENT_ALPHA: u8 = 64;
const LIGHT_INK: Color32 = Color32::from_rgb(250, 250, 252);
const DARK_INK: Color32 = Color32::from_rgb(12, 12, 14);

/// Colours and radii controls paint with; every colour is opaque.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tokens {
    /// Panel background.
    pub base: Color32,
    /// Troughs, plots and scroll tracks.
    pub sunken: Color32,
    /// Cards and grouped content on a panel.
    pub raised: Color32,
    /// Floating windows, menus and popups.
    pub window: Color32,
    /// Text fields and resting controls.
    pub field: Color32,
    pub field_border: Color32,
    pub hover: Color32,
    pub pressed: Color32,
    pub separator: Color32,
    pub card_border: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_faint: Color32,
    pub icon: Color32,
    pub primary: Color32,
    /// Primary wash behind selections.
    pub primary_soft: Color32,
    pub primary_border: Color32,
    /// Text and icons on a `primary` fill.
    pub on_primary: Color32,
    pub secondary: Color32,
    pub secondary_soft: Color32,
    pub tertiary: Color32,
    pub tertiary_soft: Color32,
    pub success: Color32,
    pub warn: Color32,
    pub error: Color32,
    pub info: Color32,
    /// Widget corner radius.
    pub radius_sm: u8,
    /// Menu and card corner radius.
    pub radius_md: u8,
    /// Window corner radius.
    pub radius_lg: u8,
}

impl Tokens {
    /// Tokens read off `style`, plus the two roles egui has no slot for.
    pub fn derive(style: &Style, success: Color32, secondary: Color32) -> Self {
        let v = &style.visuals;
        let w = &v.widgets;
        let backdrop = if v.dark_mode {
            Color32::BLACK
        } else {
            Color32::WHITE
        };
        let base = composite(v.panel_fill, backdrop);
        let window = composite(v.window_fill, base);
        let raised = [w.noninteractive.bg_fill, v.window_fill, v.faint_bg_color]
            .into_iter()
            .map(|c| composite(c, base))
            .find(|c| *c != base)
            .unwrap_or_else(|| mix(base, ink_on(base), RAISE_MIX));
        let field = composite(v.text_edit_bg_color(), base);
        let field_border = composite(
            stroke_color(&w.inactive).unwrap_or(w.noninteractive.bg_stroke.color),
            field,
        );
        let text = composite(v.text_color(), base);
        let text_dim = composite(v.weak_text_color(), base);
        let primary = accent(v);
        let secondary = composite(secondary, base);
        let tertiary = composite(v.hyperlink_color, base);
        Self {
            base,
            sunken: composite(v.extreme_bg_color, base),
            raised,
            window,
            field,
            field_border,
            hover: composite(fill(&w.hovered), base),
            pressed: composite(fill(&w.active), base),
            separator: composite(w.noninteractive.bg_stroke.color, base),
            card_border: composite(v.window_stroke.color, raised),
            text,
            text_dim,
            text_faint: mix(base, text_dim, FAINT_MIX),
            icon: mix(text_dim, text, 0.5),
            primary,
            primary_soft: wash(primary, base),
            primary_border: mix(field, primary, BORDER_MIX),
            on_primary: ink_on(primary),
            secondary,
            secondary_soft: wash(secondary, base),
            tertiary,
            tertiary_soft: wash(tertiary, base),
            success: composite(success, base),
            warn: composite(v.warn_fg_color, base),
            error: composite(v.error_fg_color, base),
            info: tertiary,
            radius_sm: w.inactive.corner_radius.nw,
            radius_md: v.menu_corner_radius.nw,
            radius_lg: v.window_corner_radius.nw,
        }
    }

    /// The tokens last published on `ctx`, or those derived from its global style.
    pub fn get(ctx: &Context) -> Self {
        ctx.data(|d| d.get_temp::<Self>(tokens_id()))
            .unwrap_or_else(|| {
                Self::derive(
                    &ctx.global_style(),
                    theme::success_ctx(ctx),
                    theme::accent_secondary_ctx(ctx),
                )
            })
    }

    pub fn publish(ctx: &Context, tokens: Self) {
        ctx.data_mut(|d| d.insert_temp(tokens_id(), tokens));
    }

    /// Every colour token with its field name.
    pub fn colors(&self) -> [(&'static str, Color32); 26] {
        [
            ("base", self.base),
            ("sunken", self.sunken),
            ("raised", self.raised),
            ("window", self.window),
            ("field", self.field),
            ("field_border", self.field_border),
            ("hover", self.hover),
            ("pressed", self.pressed),
            ("separator", self.separator),
            ("card_border", self.card_border),
            ("text", self.text),
            ("text_dim", self.text_dim),
            ("text_faint", self.text_faint),
            ("icon", self.icon),
            ("primary", self.primary),
            ("primary_soft", self.primary_soft),
            ("primary_border", self.primary_border),
            ("on_primary", self.on_primary),
            ("secondary", self.secondary),
            ("secondary_soft", self.secondary_soft),
            ("tertiary", self.tertiary),
            ("tertiary_soft", self.tertiary_soft),
            ("success", self.success),
            ("warn", self.warn),
            ("error", self.error),
            ("info", self.info),
        ]
    }
}

fn tokens_id() -> Id {
    Id::unique("mtech.tokens")
}

/// `top` (premultiplied) painted over `under`, as an opaque colour.
pub fn composite(top: Color32, under: Color32) -> Color32 {
    let inv = 1.0 - top.a() as f32 / 255.0;
    let ch = |t: u8, u: u8| (t as f32 + u as f32 * inv).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgb(
        ch(top.r(), under.r()),
        ch(top.g(), under.g()),
        ch(top.b(), under.b()),
    )
}

/// WCAG contrast ratio between two opaque colours, from 1.0 to 21.0.
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// WCAG relative luminance of the colour's RGB channels.
fn luminance(c: Color32) -> f32 {
    let lin = |v: u8| {
        let s = v as f32 / 255.0;
        if s <= 0.040_45 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
}

/// Opaque gamma-space blend from `a` (t = 0) to `b` (t = 1).
pub(crate) fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let ch = |x: u8, y: u8| {
        (x as f32 + (y as f32 - x as f32) * t)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    Color32::from_rgb(ch(a.r(), b.r()), ch(a.g(), b.g()), ch(a.b(), b.b()))
}

/// `role` at [`SOFT`] opacity over `base`.
fn wash(role: Color32, base: Color32) -> Color32 {
    mix(base, role, SOFT)
}

/// The ink with more contrast on `fill`.
pub(crate) fn ink_on(fill: Color32) -> Color32 {
    if contrast(DARK_INK, fill) >= contrast(LIGHT_INK, fill) {
        DARK_INK
    } else {
        LIGHT_INK
    }
}

/// Opaque hue of a possibly premultiplied colour.
fn tint(c: Color32) -> Color32 {
    let [r, g, b, _] = c.to_srgba_unmultiplied();
    Color32::from_rgb(r, g, b)
}

/// Max minus min channel of the colour's hue.
fn chroma(c: Color32) -> u8 {
    let [r, g, b, _] = tint(c).to_array();
    r.max(g).max(b) - r.min(g).min(b)
}

/// Most chromatic of the selection and link hues opaque enough to read.
fn accent(v: &Visuals) -> Color32 {
    [
        v.selection.bg_fill,
        v.selection.stroke.color,
        v.hyperlink_color,
    ]
    .into_iter()
    .filter(|c| c.a() >= MIN_ACCENT_ALPHA)
    .map(tint)
    .max_by_key(|c| chroma(*c))
    .unwrap_or_else(|| tint(v.hyperlink_color))
}

/// The weak fill a button paints, or the strong one when the weak fill is invisible.
fn fill(state: &WidgetVisuals) -> Color32 {
    if state.weak_bg_fill.a() > 0 {
        state.weak_bg_fill
    } else {
        state.bg_fill
    }
}

/// The state's outline colour when it draws one.
fn stroke_color(state: &WidgetVisuals) -> Option<Color32> {
    (state.bg_stroke.width > 0.0 && state.bg_stroke.color.a() > 0).then_some(state.bg_stroke.color)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn translucent_style() -> Style {
        let mut style = Style::default();
        let v = &mut style.visuals;
        v.panel_fill = Color32::from_rgba_unmultiplied(10, 10, 14, 200);
        v.window_fill = Color32::from_rgba_unmultiplied(20, 20, 30, 120);
        v.extreme_bg_color = Color32::from_rgba_unmultiplied(120, 100, 220, 30);
        v.selection.bg_fill = Color32::from_rgba_unmultiplied(126, 108, 224, 80);
        v.widgets.hovered.weak_bg_fill = Color32::TRANSPARENT;
        v.widgets.inactive.bg_stroke.width = 0.0;
        v.hyperlink_color = Color32::from_rgb(150, 150, 160);
        style
    }

    fn styles() -> [Style; 3] {
        let light = Style {
            visuals: Visuals::light(),
            ..Style::default()
        };
        [Style::default(), light, translucent_style()]
    }

    #[test]
    fn derived_colours_are_opaque() {
        let accent2 = Color32::from_rgba_unmultiplied(191, 33, 101, 90);
        for style in styles() {
            let tokens = Tokens::derive(&style, Color32::from_rgb(72, 199, 142), accent2);
            for (name, c) in tokens.colors() {
                assert_eq!(c.a(), 255, "{name} is translucent");
            }
        }
    }

    #[test]
    fn surfaces_and_roles_come_from_the_style() {
        let style = translucent_style();
        let tokens = Tokens::derive(&style, Color32::GREEN, Color32::RED);
        let v = &style.visuals;
        assert_eq!(tokens.base, composite(v.panel_fill, Color32::BLACK));
        assert_eq!(tokens.window, composite(v.window_fill, tokens.base));
        assert_eq!(tokens.field, composite(v.extreme_bg_color, tokens.base));
        assert_eq!(
            tokens.hover,
            composite(v.widgets.hovered.bg_fill, tokens.base)
        );
        assert_eq!(tokens.primary, tint(v.selection.bg_fill));
        assert_eq!(
            tokens.field_border,
            composite(v.widgets.noninteractive.bg_stroke.color, tokens.field)
        );
        assert_eq!(tokens.info, tokens.tertiary);
        assert_ne!(tokens.raised, tokens.base);
    }

    #[test]
    fn on_primary_is_the_more_legible_ink() {
        for style in styles() {
            let t = Tokens::derive(&style, Color32::GREEN, Color32::RED);
            let other = if t.on_primary == DARK_INK {
                LIGHT_INK
            } else {
                DARK_INK
            };
            assert!(contrast(t.on_primary, t.primary) >= contrast(other, t.primary));
        }
    }

    #[test]
    fn compositing_and_contrast_match_their_definitions() {
        let white = Color32::WHITE;
        assert_eq!(composite(white, Color32::BLACK), white);
        assert_eq!(
            composite(Color32::from_white_alpha(128), Color32::BLACK),
            Color32::from_rgb(128, 128, 128)
        );
        assert!((contrast(Color32::BLACK, white) - 21.0).abs() < 0.01);
        assert!((contrast(white, white) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn unpublished_tokens_derive_from_the_global_style() {
        let ctx = Context::default();
        let expected = Tokens::derive(
            &ctx.global_style(),
            theme::success_ctx(&ctx),
            theme::accent_secondary_ctx(&ctx),
        );
        assert_eq!(Tokens::get(&ctx), expected);

        let published = Tokens {
            primary: Color32::from_rgb(255, 42, 109),
            ..expected
        };
        Tokens::publish(&ctx, published);
        assert_eq!(Tokens::get(&ctx), published);
    }
}
