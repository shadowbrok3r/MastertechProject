use database::{schema::{prestashop_schema::PrestashopPayload, ComputerData, CustomerData, LiveTaskPayload, Priority, Status, TaskNotePayload, TaskCreationResult, TicketData, User, assignee_names, prestashop::OrderType, entity_link::computer_has_minimal_hardware},db};
use crate::{get_current_user_from_auth, get_toast_sender, ui_tools::autocomplete::AutoCompleteTextEdit, DisplayModal, PlatformSpawner, Spawner, TaskUiActions, ToastMessage};
use crate::ui_tools::{
    controls::{
        self, BarKind, ButtonKind, IconBox, IconLabel, Metrics, SpinnerSize, TextArea, TextButton,
        TextField, Tokens,
    },
    icons::svg,
    theme,
};
use eframe::egui::{Align, Align2, Frame, Layout, Margin, Stroke, Ui, Vec2, Widget, vec2};
use database::schema::utilities::create_full_task_payload;
use database::schema::{fetch_prestashop_order, OrderLookup};
use database::schema::task_creation::{
    apply_extracted_specs, apply_prestashop_payload, EntityDraft, PrestaMapMode,
    PrestaMapOptions,
};
use super::{tabs::{display_ticket_page, display_computer_page}, task_modal::ModalAction};
use database::schema::Datetime;
use chrono::Utc;
use egui_extras::DatePickerButton;
use crossbeam::channel::{Sender, Receiver};
use log::{error, info};
use serde::{Deserialize, Serialize};

/// Width of the single-line fields.
const FIELD_W: f32 = 200.0;
/// Width of the priority dropdown.
const PRIORITY_W: f32 = 110.0;
/// Smallest width of the Submit button.
const SUBMIT_W: f32 = 130.0;
/// Width of the description area.
const DESCRIPTION_W: f32 = 350.0;
/// Longest service number the field accepts.
const SERVICE_NUMBER_MAX: usize = 8;

/// Whether `assignee` maps to a real `user` record.
#[derive(Serialize, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssigneeVerification {
    #[default]
    Unverified,
    Pending,
    Verified,
    NotFound,
}

#[derive(Serialize, Default, Debug, Clone)]
pub struct CreateTaskModal {
    pub title: String,
    pub current_page_state: ModalAction,
    pub min_width: Option<f32>,
    pub min_height: Option<f32>,
    pub default_height: Option<f32>,
    pub full_span_content: bool,
    pub store_users: Vec<User>,

    pub task_name: String,
    pub task_priority: Priority,
    pub due_date: Datetime,
    pub description: String,
    pub assignee: String,
    pub tur: Tur,
    #[serde(skip)]
    pub prestashop_api_tx: Option<Sender<PrestashopPayload>>,
    user: User,
    /// Flag indicating task creation is in progress
    pub creating_task: bool,
    /// Channel for receiving task creation result
    #[serde(skip)]
    pub creation_result_tx: Option<Sender<TaskCreationResult>>,
    #[serde(skip)]
    pub creation_result_rx: Option<Receiver<TaskCreationResult>>,
    /// Text to copy to clipboard after successful creation
    pub clipboard_text: Option<String>,
    /// Verification state of `assignee` against the `user` table.
    pub assignee_status: AssigneeVerification,
    /// The `assignee` value that `assignee_status` refers to.
    pub verified_assignee: String,
    #[serde(skip)]
    pub assignee_verify_tx: Option<Sender<(String, bool)>>,
    #[serde(skip)]
    pub assignee_verify_rx: Option<Receiver<(String, bool)>>,
    /// Opens the task a service number already has instead of creating another.
    #[serde(skip)]
    pub ui_actions_tx: Option<Sender<TaskUiActions>>,
}

// TODO This is an ugly implementation
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Tur {
    pub data: PrestashopPayload,
    pub ticket_data: TicketData,
    pub task_data: LiveTaskPayload,
    pub customer_data: CustomerData,
    pub computer_data: ComputerData,
    pub task_notes: Vec<TaskNotePayload>,
    pub store_users: Vec<User>,
}

