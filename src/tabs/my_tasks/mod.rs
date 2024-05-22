use crate::{app_state::MtechServerContext, database::methods::Displayable};
use egui::{vec2, Layout, ScrollArea, Ui, Vec2};
use log::info;

impl MtechServerContext{
    pub fn my_tasks(&mut self, ui: &mut Ui){ 
        ui.style_mut().spacing.button_padding = (4.0, 7.0).into();
        ui.vertical(|ui|{ui.add_space(8.0);});
        ui.horizontal(|ui|{ui.add_space(8.0);});
        ui.set_min_width(400.0);

        if let Some(tasks) = &mut self.task_data{
            
            // ui.allocate_ui(
            //     Vec2::new(ui.available_size_before_wrap().x, ui.available_size_before_wrap().y),
            //     |ui| 
            // {
                
                // info!("RECT X//Y: {:?}// {:?}", ui.available_size_before_wrap().x, ui.available_size_before_wrap().y);
                // info!("SIZE X//Y: {:?}// {:?}", ui.available_size().x, ui.available_size().y);
            ScrollArea::vertical()
                .hscroll(false)
                .show(ui, |ui| 
            {

                
                ui
                    .horizontal_wrapped(|ui| 
                {
                    
                    info!("SIZE X//Y: {:?}// {:?}", ui.available_size().x, ui.available_size().y);
                    // let max_rect = ui.max_rect();
                    // info!("Max rect: {:?}", max_rect);
                
                    ui.with_layout(Layout::left_to_right(egui::Align::Min).with_main_wrap(true), |ui| {
                        for task_data in tasks {
                            let _ = task_data.display_task_cards(ui).unwrap();
                        }
                    });
                });
            });
        }
    }
}