use crate::{app_state::MtechServerContext, utilities::Displayable};
use egui::{Color32, Frame, Margin, RichText, Rounding, Stroke, Direction, Layout, ScrollArea, Sense, Ui, Vec2};
use egui_extras::{Size, StripBuilder};

impl MtechServerContext{
    pub fn store_tasks(&mut self, ui: &mut Ui) {

        if let Some(tasks) = &mut self.store_tasks{
            self.store_tasks_opened = true;

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

            
                ScrollArea::horizontal()
                // .auto_shrink(false)
                .min_scrolled_width(ui.available_width())
                
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
                            .sizes(Size::remainder(), self.store_users.as_ref().unwrap().len())
                            .horizontal( |mut s| 
                        {
                            for name in self.store_users.as_ref().unwrap(){
                                s.cell(|ui|{
                                    frame.show(ui, |ui|{
                                        ui.vertical_centered_justified(|ui|{
                                            // ui.allocate_space(ui.available_size_before_wrap());
                                            ui.colored_label(Color32::WHITE, RichText::new(&name.everest_initials).heading());
                                        });
                                    });
                                });
                            }
                        });
                    });
                    strip.empty();
                    strip
                        .strip(|strip| 
                    {
                        strip
                            .sizes(Size::remainder(), self.store_users.as_ref().unwrap().len())
                            .horizontal( |mut s| 
                        {
                            for name in self.store_users.as_ref().unwrap(){
                                let filtered_tasks: Vec<_> = tasks.iter_mut().filter(
                                    |task| 
                                        *task.inner().assignee_initials.as_ref().unwrap() == *name.everest_initials
                                ).collect();
                                s.cell(|ui|{
                                    column_frame.show(ui, |ui|{
                                        ui.vertical_centered_justified(|ui|
                                        {
                                            ScrollArea::vertical()
                                                .auto_shrink(false)
                                                .show_viewport(ui, |ui, _|
                                            {
                                                for task in filtered_tasks {
                                                    task.display_task_cards(ui).unwrap();
                                                }
                                            });
                                        });
                                    });
                                });
                                
                            }
                        });
                    });
                });
            });
        
        }
    }
}
