use crate::tabs::database_viewer::row_viewer::DatabaseRowViewer;
use eframe::egui::{Response, TextEdit, Ui};
use database::schema::User;
// id, name, username, email, store, id_prestashop, id_store, authorization

impl egui_data_table::RowViewer<User> for DatabaseRowViewer {
    fn num_columns(&mut self) -> usize {
        8
    }
    
    fn show_cell_view(&mut self, ui: &mut Ui, row: &User, column: usize) {
        let _ = match column {
            0 => ui.label(row.get_id().key().to_string()),
            1 => ui.label(row.get_name()),
            2 => ui.label(row.get_username()),
            3 => ui.label(row.get_email()),
            4 => ui.label(row.get_store().as_str()),
            5 => ui.label(row.get_employee_id().unwrap_or(0).to_string()),
            6 => ui.label(row.get_store_id().unwrap_or(String::new())),
            7 => ui.label(row.get_authorization().as_str()),
            _ => ui.label("")
        };
    }
    
    fn show_cell_editor(
        &mut self,
        ui: &mut Ui,
        row: &mut User,
        column: usize,
    ) -> Option<Response> {
        match column {
            0 => Some(ui.label(row.get_id().key().to_string())),
            1 => {
                let res = TextEdit::multiline(&mut row.get_name())
                    .desired_rows(1)
                    .code_editor()
                    .show(ui)
                    .response;
                Some(res)
            }
            2 => {
                let res = TextEdit::multiline(&mut row.get_username())
                    .desired_rows(1)
                    .code_editor()
                    .show(ui)
                    .response;
                Some(res)
            }
            3 => {
                let res = TextEdit::multiline(&mut row.get_email())
                    .desired_rows(1)
                    .code_editor()
                    .show(ui)
                    .response;
                Some(res)
            }
            4 => {
                let res = TextEdit::multiline(&mut row.get_store().as_str())
                    .desired_rows(1)
                    .code_editor()
                    .show(ui)
                    .response;
                Some(res)
            }
            5 => {
                let res = TextEdit::multiline(&mut row.get_employee_id().unwrap_or(0).to_string())
                    .desired_rows(1)
                    .code_editor()
                    .show(ui)
                    .response;
                Some(res)
            }
            6 => {
                let res = TextEdit::multiline(&mut row.get_store_id().unwrap_or(String::new()))
                    .desired_rows(1)
                    .code_editor()
                    .show(ui)
                    .response;
                Some(res)
            }
            7 => {
                let res = TextEdit::multiline(&mut row.get_authorization().as_str())
                    .desired_rows(1)
                    .code_editor()
                    .show(ui)
                    .response;
                Some(res)
            }
            _ => None
        }
        .into()
    }
    
    fn set_cell_value(&mut self, src: &User, dst: &mut User, _column: usize) {
        *dst = src.clone();
    }
    
    fn new_empty_row(&mut self) -> User {
        User::default()
    }
    
    // fn clone_row(&mut self, src: &User) -> User 
    // ^^ Overriding this method is optional. In default, it'll utilize `set_cell_value` which 
    //    would be less performant during huge duplication of lines.
}