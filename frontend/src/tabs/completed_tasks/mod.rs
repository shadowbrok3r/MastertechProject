use crate::{app_state::MtechServerContext, utilities::{listen_tasks::get_my_tasks, task_functions::Displayable}};
use egui::{Align, Direction, Layout, Ui};
use log::info;

impl MtechServerContext{
    pub fn completed_tasks(&mut self, ui: &mut Ui){ 
        ui.horizontal(|ui|{ui.add_space(8.0);});

        if let Some(tasks) = &mut self.completed_tasks{
            self.completed_tasks_opened = true;
            ui.with_layout(Layout::from_main_dir_and_cross_align(Direction::TopDown, Align::Min).with_main_wrap(true), |ui| {
                for task_data in tasks {
                    let _ = task_data.display_task_cards(ui).unwrap();
                }
            });
        }
    }
}