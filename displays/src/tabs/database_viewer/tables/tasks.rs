use crate::{tabs::database_viewer::row_viewer::DatabaseRowViewer, Interaction};
use database::schema::TaskPayload;
use eframe::egui::{Response, Ui};


// id, task_name, assignee, due_date, service_number, service_ticket, priority, status, completed, task_description
impl egui_data_table::RowViewer<TaskPayload> for DatabaseRowViewer {
    fn num_columns(&mut self) -> usize { 10 }
    
    fn show_cell_view(&mut self, ui: &mut Ui, row: &TaskPayload, column: usize) {
        let _ = match column {
            0 => ui.label(row.id.key().to_string()),
            1 => ui.label(&row.task_name),
            2 => ui.label(row.assignee.key().to_string()),
            3 => ui.label(&row.due_date.to_string()),
            4 => ui.label(&row.service_number.clone().unwrap_or_default()),
            5 => {
                if let Some(ticket) = &row.service_ticket {
                    ui.label(ticket.id.key().to_string())
                } else {
                    ui.label("")
                }
            },
            6 => ui.label(row.priority.as_str()),
            7 => ui.label(row.status.as_str()),
            8 => ui.label(row.completed.to_string()),
            9 => ui.label(&row.task_description),
            _ => ui.label("")
        };
    }
    
    fn show_cell_editor(
        &mut self,
        ui: &mut Ui,
        row: &mut TaskPayload,
        column: usize,
    ) -> Option<Response> {
        match column {
            0 => Some(ui.label(row.id.key().to_string())),
            1 => Some(row.interact_task_name(ui)),
            2 => Some(ui.label(row.assignee.key().to_string())),
            3 => Some(row.interact_due_date(ui)),
            4 => Some(row.interact_service_number(ui)),
            6 => Some(row.interact_priority(ui)),
            7 => Some(row.interact_status(&self.current_user, ui)),
            8 => Some(row.interact_completed(ui)),
            9 => Some(row.interact_task_description(ui)),
            _ => None,
        }
        .into()
    }
    
    fn set_cell_value(&mut self, src: &TaskPayload, dst: &mut TaskPayload, _column: usize) {
        *dst = src.clone();
    }
    
    fn new_empty_row(&mut self) -> TaskPayload {
        TaskPayload::default()
    }
}