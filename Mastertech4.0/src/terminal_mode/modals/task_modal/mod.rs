//! Task Modal - A modal dialog for viewing and editing task details
//!
//! This modal displays task information in a tabbed interface with pages for:
//! - Ticket Info
//! - Computer Info
//! - Software Info
//! - Task History
//! - Task Notes

use ratatui::layout::Rect;
use database::schema::{
    ComputerData, CustomerData, LiveTaskPayload, RecordIdExt, TicketData, User,
    TaskNotePayload, TaskHistory,
};
use std::cell::RefCell;
use crate::terminal_mode::{
    events::action_handler::WidgetId,
    styling::CATPPUCCINTHEME,
    widgets::button::Button,
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
}

impl<'a> TaskModal<'a> {
    pub fn new(task: LiveTaskPayload, store_users: Vec<User>, current_user: User) -> Self {
        let modal_id = task.id.key_string();
        let task_id = task.id.clone();
        
        // Create tab buttons
        let mut tab_buttons = Vec::new();
        for page in ModalPage::all() {
            let btn = Button::new(page.title(), WidgetId(page.widget_id().to_string()))
                .theme(CATPPUCCINTHEME)
                .as_tab();
            tab_buttons.push(btn);
        }
        
        // Mark first tab as selected
        if let Some(first) = tab_buttons.first() {
            first.set_selected(true);
        }
        
        // Create close button
        let close_btn = Button::new("✕ Close", WidgetId("TaskModalClose".to_string()))
            .theme(CATPPUCCINTHEME);
        
        // Spawn async data loading
        tokio::spawn(async move {
            // Load associated data
            if let Ok(ticket) = TicketData::get_associated_ticket(task_id.clone()).await {
                log::info!("Loaded ticket for task modal: {:?}", ticket.id);
            }
            if let Ok(customer) = CustomerData::get_associated_customer(task_id.clone()).await {
                log::info!("Loaded customer for task modal: {}", customer.name);
            }
            if let Ok(computer) = ComputerData::get_associated_computer(task_id.clone()).await {
                log::info!("Loaded computer for task modal: {}", computer.hostname);
            }
        });
        
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
        }
    }
    
    /// Set the active tab and update button states
    pub fn set_active_tab(&mut self, page: ModalPage) {
        *self.current_page.borrow_mut() = page;
        
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
}
