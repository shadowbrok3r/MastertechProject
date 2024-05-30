use database::Database;
use eframe::egui::Ui;
use egui::{Color32, Stroke};
use database::schema::{Priority, TaskPayload, User};
use serde::Serialize;

use crate::app_state::MtechServerContext;

use super::create_task::CreateTaskModal;
use super::{ColumnLayout, Filters, Sortable};

use super::modal::{Modal, ModalHandler};


#[derive(Serialize)]
pub struct TaskLayout{
    pub task_opts: Option<TaskLayoutOpts>,
    // pub create_task_modal: bool,
    // pub sort_options: SortTasks,
    // pub store_users: &Option<Vec<User>>,
}
pub struct SortTasks{
    pub sort_by_status: bool,
    pub sort_by_priority: Option<Priority>,
    pub sort_by_complete: Option<bool>,
    pub sort_by_current_user: Option<User> 
}

impl TaskLayout for MtechServerContext{
    fn new(
        tasks: Vec<TaskPayload>, 
        filters: Vec<Filters>,
        column_names: Vec<String>,
        database: Database,
        // create_task_modal: bool,
    ) -> TaskL {
        let task_opts = TaskLayoutOpts{
            tasks,
            style_options: TaskStyles::default(),
            filters,
            column_names,
            database,
            // create_task_modal: CreateTaskModal::new(),
            // modal_handler: ModalHandler::default(),
        };
        
        Self { 
            task_opts: Some(task_opts),
            // create_task_modal: false,
        }
    }

    fn display(
        &mut self,
        ui: &mut Ui,
        store_users: &Option<Vec<User>>,
        status: bool,
        priority: &Option<Priority>,
        complete: &Option<bool>,
        current_user: &Option<User> 
    ){
        if let Some(task_opts) = &mut self.task_opts{
            let col_names = task_opts.column_names.clone();
            let db = task_opts.database.clone();
            let filters = &task_opts.filters.clone();
            
            task_opts.tasks.sort_task_payloads();
            
            self.layout_task_cols(
                ui, 
                col_names, 
                db, 
                filters, 
                &store_users,
                status,
                &priority,
                &complete,
                &current_user
            );
        }
    }

    fn set_styles(&mut self, ui: &mut Ui){
        if let Some(task_opts) = &self.task_opts{

            ui.style_mut().visuals.selection.stroke.color = task_opts.style_options.selection_stroke_color;
            ui.style_mut().visuals.selection.bg_fill = task_opts.style_options.selection_bg_fill;
            
            ui.style_mut().visuals.widgets.inactive.bg_fill = task_opts.style_options.widgets_inactive_bg_fill;
            ui.style_mut().visuals.widgets.inactive.fg_stroke = task_opts.style_options.widgets_inactive_fg_stroke;
            ui.style_mut().visuals.widgets.inactive.weak_bg_fill = task_opts.style_options.widgets_inactive_weak_bg_fill;
            ui.style_mut().visuals.widgets.inactive.bg_stroke = task_opts.style_options.widgets_inactive_bg_stroke;
            
            ui.style_mut().visuals.widgets.open.bg_fill = task_opts.style_options.widgets_open_bg_fill;
            ui.style_mut().visuals.widgets.open.weak_bg_fill = task_opts.style_options.widgets_open_weak_bg_fill;
            
            ui.style_mut().visuals.widgets.active.weak_bg_fill = task_opts.style_options.widgets_active_weak_bg_fill;
            
            ui.style_mut().visuals.widgets.hovered.weak_bg_fill = task_opts.style_options.widgets_hovered_weak_bg_fill;
            ui.style_mut().visuals.widgets.hovered.bg_fill = task_opts.style_options.widgets_hovered_bg_fill;
            ui.style_mut().visuals.widgets.hovered.bg_stroke = task_opts.style_options.widgets_hovered_bg_stroke;
            
            ui.style_mut().visuals.widgets.hovered.expansion = 2.0;
        }
    }
}


#[derive(Serialize)]
pub struct TaskLayoutOpts{
    pub tasks: Vec<TaskPayload>,
    pub style_options: TaskStyles,
    pub filters: Vec<Filters>,
    pub column_names: Vec<String>,
    #[serde(skip)]
    pub database: Database,
    // pub create_task_modal: CreateTaskModal,
    // pub modal_handler: ModalHandler,
}

impl TaskLayoutOpts{
    pub fn new(
        tasks: Vec<TaskPayload>, 
        filters: Vec<Filters>,
        column_names: Vec<String>,
        database: Database,
        // ui: &mut Ui,
    ) -> Self {    
        Self { 
            tasks,
            style_options: TaskStyles::default(),
            filters,
            column_names,
            database,
            // task_modal: TaskModal::default(),
            // create_task_modal: CreateTaskModal::new(),
            // modal_handler: ModalHandler::default()
        }
    }
}

impl Default for TaskLayout{
    fn default() -> Self {
        Self {
            // create_task_modal: false,
            task_opts: None,
        }
    }
}

#[derive(Serialize)]
pub struct TaskStyles{
    selection_stroke_color:  Color32, //  = Color32::BLACK,
    selection_bg_fill: Color32, //  Color32::from_rgb(120, 10, 120),
    widgets_inactive_bg_fill:  Color32, //  = Color32::GOLD,
    widgets_inactive_fg_stroke:  Stroke, //  = Stroke::new(1.0, Color32::WHITE),
    widgets_inactive_weak_bg_fill:  Color32, //  = Color32::from_rgb(20, 20, 25),
    widgets_inactive_bg_stroke:  Stroke, //  = Stroke::new(1.0, Color32::from_rgb(80, 80, 80)),
    widgets_open_bg_fill:  Color32, //  = Color32::from_black_alpha(50),
    widgets_open_weak_bg_fill:  Color32, //  = Color32::from_black_alpha(50),
    widgets_active_weak_bg_fill:  Color32, //  = Color32::from_rgb(30,30,30),
    widgets_hovered_weak_bg_fill:  Color32, //  = Color32::TRANSPARENT,
    widgets_hovered_bg_fill:  Color32, //  = Color32::from_rgb(12, 12, 12),
    widgets_hovered_bg_stroke:  Stroke, //  = Stroke::new(1.0, Color32::from_rgb(200, 20, 200)),
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
