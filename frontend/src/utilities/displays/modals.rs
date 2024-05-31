use database::{schema::TaskPayload, Database};
use egui::Ui;
use log::info;
use serde::Serialize;

use crate::utilities::Displayable;

#[derive(Serialize, Default, Clone)]
pub enum ModalType{
    CreateTaskModal,
    TaskModal(String),
    #[default]
    Null,
}


impl ModalType{
    pub fn create_task_modal(&mut self, _ui: &mut Ui){
        info!("Creating a task!!");
    }
    pub fn task_modal(&mut self, ui: &mut Ui, task: &mut TaskPayload, database: Database){
        info!("Here is a task: {:?}", task.id);
        task.task_modal(ui, database);
    }
    pub fn other(&mut self, _ui: &mut Ui){
        info!("No modal...");
    }
}

