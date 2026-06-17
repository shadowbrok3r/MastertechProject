use crate::{channel_manager::ChannelManager, chats::ChatView, Spawner};
use crate::ui_tools::{icons, theme};
use eframe::egui::{Color32, ComboBox, Hyperlink, Id, Label, Widget};
use database::schema::{Store, TaskNotePayload, User, LiveTaskPayload, helper_traits::parse_email_user, prestashop::{Prestashop, OrderState}, prestashop_schema::{Employee, MissedCallOrder, PrestashopPayload}};
use std::collections::HashMap;
use database::schema::prestashop::xml::{modify_xml, remove_xml_tag};
use database::xidax_order_url;
use chrono::{DateTime, NaiveDateTime, Utc};
use egui_data_table::{viewer::RowCodec, RowViewer};
use crate::PlatformSpawner;
use crossbeam::channel::{Receiver, Sender};
use egui_extras::Column;
use log::info;

use super::codec::Codec;

/// In-table edits that must be reflected immediately, emitted from the
/// row comboboxes and applied to the cached table by the owning view.
pub enum RowFieldUpdate {
    Status { order_id: String, new_state: String },
    SalesRep { order_id: String, employee: Employee },
    SplitRep { order_id: String, employee: Option<Employee> },
}

#[derive(serde::Serialize)]
pub struct TaskRowViewer {
    pub filter: String,
    row_protection: bool,
    pub selected: Option<PrestashopPayload>,
    order_data: PrestashopPayload,
    pub chat_view: ChatView,
    #[serde(skip)]
    pub notes_channel: (Sender<Vec<TaskNotePayload>>, Receiver<Vec<TaskNotePayload>>),
    #[serde(skip)]
    pub tur_channel: (Sender<PrestashopPayload>, Receiver<PrestashopPayload>),
    #[serde(skip)]
    pub create_task_channel: (Sender<PrestashopPayload>, Receiver<PrestashopPayload>),
    #[serde(skip)]
    pub field_update_channel: (Sender<RowFieldUpdate>, Receiver<RowFieldUpdate>),
    #[serde(skip)]
    pub note_created_channel: (Sender<String>, Receiver<String>),
    pub missed_calls: Vec<MissedCallOrder>,
    pub store_selection: u64,
    #[serde(skip)]
    pub users: Vec<User>,
    #[serde(skip)]
    pub existing_tasks: HashMap<String, LiveTaskPayload>,
    #[serde(skip)]
    pub open_task_channel: (Sender<LiveTaskPayload>, Receiver<LiveTaskPayload>),
    pub first_run: bool,
}

impl Default for TaskRowViewer {
    fn default() -> Self {
        let notes_channel = <Vec<TaskNotePayload>>::create_unbounded_channel();
        let tur_channel = PrestashopPayload::create_unbounded_channel();
        let create_task_channel = PrestashopPayload::create_unbounded_channel();
        let field_update_channel = crossbeam::channel::unbounded();
        let note_created_channel = crossbeam::channel::unbounded();
        let open_task_channel = crossbeam::channel::unbounded();
        Self {
            notes_channel,
            tur_channel,
            create_task_channel,
            field_update_channel,
            note_created_channel,
            open_task_channel,
            existing_tasks: HashMap::new(),
            filter: Default::default(),
            row_protection: Default::default(),
            selected: Default::default(),
            chat_view: ChatView::default(),
            order_data: PrestashopPayload::default(),
            missed_calls: Vec::new(),
            store_selection: Store::RIV.into_store_id() as u64,
            users: Vec::new(),
            first_run: true,
        }
    }
}

impl TaskRowViewer {
    /// Rebuilds the service-number to task lookup from the task list.
    pub fn sync_existing_tasks(&mut self, tasks: &[LiveTaskPayload]) {
        self.existing_tasks.clear();
        for task in tasks {
            if let Some(service_number) = &task.service_number {
                if !service_number.is_empty() {
                    self.existing_tasks.insert(service_number.clone(), task.clone());
                }
            }
        }
    }
}