impl CreateTaskModal {
    /// Create a new modal with the given title.
    pub fn new(
        title: &str,
        store_users: Vec<User>,
        prestashop_api_tx: Sender<PrestashopPayload>,
    ) -> Self {
        let (creation_result_tx, creation_result_rx) = crossbeam::channel::unbounded();
        let (assignee_verify_tx, assignee_verify_rx) = crossbeam::channel::unbounded();
        Self {
            title: title.to_owned(),
            min_width: Some(500.0),
            min_height: Some(500.0),
            default_height: Some(500.0),
            full_span_content: false,
            due_date: Utc::now().into(),
            store_users,
            prestashop_api_tx: Some(prestashop_api_tx),
            tur: Tur::default(),
            current_page_state: ModalAction::TicketInfoPage,
            user: if let Some(user) = get_current_user_from_auth() {
                user
            } else {
                User::default()
            },
            creating_task: false,
            creation_result_tx: Some(creation_result_tx),
            creation_result_rx: Some(creation_result_rx),
            clipboard_text: None,
            assignee_verify_tx: Some(assignee_verify_tx),
            assignee_verify_rx: Some(assignee_verify_rx),
            ..Default::default()
        }
    }

    pub fn with_ui_actions(mut self, tx: Sender<TaskUiActions>) -> Self {
        self.ui_actions_tx = Some(tx);
        self
    }

    pub fn update_tur_info(&mut self, tur: Tur) {
        self.tur = tur;
        let name = self.tur.customer_data.name.clone();
        let service_num = self.tur.ticket_data.service_number.clone();
        let computer = &mut self.tur.computer_data;
        if !computer.device_mfg.is_none() || !computer.device_serial.is_none() || !computer.cpu.is_empty() || !computer.gpu.is_empty() || !computer.ram.is_empty() {
            self.tur.ticket_data.computer = Some(computer.id.clone());
        }

        if !service_num.is_empty() && !name.is_empty()
        {
            self.task_name = format!(
                "{} - {}",
                self.tur.customer_data.name,
                self.tur.ticket_data.service_number
            );
        }
    }
}

