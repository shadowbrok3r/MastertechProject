use chrono::{DateTime, NaiveDate, Utc};
use eframe::egui::Ui;
use egui::{Button, Color32, ComboBox, Id, Layout, Margin, Stroke, TextEdit, Vec2, Widget};
use egui_extras::{DatePickerButton, Size, StripBuilder};

use crate::database::{
    methods::Displayable, 
    schema::{
        Priority, Status, TaskPayload
    }
};

impl Displayable for TaskPayload{
    fn display_task_cards(&mut self, ui: &mut Ui)  -> anyhow::Result<(), anyhow::Error> {
        // ui.horizontal_wrapped(|ui| {
            // let clip = ui.clip_rect();
            // ui.set_clip_rect(clip);


                // egui::Frame::default()
                //     .stroke(ui.visuals().widgets.inactive.bg_stroke)
                //     .rounding(ui.visuals().widgets.active.rounding)
                //     .fill(ui.visuals().extreme_bg_color)
                //     .inner_margin(Margin::same(4.0))
                //     .outer_margin(Margin::same(5.0))
                //     .show(ui, |ui| 
                // {
                ui.allocate_ui_with_layout(Vec2::new(200.0, 400.0), 
                    Layout::left_to_right(egui::Align::Min)
                    .with_main_wrap(true), |ui| 
                {
                    // ui.painter().add(shape)
                    ui.set_height(200.0);
                    ui.set_width(400.0);
                    StripBuilder::new(ui)
                        .cell_layout(Layout::top_down_justified(egui::Align::Center))
                        .size(Size::relative(0.2))
                        .size(Size::relative(0.6))
                        .size(Size::relative(0.2))
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
                                    .cell_layout(Layout::left_to_right(egui::Align::Max))
                                    .size(Size::remainder())
                                    .size(Size::remainder())
                                    .vertical( |mut s| 
                                {
                                    s.cell(|ui|{
                                        ui.with_layout(Layout::centered_and_justified(egui::Direction::TopDown), |ui|{
                                            ui.vertical_centered_justified(|ui| {
                                                self.interact_task_description(ui);
                                            });
                                        });
                                    });
                                    s.cell(|ui|{
                                        ui.with_layout(Layout::centered_and_justified(egui::Direction::TopDown).with_main_wrap(true), |ui|{
                                            ui.vertical_centered_justified(|ui| {
                                                self.interact_task_description(ui);
                                            });
                                        });
                                    });
                                });
                            });
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
        // });   
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
        TextEdit::singleline(&mut self.task_name).horizontal_align(egui::Align::Center).ui(ui);
    }

    fn interact_task_description(&mut self, ui: &mut Ui) {
        if let Some(description) = &self.task_description{
            ui.label(
                egui::RichText::new(description).color(egui::Color32::WHITE)
            );
        }else{
            egui::Frame::default()
                .rounding(ui.visuals().widgets.noninteractive.rounding)
                .stroke(ui.visuals().widgets.active.bg_stroke)
                .fill(Color32::from_black_alpha(100))
                .show(ui, |ui| 
            {
                let mut task_description = "No task description";
                let text_edit = TextEdit::multiline(&mut task_description)
                    .desired_rows(4)
                    .desired_width(ui.available_width())
                    .horizontal_align(egui::Align::Center);
                let x = ui.add(text_edit);

                if x.hovered(){
                    x.highlight();
                }
            });
        }
    }

    fn interact_recommendations(&mut self, ui: &mut Ui){
        let mut recommendations = "These are test checkin notes";
        egui::Frame::default()
            .rounding(ui.visuals().widgets.noninteractive.rounding)
            .stroke(ui.visuals().widgets.active.bg_stroke)
            .fill(Color32::from_black_alpha(100))
            .show(ui, |ui| 
        {
            let text_edit = TextEdit::multiline(&mut recommendations)
                .desired_rows(4)
                .desired_width(ui.available_width())
                .horizontal_align(egui::Align::Center)
                .show(ui).response;
                                                                    
            if text_edit.hovered(){
                text_edit.highlight();
            }
        });
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
            .selected_text(format!("{:?}", &self.status))
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
                .height(ui.available_height())
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