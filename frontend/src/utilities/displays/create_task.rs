use database::{schema::User, Database};
use egui::Ui;
use serde::Serialize;

#[derive(Serialize)]
pub struct CreateTaskModal {
    is_modal_open: bool,

}

impl CreateTaskModal {
    pub fn new() -> Self {
        Self {
            is_modal_open: false,
        }
    }

    fn task_modal(&mut self, ui: &mut Ui, _database: Database) {
        ui.label("Task Modal Content");

        if ui.button("Close").clicked() {
            self.is_modal_open = false;
        }
    }

    pub fn display_task_cards(&mut self, ui: &mut Ui, database: Database, _store_users: &Vec<User>) -> anyhow::Result<(), anyhow::Error> {
        if ui.button("O").clicked() {
            self.is_modal_open = true;
        }

        if self.is_modal_open {
            self.task_modal(ui, database.clone());
        }

        Ok(())
    }
}