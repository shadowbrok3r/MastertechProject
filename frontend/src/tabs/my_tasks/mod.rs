use crate::{app_state::MtechServerContext, utilities::{display_tasks::{setup_display, Filters}, Sortable}};
use egui::Ui;
use log::info;

impl MtechServerContext{
    pub fn my_tasks(&mut self, ui: &mut Ui){ 
        ui.horizontal(|ui|{ui.add_space(8.0);});

        if let Some(tasks) = &mut self.my_tasks{
            self.my_tasks_opened = true;
            let col_names = vec!["Todo".to_string(), "In Repair".to_string(), "Complete".to_string()];
            let database = self.database.as_ref().unwrap().clone();
            tasks.sort_task_payloads();

            let filters = vec![
                Filters::FilterStatus
            ];

            setup_display(ui, 
                col_names, 
                &mut *tasks, 
                database, 
                &filters, 
                &self.store_users,
                true,
                &None,
                &None,
                &self.current_user
            );
        }
    }
}

/*
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
                                    ui.colored_label(Color32::WHITE, RichText::new("In Repair").heading());
                                });
                            });
                        });
                        s.cell(|ui|{
                            frame.show(ui, |ui|{
                                ui.vertical_centered_justified(|ui|{

                                    ui.allocate_space(ui.available_size_before_wrap());
                                    ui.colored_label(Color32::WHITE, RichText::new("Complete").heading());
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
*/