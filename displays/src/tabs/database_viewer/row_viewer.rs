use std::cmp::Ordering;

use database::schema::{ComputerData, CustomerData, TaskNotePayload, TaskPayload, TicketPayload, User};
use eframe::egui::TextEdit;
use egui_data_table::{viewer::RowCodec, RowViewer};
use egui_extras::Column;
use serde::{Deserialize, Serialize};
use crate::{get_current_user_from_auth, tabs::task_audit::codec::Codec, Interaction};

use super::DatabaseViewer;

/// Every logic is defined in `Viewer`
#[derive(serde::Serialize)]
pub struct DatabaseRowViewer {
    pub current_user: User,
    pub filter: String,
    pub selected: DatabaseTable,
    user_data: User,
    task_data: TaskPayload,
    ticket_data: TicketPayload
}

#[derive(PartialEq, Serialize, Deserialize, Clone)]
pub enum DatabaseTable {
    Task(TaskPayload),
    // Customer(CustomerData),
    // Ticket(TicketPayload),
    // Computer(ComputerData),
    // TaskNote(TaskNotePayload),
    User(User)
}

#[derive(PartialEq, Serialize, Deserialize, Clone, Default)]
pub enum DatabaseTableSelection {
    #[default]
    Task,
    User
}

impl DatabaseTableSelection {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Task => "Task",
            Self::User => "User"
        }
    }
}

impl Default for DatabaseTable {
    fn default() -> Self {
        Self::Task(TaskPayload::default())
    }
}

impl DatabaseTable {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Task(_) => "Task",
            // Self::Customer(_) => "Customer",
            // Self::Ticket(_) => "Ticket",
            // Self::Computer(_) => "Computer",
            // Self::TaskNote(_) => "Task Note",
            Self::User(_) => "User"
        }
    }
    
    // pub fn from_str(table: &str) -> Self {
    //     match table {
    //         "Task" => Self::Task(_),
    //         "Customer" => Self::Customer(_),
    //         "Ticket" => Self::Ticket(_),
    //         "Computer" => Self::Computer(_),
    //         "Task Note" => Self::TaskNote(_),
    //         _ => Self::default()
    //     }
    // }
}

impl Default for DatabaseRowViewer {
    fn default() -> Self {
        Self {
            current_user: if let Some(usr) = get_current_user_from_auth() {
                usr.clone()
            } else {
                User::default()
            },
            filter: Default::default(),
            // selected: Default::default(),
            user_data: User::default(),
            task_data: TaskPayload::default(),
            ticket_data: TicketPayload::default(),
            selected: DatabaseTable::default(),
        }
    }
}

impl RowViewer<DatabaseTable> for DatabaseRowViewer {
    // fn try_create_codec(&mut self, _: bool) -> Option<impl RowCodec<DatabaseTable>> {
    //     Some(Codec)
    // }

    fn num_columns(&mut self) -> usize {
        match self.selected {
            DatabaseTable::Task(_) => 10,
            DatabaseTable::User(_) => 8,
        }
    }

