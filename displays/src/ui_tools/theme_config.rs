use eframe::egui::{scroll_area::ScrollBarVisibility, style::{HandleShape, NumericColorSpace, Selection, TextCursorStyle, WidgetVisuals, Widgets}, Align, Color32, Context, CursorIcon, Frame, Id, Layout, Margin, ScrollArea, Shadow, Stroke, Style, Ui, Visuals};
use crate::{ui_tools::{encode_theme, fonts::{self, TextScale}, glass_backdrop::{self, GlassParams}, metrics::{self, apply_metrics, type_scale, Density, Metrics}, rerun_mtech::{RerunMtech, RerunMtechOled}, theme_chrome::{default_egui_chrome, legacy_classic_chrome, mtech_noir_chrome, mtech_noir_glass_chrome, mtech_noir_glass_params, shipped_chrome}, tokyo_dark::{TokyoNight, TokyoNightStorm}, SavedTheme}, PlatformSpawner, Spawner};
use serde::{Deserialize, Serialize};
use crossbeam::channel::Sender;
use database::schema::User;
use std::sync::Arc;

use super::carl_dark::{paint_aesthetix_colors, Aesthetix, CarlDark};
use super::galactic_glass;
use super::mtech_glass::{glass_params_for_style, glass_style, glassify, legacy_glass_style, MtechGlass};
use super::mtech_studio;
use super::neon_glass::{self, NeonPalette};
use super::soft_glass::{self, SoftPalette};
use super::tokens::Tokens;
use super::{controls, icons};
use super::controls::{BarKind, ButtonKind, MenuButton, TextButton, ValueField};
use super::decode_theme;

/// Width of the blur radius and corner fields.
const BLUR_FIELD_W: f32 = 72.0;
/// Width of the presence slider track.
const PRESENCE_W: f32 = 120.0;

/// Preset picker groups, each under an optional title.
const PRESET_GROUPS: [(&str, &[PresetStyles]); 6] = [
    ("", &[
        PresetStyles::ShippedClassic,
        PresetStyles::MtechStudio,
        PresetStyles::LegacyClassic,
        PresetStyles::DefaultEgui,
        PresetStyles::MtechNoir,
        PresetStyles::MtechNoirGlass,
    ]),
    ("Neon glass · AMOLED", &[
        PresetStyles::NebulaGlass,
        PresetStyles::AuroraGlass,
        PresetStyles::SupernovaGlass,
        PresetStyles::EventHorizonGlass,
        PresetStyles::GalacticGlass,
    ]),
    ("Soft glass · low chroma", &[
        PresetStyles::ObsidianGlass,
        PresetStyles::VelvetGlass,
        PresetStyles::TwilightGlass,
        PresetStyles::QuartzGlass,
    ]),
    ("Colors only · legacy widgets", &[
        PresetStyles::CarlDarkColors,
        PresetStyles::TokyoNightStormColors,
        PresetStyles::TokyoNightColors,
        PresetStyles::RerunMtechColors,
        PresetStyles::RerunMtechOledColors,
        PresetStyles::MtechGlassColors,
    ]),
    ("Colors + widget chrome", &[
        PresetStyles::CarlDarkFull,
        PresetStyles::TokyoNightStormFull,
        PresetStyles::TokyoNightFull,
        PresetStyles::RerunMtechFull,
        PresetStyles::RerunMtechOledFull,
        PresetStyles::MtechGlassFull,
    ]),
    ("", &[PresetStyles::Custom]),
];

/// Applies the shipped preset at the default density and text size.
pub fn bootstrap_startup_theme(ctx: &Context) {
    metrics::set_density(ctx, Density::default());
    set_text_scale(ctx, TextScale::default());
    apply_preset(ctx, PresetStyles::ShippedClassic);
}

/// Installs `style` under the current density's metrics overlay and publishes its tokens.
pub fn install_style(ctx: &Context, style: Style) {
    metrics::install_overlaid_style(ctx, style);
    if let Some(tokens) = exact_tokens(&ctx.global_style()) {
        Tokens::publish(ctx, tokens);
    }
}

/// Hand-tuned tokens of a preset, for presets that define them.
fn preset_tokens(preset: PresetStyles) -> Option<Tokens> {
    match preset {
        PresetStyles::MtechStudio => Some(mtech_studio::TOKENS),
        _ => None,
    }
}

/// The hand-tuned tokens of the preset `style` matches, if it defines any.
fn exact_tokens(style: &Style) -> Option<Tokens> {
    let style = normalized(style);
    MATCHABLE_PRESETS
        .into_iter()
        .filter_map(|p| preset_tokens(p).map(|t| (p, t)))
        .find(|(p, _)| styles_visually_equal(&style_for_preset(*p), &style))
        .map(|(_, t)| t)
}

/// Switches the density and re-installs the current style under it.
pub fn set_density(ctx: &Context, density: Density) {
    if metrics::density(ctx) == density {
        return;
    }
    metrics::set_density(ctx, density);
    install_style(ctx, (*ctx.global_style()).clone());
}

/// Switches the UI text size, rebuilding the font atlas when it changes.
pub fn set_text_scale(ctx: &Context, scale: TextScale) {
    fonts::set_text_scale(ctx, scale, crate::app_state::font_definitions_scaled);
}

/// Applies a user-saved theme, falling back to shipped when missing or blank.
pub fn apply_user_color_scheme(ctx: &Context, bytes: &[u8]) {
    if bytes.is_empty() {
        bootstrap_startup_theme(ctx);
        return;
    }
    match decode_theme(bytes) {
        Ok(theme) if is_blank_default_style(&theme.style) => {
            log::info!("Saved color scheme is egui default; using shipped theme");
            bootstrap_startup_theme(ctx);
        }
        Ok(theme) => apply_saved_theme(ctx, theme),
        Err(e) => {
            log::error!("Saved color scheme decode failed: {e:?}");
            bootstrap_startup_theme(ctx);
        }
    }
}

/// Applies a saved theme: its style, its preset's semantic colors when recognizable, and its glass
/// material. Unrecognized styles (glassified / uploaded) fall back to the Custom semantics, which
/// match the app-wide defaults, rather than ShippedClassic's teal/blue.
pub fn apply_saved_theme(ctx: &Context, theme: SavedTheme) {
    let style = normalized(&theme.style);
    let (preset, style, glass) = match (exact_preset(&style), legacy_preset(&style)) {
        (Some(preset), _) => (preset, theme.style, theme.glass),
        // A legacy-alias match installs the preset's current style.
        (None, Some(preset)) => (
            preset,
            style_for_preset(preset),
            theme.glass.filter(|g| *g != GlassParams::OFF),
        ),
        (None, None) => (PresetStyles::Custom, theme.style, theme.glass),
    };
    metrics::set_density(ctx, theme.density);
    apply_preset_semantics(ctx, preset);
    install_style(ctx, style);
    set_text_scale(ctx, theme.text_scale);
    // A record that carries its own material wins; one written before glass existed (or an imported
    // bare egui Style) falls back to whatever its preset asks for.
    if let Some(glass) = glass {
        glass_backdrop::set_params(ctx, glass);
    }
}

/// Clears egui memory, then re-installs the current theme, density and text size into it.
pub fn reset_memory(ctx: &Context) {
    let saved = SavedTheme::current(ctx);
    ctx.memory_mut(|mem| *mem = Default::default());
    apply_saved_theme(ctx, saved);
}

/// Stores a preset's name, its semantic colors, read by [`install_style`], and its glass material.
fn apply_preset_semantics(ctx: &Context, preset: PresetStyles) {
    set_installed_preset(ctx, preset);
    let (success, accent2) = semantic_colors_for_preset(preset);
    crate::ui_tools::theme::set_success_color(ctx, success);
    crate::ui_tools::theme::set_accent_secondary(ctx, accent2);
    glass_backdrop::set_params(ctx, glass_params_for_preset(preset));
}

fn installed_preset_id() -> Id {
    Id::unique("mtech.theme.installed_preset")
}

/// The preset the installed style was applied as; Custom when none was recorded.
fn installed_preset(ctx: &Context) -> PresetStyles {
    ctx.data(|d| d.get_temp::<PresetStyles>(installed_preset_id()))
        .unwrap_or(PresetStyles::Custom)
}

fn set_installed_preset(ctx: &Context, preset: PresetStyles) {
    ctx.data_mut(|d| d.insert_temp(installed_preset_id(), preset));
}

/// Syncs the editor's fields and preset label to the style and preset installed on `ctx`.
pub(crate) fn sync_editor_config(config: &mut ThemeConfig, ctx: &Context) {
    sync_config_from_style(config, &ctx.global_style());
    config.preset_style = installed_preset(ctx);
}

/// Compares styles ignoring `number_formatter`, whose PartialEq is Arc pointer identity.
fn styles_visually_equal(a: &Style, b: &Style) -> bool {
    let mut a = a.clone();
    a.number_formatter = b.number_formatter.clone();
    a == *b
}

/// `style` under the Regular metrics overlay, the form presets are compared in.
fn normalized(style: &Style) -> Style {
    let mut style = style.clone();
    apply_metrics(&mut style, &Metrics::for_density(Density::Regular));
    style
}

/// The preset a normalized style equals.
fn exact_preset(normal: &Style) -> Option<PresetStyles> {
    MATCHABLE_PRESETS
        .into_iter()
        .find(|p| styles_visually_equal(&style_for_preset(*p), normal))
}

/// The preset a normalized style was saved as before that preset's colours changed.
fn legacy_preset(normal: &Style) -> Option<PresetStyles> {
    styles_visually_equal(&normalized(&legacy_glass_style()), normal)
        .then_some(PresetStyles::MtechGlassFull)
}

/// Every preset a saved style can be recognised as.
pub(crate) const MATCHABLE_PRESETS: [PresetStyles; 27] = [
    PresetStyles::ShippedClassic,
    PresetStyles::MtechStudio,
    PresetStyles::LegacyClassic,
    PresetStyles::DefaultEgui,
    PresetStyles::MtechNoir,
    PresetStyles::MtechNoirGlass,
    PresetStyles::NebulaGlass,
    PresetStyles::AuroraGlass,
    PresetStyles::SupernovaGlass,
    PresetStyles::EventHorizonGlass,
    PresetStyles::GalacticGlass,
    PresetStyles::ObsidianGlass,
    PresetStyles::VelvetGlass,
    PresetStyles::TwilightGlass,
    PresetStyles::QuartzGlass,
    PresetStyles::CarlDarkColors,
    PresetStyles::CarlDarkFull,
    PresetStyles::TokyoNightStormColors,
    PresetStyles::TokyoNightStormFull,
    PresetStyles::TokyoNightColors,
    PresetStyles::TokyoNightFull,
    PresetStyles::RerunMtechColors,
    PresetStyles::RerunMtechFull,
    PresetStyles::RerunMtechOledColors,
    PresetStyles::RerunMtechOledFull,
    PresetStyles::MtechGlassColors,
    PresetStyles::MtechGlassFull,
];

fn is_blank_default_style(style: &Style) -> bool {
    let style = normalized(style);
    let dark_default = Style { visuals: Visuals::dark(), ..Style::default() };
    [Style::default(), dark_default]
        .iter()
        .any(|blank| styles_visually_equal(&style, &normalized(blank)))
}

