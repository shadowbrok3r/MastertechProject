use eframe::egui::{popup_below_widget, Align, Button, Color32, ComboBox, Frame, Layout, Margin, NumExt, PopupCloseBehavior, RichText, ScrollArea, Spinner, TextEdit, Ui, Vec2, Widget};
use database::schema::{LiveTaskPayload, Record, Store, TaskPayload, User};
use crate::{Displayable, SortDirection, Sortable, TaskUiActions};
use std::collections::{BTreeMap, HashMap};
use crate::get_current_user_from_auth;
use crossbeam::channel::Sender;
use database::{self, DATABASE};
use std::collections::BTreeSet;
use serde::Deserialize;
use surrealdb::RecordId;
use serde::Serialize;
use chrono::Utc;
use crate::{PlatformSpawner, Spawner};

#[derive(Serialize, Deserialize)]
pub struct TaskLayout{
    pub search_inputs: HashMap<String, String>,
    pub task_map: BTreeMap<String, Vec<TaskPayload>>,
    pub column_names: Vec<String>,
    pub assignees: Vec<User>,
    pub open_menu: bool,
    pub action: TaskUiActions,
    pub task: Option<String>,
    pub sort_by: HashMap<String, SortOptions>,
    pub last_sort_field: Option<SortField>,    
    pub loading: bool,
    new_status: String,
    user: User,
    search_results: Option<Vec<TaskPayload>>, // Add search results
}

pub struct LayoutConfig {
    // Valid keys for task_map (statuses or username)
    pub valid_keys: Vec<String>,
    pub key_provider: Box<dyn Fn(&[User]) -> Vec<String>>, // Generate keys from store_users
    pub filter: Box<dyn Fn(&LiveTaskPayload, &Option<User>, &[User], &Store) -> bool>,
    // Whether to call update_assignees
    pub update_assignees: bool,
}

#[derive(Clone, Default, PartialEq, Serialize, Debug, Deserialize)]
pub struct SortOptions {
    pub field: SortField,
    pub direction: SortDirection,
}

#[derive(Clone, Default, PartialEq, Serialize, Debug, Deserialize)]
pub enum SortField {
    #[default]
    Default,
    Date,
    Name,
}

impl TaskLayout { 
    const COL_W: f32 = 450.0; // <- single source of truth
    const HEADER_H: f32 = 48.0;          // rough pixel height of the header frame
    
    pub fn new(
        task_map: BTreeMap<String, Vec<TaskPayload>>, 
        column_names: Vec<String>,
        assignees: Vec<User>,
        search_results: Option<Vec<TaskPayload>>, // Add parameter
    ) -> Self  {
        Self {  
            task_map, 
            column_names,
            assignees, 
            search_inputs: HashMap::new(), 
            open_menu: false, 
            action: TaskUiActions::None,
            task: None,
            sort_by: HashMap::new(),
            last_sort_field: None,
            loading: false,
            new_status: String::new(),
            user: get_current_user_from_auth().unwrap_or_default(),
            search_results,
        }
    }

    pub fn update_assignees(&mut self, assignees: Vec<User>) -> &mut Self {
        self.assignees = assignees;
        self
    }

    pub fn update_col_names(&mut self, column_names: Vec<String>) -> &mut Self {
        self.column_names = column_names;
        self
    }
    
