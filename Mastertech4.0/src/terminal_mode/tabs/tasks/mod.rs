use crate::terminal_mode::context::TerminalContext;
use crate::terminal_mode::fx::{EffectStage, UniqueEffectId};
use crate::terminal_mode::events::action_handler::WidgetId;
use crate::terminal_mode::styling::ThemeRole;
use crate::terminal_mode::widgets::button::Button;
use crate::terminal_mode::widgets::dropdown_menu::DropdownMenu;
use crate::terminal_mode::widgets::menu_item::MenuItem;
use crate::terminal_mode::modals::TaskModal;
use std::{cell::RefCell, sync::{Arc, Mutex}};
use database::schema::{LiveTaskPayload, Priority, RecordIdExt, Status, User};
use crate::terminal_mode::widgets::tui_scroll_view::ScrollViewState;
use ratatui::widgets::TableState;
use ratatui::layout::Rect;
use reqwest::Client;

pub mod action_handler;
pub mod render;

/// Column identifiers for sorting
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortColumn {
    #[default]
    DueDate,
    Status,
    Priority,
}

/// Which set of tasks the list shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TaskFilter {
    #[default]
    My,
    Store,
    Completed,
}

impl TaskFilter {
    pub fn label(&self) -> &'static str {
        match self {
            TaskFilter::My => "My Tasks",
            TaskFilter::Store => "Store Tasks",
            TaskFilter::Completed => "Completed",
        }
    }

    pub const ALL: [TaskFilter; 3] = [TaskFilter::My, TaskFilter::Store, TaskFilter::Completed];
}

/// Sort direction
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortDirection {
    #[default]
    Ascending,
    Descending,
}

impl SortDirection {
    pub fn toggle(&mut self) {
        *self = match self {
            SortDirection::Ascending => SortDirection::Descending,
            SortDirection::Descending => SortDirection::Ascending,
        };
    }
}

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

pub struct TasksTab<'a> {
    state: RefCell<TableState>,
    pub items: Vec<LiveTaskPayload>,
    widths: Vec<u16>, // Column widths (first 5 columns, description fills remaining)
    scroll_state: RefCell<ScrollViewState>,
    pub _client: Client,
    ctx: Arc<Mutex<TerminalContext>>,
    pub store_users: Vec<User>,
    pub current_user: User,
    /// Table area for mouse hit-testing
    pub table_area: RefCell<Rect>,
    /// Visible row screen rects paired with their item index (for hit-testing).
    pub row_areas: RefCell<Vec<(usize, Rect)>>,
    /// Current edit mode
    pub edit_mode: RefCell<EditMode>,
    /// Effect stage for animations
    pub effect_stage: RefCell<EffectStage<UniqueEffectId>>,
    /// Current sort column
    pub sort_column: RefCell<SortColumn>,
    /// Current sort direction
    pub sort_direction: RefCell<SortDirection>,
    /// Hovered row index (for mouse hover highlighting)
    pub hovered_row: RefCell<Option<usize>>,
    /// Hovered column index (for header hover)
    pub hovered_header_col: RefCell<Option<usize>>,
    /// Currently open task modal (if any)
    pub open_task_modal: RefCell<Option<TaskModal<'static>>>,
    /// Sortable header buttons
    pub sort_due_btn: Button<'a>,
    pub sort_status_btn: Button<'a>,
    pub sort_priority_btn: Button<'a>,
    /// Current task filter (My / Store / Completed).
    pub filter: RefCell<TaskFilter>,
    /// Set when the filter changes so `check_tasks` rebuilds the list.
    pub filter_dirty: RefCell<bool>,
    /// Trigger button that opens the filter dropdown.
    pub filter_trigger: Button<'a>,
    /// The filter dropdown overlay.
    pub filter_dropdown: RefCell<DropdownMenu>,
}