#[derive(Serialize, Clone, Deserialize, Debug, PartialEq)]
pub struct ThemeConfig {
    /// Editor background
    pub background_color: Color32,
    /// Editor foreground
    pub foreground_color: Color32,
    /// Background for inactive widgets
    pub widget_bg_fill: Color32,
    /// Weak background for widgets
    pub widget_weak_bg_fill: Color32,
    /// Widget background stroke color
    pub widget_bg_stroke_color: Color32,
    /// Widget foreground stroke color
    pub widget_fg_stroke_color: Color32,
    /// Background for hovered widgets
    pub hovered_bg_fill: Color32,
    /// Weak background for hovered widgets
    pub hovered_weak_bg_fill: Color32,
    /// Stroke for hovered
    pub hovered_bg_stroke_color: Color32,
    /// Foreground for hovered
    pub hovered_fg_stroke_color: Color32,
    /// Background for active widgets
    pub active_bg_fill: Color32,
    /// Weak background for active widgets
    pub active_weak_bg_fill: Color32,
    /// Stroke for active widgets
    pub active_bg_stroke_color: Color32,
    /// Foreground for active widgets
    pub active_fg_stroke_color: Color32,
    /// Background for open widgets
    pub open_bg_fill: Color32,
    /// Weak background for open widgets
    pub open_weak_bg_fill: Color32,
    /// Stroke for open widgets
    pub open_bg_stroke_color: Color32,
    /// Foreground for open widgets
    pub open_fg_stroke_color: Color32,
    /// Selection background
    pub selection_bg_fill: Color32,
    /// Selection stroke
    pub selection_stroke_color: Color32,
    /// Subtle background
    pub faint_bg_color: Color32,
    /// Very dark background for contrast
    pub extreme_bg_color: Color32,
    /// Code block background
    pub code_bg_color: Color32,
    /// Border color for windows/panels
    pub border_color: Color32,
    /// Default text color
    pub text_color: Color32,
    /// Error text color
    pub error_color: Color32,
    /// Warning text color
    pub warn_color: Color32,
    /// Hyperlink color
    pub link_color: Color32,
    /// Window stroke color
    pub window_stroke_color: Color32,
    /// Uniform rounding for visuals
    pub rounding: eframe::egui::CornerRadius,
    #[serde(skip)]
    pub preset_style: PresetStyles
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            background_color: Color32::from_rgb(10, 10, 13),
            foreground_color: Color32::from_rgb(169, 177, 214),
            widget_bg_fill: Color32::from_rgb(20, 20, 22),
            widget_weak_bg_fill: Color32::from_rgb(20, 20, 22),
            widget_bg_stroke_color: Color32::from_rgb(50, 50, 60),
            widget_fg_stroke_color: Color32::from_rgb(169, 177, 214),
            hovered_bg_fill: Color32::from_rgb(35, 35, 40),
            hovered_weak_bg_fill: Color32::from_rgb(40, 40, 45),
            hovered_bg_stroke_color: Color32::from_rgba_premultiplied(120, 20, 120, 100),
            hovered_fg_stroke_color: Color32::from_rgb(155, 104, 227),
            active_bg_fill: Color32::from_rgb(28, 28, 28),
            active_weak_bg_fill: Color32::from_rgb(28, 28, 28),
            active_bg_stroke_color: Color32::from_rgb(90, 90, 100),
            active_fg_stroke_color: Color32::from_rgb(169, 177, 214),
            open_bg_fill: Color32::from_rgb(30, 30, 35),
            open_weak_bg_fill: Color32::from_rgb(35, 35, 40),
            open_bg_stroke_color: Color32::from_rgb(100, 100, 110),
            open_fg_stroke_color: Color32::from_rgb(169, 177, 214),
            selection_bg_fill: Color32::from_rgba_premultiplied(90, 55, 88, 90),
            selection_stroke_color: Color32::from_rgba_premultiplied(81, 92, 126, 50),
            faint_bg_color: Color32::from_rgb(20, 20, 25),
            extreme_bg_color: Color32::from_rgb(15, 15, 20),
            code_bg_color: Color32::from_rgb(20, 20, 27),
            border_color: Color32::from_rgb(16, 16, 23),
            text_color: Color32::from_rgb(219, 199, 245),
            error_color: Color32::from_rgb(227, 104, 176),
            warn_color: Color32::from_rgb(191, 33, 101),
            link_color: Color32::from_rgb(155, 104, 227),
            window_stroke_color: Color32::from_rgb(42, 195, 222),
            rounding: eframe::egui::CornerRadius::same(4),
            preset_style: PresetStyles::ShippedClassic,
        }
    }
}

/// Default desktop theme (shipped monospace chrome from [`crate::STYLE`]).
pub fn default_app_style() -> Arc<Style> {
    Arc::new(style_for_preset(PresetStyles::ShippedClassic))
}

/// Applies the shipped [`crate::STYLE`] preset at startup.
pub fn apply_shipped_style(ctx: &Context) {
    bootstrap_startup_theme(ctx);
}

/// Applies the shipped [`crate::STYLE`] preset.
pub fn apply_default_theme(ctx: &Context) {
    apply_shipped_style(ctx);
}

/// A preset's style under the Regular metrics overlay.
pub fn style_for_preset(preset: PresetStyles) -> Style {
    let mut style = preset_chrome(preset);
    apply_metrics(&mut style, &Metrics::for_density(Density::Regular));
    style
}

/// A preset's style as its theme defines it, before the metrics overlay.
fn preset_chrome(preset: PresetStyles) -> Style {
    match preset {
        PresetStyles::ShippedClassic => shipped_chrome(),
        PresetStyles::MtechStudio => mtech_studio::studio_style(),
        PresetStyles::LegacyClassic => legacy_classic_chrome(),
        PresetStyles::DefaultEgui => default_egui_chrome(),
        PresetStyles::MtechNoir => mtech_noir_chrome(),
        PresetStyles::MtechNoirGlass => mtech_noir_glass_chrome(),
        PresetStyles::NebulaGlass
        | PresetStyles::AuroraGlass
        | PresetStyles::SupernovaGlass
        | PresetStyles::EventHorizonGlass => neon_glass::neon_style(neon_palette_for_preset(preset)),
        PresetStyles::GalacticGlass => galactic_glass::galactic_style(),
        PresetStyles::ObsidianGlass
        | PresetStyles::VelvetGlass
        | PresetStyles::TwilightGlass
        | PresetStyles::QuartzGlass => soft_glass::soft_style(soft_palette_for_preset(preset)),
        PresetStyles::CarlDarkColors => colors_only(&CarlDark),
        PresetStyles::CarlDarkFull => CarlDark.custom_style(),
        PresetStyles::TokyoNightStormColors => colors_only(&TokyoNightStorm),
        PresetStyles::TokyoNightStormFull => TokyoNightStorm.custom_style(),
        PresetStyles::TokyoNightColors => colors_only(&TokyoNight),
        PresetStyles::TokyoNightFull => TokyoNight.custom_style(),
        PresetStyles::RerunMtechColors => colors_only(&RerunMtech),
        PresetStyles::RerunMtechFull => RerunMtech.custom_style(),
        PresetStyles::RerunMtechOledColors => colors_only(&RerunMtechOled),
        PresetStyles::RerunMtechOledFull => RerunMtechOled.custom_style(),
        PresetStyles::MtechGlassColors => colors_only(&MtechGlass),
        PresetStyles::MtechGlassFull => MtechGlass.custom_style(),
        PresetStyles::Custom => ThemeConfig::default().to_style(),
    }
}

fn colors_only(theme: &dyn Aesthetix) -> Style {
    let mut style = legacy_classic_chrome();
    paint_aesthetix_colors(&mut style, theme);
    style
}

pub fn semantic_colors_for_preset(preset: PresetStyles) -> (Color32, Color32) {
    match preset {
        PresetStyles::ShippedClassic | PresetStyles::LegacyClassic | PresetStyles::DefaultEgui => (
            Color32::from_rgb(42, 172, 170),
            Color32::from_rgb(56, 114, 238),
        ),
        PresetStyles::MtechStudio => mtech_studio::studio_semantic_colors(),
        // Emerald reads against the OLED base; the orchid is the palette's own selection tint.
        PresetStyles::MtechNoir | PresetStyles::MtechNoirGlass => (
            Color32::from_rgb(90, 220, 160),
            Color32::from_rgb(233, 130, 255),
        ),
        PresetStyles::NebulaGlass
        | PresetStyles::AuroraGlass
        | PresetStyles::SupernovaGlass
        | PresetStyles::EventHorizonGlass => {
            neon_glass::neon_semantic_colors(neon_palette_for_preset(preset))
        }
        PresetStyles::GalacticGlass => galactic_glass::galactic_semantic_colors(),
        PresetStyles::ObsidianGlass
        | PresetStyles::VelvetGlass
        | PresetStyles::TwilightGlass
        | PresetStyles::QuartzGlass => {
            soft_glass::soft_semantic_colors(soft_palette_for_preset(preset))
        }
        PresetStyles::CarlDarkColors | PresetStyles::CarlDarkFull => {
            (CarlDark.fg_success_text_color_visuals(), CarlDark.secondary_accent_color_visuals())
        }
        PresetStyles::TokyoNightStormColors | PresetStyles::TokyoNightStormFull => (
            TokyoNightStorm.fg_success_text_color_visuals(),
            TokyoNightStorm.secondary_accent_color_visuals(),
        ),
        PresetStyles::TokyoNightColors | PresetStyles::TokyoNightFull => (
            TokyoNight.fg_success_text_color_visuals(),
            TokyoNight.secondary_accent_color_visuals(),
        ),
        PresetStyles::RerunMtechColors | PresetStyles::RerunMtechFull => (
            RerunMtech.fg_success_text_color_visuals(),
            RerunMtech.secondary_accent_color_visuals(),
        ),
        PresetStyles::RerunMtechOledColors | PresetStyles::RerunMtechOledFull => (
            RerunMtechOled.fg_success_text_color_visuals(),
            RerunMtechOled.secondary_accent_color_visuals(),
        ),
        PresetStyles::MtechGlassColors | PresetStyles::MtechGlassFull => (
            MtechGlass.fg_success_text_color_visuals(),
            MtechGlass.secondary_accent_color_visuals(),
        ),
        PresetStyles::Custom => (Color32::from_rgb(72, 199, 142), Color32::from_rgb(191, 33, 101)),
    }
}

/// The palette behind each neon glass preset. Panics only for presets outside that family, which
/// the callers below never pass.
fn neon_palette_for_preset(preset: PresetStyles) -> &'static NeonPalette {
    match preset {
        PresetStyles::NebulaGlass => &neon_glass::NEBULA,
        PresetStyles::AuroraGlass => &neon_glass::AURORA,
        PresetStyles::SupernovaGlass => &neon_glass::SUPERNOVA,
        PresetStyles::EventHorizonGlass => &neon_glass::EVENT_HORIZON,
        other => unreachable!("{other:?} is not a neon glass preset"),
    }
}

/// The palette behind each soft glass preset. Panics only for presets outside that family, which
/// the callers below never pass.
fn soft_palette_for_preset(preset: PresetStyles) -> &'static SoftPalette {
    match preset {
        PresetStyles::ObsidianGlass => &soft_glass::OBSIDIAN,
        PresetStyles::VelvetGlass => &soft_glass::VELVET,
        PresetStyles::TwilightGlass => &soft_glass::TWILIGHT,
        PresetStyles::QuartzGlass => &soft_glass::QUARTZ,
        other => unreachable!("{other:?} is not a soft glass preset"),
    }
}

/// The backdrop-blur material a preset draws its glass against. Presets that paint opaque chrome
/// return [`GlassParams::OFF`], so switching away from a glass theme turns real frosting off.
pub fn glass_params_for_preset(preset: PresetStyles) -> GlassParams {
    match preset {
        PresetStyles::MtechStudio => mtech_studio::studio_glass_params(),
        PresetStyles::MtechNoirGlass => mtech_noir_glass_params(),
        PresetStyles::MtechGlassFull => glass_params_for_style(&glass_style()),
        PresetStyles::NebulaGlass
        | PresetStyles::AuroraGlass
        | PresetStyles::SupernovaGlass
        | PresetStyles::EventHorizonGlass => {
            neon_glass::neon_glass_params(neon_palette_for_preset(preset))
        }
        PresetStyles::GalacticGlass => galactic_glass::galactic_glass_params(),
        PresetStyles::ObsidianGlass
        | PresetStyles::VelvetGlass
        | PresetStyles::TwilightGlass
        | PresetStyles::QuartzGlass => {
            soft_glass::soft_glass_params(soft_palette_for_preset(preset))
        }
        _ => GlassParams::OFF,
    }
}

pub fn apply_preset(ctx: &Context, preset: PresetStyles) {
    apply_preset_semantics(ctx, preset);
    install_style(ctx, style_for_preset(preset));
}

