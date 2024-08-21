use std::collections::{BTreeMap, HashMap};
use crossbeam::channel::Sender;
use database::{self, DATABASE};
use database::schema::{Priority, Record, TaskPayload, User};
use log::info;
use surrealdb::sql::{Id, Thing};
use wasm_bindgen_futures::spawn_local;
use std::borrow::BorrowMut;
use std::collections::BTreeSet;
use chrono::{DateTime, Utc};
use eframe::egui::{popup_below_widget, Align, Button, Color32, Frame, Layout, Margin, PopupCloseBehavior, RichText, Rounding, ScrollArea, Stroke, TextEdit, Ui, Vec2, Widget};
use egui_extras::{Size, Strip, StripBuilder};
use crate::utilities::{FilterTasks, Sortable, TaskUiActions, Displayable};
use structdiff::StructDiff;

// use super::sub_menu::sub_menu;

pub struct SortTasks{
    pub sort_by_status: bool,
    pub sort_by_priority: Option<Priority>,
    pub sort_by_complete: Option<bool>,
    pub sort_by_current_user: Option<User> 
}


#[derive(Difference)]
pub struct TaskLayout{
    #[difference(skip)]
    pub search_inputs: HashMap<String, String>,
    #[difference(collection_strategy = "unordered_map_like", map_equality = "key_and_value")]
    pub task_map: BTreeMap<String, Vec<TaskPayload>>,
    #[difference(collection_strategy="ordered_array_like")]
    pub column_names: Vec<String>,
    pub assignees: Vec<User>,
    pub open_menu: bool,
    #[difference(skip)]
    pub action: TaskUiActions,
    pub task: Option<Id>,
    #[difference(skip)]
    pub ui_actions_tx: Sender<TaskUiActions>,
}

impl TaskLayout { 
    pub fn new(task_map: BTreeMap<String, Vec<TaskPayload>>, column_names: Vec<String>, ui_actions_tx: Sender<TaskUiActions>, assignees: Vec<User>) -> Self 
    {
        Self {  
            task_map, 
            column_names, 
            ui_actions_tx, 
            assignees, 
            search_inputs: HashMap::new(), 
            open_menu: false, 
            action: TaskUiActions::None,
            task: None
        }
    }

    pub fn update_assignees(&mut self, assignees: Vec<User>) -> &mut Self {
        self.assignees = assignees;
        self
    }

    pub fn update_tasks(&mut self, new_map: BTreeMap<String, Vec<TaskPayload>>) -> &mut Self {
        for (key, new_payloads) in new_map.into_iter() {
            if let Some(existing_payloads) = self.task_map.get_mut(&key) {
                // Ensure we have the same length of vectors, or handle mismatches
                for (existing, new) in existing_payloads.iter_mut().zip(new_payloads.iter()) {
                    // Compute the diffs between the existing and new payloads
                    let diffs = existing.diff(&new);
                    // Apply the diffs to the existing payload
                    existing.apply_mut(diffs);
                }
                
                // If new_payloads has more items than existing_payloads, add them
                if new_payloads.len() > existing_payloads.len() {
                    existing_payloads.extend(new_payloads[existing_payloads.len()..].iter().cloned());
                }
            } else {
                // Insert new key and its associated task payloads if it does not exist
                self.task_map.insert(key, new_payloads);
            }
        }
        self
    }

    pub fn update_col_names(&mut self, column_names: Vec<String>) -> &mut Self {
        self.column_names = column_names;
        self
    }

    pub fn layout_cols(&mut self, ui: &mut Ui) {
        ui.style_mut().visuals.window_rounding = Rounding::same(10.0);
        let column_width = Size::exact(450.0);
        
        ScrollArea::horizontal()
            .show_viewport(ui, |ui, _|
        {
            let x: f32 = ui.available_height() - 40.0;
            StripBuilder::new(ui)
                .cell_layout(Layout::top_down_justified(Align::Center))
                .size(Size::exact(30.0))
                .size(Size::exact(5.0))
                .size(Size::exact(x))
                .vertical(|mut strip| 
            {
                strip.strip(|strip| 
                {
                    strip.sizes(column_width, self.column_names.len()).horizontal( |strip| self.headers(strip));
                });
                
                strip.empty();
                
                strip.strip(|strip| 
                {
                    strip.sizes(column_width, self.column_names.len()).horizontal( |mut strip| 
                    {
                        // for (name, tasks) in self.task_map.iter_mut() {
                            self.columns(strip.borrow_mut());
                        // }
                    });
                });
            });
        });
    }

