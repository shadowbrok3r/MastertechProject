use std::collections::HashMap;
use crossbeam::channel::Sender;
use database::Database;
use database::schema::{Priority, TaskPayload, User};
use log::info;
use serde::Serialize;
use std::borrow::BorrowMut;
use std::collections::BTreeSet;
use chrono::{DateTime, Utc};
use egui::{Align, Button, FontId, RichText, ScrollArea, TextEdit, Vec2, Widget};
use egui::{Color32, Frame, Layout, Margin, Rounding, Stroke};
use egui_extras::{Size, Strip, StripBuilder};
use crate::utilities::{ColumnLayout, Displayable, FilterTasks, Sortable, TaskUiActions};

pub struct SortTasks{
    pub sort_by_status: bool,
    pub sort_by_priority: Option<Priority>,
    pub sort_by_complete: Option<bool>,
    pub sort_by_current_user: Option<User> 
}


#[derive(Serialize)]
pub struct TaskLayout{
    pub search_inputs: HashMap<String, String>,
    pub task_map: HashMap<String, Vec<TaskPayload>>,
    pub column_names: Vec<String>,
    #[serde(skip)]
    pub database: Database,
    #[serde(skip)]
    pub ui_actions_tx: Sender<TaskUiActions>,
    pub assignees: Option<Vec<User>>
}

impl TaskLayout { 
    pub fn new(
        task_map: HashMap<String, Vec<TaskPayload>>,
        column_names: Vec<String>,
        database: Database,
        ui_actions_tx: Sender<TaskUiActions>,
        assignees: Option<Vec<User>>,
    ) -> Self {
        Self {  task_map, column_names, database, ui_actions_tx, search_inputs: HashMap::new(), assignees }
    }

    pub fn update_tasks(&mut self,task_map: HashMap<String, Vec<TaskPayload>>, column_names: Vec<String>) {
        self.task_map = task_map;
        self.column_names = column_names;
    }
}

impl ColumnLayout for TaskLayout {
    fn layout_cols(
        &mut self,
        ui: &mut egui::Ui
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
                        .sizes(column_width, self.column_names.len())
                        .horizontal( |strip| self.headers(strip));
                });
                strip.empty();
                strip
                    .strip(|strip| 
                {
                    strip
                        .sizes(column_width, self.column_names.len())
                        .horizontal( |mut strip| 
                    {
                        self.columns(
                            strip.borrow_mut(),
                        );
                    });
                });
            });
        });
    }

    fn columns(
        &mut self,
        s: &mut Strip,
    ) {
        let column_frame = Frame::default()
            .fill(Color32::from_rgb(12, 12, 14))
            .inner_margin(Margin::same(8.0))
            .rounding(Rounding::same(10.0))
            .stroke(Stroke::new(1.0, Color32::from_additive_luminance(50)));

        let mut inputs = BTreeSet::new();

        for (name, tasks) in self.task_map.iter_mut() {
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
                            let search_input = self.search_inputs.get(name).cloned().unwrap_or_default();
                            if !search_input.is_empty(){
                                for mut task in tasks.filter_by_task_name(inputs.clone(), search_input.clone()){
                                    if let Some(store_users) = &self.assignees {
                                        let action = task.display_cards(ui, self.database.to_owned(), &store_users.as_ref());
                                        if let Some(action) = action{
                                            match action{
                                                TaskUiActions::OpenTaskModal(task) => {
                                                    let _ = self.ui_actions_tx.try_send(TaskUiActions::OpenTaskModal(task));
                                                },
                                                _ => ()
                                            }
                                        }
                                    }
                                }
                            }else{
                                for task in tasks {
                                    if let Some(store_users) = &self.assignees {
                                        let action = task.display_cards(ui, self.database.to_owned(), &store_users.as_ref());
                                        if let Some(action) = action{
                                            match action{
                                                TaskUiActions::OpenTaskModal(task) => {
                                                    let _ = self.ui_actions_tx.try_send(TaskUiActions::OpenTaskModal(task));
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
    

    fn headers(&mut self, mut s: Strip){
        let header_frame = Frame::default()
            .fill(Color32::from_rgb(20, 20, 25))
            .inner_margin(Margin::same(4.0))
            .outer_margin(Margin::symmetric(4.0, 1.0))
            .rounding(Rounding::same(5.0))
            .stroke(Stroke::new(1.0, Color32::from_additive_luminance(50)));

        for (name, tasks) in self.task_map.iter(){
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

                            ui.add_space(30.0);

                            if button.clicked(){
                                let _ = self.ui_actions_tx.try_send(TaskUiActions::CreateTaskModal);
                            }

                            let mut count = 0;
                            let current_date = Utc::now().date_naive();
                            for task in tasks{
                                let due_date = DateTime::parse_from_rfc3339(&task.due_date)
                                    .expect("Invalid date format")
                                    .with_timezone(&Utc)
                                    .date_naive();

                                if due_date < current_date {
                                    count += 1;
                                }
                            }
                            if count > 0{
                                ui.label("Overdue Tasks");
                                ui.add_space(5.0);
                                ui.colored_label(Color32::RED, format!("{count}"));
                            }
                        });
                    });
                });
            });
        }
    }

}