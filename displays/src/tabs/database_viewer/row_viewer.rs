
use crate::{get_current_user_from_auth, get_database_users, Interaction};
use database::schema::{ComputerData, CustomerData, TaskPayload, TicketPayload, User, COMPUTER_TABLE, CUSTOMER_TABLE};
use eframe::egui::{Color32, Layout, Response, RichText, TextEdit};
use egui_data_table::RowViewer;
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use egui_extras::Column;
use surrealdb::RecordId;
use std::cmp::Ordering;

/// Every logic is defined in `Viewer`
#[derive(serde::Serialize)]
pub struct DatabaseRowViewer {
    pub current_user: User,
    pub store_users: Vec<User>,
    pub filter: String,
    pub selected: DatabaseTable,
    user_data: User,
    pub selected_table: DatabaseTableSelection,
}

#[derive(PartialEq, Serialize, Deserialize, Clone)]
pub enum DatabaseTable {
    Task(TaskPayload),
    Service(TicketPayload),
    Customer(CustomerData),
    Computer(ComputerData),
    // TaskNote(TaskNotePayload),
    User(User)
}

#[derive(PartialEq, Serialize, Deserialize, Clone, Default)]
pub enum DatabaseTableSelection {
    #[default]
    Task,
    Service,
    Customer,
    Computer,
    User,
}

impl DatabaseTableSelection {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Task => "Task",
            Self::User => "User",
            Self::Computer => "Computer",
            Self::Service => "Service",
            Self::Customer => "Customer",
            // Self::TaskNote => "Note",
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
            Self::Customer(_) => "Customer",
            Self::Service(_) => "Service",
            Self::Computer(_) => "Computer",
            // Self::TaskNote(_) => "Task Note",
            Self::User(_) => "User"
        }
    }
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
            store_users: get_database_users(),
            user_data: User::default(),
            selected: DatabaseTable::default(),
            selected_table: Default::default(), 
        }
    }
}

impl RowViewer<DatabaseTable> for DatabaseRowViewer {
    // fn try_create_codec(&mut self, _: bool) -> Option<impl RowCodec<DatabaseTable>> {
    //     Some(Codec)
    // }

    fn num_columns(&mut self) -> usize {
        match self.selected {
            DatabaseTable::Task(_) => 9,
            DatabaseTable::User(_) => 8,
            DatabaseTable::Service(_) => 8,
            DatabaseTable::Customer(_) => 4,
            DatabaseTable::Computer(_) => 11,
        }
    }

