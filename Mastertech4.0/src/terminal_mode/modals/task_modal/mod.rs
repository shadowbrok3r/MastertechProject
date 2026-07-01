//! Task Modal - A modal dialog for viewing and editing task details
//!
//! This modal displays task information in a tabbed interface with pages for:
//! - Ticket Info (editable status / assignee / priority / description)
//! - Computer Info
//! - Software Info
//! - Task History
//! - Task Notes (chat view with note sending)

use ratatui::layout::Rect;
use ratatui::style::Style;
use database::schema::{
    random_record_id, ComputerData, CustomerData, LiveTaskPayload, Priority, RecordId,
    RecordIdExt, Status, TaskHistory, TaskNotePayload, TicketData, User, TASK_NOTE_TABLE,
};
use std::cell::RefCell;
use std::time::Instant;
use chrono::Utc;
use crossbeam::channel::{Receiver, Sender};
use crate::terminal_mode::{
    events::action_handler::WidgetId,
    styling::{CATPPUCCIN, ThemeRole},
    widgets::button::Button,
    widgets::tui_textarea::TextArea,
};

pub mod action_handler;
pub mod render;

/// The current page/tab being displayed in the task modal
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ModalPage {
    #[default]
    TicketInfo,
    ComputerInfo,
    SoftwareInfo,
    TaskHistory,
    TaskNotes,
}

impl ModalPage {
    pub fn all() -> &'static [ModalPage] {
        &[
            ModalPage::TicketInfo,
            ModalPage::ComputerInfo,
            ModalPage::SoftwareInfo,
            ModalPage::TaskHistory,
            ModalPage::TaskNotes,
        ]
    }

    pub fn title(&self) -> &'static str {
        match self {
            ModalPage::TicketInfo => "Ticket",
            ModalPage::ComputerInfo => "Computer",
            ModalPage::SoftwareInfo => "Software",
            ModalPage::TaskHistory => "History",
            ModalPage::TaskNotes => "Notes",
        }
    }

    pub fn widget_id(&self) -> &'static str {
        match self {
            ModalPage::TicketInfo => "TaskModalTicketTab",
            ModalPage::ComputerInfo => "TaskModalComputerTab",
            ModalPage::SoftwareInfo => "TaskModalSoftwareTab",
            ModalPage::TaskHistory => "TaskModalHistoryTab",
            ModalPage::TaskNotes => "TaskModalNotesTab",
        }
    }

    pub fn index(&self) -> usize {
        match self {
            ModalPage::TicketInfo => 0,
            ModalPage::ComputerInfo => 1,
            ModalPage::SoftwareInfo => 2,
            ModalPage::TaskHistory => 3,
            ModalPage::TaskNotes => 4,
        }
    }

    pub fn from_index(idx: usize) -> Self {
        match idx {
            0 => ModalPage::TicketInfo,
            1 => ModalPage::ComputerInfo,
            2 => ModalPage::SoftwareInfo,
            3 => ModalPage::TaskHistory,
            4 => ModalPage::TaskNotes,
            _ => ModalPage::TicketInfo,
        }
    }

    pub fn from_widget_id(id: &str) -> Option<Self> {
        match id {
            "TaskModalTicketTab" => Some(ModalPage::TicketInfo),
            "TaskModalComputerTab" => Some(ModalPage::ComputerInfo),
            "TaskModalSoftwareTab" => Some(ModalPage::SoftwareInfo),
            "TaskModalHistoryTab" => Some(ModalPage::TaskHistory),
            "TaskModalNotesTab" => Some(ModalPage::TaskNotes),
            _ => None,
        }
    }
}

/// Which UI element currently receives key input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ModalFocus {
    #[default]
    Normal,
    EditDescription,
    NoteInput,
    Selector,
}

/// Popup selector state for the editable task fields.
#[derive(Debug, Clone)]
pub enum FieldSelector {
    Status { options: Vec<Status>, idx: usize },
    Assignee { options: Vec<(String, String)>, idx: usize },
    Priority { options: Vec<Priority>, idx: usize },
}

