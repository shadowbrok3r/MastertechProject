use database::Database;
use eframe::egui::Ui;
use egui::{Button, CollapsingHeader, Widget};
use egui::{Color32, Frame, Layout, Margin, Rounding, Stroke};
use egui_extras::{Size, StripBuilder};
use database::schema::{TaskPayload, User};
use serde::Serialize;

use crate::utilities::{Displayable, Interaction};

use super::{ColumnLayout, Sortable, TaskUiActions};

pub mod modals;
pub mod create_task;
pub mod column_layout;
pub mod task_layout;
pub mod modal_handler;

#[derive(Clone, Serialize)]
pub enum Filters{
    FilterAssignee,
    FilterCompleted,
    FilterStatus,
    FilterPriority,
    // FilterDate
}



impl Displayable for TaskPayload{
    fn display_task_cards(
        &mut self, 
        ui: &mut Ui, 
        database: Database, 
        store_users: &Vec<User>
    )  -> Option<TaskUiActions>{
        setup_task_styling(ui);

        let mut res: Option<TaskUiActions> = None;

        Frame::default()
            .fill(Color32::from_rgb(20, 20, 28))
            .inner_margin(Margin::same(8.0))
            .outer_margin(Margin::same(5.0))
            .rounding(Rounding::same(15.0))
            .stroke(Stroke::new(1.0, Color32::DARK_GRAY))
            .show(ui, |ui| 
        {
            ui.set_max_height(300.0);
            ui.set_min_height(67.0);
            ui.set_width(400.0);

            StripBuilder::new(ui)
                .cell_layout(Layout::top_down_justified(egui::Align::Center))
                .size(Size::exact(15.0))// Task Header
                .size(Size::exact(4.0))
                .size(Size::exact(15.0))// Task Footer
                .size(Size::exact(4.0))
                .size(Size::initial(20.0).at_most(200.0))// Task Body
                .vertical(|mut strip| 
            {
                strip.strip(|strip| 
                {
                    strip
                        .cell_layout(Layout::left_to_right(egui::Align::Min))
                        .cell_layout(Layout::left_to_right(egui::Align::Center))
                        .cell_layout(Layout::left_to_right(egui::Align::Max))
                        .size(Size::relative(0.1))
                        .size(Size::remainder())
                        .size(Size::relative(0.1))
                        .size(Size::relative(0.1))
                        .horizontal( |mut s| 
                    {
                        s.cell(|ui|{
                            ui.with_layout(Layout::centered_and_justified(egui::Direction::TopDown), |ui|{
                                self.interact_assignee_initials(ui, database.clone(), store_users);
                            });
                        });

                        s.cell(|ui|{
                            ui.with_layout(Layout::centered_and_justified(egui::Direction::TopDown), |ui|{
                                self.interact_task_name(ui, database.clone());
                            });
                        });
                        s.cell(|ui|{
                            ui.with_layout(Layout::centered_and_justified(egui::Direction::TopDown), |ui|{
                                if Button::new("⮫").small().ui(ui).clicked(){
                                    // self.create_task_modal = true;
                                    res = Some(TaskUiActions::OpenTaskModal(self.id.as_ref().unwrap().0.id.to_string()))
                                    // self.task_modal(ui, database.clone());
                                    
                                }
                            });
                        });
                        s.cell(|ui|{
                            ui.with_layout(Layout::centered_and_justified(egui::Direction::TopDown), |ui|{
                                self.interact_completed(ui, database.clone());
                            });
                        });
                    });
                });
                strip.empty();
                
                strip.strip(|strip| 
                {
                    strip
                        .cell_layout(Layout::left_to_right(egui::Align::Min))
                        .cell_layout(Layout::left_to_right(egui::Align::Center))
                        .cell_layout(Layout::left_to_right(egui::Align::Max))
                        .size(Size::remainder())
                        .size(Size::remainder())
                        .size(Size::remainder())
                        .horizontal( |mut s| 
                    {
                        s.cell(|ui|{
                            ui.with_layout(Layout::centered_and_justified(egui::Direction::LeftToRight), |ui|{
                                self.interact_priority(ui, database.clone());
                            });
                        });
                        s.cell(|ui|{
                            ui.with_layout(Layout::centered_and_justified(egui::Direction::TopDown), |ui|{
                                self.interact_due_date(ui, database.clone());
                            });
                        });
                        s.cell(|ui|{
                            ui.with_layout(Layout::centered_and_justified(egui::Direction::RightToLeft), |ui|{
                                self.interact_status(ui, database.clone());
                            });
                        });
                    });
                });
                strip.empty();
                strip.strip(|strip| 
                {
                    strip
                        .cell_layout(Layout::left_to_right(egui::Align::Min))
                        .size(Size::remainder())
                        .size(Size::remainder())
                        .horizontal( |mut s| 
                    {
                        s.cell(|ui|
                        {

                            let checkin_header = ui.make_persistent_id(format!("checkin_notes {:?}", self.id.as_ref().unwrap().0.id));
                            
                            let checkin_head = CollapsingHeader::new("Checkin Notes").id_source(checkin_header);
                            checkin_head
                                .show_unindented(ui, |ui| 
                            {
                                self.interact_task_description(ui, database.clone());
                            });
                        });
                        s.cell(|ui| 
                        {
                            let rec_header = ui.make_persistent_id(format!("recommendations {:?}", self.id.as_ref().unwrap().0.id));
                            let rec_head = CollapsingHeader::new("Recommendations").id_source(rec_header);
                            rec_head
                                .show_unindented(ui, |ui|
                            {
                                self.interact_task_description(ui, database.clone());
                            });
                        });
                    });
                });
            });
        });
        res
    }
}


fn setup_task_styling(ui: &mut Ui){
    ui.style_mut().visuals.selection.stroke.color = Color32::BLACK;
    ui.style_mut().visuals.selection.bg_fill = Color32::from_rgb(120, 10, 120);
    
    ui.style_mut().visuals.widgets.inactive.bg_fill = Color32::GOLD;
    ui.style_mut().visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    ui.style_mut().visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(20, 20, 25);
    ui.style_mut().visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(80, 80, 80));

    ui.style_mut().visuals.widgets.open.bg_fill = Color32::from_black_alpha(50);
    ui.style_mut().visuals.widgets.open.weak_bg_fill = Color32::from_black_alpha(50);

    ui.style_mut().visuals.widgets.active.weak_bg_fill = Color32::from_rgb(30,30,30);

    ui.style_mut().visuals.widgets.hovered.weak_bg_fill = Color32::TRANSPARENT;
    ui.style_mut().visuals.widgets.hovered.bg_fill = Color32::from_rgb(12, 12, 12);
    ui.style_mut().visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(200, 20, 200));
    // ui.style_mut().visuals.widgets.hovered.fg_stroke = Stroke::new(2.0, Color32::from_rgb(200, 20, 200));
    ui.style_mut().visuals.widgets.hovered.expansion = 2.0;
}