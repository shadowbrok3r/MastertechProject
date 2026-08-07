use eframe::egui::{scroll_area::ScrollBarVisibility, style::{HandleShape, NumericColorSpace, Selection, TextCursorStyle, WidgetVisuals, Widgets}, Align, Button, Color32, ComboBox, Context, CursorIcon, DragValue, FontFamily, FontId, Layout, ScrollArea, Shadow, Slider, Stroke, Style, Ui, Vec2, Visuals, Widget};
use crate::{ui_tools::{encode_theme, glass_backdrop::{self, GlassParams}, rerun_mtech::{RerunMtech, RerunMtechOled}, theme_chrome::{default_egui_chrome, legacy_classic_chrome, mtech_noir_chrome, mtech_noir_glass_chrome, mtech_noir_glass_params, shipped_chrome}, tokyo_dark::{TokyoNight, TokyoNightStorm}, SavedTheme}, PlatformSpawner, Spawner};
use serde::{Deserialize, Serialize};
use crossbeam::channel::Sender;
use derivative::Derivative;
use database::schema::User;
use std::sync::Arc;

use super::carl_dark::{paint_aesthetix_colors, Aesthetix, CarlDark};
use super::mtech_glass::{glass_params_for_style, glassify, MtechGlass};
use super::neon_glass::{self, NeonPalette};
use super::soft_glass::{self, SoftPalette};
use super::decode_theme;

/// Applies shipped [`crate::STYLE`] before login or when no saved scheme exists.
pub fn bootstrap_startup_theme(ctx: &Context) {
    apply_preset(ctx, PresetStyles::ShippedClassic);
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
    let preset = preset_matching_style(&theme.style).unwrap_or(PresetStyles::Custom);
    ctx.set_global_style(Arc::new(theme.style));
    apply_preset_semantics(ctx, preset);
    // A record that carries its own material wins; one written before glass existed (or an imported
    // bare egui Style) falls back to whatever its preset asks for.
    if let Some(glass) = theme.glass {
        glass_backdrop::set_params(ctx, glass);
    }
}

/// The parts of a preset that have no `egui::Style` slot: semantic colors and the glass material.
fn apply_preset_semantics(ctx: &Context, preset: PresetStyles) {
    let (success, accent2) = semantic_colors_for_preset(preset);
    crate::ui_tools::theme::set_success_color(ctx, success);
    crate::ui_tools::theme::set_accent_secondary(ctx, accent2);
    glass_backdrop::set_params(ctx, glass_params_for_preset(preset));
}

/// Syncs the theme editor's config (combo label + fields) to a style applied
/// out-of-band via the settings channel, so the editor stops showing a stale preset.
pub fn sync_editor_config(config: &mut ThemeConfig, style: &Style) {
    sync_config_from_style(config, style);
    config.preset_style = preset_matching_style(style).unwrap_or(PresetStyles::Custom);
}

/// Compares styles ignoring `number_formatter`, whose PartialEq is Arc pointer identity.
fn styles_visually_equal(a: &Style, b: &Style) -> bool {
    let mut a = a.clone();
    a.number_formatter = b.number_formatter.clone();
    a == *b
}