fn sync_config_from_style(config: &mut ThemeConfig, style: &Style) {
    config.background_color = style.visuals.window_fill;
    config.text_color = style.visuals.override_text_color.unwrap_or(Color32::WHITE);
    config.faint_bg_color = style.visuals.faint_bg_color;
    config.extreme_bg_color = style.visuals.extreme_bg_color;
    config.code_bg_color = style.visuals.code_bg_color;
    config.warn_color = style.visuals.warn_fg_color;
    config.error_color = style.visuals.error_fg_color;
    config.link_color = style.visuals.hyperlink_color;
    config.window_stroke_color = style.visuals.window_stroke.color;
    config.rounding = style.visuals.window_corner_radius;
    config.selection_bg_fill = style.visuals.selection.bg_fill;
    config.selection_stroke_color = style.visuals.selection.stroke.color;
    config.widget_bg_fill = style.visuals.widgets.noninteractive.bg_fill;
    config.widget_weak_bg_fill = style.visuals.widgets.noninteractive.weak_bg_fill;
    config.widget_bg_stroke_color = style.visuals.widgets.noninteractive.bg_stroke.color;
    config.widget_fg_stroke_color = style.visuals.widgets.noninteractive.fg_stroke.color;
    config.hovered_bg_fill = style.visuals.widgets.hovered.bg_fill;
    config.hovered_weak_bg_fill = style.visuals.widgets.hovered.weak_bg_fill;
    config.hovered_bg_stroke_color = style.visuals.widgets.hovered.bg_stroke.color;
    config.hovered_fg_stroke_color = style.visuals.widgets.hovered.fg_stroke.color;
    config.active_bg_fill = style.visuals.widgets.active.bg_fill;
    config.active_weak_bg_fill = style.visuals.widgets.active.weak_bg_fill;
    config.active_bg_stroke_color = style.visuals.widgets.active.bg_stroke.color;
    config.active_fg_stroke_color = style.visuals.widgets.active.fg_stroke.color;
    config.open_bg_fill = style.visuals.widgets.open.bg_fill;
    config.open_weak_bg_fill = style.visuals.widgets.open.weak_bg_fill;
    config.open_bg_stroke_color = style.visuals.widgets.open.bg_stroke.color;
    config.open_fg_stroke_color = style.visuals.widgets.open.fg_stroke.color;
}

impl ThemeConfig {
    /// Live controls for the theme's backdrop-blur material. Writes straight to the context, so
    /// every frosted surface picks the change up on the next frame.
    fn backdrop_blur_row(&mut self, ui: &mut Ui, ctx: &Context) {
        let available = glass_backdrop::is_available();
        let mut params = glass_backdrop::params(ctx);
        let before = params;

        ui.add_enabled_ui(available, |ui| {
            controls::toggle(ui, &mut params.enabled, "Backdrop blur").on_hover_text(
                "Blur what is behind glass surfaces on the GPU. The tint is the film \
                 painted over the blur; its alpha mixes film against blurred backdrop. \
                 Click Save to store these dials on your account — they follow you to \
                 any machine you sign in on.",
            );
        });

        if !available {
            let warn = Tokens::get(ctx).warn;
            controls::status_label(ui, icons::svg::WARNING, "GPU blur unavailable on this host", warn)
                .on_hover_text(
                    "The grab-pass backend did not come up — the app is not on the glow \
                     renderer, or the GL context is below OpenGL 3.3 / GLES 3.0. Glass \
                     surfaces fall back to flat tinted panes.",
                );
            return;
        }

        ui.add_enabled_ui(params.enabled, |ui| {
            controls::vline(ui, ui.available_height() - 8.0);
            ui.label("Radius");
            ValueField::new(&mut params.blur_radius, 0.0..=64.0)
                .suffix("px")
                .speed(0.5)
                .width(BLUR_FIELD_W)
                .show(ui);
            ui.label("Tint");
            ui.color_edit_button_srgba(&mut params.tint);
            ui.label("Corners");
            ValueField::new(&mut params.corner_radius, 0.0..=32.0)
                .suffix("px")
                .speed(0.2)
                .width(BLUR_FIELD_W)
                .show(ui);
            ui.label("Presence");
            controls::Slider::new(&mut params.presence, 0.0..=1.0)
                .width(PRESENCE_W)
                .step(0.01)
                .value("")
                .show(ui);
        });

        if params != before {
            glass_backdrop::set_params(ctx, params);
        }
    }

    /// Density and text size pickers, the only geometry an operator controls.
    fn layout_row(&mut self, ui: &mut Ui, ctx: &Context) {
        controls::bar(ui, controls::BarKind::Header, |ui| {
            let mut density = metrics::density(ctx);
            let densities = Density::ALL.map(|d| (d, d.label()));
            controls::icon_label(ui, icons::svg::ROWS, "Density");
            controls::dropdown(ui, "theme_density", &mut density, &densities, 130.0)
                .on_hover_text("Control height, padding and spacing. Click Save to keep.");
            set_density(ctx, density);

            controls::vline(ui, ui.available_height() - 8.0);

            let mut scale = fonts::text_scale(ctx);
            let scales = TextScale::ALL.map(|s| (s, s.label()));
            controls::icon_label(ui, icons::svg::TEXT_AA, "Text size");
            controls::dropdown(ui, "theme_text_scale", &mut scale, &scales, 150.0)
                .on_hover_text("Scales text without changing control geometry. Click Save to keep.");
            set_text_scale(ctx, scale);
        });
    }

    /// Colour editor over the live visuals; an edit the overlay undoes leaves the preset as is.
    fn colors_ui(&mut self, ui: &mut Ui, ctx: &Context) {
        controls::subheading(ui, icons::svg::PALETTE, "Colors");
        let mut style = (*ctx.global_style()).clone();
        let before = style.visuals.clone();
        style.visuals.ui(ui);
        if style.visuals != before {
            self.install_edit(ctx, style, &before);
        }
    }

    /// Installs an edited style; records Custom when the installed visuals differ from `before`.
    fn install_edit(&mut self, ctx: &Context, style: Style, before: &Visuals) {
        install_style(ctx, style);
        if ctx.global_style().visuals != *before {
            set_installed_preset(ctx, PresetStyles::Custom);
            sync_editor_config(self, ctx);
        }
    }

    /// Installs `chosen` and syncs the editor to it; Custom installs nothing.
    fn pick_preset(&mut self, ctx: &Context, chosen: PresetStyles) {
        if chosen == PresetStyles::Custom {
            return;
        }
        apply_preset(ctx, chosen);
        sync_editor_config(self, ctx);
    }

    /// The preset picker; applies the pick unless it is Custom or already installed.
    fn preset_menu(&mut self, ui: &mut Ui, ctx: &Context) {
        let current = installed_preset(ctx);
        let mut picked = None;
        let _ = MenuButton::new(current.as_str())
            .icon(icons::svg::SWATCHES)
            .tip("Style preset")
            .show(ui, |ui| {
                for (i, (title, presets)) in PRESET_GROUPS.iter().enumerate() {
                    if i > 0 {
                        controls::menu_separator(ui);
                    }
                    if !title.is_empty() {
                        controls::menu_inset_row(ui, |ui| controls::section_label(ui, title));
                    }
                    for &preset in presets.iter() {
                        let row = controls::MenuItem::new(preset.as_str())
                            .checked(preset == current)
                            .enabled(preset != PresetStyles::Custom)
                            .show(ui);
                        if row.clicked() {
                            picked = Some(preset);
                        }
                    }
                }
            });
        if let Some(chosen) = picked.filter(|p| *p != current) {
            self.pick_preset(ctx, chosen);
        }
    }

