use chrono::{DateTime, NaiveDate, Utc};
use eframe::{egui::Ui, wgpu::Color};
use egui::{Align, Button, Color32, ComboBox, Id, Layout, Margin, RichText, Stroke, Style, TextEdit, Vec2, Widget};
use egui_extras::{DatePickerButton, Size, StripBuilder};

use crate::database::{
    methods::Displayable, 
    schema::{
        Priority, Status, TaskPayload
    }
};

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
            ui.set_height(160.0);
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

impl TaskPayload {
    fn interact_task_name(&mut self, ui: &mut Ui) {
        TextEdit::singleline(&mut self.task_name).horizontal_align(Align::Center).vertical_align(Align::Center).ui(ui);
    }

    fn interact_task_description(&mut self, ui: &mut Ui) {
        ui.add_space(10.0);
        ui.style_mut().visuals.widgets.inactive.bg_stroke = Stroke::new(2.0, Color32::from_additive_luminance(80));
        // ui.style_mut().visuals.widgets.inactive.fg = Color32::BLACK;
        if let Some(description) = &self.task_description{
            ui.label(
                egui::RichText::new(description).color(Color32::WHITE)
            );
        }else{
            let mut task_description = "No task description";
            TextEdit::multiline(&mut task_description)
                .desired_rows(7)
                .desired_width(ui.available_width())
                .horizontal_align(egui::Align::Center)
                .ui(ui);
        }
    }

    fn interact_recommendations(&mut self, ui: &mut Ui){
        let mut recommendations = "These are test checkin notes";
        TextEdit::multiline(&mut recommendations)
            .desired_rows(4)
            .desired_width(ui.available_width())
            .horizontal_align(egui::Align::Center)
            .show(ui);
    }

    fn interact_due_date(&mut self, ui: &mut Ui) {
        let datetime: DateTime<Utc> = self.due_date.parse().expect("Failed to parse date");
        let mut formatted: NaiveDate = datetime.date_naive(); // format("%m/%d/%y");
        let id = self.id.clone().unwrap().0.id.to_string();
        let date_picker = DatePickerButton::new(&mut formatted)
            .format("%m/%d/%y")
            .id_source(id.as_str())
            .show_icon(false);

        ui.add_sized(ui.available_size(), date_picker);
    }

    fn interact_completed(&mut self, ui: &mut Ui) {
        if self.completed{
            let stroke = Stroke::new(2.0, Color32::GREEN);
            let button = Button::new("✔️").fill(ui.style().visuals.extreme_bg_color).stroke(stroke);
            ui.add_sized(ui.available_size(), button);
        }else{
            let stroke = Stroke::new(2.0, Color32::from_rgb(200, 20, 200));
            let button = Button::new("✖️").fill(ui.style().visuals.extreme_bg_color).stroke(stroke);
            ui.add_sized(ui.available_size(), button);
        }
    }

    fn interact_status(&mut self, ui: &mut Ui) {
        let mut status = Status::Todo;
        ComboBox::new(Id::new(&self.id.clone().unwrap().0.id), "")
            .selected_text(
                RichText::new(
                    format!("{:?}", &self.status)
                )
                
            )
            .width(ui.available_width())
            .height(ui.available_height())
            .show_ui(ui, |ui| 
        {
            ui.selectable_value(&mut status, Status::Todo, "Todo");
            ui.selectable_value(&mut status, Status::InRepair, "In Repair");
            ui.selectable_value(&mut status, Status::Complete, "Complete");
        });
    }

    fn _interact_dep(&mut self, ui: &mut Ui) {
        if let Some(ref mut dep) = self.dep {
            ui.label("Department:");
            ui.text_edit_singleline(dep);
        } else {
            ui.label("No department specified.");
        }
    }

    fn interact_priority(&mut self, ui: &mut Ui) {
        ComboBox::new(Id::new(&self.id.clone().unwrap().0.id), "")
        .selected_text(format!("{:?}", &self.priority.clone().unwrap_or_default()))
        .width(ui.available_width())
        .height(ui.available_height())
        .show_ui(ui, |ui| 
        {
            ui.selectable_value(&mut self.priority.clone().unwrap_or(Priority::Normal), Priority::Normal, "Normal");
            ui.selectable_value(&mut self.priority.clone().unwrap_or(Priority::Normal), Priority::Rfs, "Rfs");
            ui.selectable_value(&mut self.priority.clone().unwrap_or(Priority::Normal), Priority::Qc, "Qc");
            ui.selectable_value(&mut self.priority.clone().unwrap_or(Priority::Normal), Priority::Express, "Express");
            ui.selectable_value(&mut self.priority.clone().unwrap_or(Priority::Normal), Priority::CustomerFire, "CustomerFire");
        });
    }

    fn interact_assignee_initials(&mut self, ui: &mut Ui) {
        if let Some(assignee_initials) = &mut self.assignee_initials{
            let new_assignee = String::new();
            ComboBox::new(Id::new(&self.id.clone().unwrap().0.id), "")
                .selected_text(assignee_initials.clone())
                .width(ui.available_width())
                .height(ui.available_height()/ 2.0)
                .show_ui(ui, |ui| 
            {
                ui.selectable_value(assignee_initials, new_assignee.clone(), assignee_initials.clone());
                ui.selectable_value(assignee_initials, new_assignee.clone(), assignee_initials.clone());
                ui.selectable_value(assignee_initials, new_assignee.clone(), assignee_initials.clone());
            
            });
        }
    }

    // Add more interact methods for other fields if necessary...
}