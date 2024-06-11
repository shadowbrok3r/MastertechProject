use std::borrow::BorrowMut;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use database::Database;
use egui::{Align, Button, FontId, RichText, ScrollArea, TextEdit, Vec2, Widget};
use egui::{Color32, Frame, Layout, Margin, Rounding, Stroke};
use egui_autocomplete::AutoCompleteTextEdit;
use egui_extras::{Size, Strip, StripBuilder};
use database::schema::{TaskPayload, User};
use log::info;
use crate::utilities::{ColumnLayout, Displayable, FilterTasks, Sortable, TaskUiActions};
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
        ui.style_mut().visuals.window_rounding = Rounding::same(10.0);
        
        let column_width = Size::exact(450.0);
    
        ScrollArea::horizontal()
            .show_viewport(ui, |ui, _|
        {
            let x: f32 = ui.available_height() - 40.0;
            StripBuilder::new(ui)
                .cell_layout(Layout::top_down_justified(egui::Align::Center))
                .size(Size::exact(30.0))
                .size(Size::exact(5.0))
                .size(Size::exact(x))
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
            .fill(Color32::from_rgb(12, 12, 14))
            .inner_margin(Margin::same(8.0))
            .rounding(Rounding::same(10.0))
            .stroke(Stroke::new(1.0, Color32::from_additive_luminance(50)));

        let mut inputs = BTreeSet::new();



        for (name, mut tasks) in filter_items {
            tasks.sort_task_payloads();
            for task in tasks.iter(){
                inputs.insert(task.task_name.clone());
                inputs.insert(format!("{}",task.service_number.unwrap_or(0)));
            }
            s.cell(|ui| {
                column_frame.show(ui, |ui| {
                    ui.vertical_centered_justified(|ui| {
                        ScrollArea::vertical()
                            .auto_shrink(false)
                            .show_viewport(ui, |ui, _| 
                        {
                            let search_input = self.search_inputs.get(&name).cloned().unwrap_or_default();
                            if !search_input.is_empty(){
                                for mut task in tasks.filter_by_task_name(inputs.clone(), search_input.clone()){
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
                            }else{
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
                            }
                        });
                    });
                });
            });
        }
    }
    

    fn task_headers(&mut self, mut s: Strip, items: &HashMap<String, Vec<TaskPayload>>){
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
                        ui.with_layout(Layout::left_to_right(Align::Min), |ui| 
                        {
                            let search_input = self.search_inputs.entry(name.clone()).or_insert_with(String::new);
                            TextEdit::singleline(search_input)
                                .hint_text("Search for task")
                                .desired_width(100.0)
                                .font(FontId::proportional(14.0))
                                .ui(ui);

                            ui.add_space(ui.available_width() / 3.4);
                            ui.colored_label(Color32::WHITE, RichText::new(name.to_owned()).heading());
                        });
    
                        
                        ui.with_layout(Layout::right_to_left(Align::Max), |ui| 
                        {
                            let button = Button::new(
                                RichText::new("✚")
                                    .raised()
                                    .color(Color32::LIGHT_RED)
                                )
                                .fill(Color32::TRANSPARENT)
                                .min_size(Vec2::new(30.0, 20.0))
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
