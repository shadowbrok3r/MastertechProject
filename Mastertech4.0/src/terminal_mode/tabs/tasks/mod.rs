use crate::terminal_mode::context::TerminalContext;
use crate::terminal_mode::fx::{EffectStage, UniqueEffectId};
use std::{cell::RefCell, sync::{Arc, Mutex}};
use database::schema::{LiveTaskPayload, Priority, RecordIdExt, Status, User};
use crate::terminal_mode::widgets::tui_scroll_view::ScrollViewState;
use ratatui::widgets::TableState;
use ratatui::layout::Rect;
use reqwest::Client;

pub mod action_handler;
pub mod render;

/// Represents the edit state when a cell is being edited
#[derive(Debug, Clone, PartialEq)]
pub enum EditMode {
    None,
    Status { row: usize, options: Vec<Status>, selected_idx: usize },
    Assignee { row: usize, options: Vec<(String, String)>, selected_idx: usize }, // (user_id, username)
    Priority { row: usize, options: Vec<Priority>, selected_idx: usize },
    DueDate { row: usize },
}

impl Default for EditMode {
    fn default() -> Self {
        Self::None
    }
}

pub struct TasksTab {
    state: RefCell<TableState>,
    pub items: Vec<LiveTaskPayload>,
    widths: Vec<u16>, // Column widths
    scroll_state: RefCell<ScrollViewState>,
    pub _client: Client,
    ctx: Arc<Mutex<TerminalContext>>,
    pub store_users: Vec<User>,
    pub current_user: User,
    /// Table area for mouse hit-testing
    pub table_area: RefCell<Rect>,
    /// Row areas for mouse click detection
    pub row_areas: RefCell<Vec<Rect>>,
    /// Current edit mode
    pub edit_mode: RefCell<EditMode>,
    /// Effect stage for animations
    pub effect_stage: RefCell<EffectStage<UniqueEffectId>>,
    /// Whether effects have been initialized
    pub effects_init: RefCell<bool>,
}

impl TasksTab {
    pub fn new(_client: Client, ctx: Arc<Mutex<TerminalContext>>) -> Self {

        Self {
            ctx,
            _client,
            state: RefCell::new(TableState::default()),
            items: Vec::new(),
            widths: Vec::new(),
            scroll_state: RefCell::new(ScrollViewState::default()),
            store_users: Vec::new(),
            current_user: User::default(),
            table_area: RefCell::new(Rect::default()),
            row_areas: RefCell::new(Vec::new()),
            edit_mode: RefCell::new(EditMode::None),
            effect_stage: RefCell::new(EffectStage::default()),
            effects_init: RefCell::new(false),
        }
    }

    pub fn check_tasks(&mut self) {
        if let Ok(mut ctx) = self.ctx.lock() {
            // Update store users from context
            if !ctx.store_users.is_empty() && self.store_users.is_empty() {
                self.store_users = ctx.store_users.clone();
            }
            
            if ctx.new_tasks {
                ctx.new_tasks = false;
                self.items.clear();
                for task in ctx.tasks.iter() {
                    if task.assignee == self.current_user.get_id() && !task.completed {
                        self.items.push(task.clone());
                    }
                }
        
                self.widths = Self::calculate_widths(&self.items, &self.store_users);
            }
            if !ctx.user.get_name().is_empty() {
                self.current_user = ctx.user.clone();
            }
        }
    }
    
    /// Get username from user ID
    pub fn get_username(&self, user_id: &database::schema::RecordId) -> String {
        self.store_users
            .iter()
            .find(|u| &u.get_id() == user_id)
            .map(|u| u.get_username().to_owned())
            .unwrap_or_else(|| user_id.key_string())
    }
    
    /// Check if a task at the given row can be edited (is assigned to current user)
    pub fn can_edit(&self, row: usize) -> bool {
        if let Some(task) = self.items.get(row) {
            task.assignee == self.current_user.get_id()
        } else {
            false
        }
    }
    
