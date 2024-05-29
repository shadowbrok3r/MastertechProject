use database::{schema::{TicketPayload, User}, Database};
use egui::Ui;
use serde::Serialize;

#[derive(Serialize)]
pub struct TaskModal {
    pub ticket_payload: Option<TicketPayload>,
    pub is_modal_open: bool,
}

impl Default for TaskModal{
    fn default() -> Self {
        Self { 
            ticket_payload: None, 
            is_modal_open: false
        }
    }
}

impl TaskModal {
    pub fn new(ticket_payload: TicketPayload) -> Self {
        Self {
            ticket_payload: Some(ticket_payload),
            is_modal_open: false,
        }
    }

    fn task_modal(&mut self, ui: &mut Ui, _database: Database) {
        ui.label("Task Modal Content");

        if ui.button("Close").clicked() {
            self.is_modal_open = false;
        }
    }

    pub fn display(&mut self, ui: &mut Ui, database: Database, _store_users: &Vec<User>) -> anyhow::Result<(), anyhow::Error> {
        if ui.button("O").clicked() {
            self.is_modal_open = true;
        }

        if self.is_modal_open {
            self.task_modal(ui, database.clone());
        }

        Ok(())
    }
}