impl DisplayModal for CreateTaskModal {
    fn display(&mut self, ui: &mut Ui, action_handler: &mut dyn FnMut(ModalAction)) -> Option<ModalAction> {
        // Check for creation result
        if let Some(rx) = &self.creation_result_rx {
            if let Ok(result) = rx.try_recv() {
                self.creating_task = false;
                let toast_tx = get_toast_sender();
                match result {
                    TaskCreationResult::Created { service_number } => {
                        let _ = toast_tx.try_send(ToastMessage::Success(
                            format!("Task created for service #{service_number}")
                        ));
                        // Copy description to clipboard on success
                        if let Some(text) = self.clipboard_text.take() {
                            ui.ctx().copy_text(text);
                        }
                        self.current_page_state = ModalAction::Close;
                    },
                    TaskCreationResult::AlreadyExists { service_number } => {
                        let _ = toast_tx.try_send(ToastMessage::Warning(
                            format!("Task already exists for service #{service_number}")
                        ));
                        // Still copy to clipboard and close since task exists
                        if let Some(text) = self.clipboard_text.take() {
                            ui.ctx().copy_text(text);
                        }
                        self.current_page_state = ModalAction::Close;
                    },
                    TaskCreationResult::Updated { service_number } => {
                        let _ = toast_tx.try_send(ToastMessage::Info(
                            format!("Task updated for service #{service_number}")
                        ));
                        // Copy to clipboard on update
                        if let Some(text) = self.clipboard_text.take() {
                            ui.ctx().copy_text(text);
                        }
                        self.current_page_state = ModalAction::Close;
                    },
                    TaskCreationResult::Error { message } => {
                        let _ = toast_tx.try_send(ToastMessage::Error(
                            format!("Error creating task: {message}")
                        ));
                        // Don't close on error - let user try again
                        self.clipboard_text = None;
                    },
                }
            }
        }
        
        let avail_size = Vec2::new(500.0, 500.0);
        let m = Metrics::get(ui.ctx());
        ui.set_min_size(avail_size);
        ui.set_max_size(avail_size);
        ui.vertical_centered(|ui| {
            let pages = [
                (ModalAction::TicketInfoPage, svg::NOTE_PENCIL, "New task"),
                (ModalAction::ComputerInfoPage, svg::DESKTOP_TOWER, "Computer"),
                (ModalAction::ImportTask, svg::TICKET, "Ticket"),
            ];
            controls::bar(ui, BarKind::Tool, |ui| {
                let side = IconBox::Md.side(&m);
                let gaps = (pages.len() - 1) as f32 * ui.spacing().item_spacing.x;
                let row_w = pages.len() as f32 * side + gaps;
                ui.add_space(((ui.available_width() - row_w) / 2.0).max(0.0));
                for (page, icon, tip) in pages {
                    let selected = self.current_page_state == page;
                    if controls::icon_button(ui, icon, IconBox::Md, selected, tip).clicked() {
                        self.current_page_state = page;
                    }
                }
            });

            ui.add_space(2.0 * m.gap.y);

            ui.horizontal_centered(|ui| {
                match self.current_page_state {
                    ModalAction::TicketInfoPage => {
                        if let Some(tx) = self.prestashop_api_tx.clone()
                        {
                            ui.add_space(m.gap.x);
                            let action = self
                            .create_task(
                                ui, 
                                tx.clone()
                            );
                            if let ModalAction::Close = action {
                                action_handler(ModalAction::Close);
                            }
                        }
                    }
                    ModalAction::ImportTask => {
                        let store_users = self.store_users.clone();
                        let tur = &mut self.tur;
                        let mut seb_checking = false; // Not used in create task modal
                        display_ticket_page(
                            ui,
                            &mut tur.task_data,
                            Some(&mut tur.ticket_data),
                            Some(&mut tur.customer_data),
                            Some(&mut tur.computer_data),
                            avail_size,
                            &store_users,
                            self.user.clone(),
                            None, // No SEB channel for create task
                            &mut seb_checking,
                            None, // No customer modal for create task
                            None, // No service history for create task
                            Some(&mut tur.data.order.current_state),
                        );
                    },
                    ModalAction::ComputerInfoPage => {
                        let tur = &mut self.tur;
                        display_computer_page(
                            ui, 
                            Some(&mut tur.ticket_data), 
                            Some(&mut tur.computer_data), 
                        )
                    },
                    _ => {}
                };
            });
        });

        if self.current_page_state == ModalAction::Close {
            action_handler(ModalAction::Close);
        }
        Some(self.current_page_state.clone())
    }
}

