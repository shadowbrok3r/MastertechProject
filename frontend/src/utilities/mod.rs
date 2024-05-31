use crossbeam::channel::Sender;
use displays::Filters;
use egui::{Frame, Grid, Id, Response, RichText, Ui};
use database::{schema::{Priority, Status, Store, TaskId, TaskNoteId, TaskPayload, TicketData, TicketId, TicketPayload, User, UserId}, Database};
use egui_extras::Strip;
use serde::{Deserialize, Serialize};
use std::fmt::Debug;

// use serde::{Deserialize, Serialize};
// use std::fmt::Debug;

pub mod displays;
pub mod update_tasks;
pub mod get_tasks;
pub mod interact_tasks;
pub mod get_other;
pub mod task_crud;
pub mod filter;
pub mod handle_live_data;
pub mod sortable;

#[derive(Debug)]
pub enum TaskUiActions{
    OpenTaskModal(String)
}
// This is hot garbage, i need to impl Displayable for TASKLAYOUT, not TaskPayload. i need to split Ui from data functionality
pub trait Displayable{ 
    fn display_task_cards(&mut self, ui: &mut Ui, database: Database, store_users: &Vec<User>) -> Option<TaskUiActions>;
    // fn task_headers(&mut self,  s: Strip, column_names: Vec<String>, header_frame: Frame);
    fn task_modal<ID>(&mut self, ui: &mut Ui, database: Database) //, data: &[T] <T: Aggregatable<ID>, ID: Debug>
    where
        Self: Aggregatable<ID> + Task,
        ID: Debug,
    {


        
        Grid::new(Id::new(format!("Grid ")))// self.id.as_ref().unwrap().0.id.clone()
            .num_columns(4)
            .show(ui, |ui| 
        {
            // for data in data{
                // ui.label(RichText::new("Rep"));
                // ui.label(RichText::new(format!("{:?}", self.checkin_rep)));
                // ui.label(RichText::new("Split Rep"));
                // ui.label(RichText::new(format!("{:?}", self.sales_rep)));
                // ui.label(RichText::new("Phone #"));
                // ui.label(RichText::new(format!("{:?}", self)));
                // ui.label(RichText::new("Phone #2"));
                // ui.label(RichText::new(format!("{:?}", self.rep)));
                // ui.label(RichText::new("Email"));
                // ui.label(RichText::new(format!("{:?}", self.rep)));
                ui.label(RichText::new(format!("ID: {:?}", self.get_id())));
                ui.label(RichText::new(format!("Name: {}", self.get_task_name())));
                // ui.label(RichText::new(format!("Due Date: {}", self.get_due_date())));
                ui.label(RichText::new(format!("Priority: {:?}", self.get_priority())));
                ui.end_row();
                ui.label(RichText::new(format!("get_task_name: {:?}", self.get_task_name())));
                ui.label(RichText::new(format!("get_service_ticket: {:?}", self.get_service_ticket())));
                ui.label(RichText::new(format!("get_everest_initials: {:?}", self.get_everest_initials())));
                ui.label(RichText::new(format!("get_task_description: {:?}", self.get_task_description())));
                ui.end_row();
                ui.label(RichText::new(format!("get_assignee: {:?}", self.get_assignee())));
                ui.label(RichText::new(format!("get_service_number: {:?}", self.get_service_number())));
                ui.label(RichText::new(format!("get_due_date: {:?}", self.get_due_date())));
                ui.label(RichText::new(format!("get_priority: {:?}", self.get_priority())));
                ui.end_row();
                ui.label(RichText::new(format!("get_task_note: {:?}", self.get_task_note())));
                ui.label(RichText::new(format!("is_completed: {:?}", self.is_completed())));
                ui.label(RichText::new(format!("get_status: {:?}", self.get_status())));
                ui.label(RichText::new(format!("get_dep: {:?}", self.get_dep())));
            // }
        });

    }
}

// This is hot garbage, i need to impl Displayable for TASKLAYOUT, not TaskPayload. i need to split Ui from data functionality
pub trait ColumnLayout{
    fn layout_task_cols(&mut self, ui: &mut Ui, column_names: Vec<String>, database: Database,filters: &Vec<Filters>, assignees: &Option<Vec<User>>,status: bool,priority: &Option<Priority>,complete: &Option<bool>,current_user: &Option<User>);
    fn task_columns(&mut self,s: Strip, filters: &Vec<Filters>,assignees: &Option<Vec<User>>,status: bool,priority: &Option<Priority>,complete: &Option<bool>,current_user: &Option<User>,database: Database,column_frame: Frame);
    fn filter_items(&mut self,filters: &Vec<Filters>, assignee: &Option<User>,status: &Option<Status>,priority: &Option<Priority>,complete: &Option<bool>) -> Vec<TaskPayload>;
    fn task_headers(&mut self, s: Strip, column_names: Vec<String>, header_frame: Frame);
}

pub trait Updatable { // This is correctly implemented
    fn update_completed(&mut self, completed: bool, db: Database);
    fn update_due_date(&mut self, due_date: String, db: Database);
    fn update_assignee_initials(&mut self, initials: String, db: Database);
    fn update_task_name(&mut self, name: String, db: Database);
    fn update_status(&mut self, status: Status, db: Database);
    fn update_dep(&mut self, store: Store, db: Database);
    fn update_priority(&mut self, priority: Option<Priority>, db: Database);
    fn update_task_description(&mut self, description: Option<String>, db: Database);
}

