
use database::Database;
use eframe::egui::Ui;
use egui::ScrollArea;
use egui::{Color32, Frame, Layout, Margin, RichText, Rounding, Stroke};
use egui_extras::{Size, StripBuilder};
use database::schema::{User, TaskPayload};
use log::info;

use super::{Displayable, FilterTasks};
use super::Interaction;


impl Displayable for TaskPayload{
    fn display_task_cards(&mut self, ui: &mut Ui, database: Database, store_users: &Vec<User>)  -> anyhow::Result<(), anyhow::Error> {

        ui.style_mut().visuals.selection.stroke.color = Color32::from_additive_luminance(255);
        ui.style_mut().visuals.widgets.hovered.bg_stroke = Stroke::new(2.0, Color32::from_rgb(200, 20, 200));
        ui.style_mut().visuals.widgets.inactive.bg_fill = Color32::from_additive_luminance(255);
        ui.style_mut().visuals.widgets.hovered.expansion = 2.0;

        let frame = Frame::default()
            .fill(Color32::from_rgb(7, 7, 13))
            .inner_margin(Margin::same(4.0))
            .outer_margin(Margin::same(10.0))
            .rounding(Rounding::same(5.0))
            .stroke(Stroke::new(1.0, Color32::from_additive_luminance(200)));

        frame.show(ui, |ui| {
            ui.set_max_height(160.0);
            ui.set_width(370.0);

            StripBuilder::new(ui)
                .cell_layout(Layout::top_down_justified(egui::Align::Center))
                .size(Size::relative(0.1))
                .size(Size::relative(0.8))
                .size(Size::relative(0.1))
                .vertical(|mut strip| {
                    strip.strip(|strip| {
                        strip
                            .cell_layout(Layout::left_to_right(egui::Align::Min))
                            .cell_layout(Layout::left_to_right(egui::Align::Center))
                            .cell_layout(Layout::left_to_right(egui::Align::Max))
                            .size(Size::relative(0.2))
                            .size(Size::remainder())
                            .size(Size::relative(0.2))
                            .horizontal( |mut s| 
                        {
                            s.cell(|ui|{
                                ui.with_layout(Layout::centered_and_justified(egui::Direction::TopDown), |ui|{
                                    // if self.interact_assignee_initials(ui, database.clone(), store_users).unwrap().changed(){
                                    //     info!("interact_assignee_initials changed: {:?}// {:?}", self.id, self.task_name);
                                    // }
                                    
                                });
                            });
                            s.cell(|ui|{
                                ui.with_layout(Layout::centered_and_justified(egui::Direction::TopDown), |ui|{
                                    self.interact_task_name(ui, database.clone());
                                });
                            });
                            s.cell(|ui|{
                                ui.with_layout(Layout::centered_and_justified(egui::Direction::TopDown), |ui|{
                                    self.interact_completed(ui, database.clone());
                                });
                            });
                        });
                    });

                    strip.strip(|strip| {
                        strip
                            .cell_layout(Layout::left_to_right(egui::Align::Min))
                            .cell_layout(Layout::right_to_left(egui::Align::Max))
                            .size(Size::remainder())
                            .size(Size::remainder())
                            .horizontal( |mut s| 
                        {
                            s.cell(|ui|{
                                ui.with_layout(Layout::centered_and_justified(egui::Direction::TopDown), |ui|{
                                    self.interact_task_description(ui, database.clone());
                                });
                            });
                            s.cell(|ui|{
                                ui.with_layout(Layout::centered_and_justified(egui::Direction::TopDown), |ui|{
                                    self.interact_task_description(ui, database.clone());
                                });
                            });
                        });
                    });
                    strip.strip(|strip| {
                        strip
                            .cell_layout(Layout::left_to_right(egui::Align::Min))
                            .cell_layout(Layout::left_to_right(egui::Align::Center))
                            .cell_layout(Layout::left_to_right(egui::Align::Max))
                            .size(Size::relative(0.3))
                            .size(Size::remainder())
                            .size(Size::relative(0.3))
                            .horizontal( |mut s| 
                        {
                            s.cell(|ui|{
                                ui.with_layout(Layout::centered_and_justified(egui::Direction::TopDown), |ui|{
                                    self.interact_due_date(ui, database.clone());
                                });
                            });
                            s.cell(|ui|{
                                ui.with_layout(Layout::centered_and_justified(egui::Direction::TopDown), |ui|{
                                    self.interact_priority(ui, database.clone());
                                });
                            });
                            s.cell(|ui|{
                                ui.with_layout(Layout::centered_and_justified(egui::Direction::TopDown), |ui|{
                                    if self.interact_status(ui, database).unwrap().changed(){
                                        info!("interact_status changed: {:?}// {:?}", self.id, self.task_name);
                                    }
                                });
                            });
                        });
                    });
            });
        });
        
        /* 
            let header_id = ui.make_persistent_id(&task_data.task_name);
            CollapsingState::load_with_default_open(ui.ctx(), header_id, false)
            .show_header(ui, |ui| {
                ui.toggle_value(&mut stuff, &task_data.task_name);
            })
            .body_unindented(|ui| {
                ui.label("The body is always custom");
            });
        */
        Ok(())
    }

    // fn display_table(&mut self, ui: &mut Ui, tasks: Vec<TaskPayload>) -> anyhow::Result<(), anyhow::Error> {Ok(())}
}

// // pub fn setup_display(&mut self, ui: &mut egui::Ui, column_names: Vec<String>) {
pub fn setup_display(
    ui: &mut egui::Ui, 
    column_names: Vec<String>, 
    tasks: &mut Vec<TaskPayload>, 
    database: Database,
    store_users: &Vec<User>
) {
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
                    .sizes(Size::remainder(), column_names.len())
                    .horizontal( |mut s| 
                {
                    for name in &column_names{
                        s.cell(|ui|{
                            frame.show(ui, |ui|{
                                ui.vertical_centered_justified(|ui|{
                                    ui.colored_label(Color32::WHITE, RichText::new(name.clone()).heading());
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
                    .sizes(Size::remainder(), column_names.len())
                    .horizontal( |mut s| 
                {
                    for name in &column_names{
                        // let mut filtered = tasks
                        //     .filter_by_completed(false)
                        //     .filter_by_assignee(&name.clone());
                        

                        s.cell(|ui|{
                            column_frame.show(ui, |ui|{
                                ui.vertical_centered_justified(|ui|
                                {
                                    ScrollArea::vertical()
                                        .auto_shrink(false)
                                        .show_viewport(ui, |ui, _|
                                    {
                                        for task in tasks.iter_mut() {
                                            task.display_task_cards(ui, database.clone(), store_users).unwrap();
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