impl CreateTaskModal {
    pub fn create_task(
        &mut self,
        ui: &mut Ui,
        prestashop_api_tx: Sender<PrestashopPayload>,
    ) -> ModalAction {
        if let Some(rx) = &self.assignee_verify_rx {
            while let Ok((checked, exists)) = rx.try_recv() {
                if checked == self.assignee {
                    self.assignee_status = if exists {
                        AssigneeVerification::Verified
                    } else {
                        AssigneeVerification::NotFound
                    };
                    self.verified_assignee = checked;
                }
            }
        }
        if self.assignee_status == AssigneeVerification::Pending {
            ui.ctx().request_repaint();
        }

        let m = Metrics::get(ui.ctx());
        ui.vertical_centered(|ui| {
            let mut lost_focus = false;
            self.tur.tur_sheet(ui, prestashop_api_tx.clone());

            ui.add_space(2.0 * m.gap.y);

            TextField::new(&mut self.task_name)
                .hint("Task Name")
                .width(FIELD_W)
                .icon(svg::NOTE_PENCIL)
                .show(ui);

            ui.add_space(2.0 * m.gap.y);
            let (inputs, local) = assignee_names(&self.store_users, self.user.get_store());

            let t = Tokens::get(ui.ctx());
            let assignee = &mut self.assignee;
            let r = ui
                .allocate_ui_with_layout(
                    vec2(FIELD_W, m.control_h),
                    Layout::top_down(Align::Min),
                    |ui| {
                        Frame::new()
                            .fill(t.field)
                            .stroke(Stroke::new(1.0, t.field_border))
                            .corner_radius(t.radius_sm)
                            .show(ui, |ui| {
                                AutoCompleteTextEdit::new(assignee, inputs)
                                    .highlight_matches(true)
                                    .max_suggestions(3)
                                    .group_by(move |name| usize::from(!local.contains(name)))
                                    .set_text_edit_properties(move |text_edit| {
                                        text_edit
                                            .hint_text("Assignee")
                                            .desired_width(FIELD_W - 2.0)
                                            .desired_rows(1)
                                            .frame(Frame::NONE.inner_margin(Margin::symmetric(
                                                (m.pad.x - 1.0) as i8,
                                                0,
                                            )))
                                            .min_size(vec2(0.0, m.control_h - 2.0))
                                            .align(Align2::LEFT_CENTER)
                                    })
                                    .ui(ui)
                            })
                            .inner
                    },
                )
                .inner;
        
            if r.lost_focus() {
                lost_focus = true;
            }

            if self.assignee != self.verified_assignee {
                self.assignee_status = AssigneeVerification::Unverified;
            }

            let exact_match = self.store_users.iter().any(|u| u.is_active() && u.get_username() == self.assignee);
            if !self.assignee.is_empty()
                && self.assignee_status == AssigneeVerification::Unverified
                && (lost_focus || exact_match)
            {
                self.assignee_status = AssigneeVerification::Pending;
                self.verified_assignee = self.assignee.clone();
                if let Some(tx) = self.assignee_verify_tx.clone() {
                    let assignee = self.assignee.clone();
                    PlatformSpawner::spawn(async move {
                        let exists = User::username_exists(assignee.clone()).await.unwrap_or(false);
                        let _ = tx.try_send((assignee, exists));
                    });
                }
            }

            match self.assignee_status {
                AssigneeVerification::Pending => {
                    controls::spinner(ui, SpinnerSize::Sm);
                    ui.colored_label(theme::info(ui), "Verifying assignee...");
                }
                AssigneeVerification::NotFound => {
                    IconLabel::new(svg::USER_MINUS, "No such user - pick a valid assignee")
                        .tone(theme::error(ui))
                        .show(ui);
                }
                AssigneeVerification::Verified => {
                    IconLabel::new(svg::USER_CHECK, "Assignee verified")
                        .tone(theme::success(ui))
                        .show(ui);
                }
                AssigneeVerification::Unverified => {}
            }

            ui.add_space(2.0 * m.gap.y);

            let priorities = Priority::VALUES;
            let priority_options: Vec<(Priority, &str)> =
                priorities.iter().map(|p| (p.clone(), p.as_str())).collect();
            ui.allocate_ui_with_layout(
                Vec2::new(ui.available_width(), m.control_h),
                Layout::left_to_right(Align::Center),
                |ui| {
                    ui.columns(2, |cols| {
                        cols[0].with_layout(Layout::right_to_left(Align::Center), |ui| {
                            controls::dropdown(
                                ui,
                                "PriorityComboBox",
                                &mut self.task_priority,
                                &priority_options,
                                PRIORITY_W,
                            )
                            .on_hover_text("Priority");
                        });
                        cols[1].with_layout(Layout::left_to_right(Align::Center), |ui| {
                            let mut due_date = crate::to_jiff_date(&self.due_date);
                            let date_picker = DatePickerButton::new(&mut due_date)
                                .calendar_week(false)
                                .format("%m/%d/%y")
                                .show_icon(true)
                                .ui(ui);

                            if date_picker.changed() {
                                self.due_date = crate::apply_jiff_date(&self.due_date, &due_date).into();
                            }
                        });
                    });
                },
            );

            ui.add_space(2.0 * m.gap.y);

            let r = ui
                .allocate_ui_with_layout(
                    vec2(DESCRIPTION_W, 0.0),
                    Layout::top_down(Align::Min),
                    |ui| {
                        TextArea::new(&mut self.description)
                            .hint("Task Description")
                            .rows(10)
                            .mono(true)
                            .tab_indents(true)
                            .width(DESCRIPTION_W)
                            .show(ui)
                    },
                )
                .inner;

            if lost_focus {
                r.request_focus();
            }

            ui.add_space(2.0 * m.gap.y);
            let pulling_ticket = !self.tur.ticket_data.service_number.is_empty();
            let check = !self.task_name.is_empty() && !self.description.is_empty() && !self.assignee.is_empty();

            let enabled = if (pulling_ticket && check) || (check) { true } else { false };
            let assignee_verified = self.assignee_status == AssigneeVerification::Verified
                && self.assignee == self.verified_assignee;
            let enabled = enabled && !self.creating_task && assignee_verified;

            // Show spinner when creating task
            if self.creating_task {
                controls::spinner(ui, SpinnerSize::Md);
                ui.colored_label(theme::info(ui), "Creating...");
            }

            let btn = TextButton::new("Submit")
                .kind(ButtonKind::Primary)
                .prominent(true)
                .icon(svg::PAPER_PLANE_RIGHT)
                .min_width(SUBMIT_W)
                .enabled(enabled);
            if btn.show(ui).clicked() {
                info!("ASSIGNEE: {:?}", self.assignee.clone());
                let assignee = self.assignee.clone();
                let mut payload = self.tur.clone();                   
                payload.task_data.priority = self.task_priority.clone();
                payload.task_data.created_at = Utc::now().into();
                payload.task_data.due_date = self.due_date.clone();
                payload.task_data.completed = false;
                
                // Set status to QC for non-service orders (sales orders, R2R, BSD, RCI)
                let order_type = OrderType::from_id_str(&payload.data.order.id_order_type);
                if order_type != OrderType::ServiceOrder {
                    payload.task_data.status = Status::Qc;
                    info!("Order type is {:?}, setting task status to QC", order_type);
                } else {
                    payload.task_data.status = Status::Todo;
                }
                
                payload.task_data.task_name = self.task_name.clone();
                payload.task_data.task_description = self.description.clone();
                payload.task_data.service_number = Some(payload.ticket_data.service_number.clone());
                
                // Store the description to copy to clipboard after success
                self.clipboard_text = Some(self.description.clone());
                self.creating_task = true;
                
                let usr = &mut User::default();
                for user in self.store_users.iter().filter(|u| u.is_active()) {
                    if assignee == user.get_username() {
                        log::info!("Got {:?} from assignee: {assignee:?}", user.get_name());
                        *usr = user.clone();
                    }
                }

                let task = payload.task_data.clone();
                let result_tx = self.creation_result_tx.clone();
                let ui_tx = self.ui_actions_tx.clone();
                let service_number = payload.ticket_data.service_number.clone();
                
                // Check if we have a service number but haven't pulled the order yet
                // (customer name will be empty if not pulled)
                let needs_pull = !service_number.is_empty() && payload.customer_data.name.is_empty();
                
                PlatformSpawner::spawn(async move {
                    let mut payload = payload;
                    let task_id = task.id.clone();

                    if open_existing_task(&service_number, &task_id, ui_tx.as_ref()).await {
                        if let Some(tx) = result_tx {
                            let _ = tx.try_send(TaskCreationResult::AlreadyExists { service_number });
                        }
                        return;
                    }

                    // If service number is entered but data wasn't pulled, pull it now
                    if needs_pull {
                        info!("Service number entered but order not pulled, fetching now: {}", service_number);
                        match fetch_prestashop_order(OrderLookup::ServiceNumber(service_number.clone())).await {
                            Ok(presta_data) => {
                                info!("Successfully fetched prestashop data for order {}", service_number);
                                // The shared mapper reassigns task.id and rebuilds notes
                                // against it; the id captured before this spawn is the one
                                // the create path uses, so it is restored afterward.
                                let keep_task_id = payload.task_data.id.clone();
                                let mut draft = EntityDraft {
                                    customer: payload.customer_data.clone(),
                                    ticket: payload.ticket_data.clone(),
                                    computer: payload.computer_data.clone(),
                                    task: payload.task_data.clone(),
                                    task_notes: payload.task_notes.clone(),
                                };
                                apply_prestashop_payload(
                                    &presta_data,
                                    &mut draft,
                                    &PrestaMapOptions {
                                        mode: PrestaMapMode::Bench,
                                        ..Default::default()
                                    },
                                );
                                let specs = presta_data.order.extract_specs().await;
                                apply_extracted_specs(&mut draft, &specs);
                                draft.task.id = keep_task_id.clone();
                                for note in &mut draft.task_notes {
                                    note.task_id = Some(keep_task_id.clone());
                                }
                                payload.customer_data = draft.customer;
                                payload.ticket_data = draft.ticket;
                                payload.computer_data = draft.computer;
                                payload.task_notes = draft.task_notes;

                                // Also update task status based on order type
                                let order_type = OrderType::from_id_str(&presta_data.order.id_order_type);
                                if order_type != OrderType::ServiceOrder {
                                    payload.task_data.status = Status::Qc;
                                    info!("Auto-pulled order type is {:?}, setting task status to QC", order_type);
                                }
                            }
                            Err(e) => {
                                error!("Failed to fetch prestashop data: {:?}", e);
                                // Continue anyway with what we have
                            }
                        }
                    }
                    
                    if !payload.ticket_data.service_number.is_empty() {

                        if payload.ticket_data.salesman.is_empty() {
                            info!("Salesman was empty, assigning current user");
                            payload.ticket_data.salesman = assignee.clone();
                            info!("TicketData.Salesman: {:?}\nAssignee: {:?}", 
                                payload.ticket_data.salesman.clone(), 
                                assignee.clone()
                            );
                        }
                        
                        let send_specs = computer_has_minimal_hardware(&payload.computer_data);
                        // Honor the verified assignee field; fall back to salesman/creator if it can't resolve.
                        let assignee_override = match User::query_user_from_email(assignee.clone()).await {
                            Ok(user) => Some(user.get_id()),
                            Err(e) => {
                                log::warn!("Assignee '{assignee}' did not resolve ({e:?}); deferring to salesman/creator fallback");
                                None
                            }
                        };
                        let create_task_result = create_full_task_payload(
                            payload.ticket_data.into(),
                            payload.customer_data.clone(),
                            payload.computer_data.clone(),
                            task.into(),
                            payload.task_notes,
                            send_specs,
                            false,
                            assignee_override,
                        ).await;
                        info!("create_task_result: {create_task_result:?}");
                        if let TaskCreationResult::AlreadyExists { service_number } = &create_task_result {
                            open_existing_task(service_number, &task_id, ui_tx.as_ref()).await;
                        }

                        // Send result through channel
                        if let Some(tx) = result_tx {
                            let _ = tx.try_send(create_task_result);
                        }

                    } else {
                        info!("Creating Regular Task");
                        match User::query_user_from_email(assignee).await {
                            Ok(user) => {
                                payload.task_data.assignee = user.get_id();

                                log::info!("Payload: {payload:?}");
                                let query: Result<_, surrealdb::Error> = db()
                                    .query("CREATE task CONTENT $content")
                                    .bind(("content", payload.task_data))
                                    .await;

                                match query {
                                    Ok(mut res) => {
                                        let _: Option<database::schema::Record> = res.take(0).unwrap_or_default();
                                        if let Some(tx) = result_tx {
                                            let _ = tx.try_send(TaskCreationResult::Created { 
                                                service_number: "regular_task".to_string() 
                                            });
                                        }
                                    },
                                    Err(e) => {
                                        error!("Error creating task: {e:?}");
                                        if let Some(tx) = result_tx {
                                            let _ = tx.try_send(TaskCreationResult::Error { 
                                                message: format!("{e}") 
                                            });
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                error!("Error getting user: {e:?}");
                                if let Some(tx) = result_tx {
                                    let _ = tx.try_send(TaskCreationResult::Error { 
                                        message: format!("{e}") 
                                    });
                                }
                            },
                        }
                    }
                });
            }
        });

        self.current_page_state.clone()
    }
}

/// Opens the task `service_number` already has, other than `own`; true when one exists.
async fn open_existing_task(
    service_number: &str,
    own: &database::schema::RecordId,
    ui_tx: Option<&Sender<TaskUiActions>>,
) -> bool {
    let existing = match database::schema::find_service_task(service_number).await {
        Ok(found) => found.map(|t| t.id).filter(|id| id != own),
        Err(e) => {
            log::warn!("Existing task lookup for #{service_number} failed: {e:?}");
            None
        }
    };
    let Some(id) = existing else { return false };
    info!("Service #{service_number} already has task {id:?}; opening it");
    if let Some(tx) = ui_tx {
        let _ = tx.try_send(TaskUiActions::OpenTaskModalById(id));
    }
    true
}

impl Tur {
    pub fn set_store_users(&mut self, users: Vec<User>) -> &mut Self {
        self.store_users = users;
        self
    }

    pub fn tur_sheet(&mut self, ui: &mut Ui, prestashop_api_tx: Sender<PrestashopPayload>) {
        let check = !self.ticket_data.service_number.is_empty();
        let m = Metrics::get(ui.ctx());
        let pull = TextButton::new("Pull Order")
            .icon(svg::CLOUD_ARROW_DOWN)
            .enabled(check);
        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        let upload = TextButton::new("Upload TUR").icon(svg::UPLOAD_SIMPLE);
        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        let row_w = pull.size(ui).x + ui.spacing().item_spacing.x + upload.size(ui).x;
        #[cfg(any(target_os = "ios", target_os = "android"))]
        let row_w = pull.size(ui).x;

        ui.horizontal_top(|ui| {
            ui.add_space(((ui.available_width() - row_w) / 2.0).max(0.0));
            if pull.show(ui).clicked() {
                let service_num = self.ticket_data.service_number.clone();
                self.ticket_data = TicketData::default();
                self.task_data = LiveTaskPayload::default();
                self.customer_data = CustomerData::default();
                self.ticket_data.service_number = service_num;
                // self.task_notes = Vec::new::<Vec<TaskNotePayload>>();
                Self::presta_api(prestashop_api_tx, self.ticket_data.service_number.clone());
            }
        
            #[cfg(not(any(target_os = "ios", target_os = "android")))]
            let upload = upload.show(ui);

            #[cfg(not(any(target_os = "ios", target_os = "android")))]
            if upload.clicked() {
                // let tx = tx.clone();
                PlatformSpawner::spawn(async move {
                    if let Some(_file) = rfd::AsyncFileDialog::new()
                        .set_file_name("tur.json")
                        .pick_file()
                        .await
                    {
                        // match serde_json::from_slice::<Tur>(&file.read().await) {
                        //     Ok(tur) => { let _ = tx.try_send(tur); },
                        //     Err(e) => log::error!("Error converting bytes to Theme: {e:?}"),
                        // }
                    }
                });
            }
        });

        ui.add_space(2.0 * m.gap.y);

        TextField::new(&mut self.ticket_data.service_number)
            .hint("Service #")
            .width(FIELD_W)
            .icon(svg::TICKET)
            .show(ui);
        if let Some((cut, _)) = self.ticket_data.service_number.char_indices().nth(SERVICE_NUMBER_MAX) {
            self.ticket_data.service_number.truncate(cut);
        }
    }

    pub fn presta_api(prestashop_api_tx: Sender<PrestashopPayload>, input: String) {
        let tx = prestashop_api_tx.clone();
        if !input.is_empty() {
            PlatformSpawner::spawn(async move {
                if let Ok(payload) =
                    fetch_prestashop_order(OrderLookup::ServiceNumber(input)).await
                {
                    let _ = tx.try_send(payload);
                }
            });
        }
    }
}