impl RowViewer<PrestashopPayload> for TaskRowViewer {
    fn try_create_codec(&mut self, _: bool) -> Option<impl RowCodec<PrestashopPayload>> { Some(Codec) }
    fn row_filter_hash(&mut self) -> &impl std::hash::Hash { &self.filter }
    fn is_sortable_column(&mut self, column: usize) -> bool { column != 10 } // Create Task column not sortable
    fn num_columns(&mut self) -> usize { 11 }
    fn column_name(&mut self, column: usize) -> std::borrow::Cow<'static, str> {
        ["Order #", "Customer Name", "Date", "Status", "Sales Rep", "Split Rep", "# Missed Calls", "Device", "Model", "Checkin Notes", ""][column].into()
    }

    fn filter_row(&mut self, row: &PrestashopPayload) -> bool {
        let filter = self.filter.trim().to_lowercase();
        if filter.is_empty() {
            return true;
        }

        let service = row.order.associations.order_service.get(0).cloned().unwrap_or_default();
        let sales = row.sales_rep.clone().unwrap_or_default();
        let split = row.split_rep.clone().unwrap_or_default();

        // Match against the m/d/Y display date as well as the raw date_add.
        let formatted_date = NaiveDateTime::parse_from_str(&row.order.date_add, "%Y-%m-%d %H:%M:%S")
            .map(|ndt| {
                let dt: DateTime<Utc> = DateTime::from_naive_utc_and_offset(ndt, Utc);
                dt.format("%m/%d/%Y").to_string()
            })
            .unwrap_or_default();

        [
            row.order.id.to_lowercase(),
            row.customer.name.to_lowercase(),
            formatted_date,
            row.order.date_add.to_lowercase(),
            sales.firstname.to_lowercase(),
            sales.lastname.to_lowercase(),
            split.firstname.to_lowercase(),
            split.lastname.to_lowercase(),
            service.device_mfg.to_lowercase(),
            service.device_model.to_lowercase(),
            service.check_in_notes.to_lowercase(),
        ]
        .iter()
        .any(|field| field.contains(&filter))
    }

    fn show_cell_view(&mut self, ui: &mut eframe::egui::Ui, row: &PrestashopPayload, column: usize) {
        let style = ui.style_mut();
        style.interaction.multi_widget_text_select = false;
        style.interaction.selectable_labels = false;

        let _ = match column {
            0 => { 
                // ui.colored_label(ui.style().visuals.hyperlink_color, format!(" {}", row.order.id.clone())); 
                let url = row.order.id.clone();
                let res = Hyperlink::from_label_and_url(
                    format!(" {}", url), 
                    xidax_order_url(&url)
                )
                .open_in_new_tab(true)
                .ui(ui);

                if res.clicked() {
                    log::error!("Clicked on order: {}", row.order.id);
                }
            },
            1 => { 
                if ui.button(format!(" {} ⬈", row.customer.name.clone())).clicked() {
                    log::error!("Clicked on customer: {}", row.customer.name);
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
                // Status ComboBox
                let current_state = &row.order.current_state;
                let current_display = OrderState::from_id_str(current_state);
                let order_id = row.order.id.clone();
                
                ComboBox::from_id_salt(Id::new(format!("status_{}", order_id)))
                    .selected_text(current_display)
                    .width(100.)
                    .show_ui(ui, |ui| {
                        for state in [OrderState::CheckinShelf, OrderState::InRepair, OrderState::DoneShelf] {
                            let is_current = state.to_id_str() == current_state;
                            if ui.selectable_label(is_current, state.as_str()).clicked() && !is_current {
                                // Only update if selecting a different status
                                let new_state = state.to_id_str().to_string();
                                let order_id_clone = order_id.clone();
                                log::info!("Status changed for order: {order_id_clone}, new_state: {new_state}");
                                let _ = self.field_update_channel.0.try_send(RowFieldUpdate::Status {
                                    order_id: order_id.clone(),
                                    new_state: new_state.clone(),
                                });
                                PlatformSpawner::spawn(async move {
                                    update_order_field(&order_id_clone, "current_state", &new_state).await;
                                });
                            }
                        }
                    });
            },
            4 => {
                // Sales Rep ComboBox
                let current_emp = row.sales_rep.clone().unwrap_or_default();
                let current_emp_id = current_emp.id.clone();
                let current_name = parse_email_user(&current_emp.email);
                let order_id = row.order.id.clone();
                let users = self.users.clone();
                
                // Check if current is the checkin shelf employee (id 1347)
                let is_checkin_shelf = current_emp_id == "1347";
                
                ComboBox::from_id_salt(Id::new(format!("sales_rep_{}", order_id)))
                    .selected_text(current_name)
                    .width(100.)
                    .height(200.)
                    .show_ui(ui, |ui| {
                        // Add CheckinShelf option to put back on checkin shelf
                        if ui.selectable_label(is_checkin_shelf, "Check-in Shelf").clicked() && !is_checkin_shelf {
                            let order_id_clone = order_id.clone();
                            log::info!("Sales Rep changed to Check-in Shelf for order: {}", order_id_clone);
                            let _ = self.field_update_channel.0.try_send(RowFieldUpdate::SalesRep {
                                order_id: order_id.clone(),
                                employee: Employee { id: "1347".to_string(), email: "Check-in Shelf".to_string(), ..Default::default() },
                            });
                            PlatformSpawner::spawn(async move {
                                update_order_field(&order_id_clone, "id_employee_sales_rep", "1347").await;
                            });
                        }
                        ui.separator();
                        for user in users.iter().filter(|u| u.is_active()) {
                            let user_emp_id = user.get_employee_id().map(|id| id.to_string()).unwrap_or_default();
                            let is_selected = user_emp_id == current_emp_id;
                            if ui.selectable_label(is_selected, user.get_username()).clicked() && !is_selected {
                                // Only update if selecting a different employee
                                if let Some(emp_id) = user.get_employee_id() {
                                    let order_id_clone = order_id.clone();
                                    let emp_id_str = emp_id.to_string();
                                    log::info!("Sales Rep changed for order: {order_id_clone}, new emp_id: {emp_id_str}");
                                    let _ = self.field_update_channel.0.try_send(RowFieldUpdate::SalesRep {
                                        order_id: order_id.clone(),
                                        employee: Employee {
                                            id: emp_id_str.clone(),
                                            email: user.get_email().to_string(),
                                            firstname: user.get_username().to_string(),
                                            ..Default::default()
                                        },
                                    });
                                    PlatformSpawner::spawn(async move {
                                        update_order_field(&order_id_clone, "id_employee_sales_rep", &emp_id_str).await;
                                    });
                                }
                            }
                        }
                    });
            },
            5 => {
                // Split Rep ComboBox
                let current_emp = row.split_rep.clone().unwrap_or_default();
                let current_emp_id = current_emp.id.clone();
                let current_name = parse_email_user(&current_emp.email);
                let order_id = row.order.id.clone();
                let users = self.users.clone();
                
                // Check if current is empty/none
                let is_none = current_emp_id.is_empty();
                
                ComboBox::from_id_salt(Id::new(format!("split_rep_{}", order_id)))
                    .selected_text(current_name)
                    .width(100.)
                    .height(200.)
                    .show_ui(ui, |ui| {
                        // Add option to clear split rep
                        if ui.selectable_label(is_none, "None").clicked() && !is_none {
                            let order_id_clone = order_id.clone();
                            log::info!("Split Rep cleared for order: {order_id_clone}");
                            let _ = self.field_update_channel.0.try_send(RowFieldUpdate::SplitRep {
                                order_id: order_id.clone(),
                                employee: None,
                            });
                            PlatformSpawner::spawn(async move {
                                update_order_field(&order_id_clone, "id_employee_split_rep", "").await;
                            });
                        }
                        ui.separator();
                        for user in users.iter().filter(|u| u.is_active()) {
                            let user_emp_id = user.get_employee_id().map(|id| id.to_string()).unwrap_or_default();
                            let is_selected = user_emp_id == current_emp_id;
                            if ui.selectable_label(is_selected, user.get_username()).clicked() && !is_selected {
                                // Only update if selecting a different employee
                                if let Some(emp_id) = user.get_employee_id() {
                                    let order_id_clone = order_id.clone();
                                    let emp_id_str = emp_id.to_string();
                                    log::info!("Split Rep changed for order: {order_id_clone}, new emp_id: {emp_id_str}");
                                    let _ = self.field_update_channel.0.try_send(RowFieldUpdate::SplitRep {
                                        order_id: order_id.clone(),
                                        employee: Some(Employee {
                                            id: emp_id_str.clone(),
                                            email: user.get_email().to_string(),
                                            firstname: user.get_username().to_string(),
                                            ..Default::default()
                                        }),
                                    });
                                    PlatformSpawner::spawn(async move {
                                        update_order_field(&order_id_clone, "id_employee_split_rep", &emp_id_str).await;
                                    });
                                }
                            }
                        }
                    });
            },
            6 => {
                let call = self.missed_calls.iter().find(|o| o.id == row.order.id).cloned();
                if let Some(missed_call) = call {
                    let num = missed_call.missing_days.len();
                    let txt = if num == 1 {
                        format!(" {num} Missed Call")
                    } else {
                        format!(" {num} Missed Calls")
                    };
                    ui.colored_label(ui.style().visuals.error_fg_color, txt);
                }
            },
            7 => { ui.label(format!(" {}", row.order.associations.order_service.get(0).cloned().unwrap_or_default().device_mfg)); },
            8 => { ui.label(format!(" {}", row.order.associations.order_service.get(0).cloned().unwrap_or_default().device_model)); },
            9 => { Label::new(format!(" {}", row.order.associations.order_service.get(0).cloned().unwrap_or_default().check_in_notes.clone())).wrap().ui(ui); },
            10 => {
                let order_id = row.order.id.clone();
                if let Some(task) = self.existing_tasks.get(&order_id).cloned() {
                    let open_task_tx = self.open_task_channel.0.clone();
                    let color = theme::success(ui);
                    if ui.button(icons::icon_colored(icons::TASK_EXISTS, color)).on_hover_text("Open Task").clicked() {
                        info!("Opening existing task for order: {order_id}");
                        let _ = open_task_tx.try_send(task);
                    }
                } else {
                    let row_clone = row.clone();
                    let create_task_tx = self.create_task_channel.0.clone();
                    if ui.button(icons::icon(icons::TASK_CREATE)).on_hover_text("Create Task").clicked() {
                        info!("Creating task for order: {}", row.order.id);
                        let _ = create_task_tx.try_send(row_clone);
                    }
                }
            },
            _ => {},
        };
    }

    fn column_render_config(&mut self, column: usize, _is_last_visible_column: bool) -> Column {
        let col_config = Column::auto();
        match column {
            0 => col_config.resizable(true).at_least(60.).at_most(60.),
            1 => col_config.resizable(true).at_least(180.).at_most(225.),
            2 => col_config.resizable(true).at_least(90.).at_most(100.),
            3 => col_config.resizable(true).at_least(110.).at_most(130.),
            4 => col_config.resizable(true).at_least(110.).at_most(150.),
            5 => col_config.resizable(true).at_least(110.).at_most(150.),
            6 => col_config.resizable(true).at_least(100.).at_most(150.),
            7 => col_config.resizable(true).at_least(100.).at_most(150.),
            8 => col_config.resizable(true).at_least(100.).at_most(150.),
            9 => col_config.resizable(true).at_least(150.),
            10 => col_config.resizable(false).at_least(30.).at_most(30.),
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
            0 => {
                let resp = Some(
                    Hyperlink::from_label_and_url(
                        format!(" {}", row.order.id.clone()), 
                        xidax_order_url(&row.order.id)
                    )
                    .open_in_new_tab(true)
                    .ui(ui)
                );
                
                if resp.is_some() {
                    log::error!("Clicked on order: {}", row.order.id);
                }

                resp
            },
            _ => None,
        }
    } ///////////// TODO

    fn on_cell_view_response(
        &mut self,
        row: &PrestashopPayload,
        column: usize,
        resp: &eframe::egui::Response,
    ) -> Option<Box<PrestashopPayload>> {
        // Skip interactive columns: 0 (Hyperlink), 3 (Status ComboBox), 4 (Sales Rep ComboBox), 
        // 5 (Split Rep ComboBox), 10 (Create Task Button)
        let is_interactive_column = matches!(column, 0 | 3 | 4 | 5 | 10);
        
        if resp.clicked() && !is_interactive_column {
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
            7 => dst.order.associations = src.order.associations.clone(), // order_service.get(0).cloned().unwrap_or_default().device_mfg.clone()
            8 => dst.sales_rep = src.sales_rep.clone(),
            9 => dst.order.associations = src.order.associations.clone(), // order_service.get(0).cloned().unwrap_or_default().check_in_notes.clone()
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
            6 => {
                let call_l = self.missed_calls.iter().find(|o| o.id == row_l.order.id).cloned();
                let call_r = self.missed_calls.iter().find(|o| o.id == row_r.order.id).cloned();
                if let (Some(missed_l), Some(missed_r)) = (call_l, call_r) {
                    missed_l.missing_days.len().cmp(&missed_r.missing_days.len())
                } else {
                    std::cmp::Ordering::Equal
                }
            }
            7 => row_l.order.associations.order_service.get(0).cloned().unwrap_or_default().device_mfg.cmp(&row_r.order.associations.order_service.get(0).cloned().unwrap_or_default().device_mfg),
            8 => row_l.order.associations.order_service.get(0).cloned().unwrap_or_default().device_model.cmp(&row_r.order.associations.order_service.get(0).cloned().unwrap_or_default().device_model),
            9 => row_l.order.associations.order_service.get(0).cloned().unwrap_or_default().check_in_notes.cmp(&row_r.order.associations.order_service.get(0).cloned().unwrap_or_default().check_in_notes),
            _ => std::cmp::Ordering::Equal
        }
    }

    fn new_empty_row(&mut self) -> PrestashopPayload { PrestashopPayload::default() }
}

/// Helper function to update an order field via Prestashop API
async fn update_order_field(order_id: &str, field: &str, new_value: &str) {
    let api = Prestashop::default();
    match api.request_raw_resource_by_id("orders", order_id).await {
        Ok(xml) => {
            match modify_xml(&xml, field, new_value) {
                Ok(new_xml) => {
                    log::info!("new_xml: {new_xml}");
                    match remove_xml_tag(&new_xml, "tax_exempt") {
                        Ok(final_xml) => {
                            match api.modify_prestashop_order(&final_xml).await {
                                Ok(_) => info!("Successfully updated {} to {} for order {}", field, new_value, order_id),
                                Err(e) => log::error!("Error modifying prestashop order: {e:?}"),
                            }
                        }
                        Err(e) => log::error!("Error removing tax_exempt tag: {:?}", e),
                    }
                }
                Err(e) => log::error!("Error modifying XML: {e:?}")
            }
        },
        Err(e) => log::error!("Error getting XML order: {e:?}"),
    }
}