    pub fn begin_edit(&mut self, task_id: &Id) -> Option<&mut TaskPayload>{
        info!("Finding ID: {task_id:?}");
        // Search for the task by ID
        for (_, tasks) in self.task_map.iter_mut(){
            for task in tasks.iter_mut(){
                if task.id.as_ref().unwrap().0.id == *task_id{
                    info!("Got a match");
                    return Some(task);
                }
            }
        }
        None
    }

    fn headers(&mut self, mut s: Strip){
        let header_frame = Frame::default()
            .fill(Color32::from_rgb(13, 13, 15))
            .inner_margin(Margin::same(4.0))
            .outer_margin(Margin::symmetric(8.0, 1.0))
            .rounding(Rounding::same(5.0))
            .stroke(Stroke::new(0.4, Color32::WHITE));

        for (name, tasks) in self.task_map.iter(){
            s.cell(|ui|{
                header_frame.show(ui, |ui|
                {
                    ui.horizontal_top(|ui| 
                    {
                        ui.with_layout(Layout::left_to_right(Align::Min), |ui| 
                        {
                            let search_input = self.search_inputs.entry(name.clone()).or_insert_with(String::new);
                            let mut margin = Margin::default();
                            margin.top = 6.0;
                            margin.left = 4.0;
                            
                            TextEdit::singleline(search_input).hint_text("Search").desired_width(100.0).margin(margin).ui(ui);

                            ui.add_space(ui.available_width() / 3.4);
                            
                            let response = Button::new(RichText::new(name.to_owned())
                                    .color(Color32::from_rgb(191, 33, 101))
                                    .size(13.0).monospace()
                                ).fill(Color32::from_rgb(22,22,22)).rounding(Rounding::same(2.)).min_size(Vec2::new(60.0, 15.0)).ui(ui);

                            if response.clicked(){
                                ui.memory_mut(|mem| mem.open_popup(format!("sub_menu-{:?}",name).into()));
                            }
                            
                            let res = popup_below_widget(
                                ui, 
                                format!("sub_menu-{:?}",name).into(), 
                                &response, 
                                PopupCloseBehavior::CloseOnClickOutside, 
                                |ui| 
                            {
                                ui.vertical_centered_justified(|ui| {
                                    ui.set_width(200.0);
                                    if ui.button("Mark all Complete").clicked(){
                                        return TaskActions::MarkComplete;
                                    }
                                    ui.add_space(5.0);
                                    if ui.button("Mark all Incomplete").clicked(){
                                        return TaskActions::MarkIncomplete;
                                    }
                                    ui.add_space(5.0);
                                    if ui.button("Mark all Due Today").clicked(){
                                        return TaskActions::MarkDueToday;
                                    }
                                    TaskActions::None
                                }).inner
                            });

                            if let Some(action) = res{
                                let ids = tasks.iter().map(|t| t.id.clone().unwrap().0).collect::<Vec<Thing>>();
                                match action{
                                    TaskActions::MarkComplete => {
                                        spawn_local(async move {
                                            for id in ids{
                                                let _x: Option<Record> = DATABASE.query("fn::mark_all_completion($record, $completion)")
                                                    .bind(("record", id.clone()))
                                                    .bind(("completion", true))
                                                    .await.unwrap().take(0).unwrap();
                                            }
                                        });
                                    },
                                    TaskActions::MarkIncomplete => {
                                        
                                        spawn_local(async move {
                                            for id in ids{
                                                let _x: Option<Record> = DATABASE.query("fn::mark_all_completion($record, $completion)")
                                                    .bind(("record", id.clone()))
                                                    .bind(("completion", false))
                                                    .await.unwrap().take(0).unwrap();
                                            }
                                        });
                                    },
                                    TaskActions::MarkDueToday => {
                                        spawn_local(async move {
                                            for id in ids{
                                                let query = "fn::mark_all_due_today($id)";
                                                info!("ID: {:?}", id.clone());
                                                let _ = DATABASE.set("id", id).await.unwrap();
                                                match DATABASE.query(query).await{
                                                    Ok(query_res) => {
                                                        match query_res.take(0){
                                                            Some(res) => info!("There was a record: {res:?}"),
                                                            None => info!("There was no record")
                                                        }
                                                    },
                                                    Err(e) => info!("Error {e:?}")
                                                }
                                            }
                                        });
                                    }, _ => {}
                                }
                            }
                        });
                        
                        ui.with_layout(Layout::right_to_left(Align::Max), |ui| 
                        {
                            let button = Button::new(
                                RichText::new("✚")
                                    .color(Color32::from_rgb(191, 33, 101))
                                )
                                .rounding(Rounding::same(2.))
                                .fill(Color32::from_rgb(22,22,22))
                                .min_size(Vec2::new(30.0, 15.0))
                                .ui(ui);

                            ui.add_space(30.0);

                            if button.clicked(){
                                let _ = self.ui_actions_tx.try_send(TaskUiActions::CreateTaskModal);
                            }

                            let mut count = 0;
                            let current_date = Utc::now().date_naive();
                            for task in tasks{
                                let due_date = DateTime::parse_from_rfc3339(&task.due_date);
                                if let Ok(date) = due_date {
                                    let date = date.with_timezone(&Utc).date_naive();
                                    if date < current_date && !task.completed{ count += 1; }
                                }
                            }
                            if count > 0 {
                                ui.label("Overdue");
                                ui.add_space(5.0);
                                ui.colored_label(Color32::DARK_RED, format!("{count}"));
                            }
                        });
                    });
                });
            });
        }
    }

