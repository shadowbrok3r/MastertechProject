use crate::{app_state::MtechServerContext, utilities::task_functions::Displayable};
use egui::{Align, Direction, Layout, Ui};

impl MtechServerContext{
    pub fn my_tasks(&mut self, ui: &mut Ui){ 
        ui.horizontal(|ui|{ui.add_space(8.0);});
        // ui.set_min_width(400.0);
        // ui.set_max_size(vec2(900.0, 400.0));

        if let Some(tasks) = &mut self.task_data{
            ui.with_layout(Layout::from_main_dir_and_cross_align(Direction::TopDown, Align::Min).with_main_wrap(true), |ui| {
                for task_data in tasks {
                    let _ = task_data.display_task_cards(ui).unwrap();
                }
            });
        }
    }
}