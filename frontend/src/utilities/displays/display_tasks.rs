
use database::Database;
use eframe::egui::Ui;
use egui::{Button, ScrollArea, Widget};
use egui::{Color32, Frame, Layout, Margin, RichText, Rounding, Stroke};
use egui_extras::{Size, Strip, StripBuilder};
use database::schema::{Priority, Status, TaskPayload, User};
use log::info;

use crate::utilities::{Displayable, FilterTasks, Interaction};


impl Displayable for TaskPayload{
    fn display_task_cards(
        &mut self, 
        ui: &mut Ui, 
        database: Database, 
        store_users: &Vec<User>
    )  -> anyhow::Result<(), anyhow::Error> {

        ui.style_mut().visuals.selection.stroke.color = Color32::from_additive_luminance(255);
        ui.style_mut().visuals.widgets.hovered.bg_stroke = Stroke::new(2.0, Color32::from_rgb(200, 20, 200));
        ui.style_mut().visuals.widgets.inactive.bg_fill = Color32::from_additive_luminance(255);
        ui.style_mut().visuals.widgets.hovered.expansion = 2.0;

        let frame = Frame::default()
            .fill(Color32::from_rgb(7, 7, 13))
            .inner_margin(Margin::same(4.0))
            .outer_margin(Margin::same(10.0))
            .rounding(Rounding::same(5.0))
            .stroke(Stroke::new(1.0, Color32::from_additive_luminance(200)));

        frame.show(ui, |ui| {
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
                                    if self.interact_assignee_initials(ui, database.clone(), store_users).unwrap().changed(){
                                        info!("interact_assignee_initials changed: {:?}// {:?}", self.id, self.task_name);
                                    }
                                });
                            });
                            s.cell(|ui|{
                                ui.with_layout(Layout::centered_and_justified(egui::Direction::TopDown), |ui|{
                                    self.interact_task_name(ui, database.clone());
                                });
                            });
                            s.cell(|ui|{
                                ui.with_layout(Layout::centered_and_justified(egui::Direction::TopDown), |ui|{
                                    self.interact_completed(ui, database.clone());
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
                                    self.interact_task_description(ui, database.clone());
                                });
                            });
                            s.cell(|ui|{
                                ui.with_layout(Layout::centered_and_justified(egui::Direction::TopDown), |ui|{
                                    self.interact_task_description(ui, database.clone());
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
                                    self.interact_due_date(ui, database.clone());
                                });
                            });
                            s.cell(|ui|{
                                ui.with_layout(Layout::centered_and_justified(egui::Direction::TopDown), |ui|{
                                    self.interact_priority(ui, database.clone());
                                });
                            });
                            s.cell(|ui|{
                                ui.with_layout(Layout::centered_and_justified(egui::Direction::TopDown), |ui|{
                                    if self.interact_status(ui, database).unwrap().changed(){
                                        info!("interact_status changed: {:?}// {:?}", self.id, self.task_name);
                                    }
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

    // fn display_table(&mut self, ui: &mut Ui, tasks: Vec<TaskPayload>) -> anyhow::Result<(), anyhow::Error> {Ok(())}
}

// // pub fn setup_display(&mut self, ui: &mut egui::Ui, column_names: Vec<String>) {
pub fn setup_display(
    ui: &mut egui::Ui, 
    column_names: Vec<String>, 
    tasks: &mut Vec<TaskPayload>, 
    database: Database,

    filters: &Vec<Filters>, 
    assignees: &Option<Vec<User>>,
    status: bool,
    priority: &Option<Priority>,
    complete: &Option<bool>,
    current_user: &Option<User>
) {
    ui.style_mut().visuals.window_rounding = Rounding::same(5.0);
    let header_frame = Frame::default()
        .fill(Color32::from_rgb(25, 25, 30))
        .inner_margin(Margin::same(4.0))
        .outer_margin(Margin::symmetric(4.0, 1.0))
        .rounding(Rounding::same(5.0))
        .stroke(Stroke::new(1.0, Color32::from_additive_luminance(50)));

    let column_frame = Frame::default()
        .fill(Color32::from_rgb(20, 20, 20))
        .inner_margin(Margin::same(8.0))
        .rounding(Rounding::same(10.0))
        .stroke(Stroke::new(1.0, Color32::from_additive_luminance(50)));

    let column_width = Size::exact(450.0);

    ScrollArea::horizontal()
        .hscroll(true)
        .show_viewport(ui, |ui, _|
    {
        StripBuilder::new(ui)
            .cell_layout(Layout::top_down_justified(egui::Align::Center))
            .size(Size::relative(0.01))
            .size(Size::relative(0.07))
            .size(Size::relative(0.92))
            .vertical(|mut strip| 
        {
            strip
                .strip(|strip| 
            {
                strip
                    .sizes(column_width, column_names.len())
                    .horizontal( |s| 
                {
                    task_headers(s, column_names.clone(), header_frame);
                });
            });
            strip.empty();
            strip
                .strip(|strip| 
            {
                strip
                    .sizes(column_width, column_names.len())
                    .horizontal( |s| 
                {
                    task_columns(     
                        s,           
                        filters,
                        tasks,
                        &assignees,
                        status,
                        &priority,
                        &complete,
                        current_user,
                        database,
                        column_frame
                    );
                });
            });
        });
    });
}
pub fn task_headers(
    mut s: Strip,
    column_names: Vec<String>,
    header_frame: Frame,
){
    for name in &column_names{
        s.cell(|ui|{
            header_frame.show(ui, |ui|{
                ui.vertical_centered_justified(|ui|{
                    ui.colored_label(Color32::WHITE, RichText::new(name.clone()).heading());
                });
                ui.with_layout(Layout::right_to_left(egui::Align::Max), |ui| {
                    let add_task = Button::new("+").fill(Color32::TRANSPARENT).stroke(Stroke::new(2.0, Color32::DARK_BLUE))
                        .ui(ui);

                    if add_task.clicked(){
                        info!("Adding task!");
                    }
                });
            });
        });
    }
}


pub fn task_columns(
    mut s: Strip, 
    filters: &Vec<Filters>, 
    tasks: &mut Vec<TaskPayload>,
    assignees: &Option<Vec<User>>,
    status: bool,
    priority: &Option<Priority>,
    complete: &Option<bool>,
    current_user: &Option<User>,
    database: Database,
    column_frame: Frame,
){
    if let Some(_) = current_user {
        if status{
            for status in Status::VALUES{
                let mut filtered = filter_items(
                    &filters,tasks,&None,&Some(status),&priority,&complete
                );

                
                s.cell(|ui| {
                    column_frame.show(ui, |ui| {
                        ui.vertical_centered_justified(|ui| {
                            ScrollArea::vertical()
                            .auto_shrink(false)
                            .show_viewport(ui, |ui, _| {
                                for task in filtered.iter_mut() {
                                    if let Some(store_users) = &assignees{
                                        task.display_task_cards(ui, database.clone(), &store_users.as_ref()).unwrap();
                                    }
                                    
                                }
                            });
                        });
                    });
                });
            }
        }else{
            let mut filtered = filter_items(
                &filters,tasks,&None,&None,&priority,&complete
            );
            
            s.cell(|ui| {
                column_frame.show(ui, |ui| {
                    ui.vertical_centered_justified(|ui| {
                        ScrollArea::vertical()
                        .auto_shrink(false)
                        .show_viewport(ui, |ui, _| {
                            for task in filtered.iter_mut() {
                                task.display_task_cards(ui, database.clone(), &assignees.as_ref().unwrap()).unwrap();
                            }
                        });
                    });
                });
            });
        }
    } else if let Some(users) = assignees {
        // Multiple users, iterate over each user
        for user in users.iter() {
            let mut user_filters = filters.clone();
            user_filters.push(Filters::FilterAssignee);

            let mut filtered = filter_items(
                &user_filters,
                tasks,
                &Some(user.clone()),
                &None,
                &priority,
                &complete,
            );

            s.cell(|ui| {
                column_frame.show(ui, |ui| {
                    ui.vertical_centered_justified(|ui| {
                        ScrollArea::vertical()
                            .auto_shrink(false)
                            .show_viewport(ui, |ui, _| 
                        {
                            for task in filtered.iter_mut() {
                                task.display_task_cards(ui, database.clone(), &assignees.as_ref().unwrap()).unwrap();
                            }
                        });
                    });
                });
            });
        }
    }
}

pub fn filter_items(
    filters: &Vec<Filters>, 
    tasks: &mut Vec<TaskPayload>,
    assignee: &Option<User>,
    status: &Option<Status>,
    priority: &Option<Priority>,
    complete: &Option<bool>,
) -> Vec<TaskPayload>{
    
    filters.into_iter().fold(tasks.to_owned(), |acc_tasks, filter| {
        match filter {
            Filters::FilterAssignee => {
                if let Some(ref user) = assignee {
                    acc_tasks.filter_by_assignee(user)
                } else {
                    acc_tasks
                }
            },
            Filters::FilterCompleted => {
                if let Some(complete) = complete {
                    acc_tasks.filter_by_completed(*complete)
                } else {
                    acc_tasks
                }
            },
            Filters::FilterStatus => {
                if let Some(status) = status {
                    acc_tasks.filter_by_status(&status)
                } else {
                    acc_tasks
                }
            },
            Filters::FilterPriority => {
                if let Some(ref priority) = priority {
                    acc_tasks.filter_by_priority(&priority)
                } else {
                    acc_tasks
                }
            },
        }
    })
}



#[derive(Clone)]
pub enum Filters{
    FilterAssignee,
    FilterCompleted,
    FilterStatus,
    FilterPriority,
    // FilterDate
}