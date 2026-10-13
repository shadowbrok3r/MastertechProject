use crate::ui_tools::controls::IconLabel;
use crate::tabs::database_viewer::row_viewer::DatabaseRowViewer;
use eframe::egui::{Color32, Response, Ui};
use database::schema::User;
use crate::ui_tools::controls::{Metrics, TextField};
use crate::ui_tools::icons::svg::{self, Icon};
use crate::ui_tools::theme;
// id, name, username, email, store, id_prestashop, id_store, authorization

impl egui_data_table::RowViewer<User> for DatabaseRowViewer {
    fn num_columns(&mut self) -> usize {
        8
    }
    
    fn show_cell_view(&mut self, ui: &mut Ui, row: &User, column: usize) {
        let _ = match column {
            0 => ui.label(row.get_id().key().to_string()),
            1 => IconLabel::new(svg::USER_CIRCLE, &row.get_name()).icon_tone(theme::faint_text(ui)).show(ui),
            2 => ui.label(row.get_username()),
            3 => IconLabel::new(svg::ENVELOPE_SIMPLE, &row.get_email()).icon_tone(theme::faint_text(ui)).show(ui),
            4 => IconLabel::new(svg::STOREFRONT, row.get_store().as_str()).icon_tone(theme::faint_text(ui)).show(ui),
            5 => ui.label(row.get_employee_id().unwrap_or(0).to_string()),
            6 => ui.label(row.get_store_id().unwrap_or(String::new())),
            7 => IconLabel::new(svg::SHIELD_CHECK, row.get_authorization().as_str()).icon_tone(theme::accent_secondary(ui)).show(ui),
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
                let res = TextField::new(&mut row.get_name())
                    .mono(true)
                    .width(ui.available_width())
                    .show(ui);
                Some(res)
            }
            2 => {
                let res = TextField::new(&mut row.get_username())
                    .mono(true)
                    .width(ui.available_width())
                    .show(ui);
                Some(res)
            }
            3 => {
                let res = TextField::new(&mut row.get_email())
                    .mono(true)
                    .width(ui.available_width())
                    .show(ui);
                Some(res)
            }
            4 => {
                let res = TextField::new(&mut row.get_store().as_str())
                    .mono(true)
                    .width(ui.available_width())
                    .show(ui);
                Some(res)
            }
            5 => {
                let res = TextField::new(&mut row.get_employee_id().unwrap_or(0).to_string())
                    .mono(true)
                    .width(ui.available_width())
                    .show(ui);
                Some(res)
            }
            6 => {
                let res = TextField::new(&mut row.get_store_id().unwrap_or(String::new()))
                    .mono(true)
                    .width(ui.available_width())
                    .show(ui);
                Some(res)
            }
            7 => {
                let res = TextField::new(&mut row.get_authorization().as_str())
                    .mono(true)
                    .width(ui.available_width())
                    .show(ui);
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