    fn column_name(&mut self, column: usize) -> std::borrow::Cow<'static, str> {
        match self.selected {
            DatabaseTable::Task(_) => ["ID", "Name", "Assignee", "Due", "Order #", "Priority", "Status", "Complete", "Description"][column].into(),
            DatabaseTable::User(_) => ["ID", "Name", "Username", "Email", "Store", "Presta ID", "Store ID", "Auth"][column].into(),
            DatabaseTable::Customer(_) => ["ID", "Name", "Phone #", "Email"][column].into(),
            DatabaseTable::Service(_) => ["ID", "SO#", "Tech", "Salesman", "Check-In Rep", "Customer", "Computer", "Check-In Note"][column].into(),
            DatabaseTable::Computer(_) => ["ID", "Hostname", "MFG", "Model", "Name", "S/N", "CPU", "GPU", "RAM", "OS", "Customer"][column].into(),
            // DatabaseTable::TaskNote(_) => ["ID", "name", "username", "email", "store", "id_prestashop", "id_store", "authorization"][column].into(),
        }
    }

    fn is_sortable_column(&mut self, column: usize) -> bool {
        match self.selected {
            DatabaseTable::Task(_) => [false, true, true, true, true, true, true, true, true][column],
            DatabaseTable::User(_) => [false, true, true, true, true, true, true, true][column],
            DatabaseTable::Customer(_) => [false, true, true, true][column],
            DatabaseTable::Service(_) => [false, true, true, true, true, true, true, true][column],
            DatabaseTable::Computer(_) => [false, true, true, true, true, true, true, true, true, true, true][column],
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
            DatabaseTable::User(user) =>
                user.get_username().contains(&self.filter)
                || user.get_name().contains(&self.filter),
            DatabaseTable::Service(ticket_payload) => 
                ticket_payload.sales_rep.contains(&self.filter)
                || ticket_payload.service_number.contains(&self.filter)
                || ticket_payload.customer.clone().unwrap_or_default().name.contains(&self.filter)
                || ticket_payload.checkin_notes.contains(&self.filter)
                || ticket_payload.checkin_rep.contains(&self.filter)
                || ticket_payload.salesman.contains(&self.filter)
                || ticket_payload.tech.contains(&self.filter),
            DatabaseTable::Customer(customer_data) => 
                customer_data.name.contains(&self.filter)
                || customer_data.email.contains(&self.filter)
                || customer_data.phone_number.contains(&self.filter)
                || customer_data.phone_number_2.contains(&self.filter),
            DatabaseTable::Computer(computer_data) => 
                computer_data.cpu.contains(&self.filter)
                || computer_data.ram.contains(&self.filter)
                || computer_data.gpu.contains(&self.filter)
                || computer_data.device_name.clone().unwrap_or_default().contains(&self.filter)
                || computer_data.device_model.clone().unwrap_or_default().contains(&self.filter)
                || computer_data.hostname.contains(&self.filter),
        }
    }

    fn show_cell_view(&mut self, ui: &mut eframe::egui::Ui, row: &DatabaseTable, column: usize) {
        // ui.add_space(5.);
        let _ = match &row {
            DatabaseTable::Task(task_payload) => {
                let checked = &mut task_payload.completed.clone();
                let _ = match column {
                    // 0 => ui.label(format!(" {}", task_payload.id.key().to_string())),
                    1 => ui.with_layout(Layout::right_to_left(eframe::egui::Align::Min), |ui| {
                        ui.add_space(2.);
                        ui.label(RichText::new(task_payload.task_name.trim()))
                    }).inner,
                    2 => {
                        let user = self.store_users
                            .iter()
                            .find(|u| u.get_id() == task_payload.assignee)
                            .cloned()
                            .unwrap_or_default();
                        ui.vertical_centered(|ui| ui.label(format!(" {}", user.get_username()))).inner
                    },
                    3 => {
                        // Convert to a DateTime with Utc timezone
                        let datetime: DateTime<Utc> = DateTime::from_naive_utc_and_offset(task_payload.due_date.clone().naive_local(), Utc);
                        // Format the DateTime into yyyy/mm/dd
                        let formatted_date = datetime.format(" %m/%d/%Y").to_string();
                        let split1 = formatted_date.split_once('/').unwrap_or_default();
                        let split2 = split1.1.split_once('/').unwrap_or_default();
                        ui.horizontal(|ui| {
                            ui.colored_label(Color32::from_rgb(42, 195, 222), format!("{}/", split1.0));
                            ui.colored_label(Color32::from_rgb(3, 252, 194), format!("{}/", split2.0));
                            ui.colored_label(Color32::from_rgb(66, 69, 245), split2.1)
                        }).inner
                    },
                    4 => ui.vertical_centered(|ui| ui.label(task_payload.service_number.clone().unwrap_or_default().trim())).inner,
                    5 => ui.vertical_centered(|ui| ui.label(format!("{}", task_payload.priority.as_str().trim()))).inner,
                    6 => ui.vertical_centered(|ui| ui.label(format!(" {}", task_payload.status.as_str().trim()))).inner,
                    7 => ui.vertical_centered(|ui| ui.checkbox(checked, "")).inner,
                    8 => ui.label(task_payload.task_description.trim()),
                    _ => ui.label(""),
                };
            },
            DatabaseTable::User(user) => {
                let _ = match column {
                    0 => ui.label(format!(" {}", user.get_id().key().to_string())),
                    1 => ui.label(format!(" {}", user.get_name())),
                    2 => ui.label(format!(" {}", user.get_username())),
                    3 => ui.with_layout(Layout::right_to_left(eframe::egui::Align::Min), |ui| ui.label(format!(" {}", user.get_email()))).response,
                    4 => ui.label(format!(" {}", user.get_store().as_str())),
                    5 => ui.with_layout(Layout::right_to_left(eframe::egui::Align::Min), |ui| ui.label(format!(" {}", user.get_employee_id().unwrap_or(0).to_string()))).response,
                    6 => ui.with_layout(Layout::right_to_left(eframe::egui::Align::Min), |ui| ui.label(format!(" {}", user.get_store_id().unwrap_or(String::new())))).response,
                    7 => ui.vertical_centered(|ui| ui.label(format!(" {}", user.get_authorization().as_str()))).response,
                    _ => ui.label(" ")
                };
            },
            DatabaseTable::Customer(customer) => {
                let _ = match column {
                    0 => ui.label(format!(" {}", customer.id.key().to_string())),
                    1 => ui.with_layout(Layout::right_to_left(eframe::egui::Align::Min), |ui| {
                        ui.add_space(2.);
                        ui.label(RichText::new(customer.name.trim()).underline())
                    }).inner,
                    2 => ui.with_layout(Layout::right_to_left(eframe::egui::Align::Min), |ui| {
                        ui.add_space(2.);
                        ui.label(RichText::new(customer.phone_number.trim()))
                    }).inner,
                    3 => ui.with_layout(Layout::right_to_left(eframe::egui::Align::Min), |ui| {
                        ui.add_space(2.);
                        ui.label(RichText::new(customer.email.trim()))
                    }).inner,
                    _ => ui.label(""),
                };
            },
            DatabaseTable::Computer(computer) => {
                let _ = match column {
                    0 => ui.label(format!(" {}", computer.id.key().to_string())),
                    1 => ui.with_layout(Layout::right_to_left(eframe::egui::Align::Min), |ui| {
                        ui.add_space(2.);
                        ui.label(RichText::new(computer.hostname.trim()).underline())
                    }).inner,
                    2 => ui.with_layout(Layout::right_to_left(eframe::egui::Align::Min), |ui| {
                        ui.add_space(2.);
                        ui.label(RichText::new(computer.device_mfg.clone().unwrap_or_default().trim()))
                    }).inner,
                    3 => ui.with_layout(Layout::right_to_left(eframe::egui::Align::Min), |ui| {
                        ui.add_space(2.);
                        ui.label(RichText::new(computer.device_model.clone().unwrap_or_default().trim()))
                    }).inner,
                    4 => ui.with_layout(Layout::right_to_left(eframe::egui::Align::Min), |ui| {
                        ui.add_space(2.);
                        ui.label(RichText::new(computer.device_name.clone().unwrap_or_default().trim()))
                    }).inner,
                    5 => ui.with_layout(Layout::right_to_left(eframe::egui::Align::Min), |ui| {
                        ui.add_space(2.);
                        ui.label(RichText::new(computer.device_serial.clone().unwrap_or_default().trim()))
                    }).inner,
                    6 => ui.with_layout(Layout::right_to_left(eframe::egui::Align::Min), |ui| {
                        ui.add_space(2.);
                        ui.label(RichText::new(computer.cpu.trim()))
                    }).inner,
                    7 => ui.with_layout(Layout::right_to_left(eframe::egui::Align::Min), |ui| {
                        ui.add_space(2.);
                        ui.label(RichText::new(computer.gpu.trim()))
                    }).inner,
                    8 => ui.with_layout(Layout::right_to_left(eframe::egui::Align::Min), |ui| {
                        ui.add_space(2.);
                        ui.label(RichText::new(computer.ram.trim()))
                    }).inner,
                    9 => ui.with_layout(Layout::right_to_left(eframe::egui::Align::Min), |ui| {
                        ui.add_space(2.);
                        ui.label(RichText::new(computer.operating_system.trim()))
                    }).inner,
                    10 => ui.with_layout(Layout::right_to_left(eframe::egui::Align::Min), |ui| {
                        ui.add_space(2.);
                        ui.label(RichText::new(computer.customer.clone().unwrap_or(RecordId::from((COMPUTER_TABLE, surrealdb::RecordIdKey::from_inner(surrealdb::sql::Id::rand())))).key().to_string()))
                    }).inner,
                    _ => ui.label(""),
                };
            },
            DatabaseTable::Service(ticket) => {
                let _ = match column {
                    0 => ui.label(format!(" {}", ticket.id.key().to_string())),
                    1 => ui.with_layout(Layout::right_to_left(eframe::egui::Align::Min), |ui| {
                        ui.add_space(2.);
                        ui.label(RichText::new(ticket.service_number.trim()).underline())
                    }).inner,
                    2 => ui.with_layout(Layout::right_to_left(eframe::egui::Align::Min), |ui| {
                        ui.add_space(2.);
                        ui.label(RichText::new(ticket.tech.trim()))
                    }).inner,
                    3 => ui.with_layout(Layout::right_to_left(eframe::egui::Align::Min), |ui| {
                        ui.add_space(2.);
                        ui.label(RichText::new(ticket.salesman.trim()))
                    }).inner,
                    4 => ui.vertical_centered(|ui| ui.label(ticket.checkin_rep.trim())).inner,
                    5 => ui.vertical_centered(|ui| ui.label(format!("{}", ticket.customer.clone().unwrap_or_default().name.trim()))).inner,
                    6 => ui.vertical_centered(|ui| ui.label(format!(" {}", ticket.computer.clone().unwrap_or_default().device_model.clone().unwrap_or_default().trim()))).inner,
                    7 => ui.label(ticket.checkin_notes.trim()),
                    _ => ui.label(""),
                };
            },
        };
        // ui.add_space(5.);
    }

    fn column_render_config(&mut self, column: usize, _is_last_visible_column: bool) -> Column {
        let col_config = Column::auto();
        match self.selected {
            DatabaseTable::Task(_) => {
                match column {
                    0 => col_config.resizable(true).at_least(20.).at_most(20.),
                    1 => col_config.resizable(true).at_least(200.).at_most(260.),
                    2 => col_config.resizable(true).at_least(90.).at_most(100.),
                    3 => col_config.resizable(true).at_least(110.).at_most(110.),
                    4 => col_config.resizable(true).at_least(60.).at_most(60.),
                    5 => col_config.resizable(true).at_least(70.).at_most(70.),
                    6 => col_config.resizable(true).at_least(80.).at_most(80.),
                    7 => col_config.resizable(true).at_least(50.).at_most(50.),
                    8 => col_config.resizable(true).at_least(600.),
                    _ => col_config,
                }
            },
            DatabaseTable::User(_) => {
                match column {
                    0 => col_config.resizable(true).at_least(180.).at_most(180.),
                    1 => col_config.resizable(true).at_least(120.).at_most(120.),
                    2 => col_config.resizable(true).at_least(155.).at_most(155.),
                    3 => col_config.resizable(true).at_least(280.).at_most(280.),
                    4 => col_config.resizable(true).at_least(60.).at_most(60.),
                    5 => col_config.resizable(true).at_least(80.).at_most(80.),
                    6 => col_config.resizable(true).at_least(60.).at_most(60.),
                    7 => col_config.resizable(true).at_least(60.).at_most(60.),
                    _ => col_config,
                }
            },
            DatabaseTable::Customer(_) => {
                match column {
                    0 => col_config.resizable(true).at_least(180.).at_most(180.),
                    1 => col_config.resizable(true).at_least(120.).at_most(120.),
                    2 => col_config.resizable(true).at_least(155.).at_most(155.),
                    3 => col_config.resizable(true).at_least(280.).at_most(280.),
                    _ => col_config,
                }
            },
            DatabaseTable::Computer(_) => {
                match column {
                    0 => col_config.resizable(true).at_least(200.).at_most(200.),
                    1 => col_config.resizable(true).at_least(140.).at_most(140.),
                    2 => col_config.resizable(true).at_least(115.).at_most(155.),
                    3 => col_config.resizable(true).at_least(115.).at_most(155.),
                    4 => col_config.resizable(true).at_least(115.).at_most(155.),
                    5 => col_config.resizable(true).at_least(115.).at_most(155.),
                    6 => col_config.resizable(true).at_least(200.).at_most(200.),
                    7 => col_config.resizable(true).at_least(200.).at_most(200.),
                    8 => col_config.resizable(true).at_least(60.).at_most(60.),
                    9 => col_config.resizable(true).at_least(150.).at_most(160.),
                    10 => col_config.resizable(true).at_least(115.).at_most(115.),
                    _ => col_config,
                }
            },
            DatabaseTable::Service(_) => {
                match column {
                    0 => col_config.resizable(true).at_least(180.).at_most(210.),
                    1 => col_config.resizable(true).at_least(100.).at_most(120.),
                    2 => col_config.resizable(true).at_least(100.).at_most(120.),
                    3 => col_config.resizable(true).at_least(100.).at_most(120.),
                    4 => col_config.resizable(true).at_least(100.).at_most(120.),
                    5 => col_config.resizable(true).at_least(100.).at_most(120.),
                    6 => col_config.resizable(true).at_least(100.).at_most(150.),
                    7 => col_config.resizable(true).at_least(250.).at_most(350.),
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
        let response: Option<Response> = match row {
            DatabaseTable::Task(task_payload) => {
                match column {
                    1 => Some(task_payload.interact_task_name(ui)),
                    2 => Some(task_payload.interact_assignee(ui, &self.store_users, &self.current_user)),
                    3 => Some(task_payload.interact_due_date(ui)),
                    4 => Some(task_payload.interact_service_number(ui)),
                    5 => Some(task_payload.interact_priority(ui)),
                    6 => Some(task_payload.interact_status(&self.current_user, ui)),
                    7 => Some(task_payload.interact_completed(ui)),
                    8 => Some(task_payload.interact_task_description(ui)),
                    _ => None,
                }
                .into()
            },
            DatabaseTable::User(user) => {
                match column {
                    0 => Some(ui.label(user.get_id().key().to_string())),
                    1 => Some(TextEdit::singleline(&mut user.get_name()).show(ui).response),
                    2 => Some(TextEdit::singleline(&mut user.get_username()).show(ui).response),
                    3 => Some(TextEdit::singleline(&mut user.get_email()).show(ui).response),
                    4 => Some(TextEdit::singleline(&mut user.get_store().as_str()).show(ui).response),
                    5 => Some(TextEdit::singleline(&mut user.get_employee_id().unwrap_or(0).to_string()).show(ui).response),
                    6 => Some(TextEdit::singleline(&mut user.get_store_id().unwrap_or(String::new())).show(ui).response),
                    7 => Some(TextEdit::singleline(&mut user.get_authorization().as_str()).show(ui).response),
                    _ => None
                }
                .into()
            },
            DatabaseTable::Customer(_customer) => None,
            DatabaseTable::Computer(_computer) => None,
            DatabaseTable::Service(_ticket) => None,
        };
        // ui.shrink_height_to_current();
        // ui.shrink_width_to_current();
        response
    }
    
    fn persist_ui_state(&self) -> bool {
        true
    }
    
    fn on_cell_view_response(
        &mut self,
        _row: &DatabaseTable,
        column: usize,
        resp: &eframe::egui::Response,
    ) -> Option<Box<DatabaseTable>> {
        match column {
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
                let user_l = self.store_users
                    .iter()
                    .find(|u| u.get_id() == task_l.assignee)
                    .cloned()
                    .unwrap_or_default();

                let user_r = self.store_users
                    .iter()
                    .find(|u| u.get_id() == task_r.assignee)
                    .cloned()
                    .unwrap_or_default();

                match column {
                    1 => task_l.task_name.cmp(&task_r.task_name),
                    2 => user_l.get_username().cmp(&user_r.get_username()),
                    3 => task_l.due_date.cmp(&task_r.due_date),
                    4 => task_l.service_number.clone().unwrap_or_default().cmp(&task_r.service_number.clone().unwrap_or_default()),
                    5 => task_l.priority.as_str().cmp(&task_r.priority.as_str()),
                    6 => task_l.status.as_str().cmp(&task_r.status.as_str()),
                    7 => task_l.completed.cmp(&task_r.completed),
                    8 => task_l.task_description.cmp(&task_r.task_description),
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
            (DatabaseTable::Service(svc_l), DatabaseTable::Service(svc_r)) => {
                match column { // "ID", "SO#", "Tech", "Salesman", "Check-In Rep", "Customer", "Computer", "Check-In Note"
                    1 => svc_l.service_number.cmp(&svc_r.service_number),
                    2 => svc_l.tech.cmp(&svc_r.tech),
                    3 => svc_l.salesman.cmp(&svc_r.salesman),
                    4 => svc_l.checkin_rep.cmp(&svc_r.checkin_rep),
                    5 => svc_l.customer.clone().unwrap_or_default().name.cmp(&svc_r.customer.clone().unwrap_or_default().name),
                    6 => svc_l.computer.clone().unwrap_or_default().device_model.cmp(&svc_r.computer.clone().unwrap_or_default().device_model),
                    7 => svc_l.checkin_notes.cmp(&svc_r.checkin_notes),
                    _ => Ordering::Equal, // Default for invalid columns
                }
            },
            (DatabaseTable::Customer(cust_l), DatabaseTable::Customer(cust_r)) => {
                match column {
                    1 => cust_l.name.cmp(&cust_r.name),
                    2 => cust_l.phone_number.cmp(&cust_r.phone_number),
                    3 => cust_l.email.cmp(&cust_r.email),
                    _ => Ordering::Equal, // Default for invalid columns
                }
            },
            (DatabaseTable::Computer(computer_l), DatabaseTable::Computer(computer_r)) => {
                match column {
                    1 => computer_l.hostname.cmp(&computer_r.hostname),
                    2 => computer_l.device_mfg.clone().unwrap_or_default().cmp(&computer_r.device_mfg.clone().unwrap_or_default()),
                    3 => computer_l.device_model.clone().unwrap_or_default().cmp(&computer_r.device_model.clone().unwrap_or_default()),
                    4 => computer_l.device_name.clone().unwrap_or_default().cmp(&computer_r.device_name.clone().unwrap_or_default()),
                    5 => computer_l.device_serial.clone().unwrap_or_default().cmp(&computer_r.device_serial.clone().unwrap_or_default()),
                    6 => computer_l.cpu.as_str().cmp(&computer_r.cpu),
                    7 => computer_l.gpu.cmp(&computer_r.gpu),
                    8 => computer_l.ram.cmp(&computer_r.ram),
                    9 => computer_l.operating_system.cmp(&computer_r.operating_system),
                    10 => computer_l.customer.clone().unwrap_or(
                        RecordId::from((CUSTOMER_TABLE, surrealdb::RecordIdKey::from_inner(surrealdb::sql::Id::rand())))
                        ).cmp(&computer_r.customer.clone().unwrap_or(
                            RecordId::from((CUSTOMER_TABLE, surrealdb::RecordIdKey::from_inner(surrealdb::sql::Id::rand())))
                            )
                        ),
                    _ => Ordering::Equal, // Default for invalid columns
                }
            },
            (_, _) => Ordering::Equal
        }
    }

    fn new_empty_row(&mut self) -> DatabaseTable {
        // Instead of requiring `Default` trait for row data types, the viewer is
        // responsible of providing default creation method.
        DatabaseTable::default()
    }
}