    /// Reset, the preset picker, and the save, export and import buttons.
    fn preset_row(&mut self, ui: &mut Ui, ctx: &Context, tx: &Sender<SavedTheme>) -> bool {
        let reset = TextButton::new("Reset to Default")
            .icon(icons::svg::ARROW_COUNTER_CLOCKWISE)
            .kind(ButtonKind::Ghost)
            .show(ui)
            .on_hover_text("Preview the shipped default theme. Nothing is saved until you click Save.");
        if reset.clicked() {
            self.pick_preset(ctx, PresetStyles::ShippedClassic);
        }
        self.preset_menu(ui, ctx);

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let save = TextButton::new("Save")
                .icon(icons::svg::FLOPPY_DISK)
                .kind(ButtonKind::Primary)
                .show(ui)
                .on_hover_text("Store this theme on your account.");
            let saved_now = save.clicked();
            if saved_now {
                let saved = SavedTheme::current(ctx);
                PlatformSpawner::spawn(async move {
                    match User::update_color_scheme(encode_theme(&saved).unwrap_or_default().into()).await {
                        Ok(_) => log::info!("Updated Color Settings"),
                        Err(e) => log::error!("Error updating color settings: {e:?}"),
                    }
                });
            }
            #[cfg(not(any(target_os = "ios", target_os = "android")))]
            Self::file_buttons(ui, ctx, tx);
            #[cfg(any(target_os = "ios", target_os = "android"))]
            let _ = tx;
            saved_now
        })
        .inner
    }

    /// Export to and import from a JSON file.
    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    fn file_buttons(ui: &mut Ui, ctx: &Context, tx: &Sender<SavedTheme>) {
        let save_local = TextButton::new("Save to file")
            .icon(icons::svg::DOWNLOAD_SIMPLE)
            .show(ui)
            .on_hover_text("Export this theme as JSON.");
        if save_local.clicked() {
            let saved = SavedTheme::current(ctx);
            PlatformSpawner::spawn(async move {
                let Ok(json_data) = super::theme_to_json(&saved) else {
                    log::error!("Error serializing settings to json");
                    return;
                };
                if let Some(file) = rfd::AsyncFileDialog::new()
                    .set_file_name("mastertech_color_scheme.json")
                    .save_file()
                    .await
                    && let Err(err) = file.write(&json_data).await
                {
                    log::error!("Failed to save file: {:?}", err);
                }
            });
        }

        let upload = TextButton::new("Upload settings")
            .icon(icons::svg::UPLOAD_SIMPLE)
            .show(ui)
            .on_hover_text("Import a theme JSON and save it to your account.");
        if upload.clicked() {
            let tx = tx.clone();
            PlatformSpawner::spawn(async move {
                let Some(file) = rfd::AsyncFileDialog::new()
                    .add_filter("Json", &["json"])
                    .set_file_name("mastertech_color_scheme.json")
                    .pick_file()
                    .await
                else {
                    return;
                };
                // Accepts an exported scheme or a bare egui Style file without glass material.
                match super::theme_from_json(&file.read().await) {
                    Ok(theme) => {
                        match User::update_color_scheme(encode_theme(&theme).unwrap_or_default().into()).await {
                            Ok(_) => log::info!("Imported color scheme saved"),
                            Err(e) => log::error!("Error saving imported scheme: {e:?}"),
                        }
                        let _ = tx.try_send(theme);
                    }
                    Err(e) => log::error!("Error converting bytes to Theme: {e:?}"),
                }
            });
        }
    }

    /// Installs the glassified installed style and its glass material as Custom.
    fn glassify_installed(&mut self, ctx: &Context) {
        let glassified = glassify(&ctx.global_style());
        glass_backdrop::set_params(ctx, glass_params_for_style(&glassified));
        install_style(ctx, glassified);
        set_installed_preset(ctx, PresetStyles::Custom);
        sync_editor_config(self, ctx);
    }

    /// Glassify and restore-from-account.
    fn restore_row(&mut self, ui: &mut Ui, ctx: &Context, tx: &Sender<SavedTheme>) {
        let glassify_btn = TextButton::new("Glassify current theme")
            .icon(icons::svg::SPARKLE)
            .show(ui)
            .on_hover_text(
                "Re-style the applied theme as tinted glass using its own colors: \
                 translucent widget fills with outlines a step brighter than each fill. \
                 Backgrounds, rounding, and fonts stay as they are. Click Save to keep.",
            );
        if glassify_btn.clicked() {
            self.glassify_installed(ctx);
        }

        let restore = TextButton::new("Restore saved theme")
            .icon(icons::svg::CLOCK_COUNTER_CLOCKWISE)
            .show(ui)
            .on_hover_text("Re-apply the color scheme saved to your account.");
        if restore.clicked() {
            let tx = tx.clone();
            PlatformSpawner::spawn(async move {
                match User::get_current_user_from_auth().await {
                    Ok(Some(user)) => {
                        let bytes = user.get_color_scheme();
                        if bytes.is_empty() {
                            log::warn!("No color scheme saved on this account");
                            let _ = crate::get_toast_sender()
                                .try_send(crate::ToastMessage::Warning("No saved theme on this account".into()));
                            return;
                        }
                        match decode_theme(&bytes) {
                            Ok(theme) => {
                                // Match the login guard: a legacy egui-default record restores the shipped theme.
                                let theme = if is_blank_default_style(&theme.style) {
                                    SavedTheme {
                                        style: style_for_preset(PresetStyles::ShippedClassic),
                                        glass: Some(glass_params_for_preset(PresetStyles::ShippedClassic)),
                                        ..theme
                                    }
                                } else {
                                    theme
                                };
                                let _ = tx.try_send(theme);
                            }
                            Err(e) => {
                                log::error!("Saved color scheme decode failed: {e:?}");
                                let _ = crate::get_toast_sender()
                                    .try_send(crate::ToastMessage::Error("Saved theme could not be read".into()));
                            }
                        }
                    }
                    Ok(None) => {
                        log::warn!("Not signed in; cannot restore saved theme");
                        let _ = crate::get_toast_sender()
                            .try_send(crate::ToastMessage::Warning("Sign in to restore your saved theme".into()));
                    }
                    Err(e) => {
                        log::error!("Error fetching user for saved theme: {e:?}");
                        let _ = crate::get_toast_sender()
                            .try_send(crate::ToastMessage::Error("Could not reach the server to restore your theme".into()));
                    }
                }
            });
        }
    }

    pub fn edit_ui(&mut self, ui: &mut Ui, ctx: &Context, tx: Sender<SavedTheme>) -> (bool, Arc<Style>) {
        sync_editor_config(self, ctx);
        let m = Metrics::get(ctx);
        let frame = Frame::side_top_panel(&ctx.global_style()).inner_margin(Margin::ZERO);
        let mut saved = false;
        eframe::egui::Panel::top("Theme Menu top bar")
            .frame(frame)
            .exact_size(3.0 * BarKind::Tool.height(&m))
            .show_separator_line(false)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                saved = controls::bar(ui, BarKind::Tool, |ui| self.preset_row(ui, ctx, &tx));
                controls::bar(ui, BarKind::Tool, |ui| self.restore_row(ui, ctx, &tx));
                controls::bar(ui, BarKind::Tool, |ui| self.backdrop_blur_row(ui, ctx));
            });

        ui.add_space(m.gap.y);

        ScrollArea::vertical()
        .scroll_bar_visibility(ScrollBarVisibility::AlwaysVisible)
        .max_width(800.)
        .show(ui, |ui|
        {
            glass_backdrop::preview(ui, glass_backdrop::params(ctx));
            ui.add_space(m.gap.y);
            self.layout_row(ui, ctx);
            if controls::section_header(ui, "theme_control_preview", icons::svg::SQUARES_FOUR, "Control preview") {
                ui.push_id("theme_control_preview", controls::gallery);
            }
            controls::hairline(ui);
            self.colors_ui(ui, ctx);
        });
        /*
        ScrollArea::vertical()
            .scroll_bar_visibility(ScrollBarVisibility::AlwaysVisible)
            .max_width(800.)
            .show(ui, |ui| 
        {

            ui.horizontal(|ui| {
                ui.label("Font Selection"); 
            
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ComboBox::new("Font Selection", "")
                        .selected_text(self.font.to_string())
                        .show_ui(ui, |ui| {
                            let fonts = ui.fonts(|f| f.clone());
                            for font in fonts.families() {
                                ui.selectable_value(
                                    &mut self.font,
                                    font.clone(),
                                    font.to_string(),
                                );
                            }
                        });
                });
            });

            ui.horizontal(|ui| {
                ui.label("Font Size"); 
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    DragValue::new(&mut self.font_size).range(std::ops::RangeInclusive::new(10., 16.)).ui(ui);
                });
            });

            ui.vertical_centered(|ui| {
                ui.heading("Widget Colors");
            });

            // Widget Colors
            ui.horizontal(|ui| {
                ui.label("Widget Background Fill"); 
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.widget_bg_fill);
                });
            });

            ui.horizontal(|ui| {
                ui.label("Widget Weak Background Fill");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.widget_weak_bg_fill);
                });
            });

            ui.horizontal(|ui| {
                ui.label("Widget Background Stroke Color");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.widget_bg_stroke_color);
                });
            });
            ui.horizontal(|ui| {
                ui.label("Widget Foreground Stroke Color");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.widget_fg_stroke_color);
                });
            });

            ui.separator();
            ui.add_space(10.);

            // Hovered Colors
            ui.vertical_centered(|ui| {
                ui.heading("Hovered Colors:");
            });

            ui.horizontal(|ui| {   
                ui.label("Hovered Background Fill");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.hovered_bg_fill);
                });
            });

            ui.horizontal(|ui| {
                ui.label("Hovered Weak Background Fill");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.hovered_weak_bg_fill);
                });
            });

            ui.horizontal(|ui| {
                ui.label("Hovered Background Stroke Color");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.hovered_bg_stroke_color);
                });
            });

            ui.horizontal(|ui| {
                ui.label("Hovered Foreground Stroke Color");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.hovered_fg_stroke_color);
                });
            });

            ui.separator();
            ui.add_space(10.);

            // Active Colors
            ui.vertical_centered(|ui| {
                ui.heading("Active Colors:");
            });

            ui.horizontal(|ui| {
                ui.label("Active Background Fill");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.active_bg_fill);
                });
            });

            ui.horizontal(|ui| {
                ui.label("Active Weak Background Fill");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.active_weak_bg_fill);
                });
            });

            ui.horizontal(|ui| {
                ui.label("Active Background Stroke Color");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.active_bg_stroke_color);
                });
            });

            ui.horizontal(|ui| {
                ui.label("Active Foreground Stroke Color");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.active_fg_stroke_color);
                });
            });

            ui.separator();
            ui.add_space(10.);

            // Open Colors
            ui.vertical_centered(|ui| {
                ui.heading("Open Colors:");
            });

            ui.horizontal(|ui| {
                ui.label("Open Background Fill");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.open_bg_fill);
                });
            });

            ui.horizontal(|ui| {
                ui.label("Open Weak Background Fill");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.open_weak_bg_fill);
                });
            });

            ui.horizontal(|ui| {
                ui.label("Open Background Stroke Color");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.open_bg_stroke_color);
                });
            });

            ui.horizontal(|ui| {
                ui.label("Open Foreground Stroke Color");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.open_fg_stroke_color);
                });
            });

            ui.separator();
            ui.add_space(10.);

            // Selection Colors
            ui.vertical_centered(|ui| {
                ui.heading("Selection Colors:");
            });

            ui.horizontal(|ui| {
                ui.label("Selection Background Fill");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.selection_bg_fill);
                });
            });

            ui.horizontal(|ui| {
                ui.label("Selection Stroke Color");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.selection_stroke_color);
                });
            });

            ui.separator();
            ui.add_space(10.);

            // Other Colors
            ui.vertical_centered(|ui| {
                ui.heading("Other Colors:");
            });
            
            ui.horizontal(|ui| {
                ui.label("Background Color");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.background_color);
                });
            });

            ui.horizontal(|ui| {
                ui.label("Foreground Color");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.foreground_color);
                });
            });

            ui.horizontal(|ui| {
                ui.label("Border Color");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.border_color);
                });
            });

            ui.horizontal(|ui| {
                ui.label("Text Color");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.text_color);
                });
            });

            ui.horizontal(|ui| {
                ui.label("Error Color");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.error_color);
                });
            });

            ui.horizontal(|ui| {
                ui.label("Warning Color");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.warn_color);
                });
            });

            ui.horizontal(|ui| {
                ui.label("Link Color");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.link_color);
                });
            });

            ui.horizontal(|ui| {
                ui.label("Faint Background Color");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.faint_bg_color);
                });
            });

            ui.horizontal(|ui| {
                ui.label("Extreme Background Color");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.extreme_bg_color);
                });
            });

            ui.horizontal(|ui| {
                ui.label("Code Background Color");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.code_bg_color);
                });
            });

            ui.separator();
            ui.add_space(10.);

            // Strokes
            ui.vertical_centered(|ui| {
                ui.heading("Strokes:");
            });

            ui.horizontal(|ui| {
                ui.label("Window Stroke:");
                ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                    ui.color_edit_button_srgba(&mut self.window_stroke_color);
                });
            });

            ui.separator();
            ui.add_space(10.);

            // eframe::egui::CornerRadius
            ui.vertical_centered(|ui| {
                ui.heading("eframe::egui::CornerRadius:");
            });
            
            ui.add(DragValue::new(&mut self.rounding.nw).speed(0.1).prefix("NW:"));
            ui.add(DragValue::new(&mut self.rounding.ne).speed(0.1).prefix("NE:"));
            ui.add(DragValue::new(&mut self.rounding.sw).speed(0.1).prefix("SW:"));
            ui.add(DragValue::new(&mut self.rounding.se).speed(0.1).prefix("SE:"));
        }); 
        */

        (saved, ctx.global_style())
    }
}

impl ThemeConfig {
    /// Create a ThemeConfig instance from an egui::Style (best-effort mapping)
    pub fn from_style(style: &Style) -> Self {
        // Map the visual sections into our ThemeConfig fields. Some fields may not have a 1:1 mapping; we approximate.
        let visuals = &style.visuals;
        let widgets = &visuals.widgets;
        Self {
            background_color: visuals.window_fill,
            foreground_color: visuals.override_text_color.unwrap_or(Color32::WHITE),
            widget_bg_fill: widgets.noninteractive.bg_fill,
            widget_weak_bg_fill: widgets.noninteractive.weak_bg_fill,
            widget_bg_stroke_color: widgets.noninteractive.bg_stroke.color,
            widget_fg_stroke_color: widgets.noninteractive.fg_stroke.color,
            hovered_bg_fill: widgets.hovered.bg_fill,
            hovered_weak_bg_fill: widgets.hovered.weak_bg_fill,
            hovered_bg_stroke_color: widgets.hovered.bg_stroke.color,
            hovered_fg_stroke_color: widgets.hovered.fg_stroke.color,
            active_bg_fill: widgets.active.bg_fill,
            active_weak_bg_fill: widgets.active.weak_bg_fill,
            active_bg_stroke_color: widgets.active.bg_stroke.color,
            active_fg_stroke_color: widgets.active.fg_stroke.color,
            open_bg_fill: widgets.open.bg_fill,
            open_weak_bg_fill: widgets.open.weak_bg_fill,
            open_bg_stroke_color: widgets.open.bg_stroke.color,
            open_fg_stroke_color: widgets.open.fg_stroke.color,
            selection_bg_fill: visuals.selection.bg_fill,
            selection_stroke_color: visuals.selection.stroke.color,
            faint_bg_color: visuals.faint_bg_color,
            extreme_bg_color: visuals.extreme_bg_color,
            code_bg_color: visuals.code_bg_color,
            border_color: visuals.window_stroke.color,
            text_color: visuals.override_text_color.unwrap_or(Color32::WHITE),
            error_color: visuals.error_fg_color,
            warn_color: visuals.warn_fg_color,
            link_color: visuals.hyperlink_color,
            window_stroke_color: visuals.window_stroke.color,
            rounding: visuals.window_corner_radius,
            preset_style: PresetStyles::Custom,
        }
    }

