use egui::{Ui, Widget};

use super::schema::{ModifyTask, Priority, Status, TaskPayload};



pub trait Displayable{
    fn display_task_cards(&mut self, ui: &mut Ui) -> anyhow::Result<(), anyhow::Error>;
}

pub trait Updatable {
    fn update_completed(&mut self, completed: bool);
    fn update_due_date(&mut self, due_date: String);
    fn update_assignee_initials(&mut self, initials: String);
    fn update_task_name(&mut self, name: String);
    fn update_status(&mut self, status: Status);
    fn update_dep(&mut self, dep: String);
    fn update_priority(&mut self, priority: Option<Priority>);
    fn update_task_description(&mut self, description: Option<String>);
}


impl Updatable for TaskPayload {
    fn update_completed(&mut self, completed: bool) {
        self.completed = completed;
    }

    fn update_due_date(&mut self, due_date: String) {
        self.due_date = due_date;
    }

    fn update_assignee_initials(&mut self, initials: String) {
        self.assignee_initials = Some(initials);
    }

    fn update_task_name(&mut self, name: String) {
        self.task_name = name;
    }

    fn update_status(&mut self, status: Status) {
        self.status = status;
    }

    fn update_dep(&mut self, dep: String) {
        self.dep = Some(dep);
    }

    fn update_priority(&mut self, priority: Option<Priority>) {
        self.priority = priority;
    }

    fn update_task_description(&mut self, description: Option<String>) {
        self.task_description = description;
    }
}

// Usage:
fn update_task_payload(task_payload: &mut TaskPayload) {
    task_payload.update_completed(true);
    task_payload.update_due_date("2024-06-01".to_string());
    task_payload.update_assignee_initials("JD".to_string());
    task_payload.update_task_name("New Task Name".to_string());
    task_payload.update_status(Status::InRepair);
    task_payload.update_dep("IT".to_string());
    task_payload.update_priority(Some(Priority::Normal));
    task_payload.update_task_description(Some("Updated task description.".to_string()));
}