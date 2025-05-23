use database::schema::{ComputerData, CustomerData, TaskNotePayload, TaskPayload, TicketPayload, User};
use egui_data_table::{viewer::RowCodec, RowViewer};
use serde::{Deserialize, Serialize};
use crate::{get_current_user_from_auth, tabs::task_audit::codec::Codec};

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

#[derive(PartialEq, Serialize, Deserialize)]
pub enum DatabaseTable {
    Task(TaskPayload),
    Customer(CustomerData),
    Ticket(TicketPayload),
    Computer(ComputerData),
    TaskNote(TaskNotePayload),
}

impl Default for DatabaseTable {
    fn default() -> Self {
        Self::Task(TaskPayload::default())
    }
}

impl DatabaseTable {
    pub fn as_str(&self) -> &str {
        match self {
            DatabaseTable::Task(_) => "Task",
            DatabaseTable::Customer(_) => "Customer",
            DatabaseTable::Ticket(_) => "Ticket",
            DatabaseTable::Computer(_) => "Computer",
            DatabaseTable::TaskNote(_) => "Task Note",
        }
    }
    
    pub fn from_str(table: &str) -> Self {
        match table {
            "Task" => Self::Task(_),
            "Customer" => Self::Customer(_),
            "Ticket" => Self::Ticket(_),
            "Computer" => Self::Computer(_),
            "Task Note" => Self::TaskNote(_),
            _ => Self::default()
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
            // selected: Default::default(),
            user_data: User::default(),
            task_data: TaskPayload::default(),
            ticket_data: TicketPayload::default(),
            selected: DatabaseTable::default(),
        }
    }
}


impl RowViewer<DatabaseTable> for DatabaseViewer {
    fn try_create_codec(&mut self, _: bool) -> Option<impl RowCodec<DatabaseTable>> {
        Some(Codec)
    }

    fn num_columns(&mut self) -> usize {
        9
    }