    /// Convert this ThemeConfig back into an egui::Style applying mapped fields.
    pub fn to_style(&self) -> Style {
        let mut style = Style::default();
        let visuals = &mut style.visuals;
        // Non-widget
        visuals.window_fill = self.background_color;
        visuals.faint_bg_color = self.faint_bg_color;
        visuals.extreme_bg_color = self.extreme_bg_color;
        visuals.code_bg_color = self.code_bg_color;
        visuals.override_text_color = Some(self.text_color);
        visuals.warn_fg_color = self.warn_color;
        visuals.error_fg_color = self.error_color;
        visuals.hyperlink_color = self.link_color;
        visuals.window_stroke.color = self.window_stroke_color;
        visuals.window_corner_radius = self.rounding;
        visuals.selection.bg_fill = self.selection_bg_fill;
        visuals.selection.stroke.color = self.selection_stroke_color;

        // Widgets mapping
        let assign = |wv: &mut WidgetVisuals, src_bg_fill: Color32, src_weak_bg_fill: Color32, stroke: Color32, fg_stroke: Color32| {
            wv.bg_fill = src_bg_fill;
            wv.weak_bg_fill = src_weak_bg_fill;
            wv.bg_stroke.color = stroke;
            wv.fg_stroke.color = fg_stroke;
        };
        assign(&mut visuals.widgets.noninteractive, self.widget_bg_fill, self.widget_weak_bg_fill, self.widget_bg_stroke_color, self.widget_fg_stroke_color);
        assign(&mut visuals.widgets.hovered, self.hovered_bg_fill, self.hovered_weak_bg_fill, self.hovered_bg_stroke_color, self.hovered_fg_stroke_color);
        assign(&mut visuals.widgets.active, self.active_bg_fill, self.active_weak_bg_fill, self.active_bg_stroke_color, self.active_fg_stroke_color);
        assign(&mut visuals.widgets.open, self.open_bg_fill, self.open_weak_bg_fill, self.open_bg_stroke_color, self.open_fg_stroke_color);

        style
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PresetStyles {
    ShippedClassic,
    MtechStudio,
    LegacyClassic,
    DefaultEgui,
    MtechNoir,
    MtechNoirGlass,
    NebulaGlass,
    AuroraGlass,
    SupernovaGlass,
    EventHorizonGlass,
    GalacticGlass,
    ObsidianGlass,
    VelvetGlass,
    TwilightGlass,
    QuartzGlass,
    CarlDarkColors,
    CarlDarkFull,
    TokyoNightStormColors,
    TokyoNightStormFull,
    TokyoNightColors,
    TokyoNightFull,
    RerunMtechColors,
    RerunMtechFull,
    RerunMtechOledColors,
    RerunMtechOledFull,
    MtechGlassColors,
    MtechGlassFull,
    Custom,
}

impl Default for PresetStyles {
    fn default() -> Self {
        Self::ShippedClassic
    }
}

impl PresetStyles {
    pub fn as_str(&self) -> &str {
        match self {
            PresetStyles::ShippedClassic => "Shipped Classic",
            PresetStyles::MtechStudio => mtech_studio::NAME,
            PresetStyles::LegacyClassic => "Legacy Classic",
            PresetStyles::DefaultEgui => "Default Egui",
            PresetStyles::MtechNoir => "MTech Noir",
            PresetStyles::MtechNoirGlass => "MTech Noir · Glass",
            PresetStyles::NebulaGlass => neon_glass::NEBULA.name,
            PresetStyles::AuroraGlass => neon_glass::AURORA.name,
            PresetStyles::SupernovaGlass => neon_glass::SUPERNOVA.name,
            PresetStyles::EventHorizonGlass => neon_glass::EVENT_HORIZON.name,
            PresetStyles::GalacticGlass => galactic_glass::NAME,
            PresetStyles::ObsidianGlass => soft_glass::OBSIDIAN.name,
            PresetStyles::VelvetGlass => soft_glass::VELVET.name,
            PresetStyles::TwilightGlass => soft_glass::TWILIGHT.name,
            PresetStyles::QuartzGlass => soft_glass::QUARTZ.name,
            PresetStyles::CarlDarkColors => "Carl Dark · Colors",
            PresetStyles::CarlDarkFull => "Carl Dark · Full",
            PresetStyles::TokyoNightStormColors => "TokyoNight Storm · Colors",
            PresetStyles::TokyoNightStormFull => "TokyoNight Storm · Full",
            PresetStyles::TokyoNightColors => "TokyoNight · Colors",
            PresetStyles::TokyoNightFull => "TokyoNight · Full",
            PresetStyles::RerunMtechColors => "Rerun MTech · Colors",
            PresetStyles::RerunMtechFull => "Rerun MTech · Full",
            PresetStyles::RerunMtechOledColors => "Rerun MTech OLED · Colors",
            PresetStyles::RerunMtechOledFull => "Rerun MTech OLED · Full",
            PresetStyles::MtechGlassColors => "MTech Glass · Colors",
            PresetStyles::MtechGlassFull => "MTech Glass · Full",
            PresetStyles::Custom => "Custom",
        }
    }

    pub fn from_str(str: &str) -> Self {
        match str {
            "Shipped Classic" => Self::ShippedClassic,
            mtech_studio::NAME => Self::MtechStudio,
            "Legacy Classic" => Self::LegacyClassic,
            "Default Egui" => Self::DefaultEgui,
            "MTech Noir" => Self::MtechNoir,
            "MTech Noir · Glass" => Self::MtechNoirGlass,
            "Nebula Glass" => Self::NebulaGlass,
            "Aurora Glass" => Self::AuroraGlass,
            "Supernova Glass" => Self::SupernovaGlass,
            "Event Horizon Glass" => Self::EventHorizonGlass,
            "Galactic Glass" => Self::GalacticGlass,
            "Obsidian Glass" => Self::ObsidianGlass,
            "Velvet Glass" => Self::VelvetGlass,
            "Twilight Glass" => Self::TwilightGlass,
            "Quartz Glass" => Self::QuartzGlass,
            "Carl Dark · Colors" | "Carl Dark" => Self::CarlDarkColors,
            "Carl Dark · Full" => Self::CarlDarkFull,
            "TokyoNight Storm · Colors" | "TokyoNight Storm" => Self::TokyoNightStormColors,
            "TokyoNight Storm · Full" => Self::TokyoNightStormFull,
            "TokyoNight · Colors" | "TokyoNight" => Self::TokyoNightColors,
            "TokyoNight · Full" => Self::TokyoNightFull,
            "Rerun MTech · Colors" | "Rerun MTech" => Self::RerunMtechColors,
            "Rerun MTech · Full" => Self::RerunMtechFull,
            "Rerun MTech OLED · Colors" => Self::RerunMtechOledColors,
            "Rerun MTech OLED · Full" => Self::RerunMtechOledFull,
            "MTech Glass · Colors" => Self::MtechGlassColors,
            "MTech Glass · Full" | "MTech Glass" => Self::MtechGlassFull,
            "Custom" => Self::Custom,
            _ => Self::Custom,
        }
    }
}

pub fn set_custom_style(config: &ThemeConfig) -> Arc<Style> {
    if config.preset_style != PresetStyles::Custom {
        return Arc::new(style_for_preset(config.preset_style));
    }

    let theme = CarlDark;
    let mut custom_style: Style = theme.custom_style();
            custom_style.interaction.selectable_labels = true;
            custom_style.interaction.interact_radius = 10.0;
            custom_style.interaction.show_tooltips_only_when_still = false;
            custom_style.interaction.tooltip_delay = 0.1;
            
            custom_style.visuals = Visuals {
                dark_mode: true,
                override_text_color: Some(config.text_color),
                widgets: Widgets {
                    noninteractive: WidgetVisuals {
                        bg_fill: config.widget_bg_fill,
                        weak_bg_fill: config.widget_weak_bg_fill,
                        bg_stroke: Stroke::new(1.0_f32, config.widget_bg_stroke_color),
                        corner_radius: config.rounding,
                        fg_stroke: Stroke::new(1.0_f32, config.widget_fg_stroke_color),
                        expansion: 0.0,
                    },
                    inactive: WidgetVisuals {
                        bg_fill: config.widget_bg_fill,
                        weak_bg_fill: Color32::from_rgb(18, 18, 20),
                        bg_stroke: Stroke::new(1.0_f32, Color32::from_rgb(80, 80, 80)),
                        corner_radius: config.rounding,
                        fg_stroke: Stroke::new(1.0_f32, config.widget_bg_stroke_color),
                        expansion: 0.2,
                    },
                    hovered: WidgetVisuals {
                        bg_fill: config.hovered_bg_fill,
                        weak_bg_fill: config.hovered_weak_bg_fill,
                        bg_stroke: Stroke::new(0.5_f32, config.hovered_bg_stroke_color),
                        corner_radius: config.rounding,
                        fg_stroke: Stroke::new(1.0_f32, config.hovered_fg_stroke_color),
                        expansion: 0.2,
                    },
                    active: WidgetVisuals {
                        bg_fill: config.active_bg_fill,
                        weak_bg_fill: config.active_weak_bg_fill,
                        bg_stroke: Stroke::new(1.0_f32, config.active_bg_stroke_color),
                        corner_radius: config.rounding,
                        fg_stroke: Stroke::new(1.0_f32, config.active_fg_stroke_color),
                        expansion: 0.2,
                    },
                    open: WidgetVisuals {
                        bg_fill: config.open_bg_fill,
                        weak_bg_fill: config.open_weak_bg_fill,
                        bg_stroke: Stroke::new(1.0_f32, config.open_bg_stroke_color),
                        corner_radius: config.rounding,
                        fg_stroke: Stroke::new(1.0_f32, config.open_fg_stroke_color),
                        expansion: 0.2,
                    },
                },
                selection: Selection {
                    bg_fill: config.selection_bg_fill,
                    stroke: Stroke::new(1.0_f32, config.selection_stroke_color),
                },
                hyperlink_color: config.link_color,
                faint_bg_color: config.faint_bg_color,
                extreme_bg_color: config.extreme_bg_color,
                code_bg_color: config.code_bg_color,
                warn_fg_color: config.warn_color,
                error_fg_color: config.error_color,
                window_fill: config.background_color,
                window_stroke: Stroke::new(1.0_f32, config.window_stroke_color),
                window_corner_radius: config.rounding,
                menu_corner_radius: config.rounding,
                panel_fill: config.background_color,
                popup_shadow: Shadow::default(),
                resize_corner_size: 10.0,
                text_cursor: TextCursorStyle::default(),
                button_frame: true,
                collapsing_header_frame: true,
                indent_has_left_vline: true,
                striped: true,
                slider_trailing_fill: true,
                handle_shape: HandleShape::Circle,
                interact_cursor: Some(CursorIcon::PointingHand),
                image_loading_spinners: true,
                numeric_color_space: NumericColorSpace::Linear,
                ..Default::default()
            };

    Arc::new(custom_style)
}

// -------------------------------------------------------------------------------------------------
// CSS Variable Export
// -------------------------------------------------------------------------------------------------
// This extra impl block adds a helper to turn the ThemeConfig into a bundle of CSS custom properties
// so the Dioxus/mobile (and web) targets can share a single canonical theme definition with the egui
// desktop target. The resulting string is meant to be injected into a <style> tag once per login or
// whenever the user saves new theme preferences.
impl ThemeConfig {
    /// Generate a `:root { ... }` block of CSS variables representing this theme.
    /// Variable prefix: `--mtk-` ("MasterTech") to reduce collision risk.
    pub fn to_css_variables(&self) -> String {
        fn color_to_rgba(c: Color32) -> String {
            let [r, g, b, a] = c.to_srgba_unmultiplied();
            format!("rgba({},{},{},{:.3})", r, g, b, a as f32 / 255.0)
        }
        use std::fmt::Write as _;

        let mut out = String::from(":root{\n");
        macro_rules! var { ($name:literal,$val:expr) => {{ let _ = write!(out, "  --mtk-{}:{};\n", $name, $val); }}; }

        var!("background-color", color_to_rgba(self.background_color));
        var!("foreground-color", color_to_rgba(self.foreground_color));
        var!("widget-bg-fill", color_to_rgba(self.widget_bg_fill));
        var!("widget-weak-bg-fill", color_to_rgba(self.widget_weak_bg_fill));
        var!("widget-bg-stroke-color", color_to_rgba(self.widget_bg_stroke_color));
        var!("widget-fg-stroke-color", color_to_rgba(self.widget_fg_stroke_color));
        var!("hovered-bg-fill", color_to_rgba(self.hovered_bg_fill));
        var!("hovered-weak-bg-fill", color_to_rgba(self.hovered_weak_bg_fill));
        var!("hovered-bg-stroke-color", color_to_rgba(self.hovered_bg_stroke_color));
        var!("hovered-fg-stroke-color", color_to_rgba(self.hovered_fg_stroke_color));
        var!("active-bg-fill", color_to_rgba(self.active_bg_fill));
        var!("active-weak-bg-fill", color_to_rgba(self.active_weak_bg_fill));
        var!("active-bg-stroke-color", color_to_rgba(self.active_bg_stroke_color));
        var!("active-fg-stroke-color", color_to_rgba(self.active_fg_stroke_color));
        var!("open-bg-fill", color_to_rgba(self.open_bg_fill));
        var!("open-weak-bg-fill", color_to_rgba(self.open_weak_bg_fill));
        var!("open-bg-stroke-color", color_to_rgba(self.open_bg_stroke_color));
        var!("open-fg-stroke-color", color_to_rgba(self.open_fg_stroke_color));
        var!("selection-bg-fill", color_to_rgba(self.selection_bg_fill));
        var!("selection-stroke-color", color_to_rgba(self.selection_stroke_color));
        var!("faint-bg-color", color_to_rgba(self.faint_bg_color));
        var!("extreme-bg-color", color_to_rgba(self.extreme_bg_color));
        var!("code-bg-color", color_to_rgba(self.code_bg_color));
        var!("border-color", color_to_rgba(self.border_color));
        var!("text-color", color_to_rgba(self.text_color));
        var!("error-color", color_to_rgba(self.error_color));
        var!("warn-color", color_to_rgba(self.warn_color));
        var!("link-color", color_to_rgba(self.link_color));
        var!("window-stroke-color", color_to_rgba(self.window_stroke_color));
        // Numeric values
        var!("font-size", format!("{:.2}px", type_scale::BODY));
        // CornerRadius is a uniform value in our usage (same for all corners via .ne)
        var!("rounding", format!("{:.2}px", self.rounding.ne));
        out.push_str("}\n");
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saved(preset: PresetStyles) -> SavedTheme {
        SavedTheme {
            style: style_for_preset(preset),
            glass: Some(glass_params_for_preset(preset)),
            density: Density::default(),
            text_scale: TextScale::default(),
        }
    }

    fn all_presets() -> impl Iterator<Item = PresetStyles> {
        MATCHABLE_PRESETS.into_iter().chain([PresetStyles::Custom])
    }

    /// The exact or legacy preset a saved style matches.
    fn preset_matching_style(style: &Style) -> Option<PresetStyles> {
        let style = normalized(style);
        exact_preset(&style).or_else(|| legacy_preset(&style))
    }

    #[test]
    fn every_preset_round_trips_as_itself_under_the_overlay() {
        for preset in MATCHABLE_PRESETS {
            let bytes = encode_theme(&saved(preset)).unwrap();
            let decoded = decode_theme(&bytes).unwrap();
            assert_eq!(
                preset_matching_style(&decoded.style),
                Some(preset),
                "{} did not round trip",
                preset.as_str(),
            );
        }
    }

    #[test]
    fn every_preset_shares_the_overlay_geometry() {
        let regular = Metrics::for_density(Density::Regular);
        for preset in all_presets() {
            let style = style_for_preset(preset);
            let name = preset.as_str();
            assert_eq!(style.spacing.interact_size.y, regular.control_h, "{name}: control height");
            assert_eq!(style.spacing.button_padding, regular.pad, "{name}: padding");
            assert!(style.override_font_id.is_none(), "{name}: font override");

            let w = &style.visuals.widgets;
            let radius = w.inactive.corner_radius;
            for state in [&w.noninteractive, &w.inactive, &w.hovered, &w.active, &w.open] {
                for width in [state.bg_stroke.width, state.fg_stroke.width] {
                    assert_eq!(width, width.round(), "{name}: fractional stroke {width}");
                }
                assert_eq!(state.expansion, 0.0, "{name}: widget grows");
                assert_eq!(state.corner_radius, radius, "{name}: states disagree on radius");
            }
        }
    }

    #[test]
    fn density_and_text_scale_round_trip_through_the_account_record() {
        let theme = SavedTheme {
            density: Density::Comfortable,
            text_scale: TextScale::Large,
            ..saved(PresetStyles::MtechNoirGlass)
        };
        let decoded = decode_theme(&encode_theme(&theme).unwrap()).unwrap();
        assert_eq!(decoded.density, Density::Comfortable);
        assert_eq!(decoded.text_scale, TextScale::Large);
        assert_eq!(decoded.glass, theme.glass);

        let from_file = super::super::theme_from_json(&super::super::theme_to_json(&theme).unwrap()).unwrap();
        assert_eq!(from_file.density, Density::Comfortable);
        assert_eq!(from_file.text_scale, TextScale::Large);
    }

    #[test]
    fn a_record_without_density_or_text_scale_loads_the_defaults() {
        let json = serde_json::json!({ "style": style_for_preset(PresetStyles::MtechNoir) });
        let theme = super::super::theme_from_json(json.to_string().as_bytes()).unwrap();
        assert_eq!(theme.density, Density::Regular);
        assert_eq!(theme.text_scale, TextScale::Normal);
        assert_eq!(preset_matching_style(&theme.style), Some(PresetStyles::MtechNoir));
    }

    // A Compact save restores as its own preset, at Compact.
    #[test]
    fn a_compact_save_restores_its_preset_and_density() {
        let ctx = Context::default();
        apply_preset(&ctx, PresetStyles::ObsidianGlass);
        set_density(&ctx, Density::Compact);
        set_text_scale(&ctx, TextScale::XLarge);
        let bytes = encode_theme(&SavedTheme::current(&ctx)).unwrap();

        let decoded = decode_theme(&bytes).unwrap();
        assert_eq!(preset_matching_style(&decoded.style), Some(PresetStyles::ObsidianGlass));

        let fresh = Context::default();
        bootstrap_startup_theme(&fresh);
        apply_user_color_scheme(&fresh, &bytes);
        let compact = Metrics::for_density(Density::Compact);
        assert_eq!(metrics::density(&fresh), Density::Compact);
        assert_eq!(fonts::text_scale(&fresh), TextScale::XLarge);
        assert_eq!(Metrics::get(&fresh), compact);
        assert_eq!(fresh.global_style().spacing.interact_size.y, compact.control_h);
        assert_eq!(glass_backdrop::params(&fresh), glass_params_for_preset(PresetStyles::ObsidianGlass));
    }

    #[test]
    fn switching_density_reinstalls_the_current_style() {
        let ctx = Context::default();
        apply_preset(&ctx, PresetStyles::MtechGlassFull);
        let fill = ctx.global_style().visuals.widgets.inactive.weak_bg_fill;

        set_density(&ctx, Density::Comfortable);
        let style = ctx.global_style();
        assert_eq!(style.spacing.interact_size.y, Metrics::for_density(Density::Comfortable).control_h);
        assert_eq!(style.visuals.widgets.inactive.weak_bg_fill, fill);
    }

    #[test]
    fn bootstrap_returns_to_the_default_density_and_text_size() {
        let ctx = Context::default();
        set_density(&ctx, Density::Compact);
        set_text_scale(&ctx, TextScale::Small);
        bootstrap_startup_theme(&ctx);
        assert_eq!(metrics::density(&ctx), Density::Regular);
        assert_eq!(fonts::text_scale(&ctx), TextScale::Normal);
    }

    // An egui-default record still reads as "no theme saved".
    #[test]
    fn a_blank_default_record_is_still_recognised() {
        let legacy = Style { visuals: Visuals::dark(), ..Style::default() };
        assert!(is_blank_default_style(&legacy));
        assert!(is_blank_default_style(&normalized(&legacy)));
        assert!(!is_blank_default_style(&style_for_preset(PresetStyles::ShippedClassic)));
    }

    // Builds every preset style (the startup restore path); none may panic on decode,
    // and a saved preset must be recovered through the encode/decode round trip.
    #[test]
    fn preset_matching_style_round_trips_all_presets() {
        let bytes = encode_theme(&saved(PresetStyles::MtechGlassFull)).unwrap();
        let decoded = decode_theme(&bytes).unwrap();
        assert_eq!(
            preset_matching_style(&decoded.style),
            Some(PresetStyles::MtechGlassFull),
        );
    }

    // The glass preset and the flat scheme it derives from must not collapse onto each other,
    // or a restart would silently swap one for the other (and its glass material with it).
    #[test]
    fn the_noir_presets_survive_a_save_and_stay_distinct() {
        for preset in [PresetStyles::MtechNoir, PresetStyles::MtechNoirGlass] {
            let bytes = encode_theme(&saved(preset)).unwrap();
            let decoded = decode_theme(&bytes).unwrap();
            assert_eq!(preset_matching_style(&decoded.style), Some(preset));
        }
        assert!(!styles_visually_equal(
            &style_for_preset(PresetStyles::MtechNoir),
            &style_for_preset(PresetStyles::MtechNoirGlass),
        ));
    }

    // The point of the account payload: a material hand-tuned on one machine must come back
    // exactly, not be re-derived from whichever preset the style happens to match.
    #[test]
    fn a_hand_tuned_material_survives_the_account_round_trip() {
        let ctx = Context::default();
        let tuned = GlassParams {
            enabled: true,
            blur_radius: 41.0,
            tint: Color32::from_rgba_unmultiplied(9, 30, 44, 96),
            corner_radius: 13.0,
            presence: 0.72,
        };

        // A Custom (glassified) style with a tuned blur, saved the way the Save button does.
        apply_preset(&ctx, PresetStyles::MtechGlassFull);
        glass_backdrop::set_params(&ctx, tuned);
        let bytes = encode_theme(&SavedTheme::current(&ctx)).unwrap();

        // A different machine, starting from the shipped theme.
        let fresh = Context::default();
        bootstrap_startup_theme(&fresh);
        assert_eq!(glass_backdrop::params(&fresh), GlassParams::OFF);

        apply_user_color_scheme(&fresh, &bytes);
        assert_eq!(glass_backdrop::params(&fresh), tuned);
    }

    const NEON_PRESETS: [PresetStyles; 4] = [
        PresetStyles::NebulaGlass,
        PresetStyles::AuroraGlass,
        PresetStyles::SupernovaGlass,
        PresetStyles::EventHorizonGlass,
    ];

    const SOFT_PRESETS: [PresetStyles; 4] = [
        PresetStyles::ObsidianGlass,
        PresetStyles::VelvetGlass,
        PresetStyles::TwilightGlass,
        PresetStyles::QuartzGlass,
    ];

    // Every generated glass preset, across both families. They share helpers and geometry, so a
    // collision between families is as likely as one within a family.
    fn generated_glass_presets() -> [PresetStyles; 8] {
        let mut all = [PresetStyles::Custom; 8];
        all[..4].copy_from_slice(&NEON_PRESETS);
        all[4..].copy_from_slice(&SOFT_PRESETS);
        all
    }

    // Each soft theme has to survive the account round trip as itself, and none may collapse onto
    // a neon theme — eight presets out of two generators is where a style collision would show.
    #[test]
    fn every_soft_preset_round_trips_and_stays_distinct() {
        for preset in SOFT_PRESETS {
            let bytes = encode_theme(&saved(preset)).unwrap();
            let decoded = decode_theme(&bytes).unwrap();
            assert_eq!(
                preset_matching_style(&decoded.style),
                Some(preset),
                "{} did not round trip",
                preset.as_str(),
            );
            assert_eq!(decoded.glass, Some(glass_params_for_preset(preset)));
        }

        let all = generated_glass_presets();
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert!(
                    !styles_visually_equal(&style_for_preset(*a), &style_for_preset(*b)),
                    "{} and {} produce the same style",
                    a.as_str(),
                    b.as_str(),
                );
            }
        }
    }

    // Every soft preset must arrive with blur on, and with a resting control frame the operator
    // can actually see — the thing that separates this family from the neon one.
    #[test]
    fn every_soft_preset_applies_glass_and_a_visible_control_frame() {
        let ctx = Context::default();
        for preset in SOFT_PRESETS {
            apply_preset(&ctx, preset);
            let params = glass_backdrop::params(&ctx);
            assert!(params.is_visible(), "{} has no glass", preset.as_str());

            let style = ctx.global_style();
            assert!(style.visuals.window_fill.a() < 255, "{} is opaque", preset.as_str());
            let frame = style.visuals.widgets.inactive.bg_stroke;
            assert!(frame.width >= 1.0 && frame.color.a() >= 150, "{} has no resting frame", preset.as_str());
        }
    }

    // Each neon theme has to survive the account round trip as itself: four themes built from one
    // generator are the most likely presets to collide and restore as each other.
    #[test]
    fn every_neon_preset_round_trips_and_stays_distinct() {
        for preset in NEON_PRESETS {
            let bytes = encode_theme(&saved(preset)).unwrap();
            let decoded = decode_theme(&bytes).unwrap();
            assert_eq!(
                preset_matching_style(&decoded.style),
                Some(preset),
                "{} did not round trip",
                preset.as_str(),
            );
            assert_eq!(decoded.glass, Some(glass_params_for_preset(preset)));
        }

        for (i, a) in NEON_PRESETS.iter().enumerate() {
            for b in &NEON_PRESETS[i + 1..] {
                assert!(
                    !styles_visually_equal(&style_for_preset(*a), &style_for_preset(*b)),
                    "{} and {} produce the same style",
                    a.as_str(),
                    b.as_str(),
                );
            }
        }
    }

    // Every neon preset must arrive with blur on; a glass theme with GlassParams::OFF is just flat
    // translucency with nothing behind it.
    #[test]
    fn every_neon_preset_applies_a_live_glass_material() {
        let ctx = Context::default();
        for preset in NEON_PRESETS {
            apply_preset(&ctx, preset);
            let params = glass_backdrop::params(&ctx);
            assert!(params.is_visible(), "{} has no glass", preset.as_str());
            assert!(params.blur_radius > 0.0);
            // Floating surfaces must stay sheer enough for that blur to read through them.
            assert!(ctx.global_style().visuals.window_fill.a() < 255);
        }
    }

    // Records written before the glass material carry a bare egui Style, and an operator may
    // still import one by hand. Those must load and take their preset's material, not fail.
    #[test]
    fn a_legacy_bare_style_record_still_loads() {
        let style = style_for_preset(PresetStyles::MtechNoirGlass);
        let legacy = serde_json::to_vec(&style).unwrap();

        let theme = super::super::theme_from_json(&legacy).unwrap();
        assert!(theme.glass.is_none());
        assert_eq!(preset_matching_style(&theme.style), Some(PresetStyles::MtechNoirGlass));

        // Applying it recovers the material from the matched preset.
        let ctx = Context::default();
        apply_saved_theme(&ctx, theme);
        assert_eq!(glass_backdrop::params(&ctx), mtech_noir_glass_params());
    }

    // Applying a preset carries its glass material onto the context, and leaving a glass preset
    // turns real frosting back off rather than leaving the last theme's blur running.
    #[test]
    fn applying_a_preset_carries_its_glass_material() {
        let ctx = Context::default();

        apply_preset(&ctx, PresetStyles::MtechNoirGlass);
        let params = glass_backdrop::params(&ctx);
        assert_eq!(params, mtech_noir_glass_params());
        assert!(params.is_visible());

        apply_preset(&ctx, PresetStyles::ShippedClassic);
        assert_eq!(glass_backdrop::params(&ctx), GlassParams::OFF);
    }

    // A saved glass theme restored from the account blob must come back with its blur, not just
    // its colors.
    #[test]
    fn a_restored_glass_style_recovers_its_material() {
        let ctx = Context::default();
        let bytes = encode_theme(&saved(PresetStyles::MtechNoirGlass)).unwrap();

        apply_user_color_scheme(&ctx, &bytes);

        assert_eq!(glass_backdrop::params(&ctx), mtech_noir_glass_params());
    }

    #[test]
    fn mtech_studio_publishes_its_exact_tokens_on_every_install_path() {
        let ctx = Context::default();
        apply_preset(&ctx, PresetStyles::MtechStudio);
        assert_eq!(Tokens::get(&ctx), mtech_studio::TOKENS);
        assert_eq!(glass_backdrop::params(&ctx), mtech_studio::studio_glass_params());

        set_density(&ctx, Density::Compact);
        assert_eq!(Tokens::get(&ctx), mtech_studio::TOKENS);

        let bytes = encode_theme(&SavedTheme::current(&ctx)).unwrap();
        let decoded = decode_theme(&bytes).unwrap();
        assert_eq!(preset_matching_style(&decoded.style), Some(PresetStyles::MtechStudio));
        assert_eq!(decoded.density, Density::Compact);

        let fresh = Context::default();
        bootstrap_startup_theme(&fresh);
        assert_ne!(Tokens::get(&fresh), mtech_studio::TOKENS);
        apply_user_color_scheme(&fresh, &bytes);
        assert_eq!(Tokens::get(&fresh), mtech_studio::TOKENS);
        assert_eq!(glass_backdrop::params(&fresh), mtech_studio::studio_glass_params());
        assert_eq!(fresh.global_style().spacing.interact_size.y, Metrics::for_density(Density::Compact).control_h);
    }

    #[test]
    fn leaving_mtech_studio_publishes_derived_tokens() {
        let ctx = Context::default();
        apply_preset(&ctx, PresetStyles::MtechStudio);
        apply_preset(&ctx, PresetStyles::ShippedClassic);
        let (success, accent2) = semantic_colors_for_preset(PresetStyles::ShippedClassic);
        assert_eq!(Tokens::get(&ctx), Tokens::derive(&ctx.global_style(), success, accent2));
    }

    #[test]
    fn every_preset_publishes_opaque_tokens_with_its_semantic_colors() {
        let ctx = Context::default();
        for preset in all_presets() {
            apply_preset(&ctx, preset);
            let tokens = Tokens::get(&ctx);
            let name = preset.as_str();
            for (slot, c) in tokens.colors() {
                assert_eq!(c.a(), 255, "{name}: {slot} is translucent");
            }
            let (success, accent2) = semantic_colors_for_preset(preset);
            assert_eq!(tokens.success, super::super::tokens::composite(success, tokens.base), "{name}: success");
            assert_eq!(tokens.secondary, super::super::tokens::composite(accent2, tokens.base), "{name}: secondary");
        }
    }

    #[test]
    fn every_preset_name_round_trips() {
        for preset in all_presets() {
            assert_eq!(PresetStyles::from_str(preset.as_str()), preset, "{}", preset.as_str());
        }
    }

    #[test]
    fn saved_themes_never_name_the_weight_families() {
        let ctx = Context::default();
        for preset in all_presets() {
            apply_preset(&ctx, preset);
            let current = SavedTheme::current(&ctx);
            let from_file = super::super::theme_to_json(&saved(preset)).unwrap();
            let file_style = super::super::theme_from_json(&from_file).unwrap().style;
            let record = decode_theme(&encode_theme(&current).unwrap()).unwrap().style;
            for style in [&current.style, &file_style, &record] {
                for (text_style, font) in &style.text_styles {
                    assert!(
                        !matches!(font.family, eframe::egui::FontFamily::Name(_)),
                        "{}: {text_style:?} saved as {:?}",
                        preset.as_str(),
                        font.family,
                    );
                }
            }
        }
    }

    #[test]
    fn resetting_memory_keeps_the_theme_density_and_text_size() {
        let ctx = Context::default();
        apply_preset(&ctx, PresetStyles::MtechStudio);
        set_density(&ctx, Density::Compact);
        set_text_scale(&ctx, TextScale::Large);

        reset_memory(&ctx);

        assert_eq!(installed_preset(&ctx), PresetStyles::MtechStudio);
        assert_eq!(Metrics::get(&ctx), Metrics::for_density(Density::Compact));
        assert_eq!(fonts::text_scale(&ctx), TextScale::Large);
        assert_eq!(Tokens::get(&ctx), mtech_studio::TOKENS);
        assert_eq!(glass_backdrop::params(&ctx), mtech_studio::studio_glass_params());
        assert_eq!(
            ctx.global_style().spacing.interact_size.y,
            Metrics::for_density(Density::Compact).control_h,
        );
    }

    #[test]
    fn an_origin_era_mtech_glass_record_loads_as_the_current_preset() {
        let json = serde_json::json!({
            "style": legacy_glass_style(),
            "glass": GlassParams::OFF,
        });
        let bytes = zstd::encode_all(json.to_string().as_bytes(), 3).unwrap();
        let decoded = decode_theme(&bytes).unwrap();
        assert_eq!(preset_matching_style(&decoded.style), Some(PresetStyles::MtechGlassFull));

        let ctx = Context::default();
        bootstrap_startup_theme(&ctx);
        apply_user_color_scheme(&ctx, &bytes);
        assert!(styles_visually_equal(
            &normalized(&ctx.global_style()),
            &style_for_preset(PresetStyles::MtechGlassFull),
        ));
        assert_eq!(
            glass_backdrop::params(&ctx),
            glass_params_for_preset(PresetStyles::MtechGlassFull),
        );
        let (success, accent2) = semantic_colors_for_preset(PresetStyles::MtechGlassFull);
        assert_eq!(super::super::theme::success_ctx(&ctx), success);
        assert_eq!(super::super::theme::accent_secondary_ctx(&ctx), accent2);

        assert_eq!(installed_preset(&ctx), PresetStyles::MtechGlassFull);
        let mut cfg = ThemeConfig::default();
        sync_editor_config(&mut cfg, &ctx);
        fn weak_fill(style: &Style) -> Color32 {
            style.visuals.widgets.noninteractive.weak_bg_fill
        }
        assert_eq!(cfg.preset_style, PresetStyles::MtechGlassFull);
        assert_eq!(cfg.widget_weak_bg_fill, weak_fill(&ctx.global_style()));
        assert_ne!(cfg.widget_weak_bg_fill, weak_fill(&legacy_glass_style()));
    }

    #[test]
    fn mtech_glass_full_carries_a_live_glass_material() {
        let ctx = Context::default();
        apply_preset(&ctx, PresetStyles::MtechGlassFull);
        let params = glass_backdrop::params(&ctx);
        assert_eq!(params, glass_params_for_style(&glass_style()));
        assert!(params.is_visible());
    }

    /// The shipped style with a non-preset hyperlink color.
    fn custom_style() -> Style {
        let mut style = style_for_preset(PresetStyles::ShippedClassic);
        style.visuals.hyperlink_color = Color32::from_rgb(1, 2, 3);
        style
    }

    fn custom_record() -> Vec<u8> {
        encode_theme(&SavedTheme {
            style: custom_style(),
            ..saved(PresetStyles::ShippedClassic)
        })
        .unwrap()
    }

    #[test]
    fn custom_saved_theme_names_custom_after_login() {
        let ctx = Context::default();
        bootstrap_startup_theme(&ctx);
        assert_eq!(installed_preset(&ctx), PresetStyles::ShippedClassic);

        apply_user_color_scheme(&ctx, &custom_record());
        assert_eq!(installed_preset(&ctx), PresetStyles::Custom);

        let mut cfg = ThemeConfig::default();
        sync_editor_config(&mut cfg, &ctx);
        assert_eq!(cfg.preset_style, PresetStyles::Custom);
        assert_eq!(cfg.background_color, ctx.global_style().visuals.window_fill);
        assert_eq!(cfg.link_color, Color32::from_rgb(1, 2, 3));
    }

    #[test]
    fn every_saved_preset_names_itself_after_login() {
        for preset in MATCHABLE_PRESETS {
            let ctx = Context::default();
            bootstrap_startup_theme(&ctx);
            apply_user_color_scheme(&ctx, &encode_theme(&saved(preset)).unwrap());
            // A Default Egui record is the blank default, which loads as Shipped Classic.
            let expected = match preset {
                PresetStyles::DefaultEgui => PresetStyles::ShippedClassic,
                other => other,
            };
            assert_eq!(installed_preset(&ctx), expected, "{}", preset.as_str());
        }
    }

    #[test]
    fn fallback_records_name_shipped_classic() {
        let blank = SavedTheme {
            style: Style {
                visuals: Visuals::dark(),
                ..Style::default()
            },
            ..saved(PresetStyles::ShippedClassic)
        };
        let records = [
            Vec::new(),
            b"not a theme".to_vec(),
            encode_theme(&blank).unwrap(),
        ];
        for bytes in records {
            let ctx = Context::default();
            apply_preset(&ctx, PresetStyles::MtechNoir);
            apply_user_color_scheme(&ctx, &bytes);
            assert_eq!(installed_preset(&ctx), PresetStyles::ShippedClassic);
        }
    }

    #[test]
    fn imported_files_name_what_they_install() {
        let ctx = Context::default();
        let preset_file =
            serde_json::to_vec(&style_for_preset(PresetStyles::MtechNoirGlass)).unwrap();
        apply_saved_theme(&ctx, super::super::theme_from_json(&preset_file).unwrap());
        assert_eq!(installed_preset(&ctx), PresetStyles::MtechNoirGlass);

        let custom_file = serde_json::to_vec(&custom_style()).unwrap();
        apply_saved_theme(&ctx, super::super::theme_from_json(&custom_file).unwrap());
        assert_eq!(installed_preset(&ctx), PresetStyles::Custom);
        assert_eq!(
            ctx.global_style().visuals.hyperlink_color,
            Color32::from_rgb(1, 2, 3)
        );
    }

    #[test]
    fn density_and_text_scale_keep_the_named_preset() {
        let ctx = Context::default();
        apply_preset(&ctx, PresetStyles::ObsidianGlass);
        set_density(&ctx, Density::Compact);
        set_text_scale(&ctx, TextScale::Large);
        assert_eq!(installed_preset(&ctx), PresetStyles::ObsidianGlass);

        apply_user_color_scheme(&ctx, &custom_record());
        set_density(&ctx, Density::Comfortable);
        set_text_scale(&ctx, TextScale::Small);
        assert_eq!(installed_preset(&ctx), PresetStyles::Custom);
    }

    #[test]
    fn reset_memory_keeps_the_named_preset() {
        let ctx = Context::default();
        apply_preset(&ctx, PresetStyles::ObsidianGlass);
        set_density(&ctx, Density::Compact);
        reset_memory(&ctx);
        assert_eq!(installed_preset(&ctx), PresetStyles::ObsidianGlass);

        apply_user_color_scheme(&ctx, &custom_record());
        reset_memory(&ctx);
        assert_eq!(installed_preset(&ctx), PresetStyles::Custom);

        let legacy =
            serde_json::json!({ "style": legacy_glass_style(), "glass": GlassParams::OFF });
        let bytes = zstd::encode_all(legacy.to_string().as_bytes(), 3).unwrap();
        apply_user_color_scheme(&ctx, &bytes);
        reset_memory(&ctx);
        assert_eq!(installed_preset(&ctx), PresetStyles::MtechGlassFull);
    }

    #[test]
    fn picking_a_preset_records_it_and_custom_is_a_no_op() {
        let ctx = Context::default();
        apply_user_color_scheme(&ctx, &custom_record());
        let mut cfg = ThemeConfig::default();
        sync_editor_config(&mut cfg, &ctx);
        assert_eq!(cfg.preset_style, PresetStyles::Custom);

        cfg.pick_preset(&ctx, PresetStyles::ShippedClassic);
        assert_eq!(installed_preset(&ctx), PresetStyles::ShippedClassic);
        assert_eq!(cfg.preset_style, PresetStyles::ShippedClassic);
        assert!(styles_visually_equal(
            &normalized(&ctx.global_style()),
            &style_for_preset(PresetStyles::ShippedClassic),
        ));

        let before = ctx.global_style();
        cfg.pick_preset(&ctx, PresetStyles::Custom);
        assert_eq!(installed_preset(&ctx), PresetStyles::ShippedClassic);
        assert_eq!(cfg.preset_style, PresetStyles::ShippedClassic);
        assert!(styles_visually_equal(&ctx.global_style(), &before));
    }

    #[test]
    fn glassify_records_custom() {
        let ctx = Context::default();
        apply_preset(&ctx, PresetStyles::MtechNoir);
        let mut cfg = ThemeConfig::default();
        cfg.glassify_installed(&ctx);
        assert_eq!(installed_preset(&ctx), PresetStyles::Custom);
        assert_eq!(cfg.preset_style, PresetStyles::Custom);
        assert_eq!(cfg.background_color, ctx.global_style().visuals.window_fill);
    }

    /// Runs `edit` on the installed visuals through the colour editor's install path.
    fn edit_colors(ctx: &Context, cfg: &mut ThemeConfig, edit: impl FnOnce(&mut Visuals)) {
        let mut style = (*ctx.global_style()).clone();
        let before = style.visuals.clone();
        edit(&mut style.visuals);
        cfg.install_edit(ctx, style, &before);
    }

    #[test]
    fn a_colour_edit_records_custom_unless_the_overlay_undoes_it() {
        let ctx = Context::default();
        let mut cfg = ThemeConfig::default();

        apply_preset(&ctx, PresetStyles::ObsidianGlass);
        edit_colors(&ctx, &mut cfg, |v| {
            v.hyperlink_color = Color32::from_rgb(1, 2, 3)
        });
        assert_eq!(installed_preset(&ctx), PresetStyles::Custom);
        assert_eq!(cfg.preset_style, PresetStyles::Custom);
        assert_eq!(cfg.link_color, Color32::from_rgb(1, 2, 3));

        apply_preset(&ctx, PresetStyles::ObsidianGlass);
        edit_colors(&ctx, &mut cfg, |v| v.widgets.hovered.expansion = 3.0);
        assert_eq!(installed_preset(&ctx), PresetStyles::ObsidianGlass);
    }

    /// Text of every galley the theme editor paints in one frame.
    fn editor_texts(ctx: &Context, editor: &mut ThemeConfig) -> Vec<String> {
        use eframe::egui::{CentralPanel, Pos2, RawInput, Rect, Shape, vec2};

        fn collect(shape: &Shape, texts: &mut Vec<String>) {
            match shape {
                Shape::Vec(shapes) => shapes.iter().for_each(|s| collect(s, texts)),
                Shape::Text(text) => texts.push(text.galley.text().to_owned()),
                _ => {}
            }
        }

        let (tx, _rx) = crossbeam::channel::unbounded();
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(860.0, 900.0))),
            ..Default::default()
        };
        let mut full = ctx.run_ui(input, |ui| {
            CentralPanel::default().show(ui, |ui| {
                let ctx = ui.ctx().clone();
                let _ = editor.edit_ui(ui, &ctx, tx.clone());
            });
        });
        full.textures_delta.clear();
        let mut texts = Vec::new();
        for clipped in &full.shapes {
            collect(&clipped.shape, &mut texts);
        }
        texts
    }

    #[test]
    fn editor_frame_shows_the_installed_preset() {
        use crate::ui_tools::headless_render::Headless;

        let h = Headless::new(eframe::egui::vec2(860.0, 900.0), 1.0);
        let shows =
            |texts: &[String], preset: PresetStyles| texts.iter().any(|t| t == preset.as_str());
        let mut editor = ThemeConfig::default();

        apply_user_color_scheme(&h.ctx, &custom_record());
        let texts = editor_texts(&h.ctx, &mut editor);
        assert!(shows(&texts, PresetStyles::Custom), "{texts:?}");
        assert!(!shows(&texts, PresetStyles::ShippedClassic), "{texts:?}");

        h.theme(PresetStyles::ObsidianGlass, Density::Regular);
        let texts = editor_texts(&h.ctx, &mut editor);
        assert!(shows(&texts, PresetStyles::ObsidianGlass), "{texts:?}");
        assert!(!shows(&texts, PresetStyles::ShippedClassic), "{texts:?}");
    }
}

