use eframe::egui::{popup_below_widget, Align, Button, Color32, ComboBox, Frame, Layout, Margin, PopupCloseBehavior, RichText, Rounding, ScrollArea, Spinner, Style, TextEdit, Ui, Vec2, Widget};
use crate::{Displayable, FilterTasks, SortDirection, Sortable, TaskUiActions};
use database::schema::{Record, TaskPayload, User};
use egui_extras::{Size, Strip, StripBuilder};
use std::collections::{BTreeMap, HashMap};
use crossbeam::channel::Sender;
use std::collections::BTreeSet;
use database::{self, DATABASE};
use structdiff::Difference;
use std::borrow::BorrowMut;
use chrono::{DateTime, Utc};
use structdiff::StructDiff;
use surrealdb::RecordId;
use std::sync::Arc;
use log::info;
use crate::{PlatformSpawner, Spawner};

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
    pub task: Option<String>,
    #[difference(skip)]
    pub ui_actions_tx: Sender<TaskUiActions>,
    #[difference(skip)]
    pub sort_by: HashMap<String, SortOptions>,
    #[difference(skip)]
    pub last_sort_field: Option<SortField>,    
    pub loading: bool
}


#[derive(Clone, Default, PartialEq)]
pub struct SortOptions {
    pub field: SortField,
    pub direction: SortDirection,
}

