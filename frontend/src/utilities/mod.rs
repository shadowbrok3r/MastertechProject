use std::fmt::Display;

use egui::{Response, Ui};
use database::{schema::{Priority, User, Status, Store, TaskPayload}, Database};

pub mod display_tasks;
pub mod update_tasks;
pub mod get_tasks;
pub mod interact_tasks;
pub mod get_other;
pub mod task_context;
pub mod filter;
pub mod handle_live_data;

pub trait Displayable{
    fn display_task_cards(&mut self, ui: &mut Ui, database: Database, store_users: &Vec<User>) -> anyhow::Result<(), anyhow::Error>;
    // fn setup_display(&mut self, column_names: Vec<String>, total_rows: usize, ui: &mut Ui);
    // fn display_table(&mut self, ui: &mut Ui, tasks: Vec<TaskPayload>) -> anyhow::Result<(), anyhow::Error>;
}

pub trait Updatable {
    fn update_completed(&mut self, completed: bool, db: Database);
    fn update_due_date(&mut self, due_date: String, db: Database);
    fn update_assignee_initials(&mut self, initials: String, db: Database);
    fn update_task_name(&mut self, name: String, db: Database);
    fn update_status(&mut self, status: Status, db: Database);
    fn update_dep(&mut self, store: Store, db: Database);
    fn update_priority(&mut self, priority: Option<Priority>, db: Database);
    fn update_task_description(&mut self, description: Option<String>, db: Database);
}

pub trait Interaction{
    fn interact_task_name(&mut self, ui: &mut Ui, database: Database) -> Option<Response>;
    fn interact_task_description(&mut self, ui: &mut Ui, database: Database) -> Option<Response>;
    fn interact_recommendations(&mut self, ui: &mut Ui, database: Database) -> Option<Response>;
    fn interact_due_date(&mut self, ui: &mut Ui, database: Database) -> Option<Response>;
    fn interact_completed(&mut self, ui: &mut Ui, database: Database) -> Option<Response>;
    fn interact_status(&mut self, ui: &mut Ui, database: Database) -> Option<Response>;
    fn interact_dep(&mut self, ui: &mut Ui, database: Database) -> Option<Response>;
    fn interact_priority(&mut self, ui: &mut Ui, database: Database) -> Option<Response>;
    fn interact_assignee_initials(&mut self, ui: &mut Ui, database: Database, store_users: &Vec<User>) -> Option<Response>;
}


pub trait FilterTasks{
    fn filter_by_assignee(&self, assignees: &String) -> Vec<TaskPayload>;
    fn filter_by_completed(&self, completed: bool) -> Vec<TaskPayload>;
    fn filter_by_status(self, status: &Status) -> Vec<TaskPayload>;
    fn filter_by_priority(&self, priority: &Priority) -> Vec<TaskPayload>;
}

pub trait LiveUpdate{
    fn handle_live_create(self, existing_tasks: &mut Vec<TaskPayload>) -> anyhow::Result<(), anyhow::Error>;
    fn handle_live_update(self, existing_tasks: &mut Vec<TaskPayload>) -> anyhow::Result<(), anyhow::Error>;
    fn handle_live_delete(self, existing_tasks: &mut Vec<TaskPayload>) -> anyhow::Result<(), anyhow::Error>;
}