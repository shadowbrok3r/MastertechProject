
use eframe::egui::Ui;
use egui::{Color32, ComboBox, Id, Layout, Stroke, Style, TextEdit, Vec2, Widget};
use egui_extras::{Size, StripBuilder};

use database::schema::TaskPayload;

use super::task_functions::{Displayable, Interaction, Updatable};

impl Displayable for TaskPayload{
    fn display_task_cards(&mut self, ui: &mut Ui)  -> anyhow::Result<(), anyhow::Error> {

        ui.style_mut().visuals.selection.stroke.color = Color32::from_additive_luminance(255);
        ui.style_mut().visuals.widgets.hovered.bg_stroke = Stroke::new(2.0, Color32::from_rgb(200, 20, 200));
        ui.style_mut().visuals.widgets.inactive.bg_fill = Color32::from_additive_luminance(255);
        ui.style_mut().visuals.widgets.hovered.expansion = 2.0;
        ui.add_space(10.0);
        ui
            .group(|ui|
        {
            ui.set_max_height(160.0);
            ui.set_width(370.0);

            StripBuilder::new(ui)
                .cell_layout(Layout::top_down_justified(egui::Align::Center))
                .size(Size::relative(0.1))
                .size(Size::relative(0.8))
                .size(Size::relative(0.1))
                .vertical(|mut strip| {
                    strip.strip(|strip| {
                        strip
                            .cell_layout(Layout::left_to_right(egui::Align::Min))
                            .cell_layout(Layout::left_to_right(egui::Align::Center))
                            .cell_layout(Layout::left_to_right(egui::Align::Max))
                            .size(Size::relative(0.2))
                            .size(Size::remainder())
                            .size(Size::relative(0.2))
                            .horizontal( |mut s| 
                        {
                            s.cell(|ui|{
                                ui.with_layout(Layout::centered_and_justified(egui::Direction::TopDown), |ui|{
                                    self.interact_assignee_initials(ui);
                                });
                            });
                            s.cell(|ui|{
                                ui.with_layout(Layout::centered_and_justified(egui::Direction::TopDown), |ui|{
                                    self.interact_task_name(ui);
                                });
                            });
                            s.cell(|ui|{
                                ui.with_layout(Layout::centered_and_justified(egui::Direction::TopDown), |ui|{
                                    self.interact_completed(ui);
                                });
                            });
                        });
                    });

                    strip.strip(|strip| {
                        strip
                            .cell_layout(Layout::left_to_right(egui::Align::Min))
                            .cell_layout(Layout::right_to_left(egui::Align::Max))
                            .size(Size::remainder())
                            .size(Size::remainder())
                            .horizontal( |mut s| 
                        {
                            s.cell(|ui|{
                                ui.with_layout(Layout::centered_and_justified(egui::Direction::TopDown), |ui|{
                                    self.interact_task_description(ui);
                                });
                            });
                            s.cell(|ui|{
                                ui.with_layout(Layout::centered_and_justified(egui::Direction::TopDown), |ui|{
                                    self.interact_task_description(ui);
                                });
                            });
                        });
                    });
                    strip.strip(|strip| {
                        strip
                            .cell_layout(Layout::left_to_right(egui::Align::Min))
                            .cell_layout(Layout::left_to_right(egui::Align::Center))
                            .cell_layout(Layout::left_to_right(egui::Align::Max))
                            .size(Size::relative(0.3))
                            .size(Size::remainder())
                            .size(Size::relative(0.3))
                            .horizontal( |mut s| 
                        {
                            s.cell(|ui|{
                                ui.with_layout(Layout::centered_and_justified(egui::Direction::TopDown), |ui|{
                                    self.interact_due_date(ui);
                                });
                            });
                            s.cell(|ui|{
                                ui.with_layout(Layout::centered_and_justified(egui::Direction::TopDown), |ui|{
                                    self.interact_priority(ui);
                                });
                            });
                            s.cell(|ui|{
                                ui.with_layout(Layout::centered_and_justified(egui::Direction::TopDown), |ui|{
                                    self.interact_status(ui);
                                });
                            });
                        });
                    });
            });
        });
        
        /* 
            let header_id = ui.make_persistent_id(&task_data.task_name);
            CollapsingState::load_with_default_open(ui.ctx(), header_id, false)
            .show_header(ui, |ui| {
                ui.toggle_value(&mut stuff, &task_data.task_name);
            })
            .body_unindented(|ui| {
                ui.label("The body is always custom");
            });
        */
        Ok(())
    }
}

