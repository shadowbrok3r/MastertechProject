use std::collections::{BTreeMap, HashMap};
use crossbeam::channel::Sender;
use database::DATABASE;
use database::schema::{Priority, Record, TaskPayload, User};
use log::info;
use serde::Serialize;
use surrealdb::sql::Id;
use wasm_bindgen_futures::spawn_local;
use std::borrow::BorrowMut;
use std::collections::BTreeSet;
use chrono::{DateTime, Utc};
use eframe::egui::{popup_below_widget, Align, Button, Color32, Frame, Layout, Margin, PopupCloseBehavior, RichText, Rounding, ScrollArea, Stroke, TextEdit, Ui, Vec2, Widget};
use egui_extras::{Size, Strip, StripBuilder};
use crate::utilities::{ColumnLayout, Displayable, FilterTasks, Sortable, TaskUiActions};

// use super::sub_menu::sub_menu;

pub struct SortTasks{
    pub sort_by_status: bool,
    pub sort_by_priority: Option<Priority>,
    pub sort_by_complete: Option<bool>,
    pub sort_by_current_user: Option<User> 
}


#[derive(Serialize)]
pub struct TaskLayout{
    pub search_inputs: HashMap<String, String>,
    pub task_map: BTreeMap<String, Vec<TaskPayload>>,
    pub column_names: Vec<String>,
    #[serde(skip)]
    // pub database: Database,
    // #[serde(skip)]
    pub ui_actions_tx: Sender<TaskUiActions>,
    pub assignees: Option<Vec<User>>,

    pub open_menu: bool,
}

impl TaskLayout { 
    pub fn new(
        task_map: BTreeMap<String, Vec<TaskPayload>>,
        column_names: Vec<String>,
        ui_actions_tx: Sender<TaskUiActions>,
        assignees: Option<Vec<User>>,
    ) -> Self {
        Self {  task_map, column_names, ui_actions_tx, search_inputs: HashMap::new(), assignees, open_menu: false }
    }

    pub fn update_tasks(&mut self, task_map: BTreeMap<String, Vec<TaskPayload>>) -> &mut Self {
        self.task_map = task_map;
        self
    }

    pub fn update_assignees(&mut self, assignees: Option<Vec<User>>) -> &mut Self {
        self.assignees = assignees;
        self
    }
    pub fn update_col_names(&mut self, column_names: Vec<String>) -> &mut Self {
        self.column_names = column_names;
        self
    }
}

