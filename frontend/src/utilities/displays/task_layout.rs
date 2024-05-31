use crossbeam::channel::Sender;
use database::{schema::{TicketData, TicketPayload}, Database};
use eframe::egui::Ui;
use egui::{Color32, Stroke};
use database::schema::{Priority, TaskPayload, User};
use serde::Serialize;
use serde_json::Value;

use super::{modals::ModalType, ColumnLayout, Filters, Sortable};


#[derive(Serialize)]
pub struct TaskLayout{
    pub tasks: Vec<TaskPayload>,
    pub style_options: TaskStyles,
    pub filters: Vec<Filters>,
    pub column_names: Vec<String>,
    #[serde(skip)]
    pub database: Database,
    pub modal: ModalType,
    pub show_modal: bool,
    #[serde(skip)]
    pub ticket_data_tx: Sender<Option<Value>>
}
pub struct SortTasks{
    pub sort_by_status: bool,
    pub sort_by_priority: Option<Priority>,
    pub sort_by_complete: Option<bool>,
    pub sort_by_current_user: Option<User> 
}

impl TaskLayout { 
    pub fn new(
        tasks: Vec<TaskPayload>, 
        filters: Vec<Filters>,
        column_names: Vec<String>,
        database: Database,
        ticket_data_tx: Sender<Option<Value>>
    ) -> Self {
        Self { 
            tasks,
            style_options: TaskStyles::default(),
            filters,
            column_names,
            database,
            show_modal: false,
            modal: ModalType::Null,
            ticket_data_tx
        }
    }

    pub fn display(
        &mut self,
        ui: &mut Ui,
        store_users: &Option<Vec<User>>,
        status: bool,
        priority: &Option<Priority>,
        complete: &Option<bool>,
        current_user: &Option<User>,
    ){
        let col_names = self.column_names.to_owned();
        let db = self.database.to_owned();
        let filters = &self.filters.to_owned();
        
        self.tasks.sort_task_payloads();
        self.style_options.set(ui);
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