pub trait Interaction{ // This is correctly implemented
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
    fn filter_by_assignee(&self, assignee: &User) -> Vec<TaskPayload>;
    fn filter_by_completed(&self, completed: bool) -> Vec<TaskPayload>;
    fn filter_by_status(&self, status: &Status) -> Vec<TaskPayload>;
    fn filter_by_priority(&self, priority: &Priority) -> Vec<TaskPayload>;
    
}

pub trait Sortable{
    fn sort_task_payloads(&mut self) -> &mut Vec<TaskPayload>;
}

pub trait LiveUpdate{
    fn handle_live_create(self, existing_tasks: &mut Vec<TaskPayload>) -> anyhow::Result<(), anyhow::Error>;
    fn handle_live_update(self, existing_tasks: &mut Vec<TaskPayload>) -> anyhow::Result<(), anyhow::Error>;
    fn handle_live_delete(self, existing_tasks: &mut Vec<TaskPayload>) -> anyhow::Result<(), anyhow::Error>;
}

pub trait Task{ // <T: Serialize + for<'a> Deserialize<'a> + Debug>
    fn get_computer_data<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(&mut self, db: Database, tx: Sender<Option<T>>);
    fn get_customer_data<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(&mut self, db: Database, tx: Sender<Option<T>>);
    fn get_service_data<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(&mut self, db: Database, tx: Sender<Option<T>>);
    fn get_task_notes<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(&mut self, db: Database, tx: Sender<Option<T>>);
    fn get_ticket_payload<T: Serialize + for<'a> Deserialize<'a> + Debug + 'static>(&mut self, db: Database, tx: Sender<Option<T>>);
    // fn create_data(&mut self, database: Database, data: T) -> anyhow::Result<Vec<Record>, anyhow::Error>;
    // fn get_data(&mut self, database: Database, data: T)    -> anyhow::Result<Vec<Record>, anyhow::Error>;
    // fn modify_data(&mut self, database: Database, data: T) -> anyhow::Result<Vec<Record>, anyhow::Error>;
    // fn delete_data(&mut self, database: Database, data: T) -> anyhow::Result<Vec<Record>, anyhow::Error>;
}


pub trait Aggregatable<ID>{
    fn get_id(&self) -> Option<ID>;
    fn get_task_name(&self) -> &str;
    fn get_service_ticket(&self) -> Option<TicketId>;
    fn get_everest_initials(&self) -> &str;
    fn get_task_description(&self) -> Option<&str>;
    fn get_assignee(&self) -> Option<UserId>;
    fn get_service_number(&self) -> Option<i32>;
    fn get_due_date(&self) -> Option<&str>;
    fn get_priority(&self) -> &Priority;
    fn get_task_note(&self) -> Option<&Vec<TaskNoteId>>;
    fn is_completed(&self) -> bool;
    fn get_status(&self) -> &Status;
    fn get_dep(&self) -> Option<&str>;
}

impl Aggregatable<TaskId> for TaskPayload {
    fn get_id(&self) -> Option<TaskId> {
        self.id.clone()
    }
    
    fn get_task_name(&self) -> &str {
        &self.task_name
    }
    
    fn get_service_ticket(&self) -> Option<TicketId> {
        self.service_ticket.clone()
    }
    
    fn get_everest_initials(&self) -> &str {
        &self.everest_initials
    }
    
    fn get_task_description(&self) -> Option<&str> {
        self.task_description.as_deref()
    }
    
    fn get_assignee(&self) -> Option<UserId> {
        self.assignee.clone()
    }
    
    fn get_service_number(&self) -> Option<i32> {
        self.service_number
    }
    
    fn get_due_date(&self) -> Option<&str> {
        Some(&self.due_date)
    }
    
    fn get_priority(&self) -> &Priority {
        &self.priority
    }
    
    fn get_task_note(&self) -> Option<&Vec<TaskNoteId>> {
        self.task_note.as_ref()
    }
    
    fn is_completed(&self) -> bool {
        self.completed
    }
    
    fn get_status(&self) -> &Status {
        &self.status
    }
    
    fn get_dep(&self) -> Option<&str> {
        self.dep.as_deref()
    }
}

impl Aggregatable<TicketId> for TicketData {
    fn get_id(&self) -> Option<TicketId> {
        self.id.clone()
    }
    
    fn get_task_name(&self) -> &str {
        &self.doc_alias // Assuming doc_alias serves as a name
    }
    
    fn get_service_ticket(&self) -> Option<TicketId> {
        Some(self.id.clone().unwrap()) // Assuming service_ticket maps to ticket ID
    }
    
    fn get_everest_initials(&self) -> &str {
        &self.sales_rep // Assuming sales_rep can be considered as initials
    }
    
    fn get_task_description(&self) -> Option<&str> {
        Some(&self.checkin_notes) // Assuming checkin_notes can be a description
    }
    
    fn get_assignee(&self) -> Option<UserId> {
        None // No direct mapping in TicketData
    }
    
    fn get_service_number(&self) -> Option<i32> {
        Some(self.service_number)
    }

    fn get_due_date(&self) -> Option<&str> {
        None
    }

    fn get_priority(&self) -> &Priority {
        unimplemented!() // No direct mapping in TicketData
    }
    
    fn get_task_note(&self) -> Option<&Vec<TaskNoteId>> {
        None // No direct mapping in TicketData
    }
    
    fn is_completed(&self) -> bool {
        false // No direct mapping in TicketData
    }
    
    fn get_status(&self) -> &Status {
        unimplemented!() // No direct mapping in TicketData
    }
    
    fn get_dep(&self) -> Option<&str> {
        Some(&self.dep)
    }
}