    /// Toggle edit mode for a specific column
    pub fn toggle_edit(&self, row: usize, col: usize) {
        let mut edit_mode = self.edit_mode.borrow_mut();
        
        // If already editing, close it
        if *edit_mode != EditMode::None {
            *edit_mode = EditMode::None;
            return;
        }
        
        // Get the task at this row
        if let Some(task) = self.items.get(row) {
            match col {
                0 => {
                    // Due Date - would need calendar widget
                    *edit_mode = EditMode::DueDate { row };
                }
                1 => {
                    // Status
                    let statuses = Status::VALUES.to_vec();
                    let current_idx = statuses.iter()
                        .position(|s| s == &task.status)
                        .unwrap_or(0);
                    *edit_mode = EditMode::Status { 
                        row, 
                        options: statuses, 
                        selected_idx: current_idx 
                    };
                }
                3 => {
                    // Assignee
                    let options: Vec<(String, String)> = self.store_users
                        .iter()
                        .filter(|u| u.is_active())
                        .map(|u| (u.get_id().key_string(), u.get_username().to_owned()))
                        .collect();
                    let current_idx = options.iter()
                        .position(|(id, _)| id == &task.assignee.key_string())
                        .unwrap_or(0);
                    *edit_mode = EditMode::Assignee { 
                        row, 
                        options, 
                        selected_idx: current_idx 
                    };
                }
                4 => {
                    // Priority
                    let priorities = Priority::VALUES.to_vec();
                    let current_idx = priorities.iter()
                        .position(|p| p == &task.priority)
                        .unwrap_or(0);
                    *edit_mode = EditMode::Priority { 
                        row, 
                        options: priorities, 
                        selected_idx: current_idx 
                    };
                }
                _ => {}
            }
        }
    }
    
    /// Move selection up in edit mode
    pub fn edit_select_prev(&self) {
        let mut edit_mode = self.edit_mode.borrow_mut();
        match &mut *edit_mode {
            EditMode::Status { selected_idx, options, .. } => {
                if *selected_idx > 0 {
                    *selected_idx -= 1;
                } else {
                    *selected_idx = options.len().saturating_sub(1);
                }
            }
            EditMode::Assignee { selected_idx, options, .. } => {
                if *selected_idx > 0 {
                    *selected_idx -= 1;
                } else {
                    *selected_idx = options.len().saturating_sub(1);
                }
            }
            EditMode::Priority { selected_idx, options, .. } => {
                if *selected_idx > 0 {
                    *selected_idx -= 1;
                } else {
                    *selected_idx = options.len().saturating_sub(1);
                }
            }
            _ => {}
        }
    }
    
    /// Move selection down in edit mode
    pub fn edit_select_next(&self) {
        let mut edit_mode = self.edit_mode.borrow_mut();
        match &mut *edit_mode {
            EditMode::Status { selected_idx, options, .. } => {
                if *selected_idx < options.len().saturating_sub(1) {
                    *selected_idx += 1;
                } else {
                    *selected_idx = 0;
                }
            }
            EditMode::Assignee { selected_idx, options, .. } => {
                if *selected_idx < options.len().saturating_sub(1) {
                    *selected_idx += 1;
                } else {
                    *selected_idx = 0;
                }
            }
            EditMode::Priority { selected_idx, options, .. } => {
                if *selected_idx < options.len().saturating_sub(1) {
                    *selected_idx += 1;
                } else {
                    *selected_idx = 0;
                }
            }
            _ => {}
        }
    }
    
    /// Confirm the current edit selection
    pub fn confirm_edit(&mut self) -> Option<(usize, String, String)> {
        let edit_mode = self.edit_mode.borrow().clone();
        match edit_mode {
            EditMode::Status { row, options, selected_idx } => {
                if let Some(status) = options.get(selected_idx) {
                    if let Some(task) = self.items.get_mut(row) {
                        task.status = status.clone();
                    }
                    *self.edit_mode.borrow_mut() = EditMode::None;
                    return Some((row, "status".to_string(), status.as_str().to_string()));
                }
            }
            EditMode::Assignee { row, options, selected_idx } => {
                if let Some((user_id, _)) = options.get(selected_idx) {
                    if let Some(task) = self.items.get_mut(row) {
                        task.assignee = database::schema::RecordId::new("user", user_id.clone());
                    }
                    *self.edit_mode.borrow_mut() = EditMode::None;
                    return Some((row, "assignee".to_string(), user_id.clone()));
                }
            }
            EditMode::Priority { row, options, selected_idx } => {
                if let Some(priority) = options.get(selected_idx) {
                    if let Some(task) = self.items.get_mut(row) {
                        task.priority = priority.clone();
                    }
                    *self.edit_mode.borrow_mut() = EditMode::None;
                    return Some((row, "priority".to_string(), priority.as_str().to_string()));
                }
            }
            _ => {}
        }
        *self.edit_mode.borrow_mut() = EditMode::None;
        None
    }
    
    /// Cancel the current edit
    pub fn cancel_edit(&self) {
        *self.edit_mode.borrow_mut() = EditMode::None;
    }
    
    /// Check if currently in edit mode
    pub fn is_editing(&self) -> bool {
        *self.edit_mode.borrow() != EditMode::None
    }
}