impl ColumnLayout for TaskLayout {
    fn layout_cols(
        &mut self,
        ui: &mut Ui
    ){
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
            .stroke(Stroke::new(1.0, Color32::from_additive_luminance(70)));

        let mut inputs = BTreeSet::new();

        for (name, tasks) in self.task_map.iter_mut() {
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
                            .show_viewport(ui, |ui, _| 
                        {
                            let search_input = self.search_inputs.get(name).cloned().unwrap_or_default();
                            if !search_input.is_empty(){
                                for mut task in tasks.filter_by_task_name(inputs.clone(), search_input.clone()){
                                    if let Some(store_users) = &self.assignees {
                                        let action = task.display_cards(ui, &store_users.as_ref());
                                        if let Some(action) = action{
                                            match action{
                                                TaskUiActions::OpenTaskModal(task) => {
                                                    let _ = self.ui_actions_tx.try_send(TaskUiActions::OpenTaskModal(task));
                                                },
                                                TaskUiActions::OpenChatModal(chat_details) => {
                                                    let _ = self.ui_actions_tx.try_send(TaskUiActions::OpenChatModal(chat_details));
                                                    info!("Opening chat");
                                                },
                                                _ => {}
                                            }
                                        }
                                    }
                                }
                            }else{
                                for task in tasks {
                                    if let Some(store_users) = &self.assignees {
                                        let action = task.display_cards(ui, &store_users.as_ref());
                                        if let Some(action) = action{
                                            match action{
                                                TaskUiActions::OpenTaskModal(task) => {
                                                    let _ = self.ui_actions_tx.try_send(TaskUiActions::OpenTaskModal(task));
                                                },
                                                TaskUiActions::OpenChatModal(chat_details) => {
                                                    let _ = self.ui_actions_tx.try_send(TaskUiActions::OpenChatModal(chat_details));
                                                    info!("Opening chat");
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
            .fill(Color32::from_rgb(13, 13, 15))
            .inner_margin(Margin::same(4.0))
            .outer_margin(Margin::symmetric(4.0, 1.0))
            .rounding(Rounding::same(5.0))
            .stroke(Stroke::new(0.4, Color32::WHITE));
        // ui.style_mut().spacing.button_padding.y = 4.0;
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
                            
                            TextEdit::singleline(search_input)
                                .hint_text("Search")
                                .desired_width(100.0)
                                .margin(margin).ui(ui);

                            ui.add_space(ui.available_width() / 3.4);
                            
                            let response = Button::new(RichText::new(name.to_owned())
                                    .color(Color32::from_rgb(191, 33, 101))
                                    .size(13.0).monospace()
                                ).fill(Color32::from_rgb(22,22,22)).rounding(Rounding::same(2.)).min_size(Vec2::new(60.0, 15.0)).ui(ui);

                            if response.clicked(){
                                ui.memory_mut(|mem| mem.open_popup(format!("sub_menu-{:?}",name).into()));
                            }
                            
                            let res = popup_below_widget(ui, format!("sub_menu-{:?}",name).into(), &response, PopupCloseBehavior::CloseOnClickOutside, |ui| {
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
                                match action{
                                    TaskActions::MarkComplete => {
                                        let id: Vec<String> = tasks.iter().map(|t| t.id.clone().unwrap().0.id.to_string()).collect::<Vec<String>>();
                                        
                                        info!("ids: {:?}", id);
                                        spawn_local(async move {
                                            let _x: Vec<Record> = DATABASE.query("fn::mark_all_completion($ids, $completion)")
                                                .bind(("ids", id))
                                                .bind(("completion", true))
                                                .await.unwrap().take(0).unwrap();
                                        });
                                    },
                                    TaskActions::MarkIncomplete => {
                                        let id = tasks.iter().map(|t| t.id.clone().unwrap().0.id).collect::<Vec<Id>>();
                                        
                                        spawn_local(async move {
                                            let _x: Vec<Record> = DATABASE.query("fn::mark_all_completion($ids, $completion)")
                                                .bind(("ids", id))
                                                .bind(("completion", false))
                                                .await.unwrap().take(0).unwrap();
                                        });
                                    },
                                    TaskActions::MarkDueToday => {
                                        let id = tasks.iter().map(|t| t.id.clone().unwrap().0.id).collect::<Vec<Id>>();
                                        
                                        spawn_local(async move {
                                            let query = "fn::mark_all_completion($ids, $completion)";
                                            let _ = DATABASE.set("ids", id);
                                            let _ = DATABASE.set("completion", true);

                                            let _x: Vec<Record> = DATABASE.query(query).await.unwrap().take(0).unwrap();
                                        });
                                    }, _ => {}
                                }
                            }
                            
                            // ui.colored_label(Color32::WHITE, RichText::new(name.to_owned()).heading());
                        });
                        
                        ui.with_layout(Layout::right_to_left(Align::Max), |ui| 
                        {
                            let button = Button::new(
                                RichText::new("✚")
                                    .color(Color32::from_rgb(191, 33, 101))
                                )
                                // .fill(Color32::TRANSPARENT)
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
                                let due_date = DateTime::parse_from_rfc3339(&task.due_date)
                                    .expect("Invalid date format")
                                    .with_timezone(&Utc)
                                    .date_naive();

                                if due_date < current_date {
                                    count += 1;
                                }
                            }
                            if count > 0{
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

}

pub enum TaskActions{
    MarkComplete,
    MarkIncomplete,
    MarkDueToday,
    None
}