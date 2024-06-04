use std::borrow::BorrowMut;
use std::collections::HashMap;

use database::Database;
use egui::{Align, Button, RichText, ScrollArea, Widget};
use egui::{Color32, Frame, Layout, Margin, Rounding, Stroke};
use egui_extras::{Size, Strip, StripBuilder};
use database::schema::{TaskPayload, User};
use crate::utilities::{ColumnLayout, Displayable, Sortable, TaskUiActions};
use super::task_layout::TaskLayout;


impl ColumnLayout for TaskLayout {
    fn layout_task_cols(
        &mut self,
        ui: &mut egui::Ui, 
        column_names: Vec<String>, 
        database: Database,
        assignees: &Option<Vec<User>>,
        filter_items: HashMap<String, Vec<TaskPayload>>
    ){
        ui.style_mut().visuals.window_rounding = Rounding::same(5.0);
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
                        .horizontal( |strip| self.task_headers(strip, &filter_items));
                });
                strip.empty();
                strip
                    .strip(|strip| 
                {
                    strip
                        .sizes(column_width, column_names.len())
                        .horizontal( |mut strip| 
                    {
                        self.task_columns(
                            strip.borrow_mut(),
                            assignees,
                            database.to_owned(),
                            filter_items,
                        );
                    });
                });
            });
        });
    }

    fn task_columns(
        &self,
        s: &mut Strip, 
        assignees: &Option<Vec<User>>,
        database: Database,
        filter_items: HashMap<String, Vec<TaskPayload>>
    ) {
        let column_frame = Frame::default()
            .fill(Color32::from_rgb(15, 15, 19))
            .inner_margin(Margin::same(8.0))
            .rounding(Rounding::same(10.0))
            .stroke(Stroke::new(1.0, Color32::from_additive_luminance(50)));

        for (_, mut tasks) in filter_items {
            tasks.sort_task_payloads();
            s.cell(|ui| {
                column_frame.show(ui, |ui| {
                    ui.vertical_centered_justified(|ui| {
                        ScrollArea::vertical()
                            .auto_shrink(false)
                            .show_viewport(ui, |ui, _| 
                        {
                            for mut task in tasks {
                                if let Some(store_users) = &assignees {
                                    let action = task.display_task_cards(ui, database.to_owned(), &store_users.as_ref());
                                    if let Some(action) = action{
                                        match action{
                                            TaskUiActions::OpenTaskModal(task) => {
                                                let _ = self.ui_actions_tx.send(TaskUiActions::OpenTaskModal(task));
                                            },
                                            _ => ()
                                        }
                                    }
                                }
                            }
                        });
                    });
                });
            });
        }
    }
    

    fn task_headers(
        &self,
        mut s: Strip,
        items: &HashMap<String, Vec<TaskPayload>>
    ){
        let header_frame = Frame::default()
            .fill(Color32::from_rgb(20, 20, 25))
            .inner_margin(Margin::same(4.0))
            .outer_margin(Margin::symmetric(4.0, 1.0))
            .rounding(Rounding::same(5.0))
            .stroke(Stroke::new(1.0, Color32::from_additive_luminance(50)));

        for (name, _) in items.iter(){
            s.cell(|ui|{
                header_frame.show(ui, |ui|
                {
                    ui.horizontal_top(|ui| 
                    {
                        ui.with_layout(Layout::left_to_right(Align::Center), |ui| 
                        {
                            ui.vertical_centered(|ui|{
                                ui.colored_label(Color32::WHITE, RichText::new(name.to_owned()).heading());
                            });
                        });
    
                        ui.with_layout(Layout::right_to_left(Align::Max), |ui| 
                        {
                            let button = Button::new(
                                RichText::new("✚")
                                    .raised()
                                    .color(Color32::LIGHT_RED)
                                )
                                .fill(Color32::TRANSPARENT)
                                .ui(ui);
                            if button.clicked(){
                                let _ = self.ui_actions_tx.send(TaskUiActions::CreateTaskModal);
                            }
                        });
                    });
                });
            });
        }
    }
}