impl<'a> TasksTab<'a> {
    pub fn new(_client: Client, ctx: Arc<Mutex<TerminalContext>>) -> Self {
        // Create sortable header buttons
        let sort_due_btn = Button::new("Due ▼", WidgetId("TasksSortDue".to_string()))
            .theme(ThemeRole::Neutral);
        let sort_status_btn = Button::new("Status", WidgetId("TasksSortStatus".to_string()))
            .theme(ThemeRole::Neutral);
        let sort_priority_btn = Button::new("Priority", WidgetId("TasksSortPriority".to_string()))
            .theme(ThemeRole::Neutral);
        let filter_trigger = Button::new(TaskFilter::My.label(), WidgetId("TasksFilter".to_string()))
            .menu_trigger();

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
            sort_column: RefCell::new(SortColumn::default()),
            sort_direction: RefCell::new(SortDirection::default()),
            hovered_row: RefCell::new(None),
            hovered_header_col: RefCell::new(None),
            open_task_modal: RefCell::new(None),
            sort_due_btn,
            sort_status_btn,
            sort_priority_btn,
            filter: RefCell::new(TaskFilter::My),
            filter_dirty: RefCell::new(false),
            filter_trigger,
            filter_dropdown: RefCell::new(DropdownMenu::new()),
        }
    }

    /// Build the dropdown rows for the filter, marking the active one.
    pub fn filter_items(&self) -> Vec<MenuItem> {
        let current = *self.filter.borrow();
        TaskFilter::ALL
            .iter()
            .map(|f| MenuItem::new(f.label()).active(*f == current))
            .collect()
    }

    /// Request a new filter; `check_tasks` applies it (label + list rebuild).
    pub fn set_filter(&self, filter: TaskFilter) {
        *self.filter.borrow_mut() = filter;
        *self.filter_dirty.borrow_mut() = true;
    }

    /// Rebuild `items` from the context task list using the current filter.
    fn rebuild_items(&mut self) {
        let filter = *self.filter.borrow();
        let me = self.current_user.get_id();
        if let Ok(ctx) = self.ctx.lock() {
            self.items.clear();
            for task in ctx.tasks.iter() {
                let keep = match filter {
                    TaskFilter::My => task.assignee == me && !task.completed,
                    TaskFilter::Store => !task.completed,
                    TaskFilter::Completed => task.completed,
                };
                if keep {
                    self.items.push(task.clone());
                }
            }
        }
        self.widths = Self::calculate_widths(&self.items, &self.store_users);
        self.sort_items();
    }
    
    /// Update sort button labels based on current sort state
    pub fn update_sort_button_labels(&mut self) {
        let sort_col = *self.sort_column.borrow();
        let sort_dir = *self.sort_direction.borrow();
        let indicator = match sort_dir {
            SortDirection::Ascending => " ▲",
            SortDirection::Descending => " ▼",
        };
        
        // Reset all labels first
        self.sort_due_btn.set_label("Due".to_string());
        self.sort_status_btn.set_label("Status".to_string());
        self.sort_priority_btn.set_label("Priority".to_string());
        
        // Add indicator to current sort column
        match sort_col {
            SortColumn::DueDate => self.sort_due_btn.set_label(format!("Due{}", indicator)),
            SortColumn::Status => self.sort_status_btn.set_label(format!("Status{}", indicator)),
            SortColumn::Priority => self.sort_priority_btn.set_label(format!("Priority{}", indicator)),
        }
    }
    
    /// Sort the items based on current sort column and direction
    pub fn sort_items(&mut self) {
        let sort_col = *self.sort_column.borrow();
        let sort_dir = *self.sort_direction.borrow();
        
        self.items.sort_by(|a, b| {
            let cmp = match sort_col {
                SortColumn::DueDate => a.due_date.cmp(&b.due_date),
                SortColumn::Status => a.status.as_str().cmp(b.status.as_str()),
                SortColumn::Priority => {
                    // Custom order: Express > Fire > RFS > QC > Normal
                    let priority_order = |p: &Priority| match p {
                        Priority::Express => 0,
                        Priority::Fire => 1,
                        Priority::Rfs => 2,
                        Priority::Qc => 3,
                        Priority::Normal => 4,
                    };
                    priority_order(&a.priority).cmp(&priority_order(&b.priority))
                }
            };
            
            match sort_dir {
                SortDirection::Ascending => cmp,
                SortDirection::Descending => cmp.reverse(),
            }
        });
    }
    
    /// Toggle sort by column - if same column, toggle direction; if different, sort ascending
    pub fn toggle_sort(&mut self, column: SortColumn) {
        let current_col = *self.sort_column.borrow();
        if current_col == column {
            self.sort_direction.borrow_mut().toggle();
        } else {
            *self.sort_column.borrow_mut() = column;
            *self.sort_direction.borrow_mut() = SortDirection::Ascending;
        }
        self.sort_items();
    }
    
    /// Open the task modal for a specific task
    pub fn open_modal(&self, task_idx: usize) {
        if let Some(task) = self.items.get(task_idx) {
            let modal = TaskModal::new(task.clone(), self.store_users.clone(), self.current_user.clone());
            self.open_task_modal.replace(Some(modal));
        }
    }
    
    /// Close the task modal
    pub fn close_modal(&self) {
        self.open_task_modal.replace(None);
    }
    
    pub fn check_tasks(&mut self) {
        let mut needs_rebuild = false;
        if let Ok(mut ctx) = self.ctx.lock() {
            // Update store users from context
            if !ctx.store_users.is_empty() && self.store_users.is_empty() {
                self.store_users = ctx.store_users.clone();
            }
            if ctx.new_tasks {
                ctx.new_tasks = false;
                needs_rebuild = true;
            }
            if !ctx.user.get_name().is_empty() {
                self.current_user = ctx.user.clone();
            }
        }

        // A filter change (set via the dropdown) also forces a rebuild.
        if *self.filter_dirty.borrow() {
            *self.filter_dirty.borrow_mut() = false;
            self.filter_trigger.set_label(self.filter.borrow().label().to_string());
            needs_rebuild = true;
        }

        if needs_rebuild {
            self.rebuild_items();
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
