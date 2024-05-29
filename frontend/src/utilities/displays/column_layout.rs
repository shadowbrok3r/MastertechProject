use database::Database;
use egui::{Button, Response, RichText, ScrollArea, Widget};
use egui::{Color32, Frame, Layout, Margin, Rounding, Stroke};
use egui_extras::{Size, Strip, StripBuilder};
use database::schema::{Priority, Status, TaskPayload, User};
use log::info;

use crate::utilities::{ColumnLayout, Displayable, FilterTasks};

use super::task_layout::TaskLayout;
use super::Filters;


impl ColumnLayout for TaskLayout {
    fn setup_display(
        &mut self,
        ui: &mut egui::Ui, 
        column_names: Vec<String>, 
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
            .fill(Color32::from_rgb(20, 20, 25))
            .inner_margin(Margin::same(4.0))
            .outer_margin(Margin::symmetric(4.0, 1.0))
            .rounding(Rounding::same(5.0))
            .stroke(Stroke::new(1.0, Color32::from_additive_luminance(50)));
    
        let column_frame = Frame::default()
            .fill(Color32::from_rgb(15, 15, 19))
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
                        let create_task = task_headers(s, column_names.clone(), header_frame);

                        if let Some(response) = create_task{
                            if response.clicked(){
                                info!("Creating task!");
                                self.create_task_modal = true;
                            }
                        }
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
                        self.task_columns(     
                            s,           
                            filters,
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
    fn task_columns(
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
    ){
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
                                            task.display_task_cards(ui, database.clone(), &store_users.as_ref()).unwrap();
                                        }
                                        
                                    }
                                });
                            });
                        });
                    });
                }
            }else{
                let mut filtered = self.filter_items(
                    &filters,&None,&None,&priority,&complete
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
    
                let mut filtered = self.filter_items(
                    &user_filters,
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
    
    fn filter_items(
        &mut self,
        filters: &Vec<Filters>, 
        assignee: &Option<User>,
        status: &Option<Status>,
        priority: &Option<Priority>,
        complete: &Option<bool>,
    ) -> Vec<TaskPayload>{
        
        filters.into_iter().fold(self.task_opts.as_ref().unwrap().tasks.to_owned(), |acc_tasks, filter| {
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
}


// impl ColumnLayout for Vec<TaskPayload> {
//     fn setup_display(
//         &mut self,
//         ui: &mut egui::Ui, 
//         column_names: Vec<String>, 
//         database: Database,
//         filters: &Vec<Filters>, 
//         assignees: &Option<Vec<User>>,
//         status: bool,
//         priority: &Option<Priority>,
//         complete: &Option<bool>,
//         current_user: &Option<User>
//     ) {
//         ui.style_mut().visuals.window_rounding = Rounding::same(5.0);
//         let header_frame = Frame::default()
//             .fill(Color32::from_rgb(20, 20, 25))
//             .inner_margin(Margin::same(4.0))
//             .outer_margin(Margin::symmetric(4.0, 1.0))
//             .rounding(Rounding::same(5.0))
//             .stroke(Stroke::new(1.0, Color32::from_additive_luminance(50)));   
//         let column_frame = Frame::default()
//             .fill(Color32::from_rgb(15, 15, 19))
//             .inner_margin(Margin::same(8.0))
//             .rounding(Rounding::same(10.0))
//             .stroke(Stroke::new(1.0, Color32::from_additive_luminance(50))); 
//         let column_width = Size::exact(450.0);
//         ScrollArea::horizontal()
//             .hscroll(true)
//             .show_viewport(ui, |ui, _|
//         {
//             StripBuilder::new(ui)
//                 .cell_layout(Layout::top_down_justified(egui::Align::Center))
//                 .size(Size::relative(0.01))
//                 .size(Size::relative(0.07))
//                 .size(Size::relative(0.92))
//                 .vertical(|mut strip| 
//             {
//                 strip
//                     .strip(|strip| 
//                 {
//                     strip
//                         .sizes(column_width, column_names.len())
//                         .horizontal( |s| 
//                     {
//                         let create_task = task_headers(s, column_names.clone(), header_frame);
//                         if let Some(response) = create_task{
//                             if response.clicked(){
//                                 info!("Creating task!");
//                             }
//                         }
//                     });
//                 });
//                 strip.empty();
//                 strip
//                     .strip(|strip| 
//                 {
//                     strip
//                         .sizes(column_width, column_names.len())
//                         .horizontal( |s| 
//                     {
//                         self.task_columns(     
//                             s,           
//                             filters,
//                             &assignees,
//                             status,
//                             &priority,
//                             &complete,
//                             current_user,
//                             database,
//                             column_frame
//                         );
//                     });
//                 });
//             });
//         });
//     }
//     fn task_columns(
//         &mut self,
//         mut s: Strip, 
//         filters: &Vec<Filters>, 
//         assignees: &Option<Vec<User>>,
//         status: bool,
//         priority: &Option<Priority>,
//         complete: &Option<bool>,
//         current_user: &Option<User>,
//         database: Database,
//         column_frame: Frame,
//     ){
//         if let Some(_) = current_user {
//             if status{
//                 for status in Status::VALUES{
//                     let mut filtered = self.filter_items(
//                         &filters,&None,&Some(status),&priority,&complete
//                     );               
//                     s.cell(|ui| {
//                         column_frame.show(ui, |ui| {
//                             ui.vertical_centered_justified(|ui| {
//                                 ScrollArea::vertical()
//                                 .auto_shrink(false)
//                                 .show_viewport(ui, |ui, _| {
//                                     for task in filtered.iter_mut() {
//                                         if let Some(store_users) = &assignees{
//                                             task.display_task_cards(ui, database.clone(), &store_users.as_ref()).unwrap();
//                                         }                                     
//                                     }
//                                 });
//                             });
//                         });
//                     });
//                 }
//             }else{
//                 let mut filtered = self.filter_items(
//                     &filters,&None,&None,&priority,&complete
//                 );           
//                 s.cell(|ui| {
//                     column_frame.show(ui, |ui| {
//                         ui.vertical_centered_justified(|ui| {
//                             ScrollArea::vertical()
//                             .auto_shrink(false)
//                             .show_viewport(ui, |ui, _| {
//                                 for task in filtered.iter_mut() {
//                                     task.display_task_cards(ui, database.clone(), &assignees.as_ref().unwrap()).unwrap();
//                                 }
//                             });
//                         });
//                     });
//                 });
//             }
//         } else if let Some(users) = assignees {
//             // Multiple users, iterate over each user
//             for user in users.iter() {
//                 let mut user_filters = filters.clone();
//                 user_filters.push(Filters::FilterAssignee); 
//                 let mut filtered = self.filter_items(
//                     &user_filters,
//                     &Some(user.clone()),
//                     &None,
//                     &priority,
//                     &complete,
//                 );
//                 s.cell(|ui| {
//                     column_frame.show(ui, |ui| {
//                         ui.vertical_centered_justified(|ui| {
//                             ScrollArea::vertical()
//                                 .auto_shrink(false)
//                                 .show_viewport(ui, |ui, _| 
//                             {
//                                 for task in filtered.iter_mut() {
//                                     task.display_task_cards(ui, database.clone(), &assignees.as_ref().unwrap()).unwrap();
//                                 }
//                             });
//                         });
//                     });
//                 });
//             }
//         }
//     }
//     fn filter_items(
//         &mut self,
//         filters: &Vec<Filters>, 
//         assignee: &Option<User>,
//         status: &Option<Status>,
//         priority: &Option<Priority>,
//         complete: &Option<bool>,
//     ) -> Self{  
//         filters.into_iter().fold(self.to_owned(), |acc_tasks, filter| {
//             match filter {
//                 Filters::FilterAssignee => {
//                     if let Some(ref user) = assignee {
//                         acc_tasks.filter_by_assignee(user)
//                     } else {
//                         acc_tasks
//                     }
//                 },
//                 Filters::FilterCompleted => {
//                     if let Some(complete) = complete {
//                         acc_tasks.filter_by_completed(*complete)
//                     } else {
//                         acc_tasks
//                     }
//                 },
//                 Filters::FilterStatus => {
//                     if let Some(status) = status {
//                         acc_tasks.filter_by_status(&status)
//                     } else {
//                         acc_tasks
//                     }
//                 },
//                 Filters::FilterPriority => {
//                     if let Some(ref priority) = priority {
//                         acc_tasks.filter_by_priority(&priority)
//                     } else {
//                         acc_tasks
//                     }
//                 },
//             }
//         })
//     }
// }


pub fn task_headers(
    mut s: Strip,
    column_names: Vec<String>,
    header_frame: Frame,
) -> Option<Response>{
    let mut response: Option<Response> = None;
    for name in &column_names{
        s.cell(|ui|{
            header_frame.show(ui, |ui|
            {
                ui.horizontal_top(|ui| 
                {
                    ui.with_layout(Layout::left_to_right(egui::Align::Center), |ui| 
                    {
                        ui.vertical_centered(|ui|{
                            ui.colored_label(Color32::WHITE, RichText::new(name.clone()).heading());
                        });
                    });

                    ui.with_layout(Layout::right_to_left(egui::Align::Max), |ui| 
                    {
                        response = Some(
                            Button::new(
                            RichText::new("✚")
                                .raised()
                                .color(Color32::LIGHT_RED)
                            )
                            .fill(Color32::TRANSPARENT)
                            .ui(ui)
                        );
                        // info!("clicked + button");
                    });
                });
            });
        });
    }
    match response{
        Some(res) => Some(res),
        None => None,
    }
}