impl FieldSelector {
    pub fn title(&self) -> &'static str {
        match self {
            FieldSelector::Status { .. } => " Set Status ",
            FieldSelector::Assignee { .. } => " Set Assignee ",
            FieldSelector::Priority { .. } => " Set Priority ",
        }
    }

    pub fn labels(&self) -> Vec<String> {
        match self {
            FieldSelector::Status { options, .. } => {
                options.iter().map(|s| s.as_str().to_string()).collect()
            }
            FieldSelector::Assignee { options, .. } => {
                options.iter().map(|(_, name)| name.clone()).collect()
            }
            FieldSelector::Priority { options, .. } => {
                options.iter().map(|p| p.as_str().to_string()).collect()
            }
        }
    }

    pub fn idx(&self) -> usize {
        match self {
            FieldSelector::Status { idx, .. }
            | FieldSelector::Assignee { idx, .. }
            | FieldSelector::Priority { idx, .. } => *idx,
        }
    }

    pub fn len(&self) -> usize {
        match self {
            FieldSelector::Status { options, .. } => options.len(),
            FieldSelector::Assignee { options, .. } => options.len(),
            FieldSelector::Priority { options, .. } => options.len(),
        }
    }

    pub fn select_prev(&mut self) {
        let len = self.len();
        let idx = match self {
            FieldSelector::Status { idx, .. }
            | FieldSelector::Assignee { idx, .. }
            | FieldSelector::Priority { idx, .. } => idx,
        };
        *idx = if *idx == 0 { len.saturating_sub(1) } else { *idx - 1 };
    }

    pub fn select_next(&mut self) {
        let len = self.len();
        let idx = match self {
            FieldSelector::Status { idx, .. }
            | FieldSelector::Assignee { idx, .. }
            | FieldSelector::Priority { idx, .. } => idx,
        };
        *idx = if *idx + 1 >= len { 0 } else { *idx + 1 };
    }
}

/// Task modal for viewing and editing task details
pub struct TaskModal<'a> {
    /// Unique ID for this modal instance
    pub modal_id: String,
    /// The task being displayed
    pub task: LiveTaskPayload,
    /// Current page/tab
    pub current_page: RefCell<ModalPage>,
    /// Tab buttons
    pub tab_buttons: Vec<Button<'a>>,
    /// Close button
    pub close_btn: Button<'a>,
    /// Associated ticket data (loaded async)
    pub ticket: RefCell<Option<TicketData>>,
    /// Associated customer data (loaded async)
    pub customer: RefCell<Option<CustomerData>>,
    /// Associated computer data (loaded async)
    pub computer: RefCell<Option<ComputerData>>,
    /// Task notes (loaded async)
    pub notes: RefCell<Vec<TaskNotePayload>>,
    /// Task history (loaded async)
    pub history: RefCell<Vec<TaskHistory>>,
    /// Store users for assignee selection
    pub store_users: Vec<User>,
    /// Current user
    pub current_user: User,
    /// Whether the modal should close
    pub should_close: RefCell<bool>,
    /// Loading state
    pub loading: RefCell<bool>,
    /// Modal area for rendering
    pub modal_area: RefCell<Rect>,
    /// Tab bar area for click detection
    pub tab_bar_area: RefCell<Rect>,
    /// Tab button areas for mouse detection
    pub tab_button_areas: RefCell<Vec<Rect>>,
    /// Scroll offset for content
    pub scroll_offset: RefCell<u16>,
    /// Which element receives key input.
    pub focus: RefCell<ModalFocus>,
    /// Open field-selector popup, if any.
    pub selector: Option<FieldSelector>,
    /// Editor for the task description (seeded on edit start).
    pub description_editor: TextArea<'static>,
    /// Persistent editor for composing a new note.
    pub note_editor: TextArea<'static>,
    /// When true the next note is created as private (DB only).
    pub note_private: bool,
    /// True while a note creation round-trip is in flight.
    pub sending_note: bool,
    /// Text of the in-flight note, restored into the editor on failure.
    pending_note_text: Option<String>,
    /// Transient feedback shown in the footer.
    pub status_line: Option<(Instant, String)>,
    /// Note input area for mouse focus.
    pub note_input_area: RefCell<Rect>,
    /// Notes scroll: lines up from the bottom (0 = stick to bottom).
    pub notes_scroll: RefCell<u16>,
    // Async data-load receivers (senders live in the spawned fetch task).
    ticket_rx: Receiver<TicketData>,
    customer_rx: Receiver<CustomerData>,
    computer_rx: Receiver<ComputerData>,
    notes_tx: Sender<Vec<TaskNotePayload>>,
    notes_rx: Receiver<Vec<TaskNotePayload>>,
    history_rx: Receiver<Vec<TaskHistory>>,
    note_result_tx: Sender<Result<(), String>>,
    note_result_rx: Receiver<Result<(), String>>,
}

