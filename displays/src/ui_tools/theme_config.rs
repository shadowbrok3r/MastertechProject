use eframe::egui::{scroll_area::ScrollBarVisibility, style::{HandleShape, NumericColorSpace, Selection, TextCursorStyle, WidgetVisuals, Widgets}, Align, Button, Color32, ComboBox, Context, CursorIcon, FontFamily, FontId, Layout, ScrollArea, Shadow, Stroke, Style, Ui, Vec2, Visuals, Widget};
use crate::{ui_tools::{encode_style, rerun_mtech::RerunMtech, tokyo_dark::{TokyoNight, TokyoNightStorm}}, PlatformSpawner, Spawner};
use serde::{Deserialize, Serialize};
use crossbeam::channel::Sender;
use derivative::Derivative;
use database::schema::User;
use serde_json::to_vec;
use std::sync::Arc;

use super::carl_dark::{Aesthetix, CarlDark};

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
            preset_style: PresetStyles::RerunMtech,
        }
    }
}

/// Default desktop theme (Rerun MTech preset).
pub fn default_app_style() -> Arc<Style> {
    Arc::new(RerunMtech.custom_style())
}

/// Applies the Rerun MTech preset and semantic accent colors to a context.
pub fn apply_default_theme(ctx: &Context) {
    RerunMtech.apply_to_ctx(ctx);
}

impl ThemeConfig {
    pub fn edit_ui(&mut self, ui: &mut Ui, ctx: &Context, tx: Sender<Style>) -> (bool, Arc<Style>) {
        let mut ret = (false, ctx.global_style());
        eframe::egui::Panel::top("Theme Menu top bar")
        .exact_size(30.)
        .show_inside(ui, |ui| {
            ui.horizontal(|ui|{
                let reset = Button::new("Reset to Default")
                    .min_size(Vec2::new(70., 25.))
                    .stroke(Stroke::new(1.0_f32, self.warn_color))
                    .ui(ui);
                
                if reset.clicked() {
                    PlatformSpawner::spawn(async move {
                        let theme = Style::default();
                            match User::update_color_scheme(
                                encode_style(
                                    &theme.clone()
                                ).unwrap_or_default().into()
                            ).await {
                                Ok(_) => log::info!("Updated Color Settings"),
                                Err(e) => log::error!("Error updating color settings: {e:?}"),
                            }
                    });
                    ret = (true, Style::default().into());
                }

                ui.add_space(10.);

                let selection = &mut self.preset_style;
                let current = selection.clone();

                ComboBox::new("Style Preset", "")
                .selected_text(selection.as_str())
                .show_ui(ui, |ui| {
                    ui.selectable_value(selection, PresetStyles::CarlDark, "Carl Dark");
                    ui.selectable_value(selection, PresetStyles::TokyoNightStorm, "TokyoNight Storm");
                    ui.selectable_value(selection, PresetStyles::TokyoNight, "TokyoNight");
                    ui.selectable_value(selection, PresetStyles::RerunMtech, "Rerun MTech");
                    ui.selectable_value(selection, PresetStyles::Custom, "Custom");
                });


                if *selection != current {
                    log::info!("Fire once");
                    let style = match *selection {
                        PresetStyles::CarlDark => CarlDark.custom_style(),
                        PresetStyles::TokyoNightStorm => TokyoNightStorm.custom_style(),
                        PresetStyles::TokyoNight => TokyoNight.custom_style(),
                        PresetStyles::RerunMtech => RerunMtech.custom_style(),
                        PresetStyles::Custom => return,
                    };

                    // Non-widget fields
                    self.background_color = style.visuals.window_fill;
                    self.text_color = style.visuals.override_text_color.unwrap_or(Color32::WHITE); // Default to WHITE if None
                    self.faint_bg_color = style.visuals.faint_bg_color;
                    self.extreme_bg_color = style.visuals.extreme_bg_color;
                    self.code_bg_color = style.visuals.code_bg_color;
                    self.warn_color = style.visuals.warn_fg_color;
                    self.error_color = style.visuals.error_fg_color;
                    self.link_color = style.visuals.hyperlink_color;
                    self.window_stroke_color = style.visuals.window_stroke.color;
                    self.rounding = style.visuals.window_corner_radius; // Assuming Rounding::same for consistency

                    // Selection fields
                    self.selection_bg_fill = style.visuals.selection.bg_fill;
                    self.selection_stroke_color = style.visuals.selection.stroke.color;

                    // Widget fields (using noninteractive as the source for shared fields)
                    self.widget_bg_fill = style.visuals.widgets.noninteractive.bg_fill;
                    self.widget_weak_bg_fill = style.visuals.widgets.noninteractive.weak_bg_fill;
                    self.widget_bg_stroke_color = style.visuals.widgets.noninteractive.bg_stroke.color;
                    self.widget_fg_stroke_color = style.visuals.widgets.noninteractive.fg_stroke.color;

                    // Hovered widget fields
                    self.hovered_bg_fill = style.visuals.widgets.hovered.bg_fill;
                    self.hovered_weak_bg_fill = style.visuals.widgets.hovered.weak_bg_fill;
                    self.hovered_bg_stroke_color = style.visuals.widgets.hovered.bg_stroke.color;
                    self.hovered_fg_stroke_color = style.visuals.widgets.hovered.fg_stroke.color;

                    // Active widget fields
                    self.active_bg_fill = style.visuals.widgets.active.bg_fill;
                    self.active_weak_bg_fill = style.visuals.widgets.active.weak_bg_fill;
                    self.active_bg_stroke_color = style.visuals.widgets.active.bg_stroke.color;
                    self.active_fg_stroke_color = style.visuals.widgets.active.fg_stroke.color;

                    // Open widget fields
                    self.open_bg_fill = style.visuals.widgets.open.bg_fill;
                    self.open_weak_bg_fill = style.visuals.widgets.open.weak_bg_fill;
                    self.open_bg_stroke_color = style.visuals.widgets.open.bg_stroke.color;
                    self.open_fg_stroke_color = style.visuals.widgets.open.fg_stroke.color;
                    let s = Arc::new(style);
                    ctx.set_global_style((s).clone());
                    let (success_c, accent2) = match *selection {
                        PresetStyles::CarlDark => (CarlDark.fg_success_text_color_visuals(), CarlDark.secondary_accent_color_visuals()),
                        PresetStyles::TokyoNightStorm => (TokyoNightStorm.fg_success_text_color_visuals(), TokyoNightStorm.secondary_accent_color_visuals()),
                        PresetStyles::TokyoNight => (TokyoNight.fg_success_text_color_visuals(), TokyoNight.secondary_accent_color_visuals()),
                        PresetStyles::RerunMtech => (RerunMtech.fg_success_text_color_visuals(), RerunMtech.secondary_accent_color_visuals()),
                        PresetStyles::Custom => return,
                    };
                    crate::ui_tools::theme::set_success_color(ctx, success_c);
                    crate::ui_tools::theme::set_accent_secondary(ctx, accent2);
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let save = Button::new("Save")
                        .min_size(Vec2::new(70., 25.))
                        .stroke(Stroke::new(1.0_f32, self.warn_color))
                        .ui(ui);
                    
                    if save.clicked() {
                        let color_settings = ctx.global_style().clone();
                        PlatformSpawner::spawn(async move {
                            match User::update_color_scheme(
                                encode_style(
                                    &color_settings.clone()
                                ).unwrap_or_default().into()
                            ).await {
                                Ok(_) => log::info!("Updated Color Settings"),
                                Err(e) => log::error!("Error updating color settings: {e:?}"),
                            }
                        });
                        let style = match self.preset_style {
                            PresetStyles::CarlDark => CarlDark.custom_style(),
                            PresetStyles::TokyoNightStorm => TokyoNightStorm.custom_style(),
                            PresetStyles::TokyoNight => TokyoNight.custom_style(),
                            PresetStyles::RerunMtech => RerunMtech.custom_style(),
                            PresetStyles::Custom => return,
                        };

                        ret = (true, style.clone().into());
                    }

                    ui.add_space(5.);

                    let save_local = Button::new("Save to file")
                        .min_size(Vec2::new(70., 25.))
                        .stroke(Stroke::new(1.0_f32, self.warn_color))
                        .ui(ui);
                    
                    if save_local.clicked() {
                        let color_settings = ctx.global_style().clone();
                        
                        PlatformSpawner::spawn(async move {
                            // Serialize the struct into JSON
                            if let Ok(json_data) = to_vec(&color_settings) {
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
                                match serde_json::from_slice::<Style>(&file.read().await) {
                                    Ok(theme) => tx.try_send(theme).unwrap(),
                                    Err(e) => log::error!("Error converting bytes to Theme: {e:?}"),
                                }
                            }
                            
                        });
                    }
                    
                });
            });

        });