// use egui_colors::{Colorix, tokens::ThemeColor};
// // use egui_fontcfg::{CustomFontPaths, FontCfgUi, FontDefsUiMsg};

// fn style_ui(
//     app: &mut App,
//     ui: &mut egui::Ui,
//     opt_colorix: &mut Option<Colorix>,
//     msg_dia: &mut MessageDialog,
// ) {
//     ui.group(|ui| {
//         let style = &mut app.cfg.style;
//         ui.heading("Font sizes");
//         let mut any_changed = false;
//         ui.horizontal(|ui| {
//             ui.label("heading");
//             any_changed |= ui
//                 .add(egui::DragValue::new(&mut style.font_sizes.heading).range(3..=100))
//                 .changed();
//         });
//         ui.horizontal(|ui| {
//             ui.label("body");
//             any_changed |= ui
//                 .add(egui::DragValue::new(&mut style.font_sizes.body).range(3..=100))
//                 .changed();
//         });
//         ui.horizontal(|ui| {
//             ui.label("monospace");
//             any_changed |= ui
//                 .add(egui::DragValue::new(&mut style.font_sizes.monospace).range(3..=100))
//                 .changed();
//         });
//         ui.horizontal(|ui| {
//             ui.label("button");
//             any_changed |= ui
//                 .add(egui::DragValue::new(&mut style.font_sizes.button).range(3..=100))
//                 .changed();
//         });
//         ui.horizontal(|ui| {
//             ui.label("small");
//             any_changed |= ui
//                 .add(egui::DragValue::new(&mut style.font_sizes.small).range(3..=100))
//                 .changed();
//         });
//         if ui.button("Reset default").clicked() {
//             *style = config::Style::default();
//             any_changed = true;
//         }
//         if any_changed {
//             crate::gui::set_font_sizes_ctx(ui.ctx(), style);
//         }
//     });
//     ui.group(|ui| {
//         let colorix = match opt_colorix {
//             Some(colorix) => colorix,
//             None => {
//                 if ui.button("Activate custom colors").clicked() {
//                     opt_colorix.insert(Colorix::global(ui.ctx(), egui_colors::utils::EGUI_THEME))
//                 } else {
//                     return;
//                 }
//             }
//         };
//         let mut clear = false;
//         ui.horizontal(|ui| {
//             colorix.themes_dropdown(ui, None, false);
//             ui.group(|ui| {
//                 ui.label("light dark toggle");
//                 colorix.light_dark_toggle_button(ui, 30.0);
//             });
//             if ui.button("Random theme").clicked() {
//                 let mut rng = rand::rng();
//                 *colorix = Colorix::global(
//                     ui.ctx(),
//                     std::array::from_fn(|_| ThemeColor::Custom(rng.random::<[u8; 3]>())),
//                 );
//             }
//         });
//         ui.separator();
//         colorix.ui_combo_12(ui, true);
//         if let Some(dirs) = config::project_dirs() {
//             ui.separator();
//             ui.horizontal(|ui| {
//                 if ui.button("Save").clicked() {
//                     let data: [[u8; 3]; 12] = colorix.theme().map(|theme| theme.rgb());
//                     if let Err(e) = std::fs::write(dirs.color_theme_path(), data.as_flattened()) {
//                         msg_dia.open(Icon::Error, "Failed to save theme", e.to_string());
//                     }
//                 };
//                 if ui.button("Remove custom colors").clicked() {
//                     if let Err(e) = std::fs::remove_file(dirs.color_theme_path()) {
//                         msg_dia.open(Icon::Error, "Failed to delete theme file", e.to_string());
//                     }
//                     clear = true;
//                 }
//             });
//         }
//         if clear {
//             ui.ctx().set_visuals(egui::Visuals::dark());
//             *opt_colorix = None;
//         }
//     });
// }