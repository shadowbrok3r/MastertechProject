use crate::{app_state::MtechServerContext, utilities::{get_tasks::get_my_tasks, update_tasks::Displayable}};
use database::schema::Status;
use egui_extras::{Column, Size, TableBuilder};
use egui::{Align, Color32, Direction, Layout, RichText, Ui};
use log::info;

impl MtechServerContext{
    pub fn my_tasks(&mut self, ui: &mut Ui){ 
        ui.horizontal(|ui|{ui.add_space(8.0);});

        if let Some(tasks) = &mut self.my_tasks{
            self.my_tasks_opened = true;
            ui.with_layout(Layout::from_main_dir_and_cross_align(Direction::TopDown, Align::Min)
            .with_main_wrap(true), |ui| {
                let col = Column::auto().at_least(400.0);
                let mut todo_tasks = Vec::new();
                let mut inrepair_tasks = Vec::new();
                let mut complete_tasks = Vec::new();
        
                for task in tasks {
                    match task.inner().status {
                        Status::Todo => todo_tasks.push(task),
                        Status::InRepair => inrepair_tasks.push(task),
                        Status::Complete => complete_tasks.push(task),
                    }
                }
        
                let max_rows = std::cmp::max(todo_tasks.len(), std::cmp::max(inrepair_tasks.len(), complete_tasks.len()));
                TableBuilder::new(ui)
                    .striped(false)
                    .columns(col, 3)
                    .header(10.0, |mut header| 
                {
                    header.col(|ui| {
                        ui.vertical_centered_justified(|ui|{
                            ui.colored_label(Color32::WHITE, RichText::new("Todo"));
                        });
                    });
                    header.col(|ui| {
                        ui.vertical_centered_justified(|ui|{
                            ui.colored_label(Color32::WHITE, RichText::new("In Repair"));
                        });
                    });
                    header.col(|ui| {
                        ui.vertical_centered_justified(|ui|{
                            ui.colored_label(Color32::WHITE, RichText::new("Complete"));
                        });
                    });
                })
                .body(|body| 
                {
                    body.rows(200.0, max_rows, |mut row| {
                        for row_index in 0..max_rows {
                            row.col(|ui| {
                                
                                if let Some(task) = todo_tasks.get_mut(row_index) {
                                    task.display_task_cards(ui).unwrap();
                                }
                            });
                            row.col(|ui| {
                                if let Some(task) = inrepair_tasks.get_mut(row_index) {
                                    task.display_task_cards(ui).unwrap();
                                }
                            });
                            row.col(|ui| {
                                if let Some(task) = complete_tasks.get_mut(row_index) {
                                    task.display_task_cards(ui).unwrap();
                                }
                            });
                        }
                    });
                });
            });
        }
    }
}