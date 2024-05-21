use egui::Ui;
use log::info;

use crate::{app_state::MtechServerContext, utilities::task_layout::TaskLayout};


impl MtechServerContext{
    pub fn my_tasks(&mut self, ui: &mut Ui){ 
        ui.style_mut().spacing.button_padding = (4.0, 7.0).into();
        ui.shrink_width_to_current();
        ui.shrink_height_to_current();
        ui.vertical(|ui|{ui.add_space(8.0);});
        ui.horizontal(|ui|{ui.add_space(8.0);});

        // info!("No tasks found: {:?}", &self.task_data);
        if let Some(tasks) = &self.task_data{
            // info!("Found tasks");
            let task_layout = TaskLayout::default();
            let _ = task_layout.task_card(tasks, ui);
        }
    }
}