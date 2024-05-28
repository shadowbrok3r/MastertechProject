use database::{schema::{TaskPayload, User}, Database};
use egui::Ui;

pub struct MyUIComponent {
    _task_payload: TaskPayload,
    is_modal_open: bool,
}

impl MyUIComponent {
    pub fn new(task_payload: TaskPayload) -> Self {
        Self {
            _task_payload: task_payload,
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