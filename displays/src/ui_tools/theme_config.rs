use std::sync::Arc;

use crossbeam::channel::Sender;
use database::DATABASE;
use eframe::egui::{scroll_area::ScrollBarVisibility, style::{HandleShape, NumericColorSpace, Selection, TextCursorStyle, WidgetVisuals, Widgets}, Align, Button, Color32, ComboBox, CursorIcon, DragValue, FontFamily, FontId, Layout, ScrollArea, Shadow, Stroke, Style, Ui, Vec2, Visuals, Widget};
use log::info;
use serde::{Deserialize, Serialize};
use serde_json::to_vec;
use derivative::Derivative;
use crate::{PlatformSpawner, Spawner};

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
    pub font_size: f32
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
            font_size: 12.0
        }
    }
}

impl ThemeConfig {
    pub fn edit_ui(&mut self, ui: &mut Ui, tx: Sender<ThemeConfig>) -> (bool, Self) {
        let mut ret = (false, self.clone());
        ui.horizontal(|ui| {
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                let reset = Button::new("Reset to Default")
                    .min_size(Vec2::new(70., 25.))
                    .stroke(Stroke::new(1., self.warn_color))
                    .ui(ui);
                
                if reset.clicked() {
                    PlatformSpawner::spawn(async move {
                        let theme = ThemeConfig::default();
                        match DATABASE 
                            .query("UPDATE $auth.id SET user_settings.color_scheme = $color_settings")
                            .bind(("color_settings", theme.clone()))
                            .await 
                        {
                            Ok(res) => info!("Res: {res:?}"),
                            Err(e) => info!("Error updating User Settings: {e:?}"),
                        }
                    });
                    ret = (true, ThemeConfig::default());
                }
            });
            ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                let save = Button::new("Save")
                    .min_size(Vec2::new(70., 25.))
                    .stroke(Stroke::new(1., self.warn_color))
                    .ui(ui);
                
                if save.clicked() {
                    let color_settings = self.clone();
                    PlatformSpawner::spawn(async move {
                        match DATABASE
                            .query("UPDATE $auth.id SET user_settings.color_scheme = $color_settings")
                            .bind(("color_settings", color_settings.clone()))
                            .await 
                        {
                            Ok(res) => info!("Result: {res:?}"),
                            Err(e) => info!("Error updating User Settings: {e:?}"),
                        }
                    });
                    ret = (true, self.clone());
                }

                ui.add_space(5.);

                let save_local = Button::new("Save to file")
                    .min_size(Vec2::new(70., 25.))
                    .stroke(Stroke::new(1., self.warn_color))
                    .ui(ui);
                
                if save_local.clicked() {
                    let color_settings = self.clone();
                    
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
                                    info!("Failed to save file: {:?}", err);
                                }
                            }
                        } else {
                            info!("Error serializing settings to json");
                        }
                    });
                }

                ui.add_space(5.);

                let upload = Button::new("Upload settings")
                    .min_size(Vec2::new(70., 25.))
                    .stroke(Stroke::new(1., self.warn_color))
                    .ui(ui);
                
                if upload.clicked() {
                    let tx = tx.clone();
                    PlatformSpawner::spawn(async move {
                        // Show the save file dialog
                        if let Some(file) = rfd::AsyncFileDialog::new()
                            .set_file_name("mastertech_color_scheme.json") // Default file name
                            .pick_file()
                            .await
                        {
                            match serde_json::from_slice::<ThemeConfig>(&file.read().await) {
                                Ok(theme) => tx.try_send(theme).unwrap(),
                                Err(e) => info!("Error converting bytes to Theme: {e:?}"),
                            }
                        }
                        
                    });
                }
                
            });
        });

        ui.add_space(10.);

        ScrollArea::vertical()
            .scroll_bar_visibility(ScrollBarVisibility::AlwaysVisible)
            .max_height(800.)
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

        ret
    }
}


pub fn set_custom_style(config: &ThemeConfig) -> Arc<Style> {
    let theme = CarlDark; // Assuming a theme object or struct
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
                bg_stroke: Stroke::new(1.0, config.widget_bg_stroke_color),
                corner_radius: config.rounding,
                fg_stroke: Stroke::new(1.0, config.widget_fg_stroke_color),
                expansion: 0.0,
            },
            inactive: WidgetVisuals {
                bg_fill: config.widget_bg_fill,
                weak_bg_fill: Color32::from_rgb(18, 18, 20),
                bg_stroke: Stroke::new(1.0, Color32::from_rgb(80, 80, 80)),
                corner_radius: config.rounding,
                fg_stroke: Stroke::new(1.0, config.widget_bg_stroke_color),
                expansion: 0.2,
            },
            hovered: WidgetVisuals {
                bg_fill: config.hovered_bg_fill,
                weak_bg_fill: config.hovered_weak_bg_fill,
                bg_stroke: Stroke::new(0.5, config.hovered_bg_stroke_color),
                corner_radius: config.rounding,
                fg_stroke: Stroke::new(1.0, config.hovered_fg_stroke_color),
                expansion: 0.2,
            },
            active: WidgetVisuals {
                bg_fill: config.active_bg_fill,
                weak_bg_fill: config.active_weak_bg_fill,
                bg_stroke: Stroke::new(1.0, config.active_bg_stroke_color),
                corner_radius: config.rounding,
                fg_stroke: Stroke::new(1.0, config.active_fg_stroke_color),
                expansion: 0.2,
            },
            open: WidgetVisuals {
                bg_fill: config.open_bg_fill,
                weak_bg_fill: config.open_weak_bg_fill,
                bg_stroke: Stroke::new(1.0, config.open_bg_stroke_color),
                corner_radius: config.rounding,
                fg_stroke: Stroke::new(1.0, config.open_fg_stroke_color),
                expansion: 0.2,
            },
        },
        selection: Selection {
            bg_fill: config.selection_bg_fill,
            stroke: Stroke::new(1.0, config.selection_stroke_color),
        },
        hyperlink_color: config.link_color,
        faint_bg_color: config.faint_bg_color,
        extreme_bg_color: config.extreme_bg_color,
        code_bg_color: config.code_bg_color,
        warn_fg_color: config.warn_color,
        error_fg_color: config.error_color,
        window_fill: config.background_color,
        window_stroke: Stroke::new(1.0, config.window_stroke_color),
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
        numeric_color_space: NumericColorSpace::Linear, // How numeric values are displayed
        ..Default::default()
    };

    // info!("config.text_color: {:?} - {:?}", config.text_color, ThemeConfig::default().text_color);

    Arc::new(custom_style)
}