    fn columns(&mut self, s: &mut Strip) {
        let column_frame = Frame::default()
            .fill(Color32::from_rgb(12, 12, 14))
            .inner_margin(Margin::same(8.0))
            .rounding(Rounding::same(10.0))
            .stroke(Stroke::new(1.0, Color32::from_additive_luminance(70)));

        let mut inputs = BTreeSet::new();

        for (name, tasks) in self.task_map.iter_mut(){
            tasks.sort_task_payloads();
            for task in tasks.iter(){
                inputs.insert(task.task_name.clone());
                inputs.insert(format!("{}",task.service_number.clone().unwrap_or_default()));
            }

            s.cell(|ui| {
                column_frame.show(ui, |ui| {
                    ui.vertical_centered_justified(|ui| {
                        ScrollArea::vertical()
                            .auto_shrink(false)
                            .show_rows(ui, 80.0, tasks.len(), |ui, _range|
                        {
                            // info!("Row {}/{}", row + 1, total_rows);
                            let search_input = self.search_inputs.get(name).cloned().unwrap_or_default();
                            if !search_input.is_empty(){
                                for mut task in tasks.filter_by_task_name(inputs.clone(), search_input.clone()){
                                    task.display_cards(ui, &self.assignees, self.ui_actions_tx.clone());
                                    // if let Some(action) = action{
                                    //     self.action = action.clone();
                                    //     self.ui_actions_tx.try_send(action).unwrap();
                                    // }
                                }
                            }else{
                                for task in &mut *tasks {
                                    task.display_cards(ui, &self.assignees, self.ui_actions_tx.clone());
                                    // if let Some(action) = action{
                                    //     // if !TaskUiActions::None = action{
                                    //         self.action = action.clone();
                                    //         info!("self.action {:?}", self.action.clone());
                                    //         self.ui_actions_tx.try_send(action).unwrap();
                                    //     // }
                                    // }
                                }
                            }
                        });
                    });
                });
            });
        }
    }
}
pub enum TaskActions{
    MarkComplete,
    MarkIncomplete,
    MarkDueToday,
    None
}