    pub fn layout_cols(&mut self, ui: &mut Ui, ui_actions_tx: Sender<TaskUiActions>) {
        ui.style_mut().visuals.window_corner_radius = ui.style().visuals.window_corner_radius;
        let style = ui.style().clone();
        let mut inputs = BTreeSet::new();

        //-----------------------------------------------------------------
        // **Grab the viewport height *before* we start nesting UIs**. A child
        // `Ui` created later with `ui.vertical(|col_ui| ...)` does *not* yet
        // know how tall it will be, so `available_height()` there collapses to
        // a tiny value. Capturing it up‑front guarantees we have the full
        // window height for every column.
        //-----------------------------------------------------------------

        let viewport_h = ui.available_height();

        let column_frame = Frame::default()
            .fill(style.visuals.window_fill) // (Color32::from_rgb(12, 12, 14))
            .inner_margin(Margin::same(6))
            .corner_radius(style.visuals.menu_corner_radius)
            .stroke(style.visuals.window_stroke);

        let header_frame = Frame::default()
            .fill(style.visuals.window_fill) // (Color32::from_rgb(13, 13, 15))
            .inner_margin(Margin::same(2))
            .outer_margin(Margin::symmetric(8, 3))
            .corner_radius(style.visuals.window_corner_radius)
            .stroke(style.visuals.window_stroke);

        // Reserve enough width that the horizontal scrollbar always shows up
        ui.set_min_width(Self::COL_W * self.column_names.len() as f32);

        ScrollArea::horizontal().max_height(viewport_h).auto_shrink(true).show(ui, |ui| {
            // ui.columns(self.column_names.len(), |ui| {
            ui.horizontal(|ui| {
                let mut i = 0;
                for (name, tasks) in self.task_map.iter_mut() {
                    // let avail_h = ui.available_height();        // full remaining window height
                    ui.vertical(|col_ui| {
                        let sort_by = self.sort_by.entry(name.clone()).or_default();
                        let direction = &sort_by.direction;
                        match sort_by.field {
                            SortField::Default => tasks.default_sort(direction.clone()),
                            SortField::Date => tasks.sort_by_date(direction.clone()),
                            SortField::Name => tasks.sort_by_name(direction.clone()),
                        };
                        
                        for task in tasks.iter(){
                            inputs.insert(task.task_name.clone());
                            inputs.insert(format!("{}", task.service_number.clone().unwrap_or_default()));
                        }

                        header_frame.show(col_ui, |ui| {
                            // The header frame’s *content* must be narrower by the
                            // 2‑px inner margin on each side, otherwise the outer
                            // stroke can extend past COL_W.
                            let content_w = Self::COL_W - 2.0 * 2.0; // inner margin*2
                            ui.set_min_width(content_w);
                            ui.set_max_width(content_w);

                            ui.horizontal(|ui| 
                            {
                                ui.with_layout(Layout::left_to_right(Align::Max), |ui| 
                                {
                                    // Disable per-column search if global search is active
                                    let search_input = if self.search_results.is_some() {
                                        &mut String::new()
                                    } else {
                                        &mut self.search_inputs.entry(name.clone()).or_insert_with(String::new).clone()
                                    };

                                    let mut margin = Margin::default();
                                    margin.top = 6;
                                    
                                    TextEdit::singleline(search_input).hint_text(" Search").desired_width(100.0).margin(margin).ui(ui);

                                    // Update search_inputs only if global search is inactive
                                    if self.search_results.is_none() {
                                        self.search_inputs.insert(name.clone(), search_input.to_string());
                                    }

                                    ui.add_space(15.);
                                    
                                    let mut count = 0;
                                    let current_date = Utc::now().date_naive();
                                    let ids = tasks.iter().map(|t| t.id.clone()).collect::<Vec<RecordId>>();
                                    
                                    for task in &mut *tasks {
                                        if task.due_date.date_naive() < current_date && !task.completed { count += 1; }
                                    }

                                    if count > 0 {
                                        ui.colored_label(Color32::DARK_RED, RichText::new(format!("{count}")).small());
                                    }

                                    ui.add_space(25.);
                                    let response = Button::new(RichText::new(name.to_owned())
                                            .color(style.visuals.warn_fg_color)
                                            .size(13.0).monospace()
                                        )
                                        .fill(style.visuals.noninteractive().bg_fill)
                                        .corner_radius(eframe::egui::CornerRadius::same(2))
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

                                    PlatformSpawner::spawn(async move {
                                        if let Some(action) = res {
                                            match action{
                                                TaskActions::MarkComplete => {
                                                    let _x: Option<Record> = DATABASE.query("fn::mark_all_completion($record, $completion)")
                                                        .bind(("record", ids.clone()))
                                                        .bind(("completion", true))
                                                        .await.unwrap().take(0).unwrap();
                                                },
                                                TaskActions::MarkIncomplete => {
                                                    let _x: Option<Record> = DATABASE.query("fn::mark_all_completion($record, $completion)")
                                                        .bind(("record", ids.clone()))
                                                        .bind(("completion", false))
                                                        .await.unwrap().take(0).unwrap();
                                                },
                                                TaskActions::MarkDueToday => {
                                                    let _x: Option<Record> = DATABASE.query("fn::mark_all_due_today($ids)")
                                                        .bind(("ids", ids.clone())).await.unwrap().take(0).unwrap();
                                                }, _ => {}
                                            }
                                        }
                                    });
                                });
                                
                                ui.with_layout(Layout::right_to_left(Align::Max), |ui| 
                                {
                                    let button = Button::new(
                                        RichText::new("✚")
                                            .color(style.visuals.warn_fg_color)
                                        )
                                        .corner_radius(style.visuals.menu_corner_radius)
                                        .fill(Color32::from_rgb(22,22,22))
                                        .min_size(Vec2::new(30.0, 15.0))
                                        .ui(ui);

                                    ui.add_space(20.0);

                                    if button.clicked(){
                                        let _ = ui_actions_tx.try_send(TaskUiActions::CreateTaskModal);
                                    }

                                    let selected = self.sort_by.entry(name.clone()).or_default();
                                    let txt = match selected.direction {
                                        SortDirection::Asc => ("↗", ui.style().visuals.warn_fg_color),
                                        SortDirection::Desc => ("↘", ui.style().visuals.error_fg_color),
                                    };
                                    let selected_text = match selected.field {
                                        SortField::Default => RichText::new(format!("Priority {}", txt.0)).color(txt.1).small(),
                                        SortField::Date => RichText::new(format!("Date {}", txt.0)).color(txt.1).small(),
                                        SortField::Name => RichText::new(format!("Name {}", txt.0)).color(txt.1).small(),
                                    };
                                    
                                    ComboBox::new(format!("SortBy for {name:?}-{i}"), "")
                                        .selected_text(selected_text)
                                        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
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

                        
                        column_frame.show(col_ui, |ui| {
                            // Compensate for 6‑px *inner* margin so the turquoise
                            // stroke remains inside COL_W.
                            let content_w = Self::COL_W - 2.0 * 6.0; // inner margin*2
                            ui.set_min_width(content_w);
                            ui.set_max_width(content_w);

                            let column_h = (viewport_h - Self::HEADER_H).at_least(0.0);
                            ui.set_min_height(column_h);
                            ui.set_max_height(column_h);

                            let row_height = 110.;
                            let total_rows = tasks.len(); 
                            ui.ctx().options_mut(|o| o.line_scroll_speed = 80.0);
                            
                            ScrollArea::vertical()
                                .max_width(Self::COL_W - 20.0)
                                // .max_height(viewport_h)
                                .auto_shrink(false)
                                .id_salt(format!("{}-{i}-scroll", name))
                                .show_rows(ui, row_height, total_rows, |ui, row_range| 
                            {
                                // ui.scroll_with_delta(Vec2::new(0.0, 300.));
                                // Retrieve search input for the current context, or default to an empty string.
                                let search_input = self.search_inputs.get(name).cloned().unwrap_or_default();

                                // Filter tasks based on search input.
                                // Assuming tasks is a Vec<TaskPayload> or similar persistent collection
                                let filtered_indices: Vec<usize> = if !search_input.is_empty() {
                                    tasks
                                        .iter()
                                        .enumerate()
                                        .filter(|(_, task)| {
                                            // Assuming filter_by_task_name returns a Vec<TaskPayload>,
                                            // adapt this to match your filtering logic
                                            task.task_name
                                                .to_lowercase()
                                                .contains(&search_input.to_lowercase())
                                        })
                                        .map(|(i, _)| i)
                                        .collect()
                                } else {
                                    (0..tasks.len()).collect() // All indices when no filter
                                };

                                // Iterate only over the rows in the current viewport range
                                for row in row_range {
                                    if !search_input.is_empty() {
                                        ui.scroll_to_cursor(Some(Align::BOTTOM));
                                    }
                                    if let Some(&task_index) = filtered_indices.get(row) {
                                        if let Some(task) = tasks.get_mut(task_index) {
                                            task.display_cards(&self.user, ui, &self.assignees, ui_actions_tx.clone());
                                        }
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
                    i += 1;
                }
                if i + 1 < self.column_names.len() {
                    ui.add_space(6.0); // spacer between columns
                }
            });
        });
    }
}
pub enum TaskActions{
    MarkComplete,
    MarkIncomplete,
    MarkDueToday,
    None
}
