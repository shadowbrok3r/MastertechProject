use database::Database;
use egui::{Button, RichText, ScrollArea, Widget};
use egui::{Color32, Frame, Layout, Margin, Rounding, Stroke};
use egui_extras::{Size, Strip, StripBuilder};
use database::schema::{Priority, Status, TaskPayload, User};
use log::info;
use crate::utilities::{ColumnLayout, Displayable, FilterTasks, ModalType, TaskUiActions};
use super::create_task_modal::CreateTaskModal;
use super::task_layout::TaskLayout;
use super::task_modal::TaskModal;
use super::Filters;


impl ColumnLayout for TaskLayout {
    fn layout_task_cols<F>(
        &mut self,
        ui: &mut egui::Ui, 
        column_names: Vec<String>, 
        database: Database,
        assignees: &Option<Vec<User>>,
        // status: bool,
        // priority: &Option<Priority>,
        // complete: &Option<bool>,
        // current_user: &Option<User>,
        mut filter_items: F
    )
    where
        F: FnMut() -> Vec<TaskPayload>,
    {
        ui.style_mut().visuals.window_rounding = Rounding::same(5.0);
        let header_frame = Frame::default()
            .fill(Color32::from_rgb(20, 20, 25))
            .inner_margin(Margin::same(4.0))
            .outer_margin(Margin::symmetric(4.0, 1.0))
            .rounding(Rounding::same(5.0))
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
                        .horizontal( |strip| self.task_headers(strip, column_names.to_owned(), header_frame));
                });
                strip.empty();
                strip
                    .strip(|strip| 
                {
                    strip
                        .sizes(column_width, column_names.len())
                        .horizontal( |strip| {
                            // self.task_columns(strip, filters,&assignees,status,&priority,&complete,current_user, database, column_frame, filter_items);
                    
                            self.task_columns(
                                strip,
                                filter_items(),
                                assignees,
                                database
                            );
                        });
                });
            });
        });
    }
    fn task_columns(
        &mut self,
        mut s: Strip, 
        filtered_tasks: Vec<TaskPayload>,
        assignees: &Option<Vec<User>>,
        database: Database,
    ) {
        let column_frame = Frame::default()
            .fill(Color32::from_rgb(15, 15, 19))
            .inner_margin(Margin::same(8.0))
            .rounding(Rounding::same(10.0))
            .stroke(Stroke::new(1.0, Color32::from_additive_luminance(50)));

        s.cell(|ui| {
            column_frame.show(ui, |ui| {
                ui.vertical_centered_justified(|ui| {
                    ScrollArea::vertical()
                        .auto_shrink(false)
                        .show_viewport(ui, |ui, _| {
                            for mut task in filtered_tasks.into_iter() {
                                if let Some(store_users) = &assignees {
                                    let action = task.display_task_cards(ui, database.to_owned(), &store_users.as_ref());
                                    if let Some(action) = action {
                                        match action {
                                            TaskUiActions::OpenTaskModal(task) => {
                                                self.show_modal = true;
                                                let mut task_modal = TaskModal::default();
                                                task_modal.database = Some(database.to_owned());
                                                task_modal.task = Some(task);
                                                self.modal = ModalType::TaskModal(task_modal);
                                            },
                                        }
                                    }
                                }
                            }
                        });
                });
            });
        });
    }
    

    fn task_headers(
        &mut self,
        mut s: Strip,
        column_names: Vec<String>,
        header_frame: Frame,
    ) {
        for name in &column_names{
            s.cell(|ui|{
                header_frame.show(ui, |ui|
                {
                    ui.horizontal_top(|ui| 
                    {
                        ui.with_layout(Layout::left_to_right(egui::Align::Center), |ui| 
                        {
                            ui.vertical_centered(|ui|{
                                ui.colored_label(Color32::WHITE, RichText::new(name.to_owned()).heading());
                            });
                        });
    
                        ui.with_layout(Layout::right_to_left(egui::Align::Max), |ui| 
                        {
                            let response = Button::new(
                                RichText::new("✚")
                                    .raised()
                                    .color(Color32::LIGHT_RED)
                                )
                                .fill(Color32::TRANSPARENT)
                                .ui(ui);

                            if response.clicked(){
                                self.show_modal = true;
                                self.modal = ModalType::CreateTaskModal(CreateTaskModal::default());
                            }

                        });
                    });
                });
            });
        }
    }
    
    fn filter_items(
        &mut self,
        filters: &Vec<Filters>, 
        assignee: &Option<User>,
        status: &Option<Status>,
        priority: &Option<Priority>,
        complete: &Option<bool>,
    ) -> Vec<TaskPayload>{
        
        filters.into_iter().fold(self.tasks.to_owned(), |acc_tasks, filter| {
            match filter {
                Filters::FilterAssignee => {
                    if let Some(ref user) = assignee {
                        info!("Filtering by assignee: {:?}", user.everest_initials);
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
}



/*
    fn task_columns<F>(
        &mut self,
        mut s: Strip, 
        filters: &Vec<Filters>, 
        assignees: &Option<Vec<User>>,
        status: bool,
        priority: &Option<Priority>,
        complete: &Option<bool>,
        current_user: &Option<User>,
        database: Database,
        column_frame: Frame,
        mut filter_items: F
    )
    where
        F: FnMut(&Vec<Filters>, &Option<&User>, &Option<bool>, &Option<Priority>, &Option<bool>) -> Vec<TaskPayload>,
    {
        if let Some(_) = current_user {
            if status{
                for status in Status::VALUES{
                    let mut filtered = self.filter_items(
                        &filters,&None,&Some(status),&priority,&complete
                    );

                    s.cell(|ui| {
                        column_frame.show(ui, |ui| {
                            ui.vertical_centered_justified(|ui| {
                                ScrollArea::vertical()
                                .auto_shrink(false)
                                .show_viewport(ui, |ui, _| {
                                    
                                    for task in filtered.iter_mut() {
                                        if let Some(store_users) = &assignees{
                                            let action = task.display_task_cards(ui, database.to_owned(), &store_users.as_ref());
                                            if let Some(action) = action{
                                                match action{
                                                    TaskUiActions::OpenTaskModal(task) => {
                                                        self.show_modal = true;
                                                        let mut task_modal = TaskModal::default();
                                                        task_modal.database = Some(database.to_owned());
                                                        task_modal.task = Some(task);
                                                        self.modal = ModalType::TaskModal(task_modal);
                                                        
                                                    },
                                                }
                                            }
                                        }
                                        
                                    }
                                });
                            });
                        });
                    });
                }
            }else if let Some(users) = assignees {
                // Multiple users, iterate over each user
                for user in users.iter() {
                    let mut user_filters = filters.to_owned();
                    user_filters.push(Filters::FilterAssignee);
        
                    let mut filtered = self.filter_items(
                        &user_filters,
                        &Some(user.to_owned()),
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
                                        let _action = task.display_task_cards(ui, database.to_owned(), &assignees.as_ref().unwrap());
                                    }
                                });
                            });
                        });
                    });
                }
            }else{
                let mut filtered: Vec<TaskPayload> = self.filter_items(
                    &filters,&None,&None,&priority,&complete
                );
                
                s.cell(|ui| {
                    column_frame.show(ui, |ui| {
                        ui.vertical_centered_justified(|ui| {
                            ScrollArea::vertical()
                            .auto_shrink(false)
                            .show_viewport(ui, |ui, _| {
                                for task in filtered.iter_mut() {
                                    let _action = task.display_task_cards(ui, database.to_owned(), &assignees.as_ref().unwrap());
                                    // info!("Action: {action:?}");
                                }
                            });
                        });
                    });
                });
            }
        } 
    }
    
*/