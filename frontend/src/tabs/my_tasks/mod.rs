use crate::{app_state::MtechServerContext, utilities::Displayable};
use database::schema::Status;
use egui_extras::{Column, Size, StripBuilder, TableBuilder};
use egui::{Color32, Direction, Frame, Layout, Margin, RichText, Rounding, ScrollArea, Sense, Stroke, Ui, Vec2};
use log::info;

impl MtechServerContext{
    pub fn my_tasks(&mut self, ui: &mut Ui){ 
        ui.horizontal(|ui|{ui.add_space(8.0);});

        info!("MyTasks is open");

        if let Some(tasks) = &mut self.my_tasks{
            self.my_tasks_opened = true;

            // let col = Column::auto().at_least(400.0);

            let mut todo_tasks = Vec::new();
            let mut inrepair_tasks = Vec::new();
            let mut complete_tasks = Vec::new();
    
            for task in tasks {
                match task.status {
                    Status::Todo => todo_tasks.push(task),
                    Status::InRepair => inrepair_tasks.push(task),
                    Status::Complete => complete_tasks.push(task),
                }
            }

            let max_rows = std::cmp::max(
                todo_tasks.len(), 
                std::cmp::max(
                    inrepair_tasks.len(), 
                    complete_tasks.len()
                )
            );

            ui.style_mut().visuals.window_rounding = Rounding::same(5.0);
            let frame = Frame::default()
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
            //     .show(ui, |ui| 
            // {
            StripBuilder::new(ui)
                .cell_layout(Layout::top_down_justified(egui::Align::Center))
                .size(Size::relative(0.01))
                .size(Size::relative(0.07))
                .size(Size::relative(0.92))
                // .size(Size::relative(0.1))
                .vertical(|mut strip| 
            {
                strip
                    .strip(|strip| 
                {
                    strip
                        .cell_layout(Layout::left_to_right(egui::Align::Min))
                        .cell_layout(Layout::left_to_right(egui::Align::Center))
                        .cell_layout(Layout::left_to_right(egui::Align::Max))
                        .size(Size::relative(0.33))
                        .size(Size::relative(0.33))
                        .size(Size::relative(0.33))
                        .horizontal( |mut s| 
                    {
                        s.cell(|ui|{
                            frame.show(ui, |ui|{
                                ui.vertical_centered_justified(|ui|{

                                    ui.allocate_space(ui.available_size_before_wrap());
                                    ui.colored_label(Color32::WHITE, RichText::new("Todo").heading());
                                });
                            });
                        });
                        s.cell(|ui|{
                            frame.show(ui, |ui|{
                                ui.vertical_centered_justified(|ui|{

                                    ui.allocate_space(ui.available_size_before_wrap());
                                    ui.colored_label(Color32::WHITE, RichText::new("Todo").heading());
                                });
                            });
                        });
                        s.cell(|ui|{
                            frame.show(ui, |ui|{
                                ui.vertical_centered_justified(|ui|{

                                    ui.allocate_space(ui.available_size_before_wrap());
                                    ui.colored_label(Color32::WHITE, RichText::new("Todo").heading());
                                });
                            });
                        });
                    });
                });
                strip.empty();
                strip
                    .strip(|strip| 
                {
                    strip
                        .cell_layout(Layout::left_to_right(egui::Align::Min))
                        .cell_layout(Layout::left_to_right(egui::Align::Center))
                        .cell_layout(Layout::left_to_right(egui::Align::Max))
                        .size(Size::relative(0.33))
                        .size(Size::relative(0.33))
                        .size(Size::relative(0.33))
                        .horizontal( |mut s| 
                    {
                        s.cell(|ui|{
                            column_frame.show(ui, |ui|{
                                ui.vertical_centered_justified(|ui|
                                {
                                    // let vec = Vec2::new(ui.available_size_before_wrap().x, ui.available_size_before_wrap().y - 10.0);
                                    // ui.allocate_at_least(vec, Sense::click());
                                    ScrollArea::vertical()
                                        .auto_shrink(false)
                                        .show_rows(ui, 210.0, max_rows, |ui, rows|
                                    {
                                        for row_index in rows{
                                            if let Some(task) = todo_tasks.get_mut(row_index) {
                                                let database = self.database.as_ref().unwrap().clone();
                                                let store_users = self.store_users.as_ref().unwrap();
                                                task.display_task_cards(ui, database, store_users).unwrap();
                                            }
                                        }
                                    });
                                });
                            });
                        });
                        s.cell(|ui|{
                            column_frame.show(ui, |ui|{
                                ui.vertical_centered_justified(|ui|{
                                    ScrollArea::vertical()
                                        .auto_shrink(false)
                                        .show_rows(ui, 210.0, max_rows, |ui, rows|
                                    {
                                        for row_index in rows{
                                            if let Some(task) = inrepair_tasks.get_mut(row_index) {
                                                if let Some(users) = &self.store_users{
                                                    let database = self.database.as_ref().unwrap().clone();
                                                    let store_users = users;
                                                    task.display_task_cards(ui, database, &store_users).unwrap();
                                                }

                                            }
                                        }
                                    });
                                });
                            });
                        });
                        s.cell(|ui|{
                            column_frame.show(ui, |ui|{
                                ui.vertical_centered_justified(|ui|{
                                    ScrollArea::vertical()
                                        .auto_shrink(false)
                                        .show_rows(ui, 210.0, max_rows, |ui, rows|
                                    {
                                        for row_index in rows{
                                            if let Some(task) = complete_tasks.get_mut(row_index) {
                                                if let Some(users) = &self.store_users{
                                                    let database = self.database.as_ref().unwrap().clone();
                                                    let store_users = users;
                                                    task.display_task_cards(ui, database, &store_users).unwrap();
                                                }

                                            }
                                        }
                                    });
                                });
                            });
                        });
                    });
                });
            });
        }
    }
}


            //     TableBuilder::new(ui)
            //         .striped(true)
            //         .cell_layout(Layout::centered_and_justified(Direction::TopDown))
            //         .columns(col, 3)
            //         .header(10.0, |mut header| 
            //     {
            //         header.col(|ui| {
            //             ui.vertical_centered_justified(|ui|{
            //                 frame.show(ui, |ui|{
            //                     ui.colored_label(Color32::WHITE, RichText::new("Todo").heading());
            //                 });
            //             });
            //         });
            //         header.col(|ui| {
            //             ui.vertical_centered_justified(|ui|{
            //                 frame.show(ui, |ui|{
            //                     ui.colored_label(Color32::WHITE, RichText::new("In Repair").heading());
            //                 });
            //             });
            //         });
            //         header.col(|ui| {
            //             ui.vertical_centered_justified(|ui|{
            //                 frame.show(ui, |ui|{
            //                     ui.colored_label(Color32::WHITE, RichText::new("Complete").heading());
            //                 });
            //             });
            //         });
            //     })
            //     .body(|body| 
            //     {
            //         body.rows(200.0, at_least_rows, |mut row| {
            //             for row_index in 0..max_rows {
            //                 row.col(|ui| {
            //                     if let Some(task) = todo_tasks.get_mut(row_index) {
            //                         task.display_task_cards(ui).unwrap();
            //                     }
            //                 });
            //                 row.col(|ui| {
            //                     if let Some(task) = inrepair_tasks.get_mut(row_index) {
            //                         task.display_task_cards(ui).unwrap();
            //                     }
            //                 });
            //                 row.col(|ui| {
            //                     if let Some(task) = complete_tasks.get_mut(row_index) {
            //                         task.display_task_cards(ui).unwrap();
            //                     }
            //                 });
            //             }
            // });