        ui.add_space(10.);

        ScrollArea::vertical()
        .scroll_bar_visibility(ScrollBarVisibility::AlwaysVisible)
        .max_width(800.)
        .show(ui, |ui| 
        {
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

#[derive(Default, Clone, Debug, PartialEq)]
pub enum PresetStyles {
    CarlDark,
    TokyoNightStorm,
    TokyoNight,
    RerunMtech,
    #[default]
    Custom
}

impl PresetStyles {
    pub fn as_str(&self) -> &str {
        match self {
            PresetStyles::CarlDark => "Carl Dark",
            PresetStyles::TokyoNightStorm => "TokyoNight Storm",
            PresetStyles::TokyoNight => "TokyoNight",
            PresetStyles::RerunMtech => "Rerun MTech",
            PresetStyles::Custom => "Custom",
        }
    }

    pub fn from_str(str: &str) -> Self {
        match str {
            "Carl Dark" => Self::CarlDark,
            "TokyoNight Storm" => Self::TokyoNightStorm,
            "TokyoNight" => Self::TokyoNight,
            "Rerun MTech" => Self::RerunMtech,
            "Custom" => Self::Custom,
            _ => Self::Custom
        }
    }
}

pub fn set_custom_style(config: &ThemeConfig) -> Arc<Style> {
    match config.preset_style {
        PresetStyles::CarlDark => {
            let mut custom_style = CarlDark.custom_style();
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
            Arc::new(custom_style)
        },
        PresetStyles::TokyoNightStorm => {
            let mut custom_style = TokyoNightStorm.custom_style();
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
            Arc::new(custom_style)
        },
        PresetStyles::TokyoNight => {
            let mut custom_style = TokyoNight.custom_style();
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
            Arc::new(custom_style)
        },
        PresetStyles::RerunMtech => default_app_style(),
        PresetStyles::Custom => {
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
                clip_rect_margin: 5.0,
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
        },
    }
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