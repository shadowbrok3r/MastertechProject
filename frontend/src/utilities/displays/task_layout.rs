use std::collections::HashMap;
use crossbeam::channel::Sender;
use database::Database;
use eframe::egui::Ui;
use egui::{Color32, Stroke};
use database::schema::{Priority, TaskPayload, User};
use serde::Serialize;
use crate::utilities::TaskUiActions;

use super::ColumnLayout;


#[derive(Serialize)]
pub struct TaskLayout{
    pub search_inputs: HashMap<String, String>,
    pub task_map: HashMap<String, Vec<TaskPayload>>,
    pub style_options: TaskStyles,
    pub column_names: Vec<String>,
    #[serde(skip)]
    pub database: Database,
    #[serde(skip)]
    pub ui_actions_tx: Sender<TaskUiActions>
}

pub struct SortTasks{
    pub sort_by_status: bool,
    pub sort_by_priority: Option<Priority>,
    pub sort_by_complete: Option<bool>,
    pub sort_by_current_user: Option<User> 
}

impl TaskLayout { 
    pub fn new(
        task_map: HashMap<String, Vec<TaskPayload>>,
        column_names: Vec<String>,
        database: Database,
        ui_actions_tx: Sender<TaskUiActions>
    ) -> Self {
        Self { 
            task_map,
            style_options: TaskStyles::default(),
            column_names,
            database,
            ui_actions_tx,
            search_inputs: HashMap::new()
        }
    }

    pub fn display(
        &mut self,
        ui: &mut Ui,
        store_users: &Option<Vec<User>>,
        task_map: HashMap<String, Vec<TaskPayload>>
    ){
        let col_names = self.column_names.clone();
        let db = self.database.clone();
  
        self.style_options.set(ui);
        self.layout_task_cols(
            ui, 
            col_names, 
            db, 
            &store_users,
            task_map
        );
    }

    pub fn set_tasks(&mut self, tasks: HashMap<String, Vec<TaskPayload>>){
        self.task_map = tasks;
    }
}

#[derive(Serialize)]
pub struct TaskStyles{
    selection_stroke_color:  Color32,
    selection_bg_fill: Color32,
    widgets_inactive_bg_fill:  Color32,
    widgets_inactive_fg_stroke:  Stroke,
    widgets_inactive_weak_bg_fill:  Color32,
    widgets_inactive_bg_stroke:  Stroke,
    widgets_open_bg_fill:  Color32,
    widgets_open_weak_bg_fill:  Color32,
    widgets_active_weak_bg_fill:  Color32,
    widgets_hovered_weak_bg_fill:  Color32,
    widgets_hovered_bg_fill:  Color32,
    widgets_hovered_bg_stroke:  Stroke,
}

impl TaskStyles{
    pub fn set(&self, ui: &mut Ui){
        ui.style_mut().visuals.selection.stroke.color = self.selection_stroke_color;
        ui.style_mut().visuals.selection.bg_fill = self.selection_bg_fill;
        
        ui.style_mut().visuals.widgets.inactive.bg_fill = self.widgets_inactive_bg_fill;
        ui.style_mut().visuals.widgets.inactive.fg_stroke = self.widgets_inactive_fg_stroke;
        ui.style_mut().visuals.widgets.inactive.weak_bg_fill = self.widgets_inactive_weak_bg_fill;
        ui.style_mut().visuals.widgets.inactive.bg_stroke = self.widgets_inactive_bg_stroke;
        
        ui.style_mut().visuals.widgets.open.bg_fill = self.widgets_open_bg_fill;
        ui.style_mut().visuals.widgets.open.weak_bg_fill = self.widgets_open_weak_bg_fill;
        
        ui.style_mut().visuals.widgets.active.weak_bg_fill = self.widgets_active_weak_bg_fill;
        
        ui.style_mut().visuals.widgets.hovered.weak_bg_fill = self.widgets_hovered_weak_bg_fill;
        ui.style_mut().visuals.widgets.hovered.bg_fill = self.widgets_hovered_bg_fill;
        ui.style_mut().visuals.widgets.hovered.bg_stroke = self.widgets_hovered_bg_stroke;
        
        ui.style_mut().visuals.widgets.hovered.expansion = 2.0;
    }
}

impl Default for TaskStyles{
    fn default() -> Self {
        Self { 
            selection_stroke_color:  Color32::BLACK,
            selection_bg_fill: Color32::from_rgb(120, 10, 120),
            widgets_inactive_bg_fill:  Color32::GOLD,
            widgets_inactive_fg_stroke:  Stroke::new(1.0, Color32::WHITE),
            widgets_inactive_weak_bg_fill:  Color32::from_rgb(20, 20, 25),
            widgets_inactive_bg_stroke:  Stroke::new(1.0, Color32::from_rgb(80, 80, 80)),
            widgets_open_bg_fill:  Color32::from_black_alpha(50),
            widgets_open_weak_bg_fill:  Color32::from_black_alpha(50),
            widgets_active_weak_bg_fill:  Color32::from_rgb(30,30,30),
            widgets_hovered_weak_bg_fill:  Color32::TRANSPARENT,
            widgets_hovered_bg_fill:  Color32::from_rgb(12, 12, 12),
            widgets_hovered_bg_stroke:  Stroke::new(1.0, Color32::from_rgb(200, 20, 200)),
        }
    }
}