impl<'a> TaskModal<'a> {
    pub fn new(task: LiveTaskPayload, store_users: Vec<User>, current_user: User) -> Self {
        let modal_id = task.id.key_string();
        let task_id = task.id.clone();

        // Create tab buttons
        let mut tab_buttons = Vec::new();
        for page in ModalPage::all() {
            let btn = Button::new(page.title(), WidgetId(page.widget_id().to_string()))
                .theme(ThemeRole::Input)
                .as_tab();
            tab_buttons.push(btn);
        }

        // Mark first tab as selected
        if let Some(first) = tab_buttons.first() {
            first.set_selected(true);
        }

        // Create close button
        let close_btn = Button::new("✕ Close", WidgetId("TaskModalClose".to_string()))
            .theme(ThemeRole::Input);

        let (ticket_tx, ticket_rx) = crossbeam::channel::unbounded();
        let (customer_tx, customer_rx) = crossbeam::channel::unbounded();
        let (computer_tx, computer_rx) = crossbeam::channel::unbounded();
        let (notes_tx, notes_rx) = crossbeam::channel::unbounded();
        let (history_tx, history_rx) = crossbeam::channel::unbounded();
        let (note_result_tx, note_result_rx) = crossbeam::channel::unbounded();

        // Spawn async data loading; results are delivered back over the channels
        // and drained each frame in `receive()`.
        {
            let (t_tx, cu_tx, co_tx, n_tx, h_tx) = (
                ticket_tx.clone(),
                customer_tx.clone(),
                computer_tx.clone(),
                notes_tx.clone(),
                history_tx.clone(),
            );
            let service_number = task.service_number.clone();
            tokio::spawn(async move {
                if let Ok(ticket) = TicketData::get_associated_ticket(task_id.clone()).await {
                    let _ = t_tx.send(ticket);
                }
                if let Ok(customer) = CustomerData::get_associated_customer(task_id.clone()).await {
                    let _ = cu_tx.send(customer);
                }
                if let Ok(computer) = ComputerData::get_associated_computer(task_id.clone()).await {
                    let _ = co_tx.send(computer);
                }
                if let Ok(notes) = TaskNotePayload::get_db_notes_from_task_id(task_id.clone()).await {
                    let _ = n_tx.send(notes);
                }
                if let Ok(history) = TaskHistory::get_history_for_task(task_id.clone()).await {
                    let _ = h_tx.send(history);
                }
                // Prestashop notes land in the same channel and are merged by id.
                if let Some(sn) = service_number {
                    if !sn.is_empty() {
                        match TaskNotePayload::get_prestashop_notes_from_service(&sn, Some(task_id.clone())).await {
                            Ok(notes) => { let _ = n_tx.send(notes); }
                            Err(e) => log::error!("Error getting prestashop notes: {e:?}"),
                        }
                    }
                }
            });
        }

        let mut note_editor = TextArea::default();
        note_editor.set_style(Style::default().fg(CATPPUCCIN.text));
        note_editor.set_cursor_line_style(Style::default());
        note_editor.set_placeholder_text("Write a note…");
        note_editor.set_selection_style(Style::default().bg(CATPPUCCIN.surface1));

        Self {
            modal_id,
            task,
            current_page: RefCell::new(ModalPage::default()),
            tab_buttons,
            close_btn,
            ticket: RefCell::new(None),
            customer: RefCell::new(None),
            computer: RefCell::new(None),
            notes: RefCell::new(Vec::new()),
            history: RefCell::new(Vec::new()),
            store_users,
            current_user,
            should_close: RefCell::new(false),
            loading: RefCell::new(true),
            modal_area: RefCell::new(Rect::default()),
            tab_bar_area: RefCell::new(Rect::default()),
            tab_button_areas: RefCell::new(Vec::new()),
            scroll_offset: RefCell::new(0),
            focus: RefCell::new(ModalFocus::Normal),
            selector: None,
            description_editor: TextArea::default(),
            note_editor,
            note_private: false,
            sending_note: false,
            pending_note_text: None,
            status_line: None,
            note_input_area: RefCell::new(Rect::default()),
            notes_scroll: RefCell::new(0),
            ticket_rx,
            customer_rx,
            computer_rx,
            notes_tx,
            notes_rx,
            history_rx,
            note_result_tx,
            note_result_rx,
        }
    }

