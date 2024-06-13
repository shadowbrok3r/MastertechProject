use chrono::{DateTime, Utc};
use database::Database;
use eframe::egui::Ui;
use egui::{Align, Button, CollapsingHeader, Direction, Widget};
use egui::{Color32, Frame, Layout, Margin, Rounding, Stroke};
use egui_extras::{Size, StripBuilder};
use database::schema::{TaskPayload, User};
use log::info;

use crate::utilities::{Displayable, Interaction};

use super::{ColumnLayout, TaskUiActions};

pub mod modals;
pub mod chats;
pub mod create_task_modal;
pub mod task_modal;
pub mod column_layout;
pub mod task_layout;

impl Displayable for TaskPayload{
    fn display_task_cards(
        &mut self, 
        ui: &mut Ui, 
        database: Database, 
        store_users: &Vec<User>
    )  -> Option<TaskUiActions>{
        setup_task_styling(ui);

        let mut res: Option<TaskUiActions> = None;
        // let task_frame_color = if self.due_date.parse::<>
        let frame_color = date_colors(self.due_date.clone(), self.completed);

        Frame::default()
            .fill(Color32::from_rgb(22,22,26))
            .inner_margin(Margin::same(8.0))
            .outer_margin(Margin::same(5.0))
            .rounding(Rounding::same(15.0))
            .stroke(Stroke::new(1.0, frame_color))
            .show(ui, |ui| 
        {
            ui.set_max_height(300.0);
            ui.set_min_height(67.0);
            ui.set_width(400.0);

            StripBuilder::new(ui)
                .cell_layout(Layout::top_down_justified(Align::Center))
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
                        .cell_layout(Layout::left_to_right(Align::Min))
                        .cell_layout(Layout::left_to_right(Align::Center))
                        .cell_layout(Layout::left_to_right(Align::Max))
                        .size(Size::relative(0.1))
                        .size(Size::remainder())
                        .size(Size::relative(0.1))
                        .size(Size::relative(0.1))
                        .horizontal( |mut s| 
                    {
                        s.cell(|ui|{
                            ui.with_layout(Layout::centered_and_justified(Direction::TopDown), |ui|{
                                self.interact_assignee_initials(ui, database.clone(), store_users);
                            });
                            
                        });

                        s.cell(|ui|{
                            ui.with_layout(Layout::centered_and_justified(Direction::TopDown), |ui|{
                                self.interact_task_name(ui, database.clone());
                            });
                        });
                        s.cell(|ui|{
                            ui.with_layout(Layout::centered_and_justified(Direction::TopDown), |ui|{
                                if Button::new("⮫").small().ui(ui).clicked(){
                                    res = Some(TaskUiActions::OpenTaskModal(self.to_owned()))
                                }
                            });
                        });
                        s.cell(|ui|{
                            ui.with_layout(Layout::centered_and_justified(Direction::TopDown), |ui|{
                                self.interact_completed(ui, database.clone());
                            });
                        });
                    });
                });
                strip.empty();
                
                strip.strip(|strip| 
                {
                    strip
                        .cell_layout(Layout::left_to_right(Align::Min))
                        .cell_layout(Layout::left_to_right(Align::Center))
                        .cell_layout(Layout::left_to_right(Align::Max))
                        .size(Size::remainder())
                        .size(Size::remainder())
                        .size(Size::remainder())
                        .horizontal( |mut s| 
                    {
                        s.cell(|ui|{
                            ui.with_layout(Layout::centered_and_justified(Direction::LeftToRight), 
                                |ui| self.interact_priority(ui, database.clone()));
                        });
                        s.cell(|ui|{
                            ui.with_layout(Layout::centered_and_justified(Direction::TopDown), 
                                |ui| self.interact_due_date(ui, database.clone()));
                        });
                        s.cell(|ui|{
                            ui.with_layout(Layout::centered_and_justified(Direction::RightToLeft),
                                |ui| self.interact_status(ui, database.clone()));
                        });
                    });
                });
                strip.empty();
                strip.strip(|strip| 
                {
                    if self.service_ticket.is_none(){
                        strip
                            .cell_layout(Layout::top_down(Align::Center))
                            .size(Size::remainder())
                            .horizontal( |mut s| 
                        {
                            s.cell(|ui|
                            {
                                let task_descrip_header = ui.make_persistent_id(format!("task_description {:?}", self.id.as_ref().unwrap().0.id));
                                let task_descrip_head = CollapsingHeader::new("Task Description").id_source(task_descrip_header);
                                task_descrip_head.show_unindented(ui, |ui| self.interact_task_description(ui, database.clone()));
                            });
                        });
                    }else{
                        strip
                            .cell_layout(Layout::left_to_right(Align::Min))
                            .size(Size::remainder())
                            .size(Size::remainder())
                            .horizontal( |mut s| 
                        {
                            s.cell(|ui|
                            {
                                let checkin_header = ui.make_persistent_id(format!("checkin_notes {:?}", self.id.as_ref().unwrap().0.id));
                                let checkin_head = CollapsingHeader::new("Checkin Notes").id_source(checkin_header);
                                checkin_head.show_unindented(ui, |ui| self.interact_checkin_notes(ui, database.clone()));
                            });
                            s.cell(|ui| 
                            {
                                let rec_header = ui.make_persistent_id(format!("recommendations {:?}", self.id.as_ref().unwrap().0.id));
                                let rec_head = CollapsingHeader::new("Recommendations").id_source(rec_header);
                                rec_head.show_unindented(ui, |ui| self.interact_recommendations(ui, database.clone()));
                            });
                        });  
                    }
                });
            });
        });
        res
    }
}

pub fn date_colors(date: String, complete: bool) -> Color32{
    let due_date = DateTime::parse_from_rfc3339(&date)
        .expect("Invalid date format")
        .with_timezone(&Utc)
        .date_naive();

    let current_date = Utc::now().date_naive();

    let overdue: bool = due_date < current_date ;
    let due_today: bool = due_date > current_date ;
    let due_tomorrow: bool = due_date == current_date.succ_opt().unwrap() ;

    if overdue && complete == false{
        Color32::from_rgb(199, 48, 103) // Pink
    }else if due_today && !complete{
        info!("due today - and not complete");
        Color32::from_rgb(240, 200, 108) // Orange
    }else if due_tomorrow {
        Color32::from_rgb(79, 232, 125) // Green
    }else{
        // Color32::from_rgb(31, 204, 178)
        Color32::from_rgb(199, 48, 103) // Pink
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