    fn column_name(&mut self, column: usize) -> std::borrow::Cow<'static, str> {
        match self.selected {
            DatabaseTable::Task(_) => ["id", "task_name", "assignee", "due_date", "service_number", "service_ticket", "priority", "status", "completed", "task_description"][column].into(),
            DatabaseTable::User(_) => ["id", "name", "username", "email", "store", "id_prestashop", "id_store", "authorization"][column].into(),
            // DatabaseTable::Customer(customer_data) => ["id", "name", "username", "email", "store", "id_prestashop", "id_store", "authorization"],
            // DatabaseTable::Ticket(ticket_payload) => ["id", "name", "username", "email", "store", "id_prestashop", "id_store", "authorization"],
            // DatabaseTable::Computer(computer_data) => ["id", "name", "username", "email", "store", "id_prestashop", "id_store", "authorization"],
            // DatabaseTable::TaskNote(task_note_payload) => ["id", "name", "username", "email", "store", "id_prestashop", "id_store", "authorization"],
        }
    }

    fn is_sortable_column(&mut self, column: usize) -> bool {
        match self.selected {
            DatabaseTable::Task(_) => [false, true, true, true, true, true, true, true, true, true][column],
            DatabaseTable::User(_) => [true, true, true, true, true, true, true, true][column],
        }
    }

    fn row_filter_hash(&mut self) -> &impl std::hash::Hash {
        &self.filter
    }

    fn filter_row(&mut self, row: &DatabaseTable) -> bool {
        match &row {
            DatabaseTable::Task(task) => 
                task.service_number.clone().unwrap_or_default().contains(&self.filter)
                || task.task_name.contains(&self.filter),
            DatabaseTable::User(user) => user.get_username().contains(&self.filter)
                || user.get_name().contains(&self.filter),
        }
    }

    fn show_cell_view(&mut self, ui: &mut eframe::egui::Ui, row: &DatabaseTable, column: usize) {
        let _ = match &row {
            DatabaseTable::Task(task_payload) => {
                let _ = match column {
                    0 => ui.label(task_payload.id.key().to_string()),
                    1 => ui.label(&task_payload.task_name),
                    2 => ui.label(task_payload.assignee.key().to_string()),
                    3 => ui.label(&task_payload.due_date.to_string()),
                    4 => ui.label(&task_payload.service_number.clone().unwrap_or_default()),
                    5 => {
                        if let Some(ticket) = &task_payload.service_ticket {
                            ui.label(ticket.id.key().to_string())
                        } else {
                            ui.label("")
                        }
                    },
                    6 => ui.label(task_payload.priority.as_str()),
                    7 => ui.label(task_payload.status.as_str()),
                    8 => ui.label(task_payload.completed.to_string()),
                    9 => ui.label(&task_payload.task_description),
                    _ => ui.label("")
                };
            },
            DatabaseTable::User(user) => {
                let _ = match column {
                    0 => ui.label(user.get_id().key().to_string()),
                    1 => ui.label(user.get_name()),
                    2 => ui.label(user.get_username()),
                    3 => ui.label(user.get_email()),
                    4 => ui.label(user.get_store().as_str()),
                    5 => ui.label(user.get_employee_id().unwrap_or(0).to_string()),
                    6 => ui.label(user.get_store_id().unwrap_or(String::new())),
                    7 => ui.label(user.get_authorization().as_str()),
                    _ => ui.label("")
                };
            },
        };
    }

    fn column_render_config(&mut self, column: usize, _is_last_visible_column: bool) -> Column {
        let col_config = Column::auto();
        match self.selected {
            DatabaseTable::Task(_) => {
                match column {
                    0 => col_config.resizable(true).at_least(60.).at_most(60.),
                    1 => col_config.resizable(true).at_least(180.).at_most(225.),
                    2 => col_config.resizable(true).at_least(90.).at_most(100.),
                    3 => col_config.resizable(true).at_least(130.).at_most(130.),
                    4 => col_config.resizable(true).at_least(100.).at_most(150.),
                    5 => col_config.resizable(true).at_least(100.).at_most(150.),
                    6 => col_config.resizable(true).at_least(100.).at_most(150.),
                    7 => col_config.resizable(true).at_least(100.).at_most(150.),
                    8 => col_config.resizable(true).at_least(100.).at_most(150.),
                    9 => col_config.resizable(true).at_least(150.),
                    _ => col_config,
                }
            },
            DatabaseTable::User(_) => {
                match column {
                    0 => col_config.resizable(true).at_least(60.).at_most(60.),
                    1 => col_config.resizable(true).at_least(180.).at_most(225.),
                    2 => col_config.resizable(true).at_least(90.).at_most(100.),
                    3 => col_config.resizable(true).at_least(130.).at_most(130.),
                    4 => col_config.resizable(true).at_least(100.).at_most(150.),
                    5 => col_config.resizable(true).at_least(100.).at_most(150.),
                    6 => col_config.resizable(true).at_least(100.).at_most(150.),
                    7 => col_config.resizable(true).at_least(100.).at_most(150.),
                    _ => col_config,
                }
            },
        }
    }
    
    fn show_cell_editor(
        &mut self,
        ui: &mut eframe::egui::Ui,
        row: &mut DatabaseTable,
        column: usize,
    ) -> Option<eframe::egui::Response> {
        match row {
            DatabaseTable::Task(task_payload) => {
                match column {
                    0 => Some(ui.label(task_payload.id.key().to_string())),
                    1 => Some(task_payload.interact_task_name(ui)),
                    2 => Some(ui.label(task_payload.assignee.key().to_string())),
                    3 => Some(task_payload.interact_due_date(ui)),
                    4 => Some(task_payload.interact_service_number(ui)),
                    6 => Some(task_payload.interact_priority(ui)),
                    7 => Some(task_payload.interact_status(&self.current_user, ui)),
                    8 => Some(task_payload.interact_completed(ui)),
                    9 => Some(task_payload.interact_task_description(ui)),
                    _ => None,
                }
                .into()
            },
            DatabaseTable::User(user) => {
                match column {
                    0 => Some(ui.label(user.get_id().key().to_string())),
                    1 => {
                        let res = TextEdit::multiline(&mut user.get_name())
                            .desired_rows(1)
                            .code_editor()
                            .show(ui)
                            .response;
                        Some(res)
                    }
                    2 => {
                        let res = TextEdit::multiline(&mut user.get_username())
                            .desired_rows(1)
                            .code_editor()
                            .show(ui)
                            .response;
                        Some(res)
                    }
                    3 => {
                        let res = TextEdit::multiline(&mut user.get_email())
                            .desired_rows(1)
                            .code_editor()
                            .show(ui)
                            .response;
                        Some(res)
                    }
                    4 => {
                        let res = TextEdit::multiline(&mut user.get_store().as_str())
                            .desired_rows(1)
                            .code_editor()
                            .show(ui)
                            .response;
                        Some(res)
                    }
                    5 => {
                        let res = TextEdit::multiline(&mut user.get_employee_id().unwrap_or(0).to_string())
                            .desired_rows(1)
                            .code_editor()
                            .show(ui)
                            .response;
                        Some(res)
                    }
                    6 => {
                        let res = TextEdit::multiline(&mut user.get_store_id().unwrap_or(String::new()))
                            .desired_rows(1)
                            .code_editor()
                            .show(ui)
                            .response;
                        Some(res)
                    }
                    7 => {
                        let res = TextEdit::multiline(&mut user.get_authorization().as_str())
                            .desired_rows(1)
                            .code_editor()
                            .show(ui)
                            .response;
                        Some(res)
                    }
                    _ => None
                }
                .into()
            },
        }
    }

    fn on_cell_view_response(
        &mut self,
        row: &DatabaseTable,
        column: usize,
        resp: &eframe::egui::Response,
    ) -> Option<Box<DatabaseTable>> {
        match column {
            0 | 1 => {
                // 
            },
            _ => {}
        }
    
        resp
            .clone()
            .on_hover_and_drag_cursor(eframe::egui::CursorIcon::Crosshair)
            .dnd_release_payload::<String>()
            .map(|_| Box::new(DatabaseTable::default()))
    }

    fn set_cell_value(
        &mut self,
        src: &DatabaseTable,
        dst: &mut DatabaseTable,
        _column: usize,
    ) {
        *dst = src.clone();
    }

    fn compare_cell(
        &self,
        row_l: &DatabaseTable,
        row_r: &DatabaseTable,
        column: usize,
    ) -> std::cmp::Ordering {
        match (row_l, row_r) {
            (DatabaseTable::Task(task_l), DatabaseTable::Task(task_r)) => {
                match column {
                    0 => task_l.id.cmp(&task_r.id),
                    1 => task_l.task_name.cmp(&task_r.task_name),
                    2 => task_l.due_date.cmp(&task_r.due_date),
                    3 => task_l.priority.as_str().cmp(&task_r.priority.as_str()),
                    4 => task_l.status.as_str().cmp(&task_r.status.as_str()),
                    _ => Ordering::Equal, // Default for invalid columns
                }
            }
            (DatabaseTable::User(user_l), DatabaseTable::User(user_r)) => {
                match column {
                    0 => user_l.get_id().cmp(&user_r.get_id()),
                    1 => user_l.get_name().cmp(&user_r.get_name()),
                    2 => user_l.get_email().cmp(&user_r.get_email()),
                    3 => user_l.get_store().cmp(&user_r.get_store()),
                    4 => user_l.is_active().cmp(&user_r.is_active()),
                    _ => Ordering::Equal, // Default for invalid columns
                }
            }
            (DatabaseTable::Task(_), DatabaseTable::User(_)) => Ordering::Less,
            (DatabaseTable::User(_), DatabaseTable::Task(_)) => Ordering::Greater,
        }
    }

    fn new_empty_row(&mut self) -> DatabaseTable {
        // Instead of requiring `Default` trait for row data types, the viewer is
        // responsible of providing default creation method.
        DatabaseTable::default()
    }
}