#[derive(Clone, Default, PartialEq)]
pub enum SortField {
    #[default]
    Default,
    Date,
    Name,
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
            task: None,
            sort_by: HashMap::new(),
            last_sort_field: None,
            loading: false
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
        ui.style_mut().visuals.window_rounding = ui.style().visuals.window_rounding;
        let column_width = Size::exact(450.0);
        let x: f32 = ui.available_height() / 1.1;
        let style = ui.style().clone();
        ScrollArea::horizontal()
            .show_viewport(ui, |ui, _|
        {
            
            StripBuilder::new(ui)
                .cell_layout(Layout::top_down_justified(Align::Center))
                .size(Size::exact(30.0))
                .size(Size::exact(5.0))
                .size(Size::exact(x))
                .vertical(|mut strip| 
            {
                // if self.column_names.len() > 0 {
                    strip.strip(|strip| 
                    {
                        strip.sizes(column_width, self.column_names.len())
                            .horizontal(
                                |strip| self.headers(strip, style.clone())
                            );
                    });

                    strip.empty();

                    strip.strip(|strip| 
                    {
                        strip.sizes(column_width, self.column_names.len())
                            .horizontal(
                                |mut strip| self.columns(strip.borrow_mut(), style.clone())
                            );
                    });
                // } else { 
                //     strip.cell(|ui| {
                //         ui.label("Loading..");
                //         Spinner::new().size(50.).color(Color32::from_rgb(150, 10, 150)).ui(ui);
                //     });
                //  }
            });
        });
    }

    pub fn begin_edit(&mut self, task_id: &String) -> Option<&mut TaskPayload>{
        info!("Finding ID: {task_id:?}");
        // Search for the task by ID
        for (_, tasks) in self.task_map.iter_mut(){
            for task in tasks.iter_mut(){
                if task.id.key().to_string() == *task_id{
                    info!("Got a match");
                    return Some(task);
                }
            }
        }
        None
    }

    fn headers(&mut self, mut s: Strip, style: Arc<Style>){
        let header_frame = Frame::default()
            .fill(style.visuals.window_fill) // (Color32::from_rgb(13, 13, 15))
            .inner_margin(Margin::same(4.0))
            .outer_margin(Margin::symmetric(8.0, 1.0))
            .rounding(style.visuals.window_rounding)
            .stroke(style.visuals.window_stroke);
        // if self.task_map.keys().len() == self.column_names.len() {
        let mut idx = 0;
            for (name, tasks) in self.task_map.iter() {
                idx += 1;
                s.cell(|ui|{
                    if ui.available_width() < 10.0 || ui.available_height() < 10.0 {
                        info!("First strip avail size: {:?}", ui.available_size());
                    }
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

                                ui.add_space(15.);
                                
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
                                    ui.small("Overdue");
                                    ui.add_space(5.0);
                                    ui.colored_label(Color32::DARK_RED, RichText::new(format!("{count}")).small());
                                }
                                ui.add_space(15.);
                                let response = Button::new(RichText::new(name.to_owned())
                                        .color(style.visuals.warn_fg_color)
                                        .size(13.0).monospace()
                                    )
                                    .fill(style.visuals.noninteractive().bg_fill)
                                    .rounding(Rounding::same(2.))
                                    .min_size(Vec2::new(60.0, 15.0))
                                    .ui(ui);

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
                                    let ids = tasks.iter().map(|t| t.id.clone()).collect::<Vec<RecordId>>();
                                    match action{
                                        TaskActions::MarkComplete => {
                                            PlatformSpawner::spawn(async move {
                                                for id in ids{
                                                    let _x: Option<Record> = DATABASE.query("fn::mark_all_completion($record, $completion)")
                                                        .bind(("record", id.clone()))
                                                        .bind(("completion", true))
                                                        .await.unwrap().take(0).unwrap();
                                                }
                                            });
                                        },
                                        TaskActions::MarkIncomplete => {
                                            PlatformSpawner::spawn(async move {
                                                for id in ids{
                                                    let _x: Option<Record> = DATABASE.query("fn::mark_all_completion($record, $completion)")
                                                        .bind(("record", id.clone()))
                                                        .bind(("completion", false))
                                                        .await.unwrap().take(0).unwrap();
                                                }
                                            });
                                        },
                                        TaskActions::MarkDueToday => {
                                            PlatformSpawner::spawn(async move {
                                                for id in ids{
                                                    let query = "fn::mark_all_due_today($id)";
                                                    info!("ID: {:?}", id.clone());
                                                    let _ = DATABASE.set("id", id).await.unwrap();
                                                    let _x: Option<Record> = DATABASE.query(query).await.unwrap().take(0).unwrap();
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
                                        .color(style.visuals.warn_fg_color)
                                    )
                                    .rounding(Rounding::same(2.))
                                    .fill(Color32::from_rgb(22,22,22))
                                    .min_size(Vec2::new(30.0, 15.0))
                                    .ui(ui);

                                ui.add_space(20.0);

                                if button.clicked(){
                                    let _ = self.ui_actions_tx.try_send(TaskUiActions::CreateTaskModal);
                                }

                                let selected = self.sort_by.entry(name.clone()).or_default();
                                let txt = match selected.direction {
                                    SortDirection::Asc => ("↗", ui.style().visuals.window_stroke.color),
                                    SortDirection::Desc => ("↘", ui.style().visuals.error_fg_color),
                                };
                                let selected_text = match selected.field {
                                    SortField::Default => RichText::new(format!("Priority {}", txt.0)).color(txt.1).small(),
                                    SortField::Date => RichText::new(format!("Date {}", txt.0)).color(txt.1).small(),
                                    SortField::Name => RichText::new(format!("Name {}", txt.0)).color(txt.1).small(),
                                };
                                ComboBox::new(format!("SortBy for {name:?}-{idx}"), "")
                                    .selected_text(selected_text)
                                    .width(70.)
                                    .show_ui(ui, |ui| {
                                        if ui.selectable_value(
                                            &mut selected.field, 
                                            SortField::Default, 
                                            RichText::new(format!("Priority {}", txt.0)).color(txt.1).small())
                                        .clicked() {
                                            if let Some(last_field) = self.last_sort_field.clone() {
                                                if last_field == SortField::Default {
                                                    // Toggle the direction if the same field is clicked again
                                                    selected.direction = match selected.direction {
                                                        SortDirection::Asc => SortDirection::Desc,
                                                        SortDirection::Desc => SortDirection::Asc,
                                                    };
                                                }
                                            }
                                            // Update the last selected field
                                            self.last_sort_field = Some(SortField::Default);
                                        }
                                        if ui.selectable_value(
                                            &mut selected.field, 
                                            SortField::Name, 
                                            RichText::new(format!("Name {}", txt.0)).color(txt.1).small())
                                        .clicked() {
                                            if let Some(last_field) = self.last_sort_field.clone() {
                                                if last_field == SortField::Name {
                                                    // Toggle the direction if the same field is clicked again
                                                    selected.direction = match selected.direction {
                                                        SortDirection::Asc => SortDirection::Desc,
                                                        SortDirection::Desc => SortDirection::Asc,
                                                    };
                                                }
                                            }
                                            // Update the last selected field
                                            self.last_sort_field = Some(SortField::Name);
                                        }
                                        if ui.selectable_value(
                                            &mut selected.field, 
                                            SortField::Date, 
                                            RichText::new(format!("Date {}", txt.0)).color(txt.1).small())
                                        .clicked() {
                                            if let Some(last_field) = self.last_sort_field.clone() {
                                                if last_field == SortField::Date {
                                                    // Toggle the direction if the same field is clicked again
                                                    selected.direction = match selected.direction {
                                                        SortDirection::Asc => SortDirection::Desc,
                                                        SortDirection::Desc => SortDirection::Asc,
                                                    };
                                                }
                                            }
                                            // Update the last selected field
                                            self.last_sort_field = Some(SortField::Date);
                                        }
                                });
                            });
                        });
                    });
                });
            }
        // }
    }

    fn columns(&mut self, s: &mut Strip, style: Arc<Style>) {
        let column_frame = Frame::default()
            .fill(style.visuals.window_fill) // (Color32::from_rgb(12, 12, 14))
            .inner_margin(Margin::same(6.0))
            .rounding(style.visuals.menu_rounding)
            .stroke(style.visuals.window_stroke);

        let mut inputs = BTreeSet::new();

        // if self.task_map.keys().len() == self.column_names.len() {
            // if self.task_map.iter().map(|m| m.1.iter().map(|i| i.))
        for (name, tasks) in self.task_map.iter_mut(){
            let sort_by = self.sort_by.entry(name.clone()).or_default();
            let direction = &sort_by.direction;
            match sort_by.field {
                SortField::Default => tasks.default_sort(),
                SortField::Date => tasks.sort_by_date(direction.clone()),
                SortField::Name => tasks.sort_by_name(direction.clone()),
            };
            
            for task in tasks.iter(){
                inputs.insert(task.task_name.clone());
                inputs.insert(format!("{}",task.service_number.clone().unwrap_or_default()));
            }

            s.cell(|ui| {
                if ui.available_width() < 10.0 || ui.available_height() < 10.0 {
                    info!("Col strip avail size: {:?}", ui.available_size());
                }
                column_frame.show(ui, |ui| {
                    ui.vertical_centered_justified(|ui| {
                        let row_height = 140.;
                        let total_rows = tasks.len(); 
                        let scroll_area = ScrollArea::vertical().auto_shrink(false);
                        ui.ctx().options_mut(|o| o.line_scroll_speed = 15.0);
                        scroll_area.show_rows(ui, row_height, total_rows, |ui, row_range| {
                            // ui.scroll_with_delta(Vec2::new(0.0, 300.));
                            // Retrieve search input for the current context, or default to an empty string.
                            let search_input = self.search_inputs.get(name).cloned().unwrap_or_default();

                            // Filter tasks based on search input.
                            let mut filtered_tasks: Vec<TaskPayload> = if !search_input.is_empty() {
                                tasks.filter_by_task_name(inputs.clone(), search_input.clone())                                
                            } else {
                                tasks.iter().cloned().collect()
                            };

                            // Iterate only over the rows in the current viewport range.
                            for row in row_range {
                                if !search_input.is_empty() {
                                    ui.scroll_to_cursor(Some(Align::BOTTOM));
                                }
                                if let Some(task) = filtered_tasks.get_mut(row) {
                                    // info!("Store users: {:?}", self.assignees);
                                    task.display_cards(ui, &self.assignees, self.ui_actions_tx.clone());
                                }
                            }
                            if self.loading {
                                ui.vertical_centered(|ui| {
                                    ui.label("Loading..");
                                    Spinner::new().size(50.).color(Color32::from_rgb(150, 10, 150)).ui(ui)
                                });
                            }                   
                        });
                    });
                });
            });
        }
        // }
    }
}
pub enum TaskActions{
    MarkComplete,
    MarkIncomplete,
    MarkDueToday,
    None
}
