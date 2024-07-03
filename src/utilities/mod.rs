use crossbeam::channel::Sender;
use eframe::egui::{Response, Ui};
use crate::database::{database::Database, schema::{Priority, Status, Store, TaskPayload, User}};
use egui_extras::Strip;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

pub mod crypto;

#[derive(Debug)]
pub enum TaskUiActions{
    OpenTaskModal(TaskPayload),
    CreateTaskModal,
    Response(Response)
}

pub trait Displayable{ 
    fn display_cards(&mut self, ui: &mut Ui, database: Database, store_users: &Vec<User>) -> Option<TaskUiActions>;
}

pub trait DisplayCards{ 
    fn display_cards(&mut self, ui: &mut Ui, name: String);
}

pub trait ColumnLayout{
    fn layout_cols(&mut self, ui: &mut Ui);
    fn columns(&mut self,s: &mut Strip);
    fn headers(&mut self, s: Strip);
    // fn card_layout(&mut self, ui: &mut Ui) -> Option<TaskUiActions>;
}

pub trait Updatable { // This is correctly implemented
    fn update_completed(&self, completed: bool, db: Database);
    fn update_due_date(&self, due_date: String, db: Database);
    fn update_assignee_initials(&self, initials: String, db: Database);
    fn update_task_name(&self, name: String, db: Database);
    fn update_status(&self, status: Status, db: Database);
    fn update_dep(&self, store: Store, db: Database);
    fn update_priority(&self, priority: Option<Priority>, db: Database);
    fn update_task_description(&self, description: Option<String>, db: Database);
    fn update_recommendations(&self, recommendations: Option<String>, db: Database);
    fn update_checkin_notes(&self, checkin_notes: Option<String>, db: Database);
    fn update_task_notes(&self, new_msg: String, db: Database);
}

pub trait Interaction{ // This is correctly implemented
    fn interact_task_name(&mut self, ui: &mut Ui, database: Database) -> Option<Response>;
    fn interact_task_description(&mut self, ui: &mut Ui, database: Database) -> Option<Response>;
    fn interact_checkin_notes(&mut self, ui: &mut Ui, database: Database) -> Option<Response>;
    fn interact_recommendations(&mut self, ui: &mut Ui, database: Database) -> Option<Response>;
    fn interact_due_date(&mut self, ui: &mut Ui, database: Database) -> Option<Response>;
    fn interact_completed(&mut self, ui: &mut Ui, database: Database) -> Option<Response>;
    fn interact_status(&mut self, ui: &mut Ui, database: Database) -> Option<Response>;
    fn interact_dep(&mut self, ui: &mut Ui, database: Database) -> Option<Response>;
    fn interact_priority(&mut self, ui: &mut Ui, database: Database) -> Option<Response>;
    fn interact_assignee_initials(&mut self, ui: &mut Ui, database: Database, store_users: &Vec<User>) -> Option<Response>;
}

pub trait FilterTasks{ 
    fn filter_by_assignee(&self, assignee: &User) -> Vec<TaskPayload>;
    fn filter_by_completion(&self, completed: bool) -> Vec<TaskPayload>;
    fn filter_by_status(&self, status: &Status) -> Vec<TaskPayload>;
    fn filter_by_priority(&self, priority: &Priority) -> Vec<TaskPayload>;
    fn filter_by_date(&self, date: &String) -> Vec<TaskPayload>;
    /// Filters a list of tasks by their name based on a fuzzy search input.
    /// # Parameters
    /// - `search`: An iterator over items of type `S` where `S` can be referenced as a string slice.
    /// - `search_input`: A string representing the search input to filter tasks by.
    ///
    /// # Returns
    /// A vector of `TaskPayload` containing the filtered tasks.
    fn filter_by_task_name<T: IntoIterator<Item = S>, S: AsRef<str> + std::fmt::Debug>(&self, name: T, search_input: String) -> Vec<TaskPayload>;
}

pub trait Sortable{
    fn sort_task_payloads(&mut self) -> &mut Vec<TaskPayload>;
}

pub trait LiveUpdate{
    fn handle_live_create(self, existing_tasks: &mut Vec<TaskPayload>) -> anyhow::Result<(), anyhow::Error>; // <T: Serialize + for<'a> Deserialize<'a>>
    fn handle_live_update(self, existing_tasks: &mut Vec<TaskPayload>) -> anyhow::Result<(), anyhow::Error>; // <T: Serialize + for<'a> Deserialize<'a>>
    fn handle_live_delete(self, existing_tasks: &mut Vec<TaskPayload>) -> anyhow::Result<(), anyhow::Error>; // <T: Serialize + for<'a> Deserialize<'a>>
}

pub trait Task{ // <T: Serialize + for<'a> Deserialize<'a> + Debug>
    fn get_computer_data<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(&mut self, db: Database, tx: Sender<Option<T>>);
    fn get_customer_data<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(&mut self, db: Database, tx: Sender<Option<T>>);
    // fn get_service_data<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(&mut self, db: Database, tx: Sender<Option<T>>);
    fn get_task_notes<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(&mut self, db: Database, tx: Sender<Option<T>>);
    fn get_ticket_payload<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(&mut self, db: Database, tx: Sender<Option<T>>);
    // fn create_data(&mut self, database: Database, data: T) -> anyhow::Result<Vec<Record>, anyhow::Error>;
    // fn get_data(&mut self, database: Database, data: T)    -> anyhow::Result<Vec<Record>, anyhow::Error>;
    // fn modify_data(&mut self, database: Database, data: T) -> anyhow::Result<Vec<Record>, anyhow::Error>;
    // fn delete_data(&mut self, database: Database, data: T) -> anyhow::Result<Vec<Record>, anyhow::Error>;
}