    /// Drain async data-load results into the modal each frame.
    pub fn receive(&mut self) {
        if let Ok(ticket) = self.ticket_rx.try_recv() {
            *self.ticket.borrow_mut() = Some(ticket);
            *self.loading.borrow_mut() = false;
        }
        if let Ok(customer) = self.customer_rx.try_recv() {
            *self.customer.borrow_mut() = Some(customer);
        }
        if let Ok(computer) = self.computer_rx.try_recv() {
            *self.computer.borrow_mut() = Some(computer);
        }
        while let Ok(notes) = self.notes_rx.try_recv() {
            self.merge_notes(notes);
            self.sending_note = false;
        }
        if let Ok(history) = self.history_rx.try_recv() {
            *self.history.borrow_mut() = history;
        }
        if let Ok(result) = self.note_result_rx.try_recv() {
            self.sending_note = false;
            match result {
                Ok(()) => {
                    self.pending_note_text = None;
                    self.set_status("Note sent");
                }
                Err(e) => {
                    // Put the unsent text back so it isn't lost.
                    if let Some(text) = self.pending_note_text.take() {
                        if self.note_editor.is_empty() {
                            self.note_editor.insert_str(text);
                        }
                    }
                    self.set_status(format!("Failed to send note: {e}"));
                }
            }
        }
    }

    /// Merge a batch of notes into the cached list, replacing by id.
    fn merge_notes(&self, incoming: Vec<TaskNotePayload>) {
        let mut notes = self.notes.borrow_mut();
        for note in incoming {
            if let Some(existing) = notes.iter_mut().find(|n| n.id == note.id) {
                *existing = note;
            } else {
                notes.push(note);
            }
        }
        notes.sort_by_key(|n| n.created_at.clone());
    }

