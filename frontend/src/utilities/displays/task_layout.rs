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
            column_names,
            database,
            ui_actions_tx,
            search_inputs: HashMap::new(),
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