/// Recovers the preset a saved style was built from, so its semantic colors survive restart.
fn preset_matching_style(style: &Style) -> Option<PresetStyles> {
    const PRESETS: [PresetStyles; 25] = [
        PresetStyles::ShippedClassic,
        PresetStyles::LegacyClassic,
        PresetStyles::DefaultEgui,
        PresetStyles::MtechNoir,
        PresetStyles::MtechNoirGlass,
        PresetStyles::NebulaGlass,
        PresetStyles::AuroraGlass,
        PresetStyles::SupernovaGlass,
        PresetStyles::EventHorizonGlass,
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
    PRESETS.into_iter().find(|p| styles_visually_equal(&style_for_preset(*p), style))
}

fn is_blank_default_style(style: &Style) -> bool {
    if styles_visually_equal(style, &Style::default()) {
        return true;
    }
    let mut dark_default = Style::default();
    dark_default.visuals = Visuals::dark();
    styles_visually_equal(style, &dark_default)
}

#[derive(Serialize, Clone, Deserialize, Debug, Derivative)]
#[derivative(PartialEq)]
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
    pub font: FontFamily,
    pub font_size: f32,
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
            font: FontFamily::Proportional,
            font_size: 14.0,
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

pub fn style_for_preset(preset: PresetStyles) -> Style {
    match preset {
        PresetStyles::ShippedClassic => shipped_chrome(),
        PresetStyles::LegacyClassic => legacy_classic_chrome(),
        PresetStyles::DefaultEgui => default_egui_chrome(),
        PresetStyles::MtechNoir => mtech_noir_chrome(),
        PresetStyles::MtechNoirGlass => mtech_noir_glass_chrome(),
        PresetStyles::NebulaGlass
        | PresetStyles::AuroraGlass
        | PresetStyles::SupernovaGlass
        | PresetStyles::EventHorizonGlass => neon_glass::neon_style(neon_palette_for_preset(preset)),
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
        PresetStyles::MtechNoirGlass => mtech_noir_glass_params(),
        PresetStyles::NebulaGlass
        | PresetStyles::AuroraGlass
        | PresetStyles::SupernovaGlass
        | PresetStyles::EventHorizonGlass => {
            neon_glass::neon_glass_params(neon_palette_for_preset(preset))
        }
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
    ctx.set_global_style(Arc::new(style_for_preset(preset)));
    apply_preset_semantics(ctx, preset);
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
        ui.horizontal(|ui| {
            let available = glass_backdrop::is_available();
            let mut params = glass_backdrop::params(ctx);
            let before = params;

            ui.add_enabled_ui(available, |ui| {
                ui.checkbox(&mut params.enabled, "Backdrop blur")
                    .on_hover_text(
                        "Blur what is behind glass surfaces on the GPU. The tint is the film \
                         painted over the blur; its alpha mixes film against blurred backdrop. \
                         Click Save to store these dials on your account — they follow you to \
                         any machine you sign in on.",
                    );
            });

            if !available {
                ui.colored_label(self.warn_color, "GPU blur unavailable on this host")
                    .on_hover_text(
                        "The grab-pass backend did not come up — the app is not on the glow \
                         renderer, or the GL context is below OpenGL 3.3 / GLES 3.0. Glass \
                         surfaces fall back to flat tinted panes.",
                    );
                return;
            }

            ui.add_enabled_ui(params.enabled, |ui| {
                ui.add_space(6.);
                ui.add(
                    DragValue::new(&mut params.blur_radius)
                        .speed(0.5)
                        .range(0.0..=64.0)
                        .prefix("radius "),
                );
                ui.add_space(6.);
                ui.label("Tint");
                ui.color_edit_button_srgba(&mut params.tint);
                ui.add_space(6.);
                ui.add(
                    DragValue::new(&mut params.corner_radius)
                        .speed(0.2)
                        .range(0.0..=32.0)
                        .prefix("corners "),
                );
                ui.add_space(6.);
                ui.add(
                    Slider::new(&mut params.presence, 0.0..=1.0)
                        .text("presence")
                        .fixed_decimals(2),
                );
            });

            if params != before {
                glass_backdrop::set_params(ctx, params);
            }
        });
    }

    pub fn edit_ui(&mut self, ui: &mut Ui, ctx: &Context, tx: Sender<SavedTheme>) -> (bool, Arc<Style>) {
        let mut ret = (false, ctx.global_style());
        // Three button rows; size from the active style so tall presets don't clip the last one.
        let panel_h = {
            let style = ctx.global_style();
            let sp = &style.spacing;
            let row_h = sp.interact_size.y.max(25.0) + 2.0 * sp.button_padding.y;
            3.0 * row_h + 2.0 * sp.item_spacing.y + 8.0
        };
        eframe::egui::Panel::top("Theme Menu top bar")
        .exact_size(panel_h)
        .show(ui, |ui| {
            ui.horizontal(|ui|{
                let reset = Button::new("Reset to Default")
                    .min_size(Vec2::new(70., 25.))
                    .stroke(Stroke::new(1.0_f32, self.warn_color))
                    .ui(ui)
                    .on_hover_text("Preview the shipped default theme. Nothing is saved until you click Save.");

                // Preview only: the account's saved scheme stays untouched until Save.
                if reset.clicked() {
                    apply_preset(ctx, PresetStyles::ShippedClassic);
                    sync_config_from_style(self, &style_for_preset(PresetStyles::ShippedClassic));
                    self.preset_style = PresetStyles::ShippedClassic;
                }

                ui.add_space(10.);

                let current = self.preset_style;

                ComboBox::new("Style Preset", "")
                .selected_text(self.preset_style.as_str())
                .width(280.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.preset_style, PresetStyles::ShippedClassic, "Shipped Classic");
                    ui.selectable_value(&mut self.preset_style, PresetStyles::LegacyClassic, "Legacy Classic");
                    ui.selectable_value(&mut self.preset_style, PresetStyles::DefaultEgui, "Default Egui");
                    ui.selectable_value(&mut self.preset_style, PresetStyles::MtechNoir, "MTech Noir");
                    ui.selectable_value(&mut self.preset_style, PresetStyles::MtechNoirGlass, "MTech Noir · Glass");
                    ui.separator();
                    ui.label("Neon glass · AMOLED");
                    ui.selectable_value(&mut self.preset_style, PresetStyles::NebulaGlass, neon_glass::NEBULA.name);
                    ui.selectable_value(&mut self.preset_style, PresetStyles::AuroraGlass, neon_glass::AURORA.name);
                    ui.selectable_value(&mut self.preset_style, PresetStyles::SupernovaGlass, neon_glass::SUPERNOVA.name);
                    ui.selectable_value(&mut self.preset_style, PresetStyles::EventHorizonGlass, neon_glass::EVENT_HORIZON.name);
                    ui.separator();
                    ui.label("Soft glass · low chroma");
                    ui.selectable_value(&mut self.preset_style, PresetStyles::ObsidianGlass, soft_glass::OBSIDIAN.name);
                    ui.selectable_value(&mut self.preset_style, PresetStyles::VelvetGlass, soft_glass::VELVET.name);
                    ui.selectable_value(&mut self.preset_style, PresetStyles::TwilightGlass, soft_glass::TWILIGHT.name);
                    ui.selectable_value(&mut self.preset_style, PresetStyles::QuartzGlass, soft_glass::QUARTZ.name);
                    ui.separator();
                    ui.label("Colors only · legacy widgets");
                    ui.selectable_value(&mut self.preset_style, PresetStyles::CarlDarkColors, "Carl Dark · Colors");
                    ui.selectable_value(&mut self.preset_style, PresetStyles::TokyoNightStormColors, "TokyoNight Storm · Colors");
                    ui.selectable_value(&mut self.preset_style, PresetStyles::TokyoNightColors, "TokyoNight · Colors");
                    ui.selectable_value(&mut self.preset_style, PresetStyles::RerunMtechColors, "Rerun MTech · Colors");
                    ui.selectable_value(&mut self.preset_style, PresetStyles::RerunMtechOledColors, "Rerun MTech OLED · Colors");
                    ui.selectable_value(&mut self.preset_style, PresetStyles::MtechGlassColors, "MTech Glass · Colors");
                    ui.separator();
                    ui.label("Colors + widget chrome");
                    ui.selectable_value(&mut self.preset_style, PresetStyles::CarlDarkFull, "Carl Dark · Full");
                    ui.selectable_value(&mut self.preset_style, PresetStyles::TokyoNightStormFull, "TokyoNight Storm · Full");
                    ui.selectable_value(&mut self.preset_style, PresetStyles::TokyoNightFull, "TokyoNight · Full");
                    ui.selectable_value(&mut self.preset_style, PresetStyles::RerunMtechFull, "Rerun MTech · Full");
                    ui.selectable_value(&mut self.preset_style, PresetStyles::RerunMtechOledFull, "Rerun MTech OLED · Full");
                    ui.selectable_value(&mut self.preset_style, PresetStyles::MtechGlassFull, "MTech Glass · Full");
                    ui.separator();
                    ui.selectable_value(&mut self.preset_style, PresetStyles::Custom, "Custom");
                });


                if self.preset_style != current {
                    if self.preset_style == PresetStyles::Custom {
                        return;
                    }
                    let chosen = self.preset_style;
                    let style = style_for_preset(chosen);
                    sync_config_from_style(self, &style);
                    let s = Arc::new(style);
                    ctx.set_global_style(s.clone());
                    apply_preset_semantics(ctx, chosen);
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let save = Button::new("Save")
                        .min_size(Vec2::new(70., 25.))
                        .stroke(Stroke::new(1.0_f32, self.warn_color))
                        .ui(ui);
                    
                    if save.clicked() {
                        // Persist whatever is currently applied (a preset OR an uploaded scheme),
                        // not a preset rebuild — else an uploaded theme gets overwritten. The glass
                        // material rides along, so a tuned blur follows the account to any machine.
                        let color_settings = ctx.global_style();
                        let saved = SavedTheme::current(ctx);
                        PlatformSpawner::spawn(async move {
                            match User::update_color_scheme(
                                encode_theme(&saved).unwrap_or_default().into()
                            ).await {
                                Ok(_) => log::info!("Updated Color Settings"),
                                Err(e) => log::error!("Error updating color settings: {e:?}"),
                            }
                        });
                        ret = (true, color_settings);
                    }

                    ui.add_space(5.);

                    #[cfg(not(any(target_os = "ios", target_os = "android")))]
                    {
                        let save_local = Button::new("Save to file")
                            .min_size(Vec2::new(70., 25.))
                            .stroke(Stroke::new(1.0_f32, self.warn_color))
                            .ui(ui);

                        if save_local.clicked() {
                            let saved = SavedTheme::current(ctx);

                            PlatformSpawner::spawn(async move {
                                // Serialize the struct into JSON
                                if let Ok(json_data) = super::theme_to_json(&saved) {
                                    // Show the save file dialog
                                    if let Some(file) = rfd::AsyncFileDialog::new()
                                        .set_file_name("mastertech_color_scheme.json") // Default file name
                                        .save_file()
                                        .await
                                    {
                                        // Write the JSON data to the selected file
                                        if let Err(err) = file.write(&json_data).await {
                                            log::error!("Failed to save file: {:?}", err);
                                        }
                                    }
                                } else {
                                    log::error!("Error serializing settings to json");
                                }
                            });
                        }

                        ui.add_space(5.);

                        let upload = Button::new("Upload settings")
                            .min_size(Vec2::new(70., 25.))
                            .stroke(Stroke::new(1.0_f32, self.warn_color))
                            .ui(ui);

                        if upload.clicked() {
                            let tx = tx.clone();
                            PlatformSpawner::spawn(async move {
                                // Show the save file dialog
                                if let Some(file) = rfd::AsyncFileDialog::new()
                                    .add_filter("Json", &["json"])
                                    .set_file_name("mastertech_color_scheme.json") // Default file name
                                    .pick_file()
                                    .await
                                {
                                    // Accepts a scheme exported by this app and a bare egui Style
                                    // file; the latter arrives with no glass material.
                                    match super::theme_from_json(&file.read().await) {
                                        Ok(theme) => {
                                            // Persist (JSON) so it replaces any legacy record and
                                            // survives restart, then apply it for this session.
                                            match User::update_color_scheme(
                                                encode_theme(&theme).unwrap_or_default().into()
                                            ).await {
                                                Ok(_) => log::info!("Imported color scheme saved"),
                                                Err(e) => log::error!("Error saving imported scheme: {e:?}"),
                                            }
                                            let _ = tx.try_send(theme);
                                        }
                                        Err(e) => log::error!("Error converting bytes to Theme: {e:?}"),
                                    }
                                }

                            });
                        }
                    }
                    #[cfg(any(target_os = "ios", target_os = "android"))]
                    let _ = &tx;

                });
            });

            ui.horizontal(|ui| {
                let glassify_btn = Button::new("Glassify current theme")
                    .min_size(Vec2::new(70., 25.))
                    .stroke(Stroke::new(1.0_f32, self.link_color))
                    .ui(ui)
                    .on_hover_text(
                        "Re-style the applied theme as tinted glass using its own colors: \
                         translucent widget fills with outlines a step brighter than each fill. \
                         Backgrounds, rounding, and fonts stay as they are. Click Save to keep.",
                    );

                if glassify_btn.clicked() {
                    let glassified = glassify(&ctx.global_style());
                    sync_config_from_style(self, &glassified);
                    self.preset_style = PresetStyles::Custom;
                    // Tinted glass now has a real backdrop to sit on where the GPU path came up.
                    glass_backdrop::set_params(ctx, glass_params_for_style(&glassified));
                    ctx.set_global_style(Arc::new(glassified));
                }

                ui.add_space(10.);

                let restore = Button::new("Restore saved theme")
                    .min_size(Vec2::new(70., 25.))
                    .stroke(Stroke::new(1.0_f32, self.link_color))
                    .ui(ui)
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
            });

            self.backdrop_blur_row(ui, ctx);
        });

        ui.add_space(10.);

        ScrollArea::vertical()
        .scroll_bar_visibility(ScrollBarVisibility::AlwaysVisible)
        .max_width(800.)
        .show(ui, |ui|
        {
            glass_backdrop::preview(ui, glass_backdrop::params(ctx));
            ui.add_space(10.);
            ctx.settings_ui(ui);
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

        ret
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
            font: style.override_font_id.clone().map(|f| f.family).unwrap_or(FontFamily::Proportional),
            font_size: style.override_font_id.clone().map(|f| f.size).unwrap_or(12.0),
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

        // Font override
        style.override_font_id = Some(FontId { family: self.font.clone(), size: self.font_size });
        style
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PresetStyles {
    ShippedClassic,
    LegacyClassic,
    DefaultEgui,
    MtechNoir,
    MtechNoirGlass,
    NebulaGlass,
    AuroraGlass,
    SupernovaGlass,
    EventHorizonGlass,
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
            PresetStyles::LegacyClassic => "Legacy Classic",
            PresetStyles::DefaultEgui => "Default Egui",
            PresetStyles::MtechNoir => "MTech Noir",
            PresetStyles::MtechNoirGlass => "MTech Noir · Glass",
            PresetStyles::NebulaGlass => neon_glass::NEBULA.name,
            PresetStyles::AuroraGlass => neon_glass::AURORA.name,
            PresetStyles::SupernovaGlass => neon_glass::SUPERNOVA.name,
            PresetStyles::EventHorizonGlass => neon_glass::EVENT_HORIZON.name,
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
            "Legacy Classic" => Self::LegacyClassic,
            "Default Egui" => Self::DefaultEgui,
            "MTech Noir" => Self::MtechNoir,
            "MTech Noir · Glass" => Self::MtechNoirGlass,
            "Nebula Glass" => Self::NebulaGlass,
            "Aurora Glass" => Self::AuroraGlass,
            "Supernova Glass" => Self::SupernovaGlass,
            "Event Horizon Glass" => Self::EventHorizonGlass,
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
            // Font settings
            let mut font = FontId::default();
            font.size = config.font_size;
            // font.family = FontFamily::Proportional;
            font.family = config.font.clone();

            // Assign custom font
            custom_style.override_font_id = Some(font);

            // Adjust spacing and interactions
            custom_style.spacing.button_padding = Vec2::new(5.0, 3.0);
            custom_style.spacing.item_spacing = Vec2::new(2.0, 1.0);
            custom_style.spacing.combo_height = 200.0;
            custom_style.spacing.combo_width = 100.0;
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
        var!("font-size", format!("{:.2}px", self.font_size));
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
        }
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