    /// Set a transient footer status message.
    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status_line = Some((Instant::now(), msg.into()));
    }

    /// Set the active tab and update button states
    pub fn set_active_tab(&mut self, page: ModalPage) {
        *self.current_page.borrow_mut() = page;
        // Editors and popups don't carry across pages.
        *self.focus.borrow_mut() = ModalFocus::Normal;
        self.selector = None;

        // Update button selected states
        for (i, btn) in self.tab_buttons.iter().enumerate() {
            btn.set_selected(i == page.index());
        }
    }

    /// Check if modal should close
    pub fn should_close(&self) -> bool {
        *self.should_close.borrow()
    }

    /// Request modal to close
    pub fn request_close(&self) {
        *self.should_close.borrow_mut() = true;
    }

    /// Get the modal ID (task key)
    pub fn get_id(&self) -> &str {
        &self.modal_id
    }

    /// Username for a user record id, falling back to the raw key.
    pub fn username_for(&self, user_id: &RecordId) -> String {
        self.store_users
            .iter()
            .find(|u| &u.get_id() == user_id)
            .map(|u| u.get_username().to_owned())
            .unwrap_or_else(|| user_id.key_string())
    }

    /// Begin editing the task description, seeding the editor.
    pub fn start_description_edit(&mut self) {
        let lines: Vec<String> = self
            .task
            .task_description
            .lines()
            .map(|l| l.to_string())
            .collect();
        let mut editor = TextArea::new(if lines.is_empty() { vec![String::new()] } else { lines });
        editor.set_style(Style::default().fg(CATPPUCCIN.text));
        editor.set_cursor_line_style(Style::default());
        editor.set_selection_style(Style::default().bg(CATPPUCCIN.surface1));
        self.description_editor = editor;
        *self.focus.borrow_mut() = ModalFocus::EditDescription;
    }

    /// Persist the edited description and update the local task.
    pub fn save_description(&mut self) {
        let text = self.description_editor.lines().join("\n");
        self.task.task_description = text;
        let task = self.task.clone();
        tokio::spawn(async move {
            if let Err(e) = task.update_task_description().await {
                log::error!("Failed to update task description: {e:?}");
            }
        });
        self.set_status("Description saved");
        *self.focus.borrow_mut() = ModalFocus::Normal;
    }

    /// Open the status selector popup.
    pub fn open_status_selector(&mut self) {
        let options: Vec<Status> = Status::VALUES
            .iter()
            .filter(|s| !matches!(s, Status::CustomStatus(_)))
            .cloned()
            .collect();
        let idx = options.iter().position(|s| s == &self.task.status).unwrap_or(0);
        self.selector = Some(FieldSelector::Status { options, idx });
        *self.focus.borrow_mut() = ModalFocus::Selector;
    }

    /// Open the assignee selector popup.
    pub fn open_assignee_selector(&mut self) {
        let options: Vec<(String, String)> = self
            .store_users
            .iter()
            .filter(|u| u.is_active())
            .map(|u| (u.get_id().key_string(), u.get_username().to_owned()))
            .collect();
        if options.is_empty() {
            self.set_status("No store users loaded yet");
            return;
        }
        let idx = options
            .iter()
            .position(|(id, _)| id == &self.task.assignee.key_string())
            .unwrap_or(0);
        self.selector = Some(FieldSelector::Assignee { options, idx });
        *self.focus.borrow_mut() = ModalFocus::Selector;
    }

    /// Open the priority selector popup.
    pub fn open_priority_selector(&mut self) {
        let options = Priority::VALUES.to_vec();
        let idx = options.iter().position(|p| p == &self.task.priority).unwrap_or(0);
        self.selector = Some(FieldSelector::Priority { options, idx });
        *self.focus.borrow_mut() = ModalFocus::Selector;
    }

    /// Apply the selector choice to the task and persist it.
    pub fn apply_selector(&mut self) {
        let Some(selector) = self.selector.take() else { return; };
        match selector {
            FieldSelector::Status { options, idx } => {
                if let Some(status) = options.get(idx) {
                    self.task.status = status.clone();
                    let task = self.task.clone();
                    let status = status.clone();
                    tokio::spawn(async move {
                        if let Err(e) = task.update_status(status).await {
                            log::error!("Failed to update task status: {e:?}");
                        }
                    });
                    self.set_status(format!("Status → {}", self.task.status.as_str()));
                }
            }
            FieldSelector::Assignee { options, idx } => {
                if let Some((user_id, username)) = options.get(idx) {
                    let assignee = RecordId::new("user", user_id.clone());
                    self.task.assignee = assignee.clone();
                    let task = self.task.clone();
                    tokio::spawn(async move {
                        if let Err(e) = task.update_assignee(assignee).await {
                            log::error!("Failed to update task assignee: {e:?}");
                        }
                    });
                    self.set_status(format!("Assignee → {username}"));
                }
            }
            FieldSelector::Priority { options, idx } => {
                if let Some(priority) = options.get(idx) {
                    self.task.priority = priority.clone();
                    let task = self.task.clone();
                    let priority = priority.clone();
                    tokio::spawn(async move {
                        if let Err(e) = task.update_priority(Some(priority)).await {
                            log::error!("Failed to update task priority: {e:?}");
                        }
                    });
                    self.set_status(format!("Priority → {}", self.task.priority.as_str()));
                }
            }
        }
        *self.focus.borrow_mut() = ModalFocus::Normal;
    }

    /// Cancel the selector popup.
    pub fn cancel_selector(&mut self) {
        self.selector = None;
        *self.focus.borrow_mut() = ModalFocus::Normal;
    }

    /// Toggle and persist the completed flag.
    pub fn toggle_completed(&mut self) {
        self.task.completed = !self.task.completed;
        let task = self.task.clone();
        tokio::spawn(async move {
            if let Err(e) = task.update_completed(task.completed).await {
                log::error!("Failed to update task completed: {e:?}");
            }
        });
        self.set_status(if self.task.completed { "Marked completed" } else { "Marked not completed" });
    }

    /// Re-fetch notes for this task (DB + Prestashop when applicable).
    pub fn refresh_notes(&mut self) {
        let task_id = self.task.id.clone();
        let service_number = self.task.service_number.clone().unwrap_or_default();
        let tx = self.notes_tx.clone();
        self.set_status("Refreshing notes…");
        tokio::spawn(async move {
            if !service_number.is_empty() {
                match TaskNotePayload::get_prestashop_notes_from_service(&service_number, Some(task_id.clone())).await {
                    Ok(notes) => { let _ = tx.try_send(notes); }
                    Err(e) => log::error!("Error refreshing prestashop notes: {e:?}"),
                }
            }
            match TaskNotePayload::get_db_notes_from_task_id(task_id).await {
                Ok(notes) => { let _ = tx.try_send(notes); }
                Err(e) => log::error!("Error refreshing task notes: {e:?}"),
            }
        });
    }

    /// Create a note from the editor contents and push it to the backend.
    pub fn send_note(&mut self) {
        let text = self.note_editor.lines().join("\n").trim().to_string();
        if text.is_empty() || self.sending_note {
            return;
        }

        let Some(id_employee) = self.current_user.get_employee_id() else {
            self.set_status("Cannot send note: current user has no employee id");
            return;
        };

        // Reuse the thread id from any already-loaded note on this task.
        let id_customer_thread = self
            .notes
            .borrow()
            .iter()
            .find_map(|n| n.id_customer_thread.clone());

        self.pending_note_text = Some(text.clone());
        let mut new_note = TaskNotePayload {
            note: text,
            task_id: Some(self.task.id.clone()),
            username: self.current_user.get_username().to_string(),
            user: self.current_user.get_id(),
            id_employee: Some(id_employee.to_string()),
            id_customer_thread,
            service_number: self.task.service_number.clone(),
            private: self.note_private,
            id: random_record_id(TASK_NOTE_TABLE),
            created_at: Utc::now().into(),
            id_customer_message: None,
        };

        self.note_editor = {
            let mut editor = TextArea::default();
            editor.set_style(Style::default().fg(CATPPUCCIN.text));
            editor.set_cursor_line_style(Style::default());
            editor.set_placeholder_text("Write a note…");
            editor.set_selection_style(Style::default().bg(CATPPUCCIN.surface1));
            editor
        };
        self.sending_note = true;
        self.set_status("Sending note…");

        let task_id = self.task.id.clone();
        let service_number = self.task.service_number.clone();
        let notes_tx = self.notes_tx.clone();
        let result_tx = self.note_result_tx.clone();
        tokio::spawn(async move {
            match new_note.handle_note_creation().await {
                Ok(_) => {
                    let _ = result_tx.try_send(Ok(()));
                    // Pull the latest notes so the new note shows up in the chat.
                    if let Some(sn) = service_number.filter(|sn| !sn.is_empty()) {
                        match TaskNotePayload::get_prestashop_notes_from_service(&sn, Some(task_id.clone())).await {
                            Ok(notes) => { let _ = notes_tx.try_send(notes); }
                            Err(e) => log::error!("Failed to refresh notes after send: {e:?}"),
                        }
                    }
                    match TaskNotePayload::get_db_notes_from_task_id(task_id).await {
                        Ok(notes) => { let _ = notes_tx.try_send(notes); }
                        Err(e) => log::error!("Failed to refresh db notes after send: {e:?}"),
                    }
                }
                Err(e) => {
                    log::error!("Failed to create task note: {e:?}");
                    let _ = result_tx.try_send(Err(format!("{e}")));
                }
            }
        });
    }
}