    fn column_name(&mut self, column: usize) -> std::borrow::Cow<'static, str> {
        ["Order #", "Customer Name", "Date", "Status", "Sales Rep", "Split Rep", "Device", "Model", "Checkin Notes"][column].into()
    }

    fn is_sortable_column(&mut self, column: usize) -> bool {
        [true, true, true, true, true, true, true, true, true][column]
    }

    fn row_filter_hash(&mut self) -> &impl std::hash::Hash {
        &self.filter
    }

    fn filter_row(&mut self, row: &PrestashopPayload) -> bool {
        row.order.id.contains(&self.filter) 
        || row.customer.name.to_lowercase().contains(&self.filter)
        || row.sales_rep.clone().unwrap_or_default().firstname.to_lowercase().contains(&self.filter)
        || row.sales_rep.clone().unwrap_or_default().lastname.to_lowercase().contains(&self.filter)
        || row.split_rep.clone().unwrap_or_default().firstname.to_lowercase().contains(&self.filter)
        || row.split_rep.clone().unwrap_or_default().lastname.to_lowercase().contains(&self.filter)
    }

    fn hotkeys(&mut self, context: &UiActionContext) -> Vec<(KeyboardShortcut, UiAction)> {
        let hotkeys = default_hotkeys(context);
        self.hotkeys.clone_from(&hotkeys);
        hotkeys
    }

    fn show_cell_view(&mut self, ui: &mut eframe::egui::Ui, row: &PrestashopPayload, column: usize) {
        let _ = match column {
            0 => { ui.colored_label(ui.style().visuals.warn_fg_color, format!(" {}", row.order.id.clone())); },
            1 => { ui.label(format!(" {}", row.customer.name.clone())); },
            2 => {
                // Parse the input into a NaiveDateTime
                let naive_datetime = NaiveDateTime::parse_from_str(&row.order.date_add, "%Y-%m-%d %H:%M:%S").unwrap_or_default();

                // Convert to a DateTime with Utc timezone
                let datetime: DateTime<Utc> = DateTime::from_naive_utc_and_offset(naive_datetime, Utc);

                // Format the DateTime into yyyy/mm/dd
                let formatted_date = datetime.format(" %m/%d/%Y").to_string();
                let split1 = formatted_date.split_once('/').unwrap_or_default();
                let split2 = split1.1.split_once('/').unwrap_or_default();
                ui.horizontal(|ui| {
                    ui.colored_label(Color32::from_rgb(42, 195, 222), format!("{}/", split1.0));
                    ui.colored_label(ui.style().visuals.error_fg_color, format!("{}/", split2.0));
                    ui.colored_label(ui.style().visuals.warn_fg_color, split2.1)
                }).inner;
            },
            3 => {
                let status = match row.order.current_state.as_str() {
                    "30" => "In Repair",
                    "40" => "Done Shelf",
                    "4" => "Shipped",
                    "29" => "Check-in Shelf",
                    "239" => "Accepted by Odoo",
                    _ => ""
                };

                ui.label(format!(" {status}"));
            },
            4 => {
                let emp = row.sales_rep.clone().unwrap_or_default();
                let split = parse_email_user(&emp.email);
                ui.label(format!(" {split}"));
            },
            5 => {
                let emp = row.split_rep.clone().unwrap_or_default();
                let split = parse_email_user(&emp.email);
                ui.label(format!(" {split}"));
            },
            6 => { ui.label(format!(" {}", row.order.associations.order_service.get(0).cloned().unwrap_or_default().device_mfg)); },
            7 => { ui.label(format!(" {}", row.order.associations.order_service.get(0).cloned().unwrap_or_default().device_model)); },
            8 => { Label::new(format!(" {}", row.order.associations.order_service.get(0).cloned().unwrap_or_default().check_in_notes.clone())).wrap().ui(ui); },
            _ => {},
        };
    }

    fn column_render_config(&mut self, column: usize, _is_last_visible_column: bool) -> Column {
        let col_config = Column::auto();
        match column {
            0 => col_config.resizable(true).at_least(60.).at_most(60.),
            1 => col_config.resizable(true).at_least(180.).at_most(225.),
            2 => col_config.resizable(true).at_least(90.).at_most(100.),
            3 => col_config.resizable(true).at_least(130.).at_most(130.),
            4 => col_config.resizable(true).at_least(100.).at_most(150.),
            5 => col_config.resizable(true).at_least(100.).at_most(150.),
            6 => col_config.resizable(true).at_least(100.).at_most(150.),
            7 => col_config.resizable(true).at_least(100.).at_most(150.),
            8 => col_config.resizable(true).at_least(150.),
            _ => col_config,
        }
    }
    
    fn show_cell_editor(
        &mut self,
        ui: &mut eframe::egui::Ui,
        row: &mut PrestashopPayload,
        column: usize,
    ) -> Option<eframe::egui::Response> {
        match column {
            0 => Some(
                Hyperlink::from_label_and_url(
                    format!(" {}", row.order.id.clone()), 
                    format!("{BASE_URL}{}", row.order.id.clone())
                )
                .open_in_new_tab(true)
                .ui(ui)
            ),
            _ => None,
        }
    }

    fn on_cell_view_response(
        &mut self,
        row: &PrestashopPayload,
        column: usize,
        resp: &eframe::egui::Response,
    ) -> Option<Box<PrestashopPayload>> {
        match column {
            0 | 1 => {
                if resp.clicked() {
                    log::info!("Clicked Col/Row: {column}/{}", row.order.id);
                    self.chat_view.messages.clear();
                    self.selected = Some(row.clone());
                    let notes_tx = self.notes_channel.0.clone();
                    let service_number = row.order.id.clone();
                    PlatformSpawner::spawn(async move {
                        match Self::get_order_notes(service_number).await {
                            Ok(notes) => notes_tx.try_send(notes).unwrap(),
                            Err(e) => log::error!("Error {e:?}"),
                        };
                    });
                }
            },
            _ => {}
        }
    
        resp
            .clone()
            .on_hover_and_drag_cursor(eframe::egui::CursorIcon::Crosshair)
            .dnd_release_payload::<String>()
            .map(|_| Box::new(PrestashopPayload::default()))
    }

    fn set_cell_value(
        &mut self,
        src: &PrestashopPayload,
        dst: &mut PrestashopPayload,
        column: usize,
    ) {
        match column {
            0 => dst.order.id = src.order.id.clone(),
            1 => dst.customer.name = src.customer.name.clone(),
            2 => dst.order.date_add = src.order.date_add.clone(),
            3 => dst.order.current_state = src.order.current_state.clone(),
            4 => dst.sales_rep = src.sales_rep.clone(),
            5 => dst.split_rep = src.split_rep.clone(),
            8 => dst.order.associations = src.order.associations.clone(), // order_service.get(0).cloned().unwrap_or_default().check_in_notes.clone()
            6 => dst.order.associations = src.order.associations.clone(), // order_service.get(0).cloned().unwrap_or_default().device_mfg.clone()
            7 => dst.sales_rep = src.sales_rep.clone(),
            _ => {},
        }
    }

    fn compare_cell(
        &self,
        row_l: &PrestashopPayload,
        row_r: &PrestashopPayload,
        column: usize,
    ) -> std::cmp::Ordering {
        match column {
            0 => row_l.order.id.cmp(&row_r.order.id),
            1 => row_l.customer.name.cmp(&row_r.customer.name),
            2 => row_l.order.date_add.cmp(&row_r.order.date_add),
            3 => row_l.order.current_state.cmp(&row_r.order.current_state),
            4 => {
                let emp = row_l.sales_rep.clone().unwrap_or_default();
                let name = format!("{} {}", emp.firstname, emp.lastname);
                let emp1 = row_r.sales_rep.clone().unwrap_or_default();
                let name1 = format!("{} {}", emp1.firstname, emp1.lastname);
                name.cmp(&name1)
            },
            5 => {
                let emp = row_l.split_rep.clone().unwrap_or_default();
                let name = format!("{} {}", emp.firstname, emp.lastname);
                let emp1 = row_r.split_rep.clone().unwrap_or_default();
                let name1 = format!("{} {}", emp1.firstname, emp1.lastname);
                name.cmp(&name1)
            },
            6 => row_l.order.associations.order_service.get(0).cloned().unwrap_or_default().device_mfg.cmp(&row_r.order.associations.order_service.get(0).cloned().unwrap_or_default().device_mfg),
            7 => row_l.order.associations.order_service.get(0).cloned().unwrap_or_default().device_model.cmp(&row_r.order.associations.order_service.get(0).cloned().unwrap_or_default().device_model),
            8 => row_l.order.associations.order_service.get(0).cloned().unwrap_or_default().check_in_notes.cmp(&row_r.order.associations.order_service.get(0).cloned().unwrap_or_default().check_in_notes),
            _ => row_l.sales_rep.clone().unwrap_or_default().lastname.cmp(&row_r.sales_rep.clone().unwrap_or_default().lastname)
        }
    }

    fn new_empty_row(&mut self) -> PrestashopPayload {
        // Instead of requiring `Default` trait for row data types, the viewer is
        // responsible of providing default creation method.
        PrestashopPayload::default()
    }
}