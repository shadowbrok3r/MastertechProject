//! Duplicate Merge Modal for Terminal Mode
//! 
//! Displays a diff view when duplicate records are detected during task creation.
//! Allows users to keep existing, use new, or merge fields from both versions.

use std::cell::RefCell;
use std::collections::HashMap;
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Tabs, Wrap},
    Frame,
};
use crossterm::event::{KeyCode, KeyEvent, MouseEvent, MouseEventKind, MouseButton};
use database::schema::{
    ComputerData, CustomerData, DuplicateCheckResult, DuplicateResolution, 
    FieldDisplay, FieldSelections, LiveTaskPayload, MergeResolution, 
    RecordId, RecordIdExt, TicketData, merge_task, merge_ticket, 
    merge_customer, merge_computer,
};

use crate::terminal_mode::styling::CATPPUCCIN;

/// The current page/tab being displayed in the merge modal
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum MergeModalPage {
    #[default]
    Task,
    ServiceOrder,
    Customer,
    Computer,
    Summary,
}

impl MergeModalPage {
    pub fn as_str(&self) -> &str {
        match self {
            MergeModalPage::Task => "Task",
            MergeModalPage::ServiceOrder => "Service",
            MergeModalPage::Customer => "Customer",
            MergeModalPage::Computer => "Computer",
            MergeModalPage::Summary => "Summary",
        }
    }
    
    pub fn all() -> Vec<MergeModalPage> {
        vec![
            MergeModalPage::Task,
            MergeModalPage::ServiceOrder,
            MergeModalPage::Customer,
            MergeModalPage::Computer,
            MergeModalPage::Summary,
        ]
    }
}

/// Duplicate Merge Modal for resolving conflicts between existing and new records
pub struct DuplicateMergeModal {
    pub title: String,
    /// The duplicate check result containing all potential conflicts
    pub check_result: DuplicateCheckResult,
    /// User's resolution choices
    pub resolution: DuplicateResolution,
    /// Current page being viewed
    pub current_page: MergeModalPage,
    /// Current tab index (0-4)
    pub tab_index: usize,
    /// Whether the modal is open
    pub is_open: bool,
    /// Whether the user has confirmed their choices
    pub confirmed: bool,
    /// Whether the user cancelled
    pub cancelled: bool,
    /// Cache of user RecordId -> username for display
    pub user_cache: HashMap<String, String>,
    /// Current field index for navigation within a page
    pub field_index: RefCell<usize>,
    /// List state for field selection
    pub list_state: RefCell<ListState>,
    /// Modal area for mouse hit-testing
    pub modal_area: RefCell<Rect>,
}

impl Default for DuplicateMergeModal {
    fn default() -> Self {
        Self {
            title: "Resolve Duplicate Records".to_string(),
            check_result: DuplicateCheckResult::default(),
            resolution: DuplicateResolution::default(),
            current_page: MergeModalPage::default(),
            tab_index: 0,
            is_open: false,
            confirmed: false,
            cancelled: false,
            user_cache: HashMap::new(),
            field_index: RefCell::new(0),
            list_state: RefCell::new(ListState::default()),
            modal_area: RefCell::new(Rect::default()),
        }
    }
}

impl DuplicateMergeModal {
    pub fn new(check_result: DuplicateCheckResult) -> Self {
        let title = format!("Duplicate Records Found - SO#{}", check_result.service_number);
        let modal = Self {
            title,
            check_result,
            resolution: DuplicateResolution::default(),
            current_page: MergeModalPage::Task,
            tab_index: 0,
            is_open: true,
            confirmed: false,
            cancelled: false,
            user_cache: HashMap::new(),
            field_index: RefCell::new(0),
            list_state: RefCell::new(ListState::default()),
            modal_area: RefCell::new(Rect::default()),
        };
        modal.list_state.borrow_mut().select(Some(0));
        modal
    }
    
    /// Add a user to the cache for display purposes
    pub fn cache_user(&mut self, id: &RecordId, username: &str) {
        self.user_cache.insert(id.key_string(), username.to_string());
    }
    
    /// Get a formatted string for an assignee
    pub fn format_assignee(&self, id: &RecordId) -> String {
        let id_str = id.key_string();
        if let Some(username) = self.user_cache.get(&id_str) {
            format!("{} ({})", username, id_str)
        } else {
            id_str
        }
    }

    pub fn open(&mut self) {
        self.is_open = true;
        self.confirmed = false;
        self.cancelled = false;
    }

    pub fn close(&mut self) {
        self.is_open = false;
    }

    pub fn is_confirmed(&self) -> bool {
        self.confirmed
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    /// Get the final resolution after user confirms
    pub fn get_resolution(&self) -> &DuplicateResolution {
        &self.resolution
    }
    
    /// Draw the modal
    pub fn draw(&self, f: &mut Frame, area: Rect) {
        if !self.is_open {
            return;
        }

        // Calculate modal size (80% of screen)
        let modal_width = (area.width as f32 * 0.85) as u16;
        let modal_height = (area.height as f32 * 0.85) as u16;
        let modal_x = (area.width - modal_width) / 2;
        let modal_y = (area.height - modal_height) / 2;
        
        let modal_area = Rect {
            x: modal_x,
            y: modal_y,
            width: modal_width,
            height: modal_height,
        };
        
        *self.modal_area.borrow_mut() = modal_area;

        // Clear the modal area
        f.render_widget(Clear, modal_area);

        // Main modal block
        let block = Block::default()
            .title(format!(" {} ", self.title))
            .title_alignment(Alignment::Center)
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(CATPPUCCIN.peach))
            .style(Style::default().bg(Color::Rgb(20, 20, 28)));

        let inner = block.inner(modal_area);
        f.render_widget(block, modal_area);

        // Layout: tabs | content | action buttons
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),      // Tabs
                Constraint::Min(10),        // Content
                Constraint::Length(3),      // Action buttons
            ])
            .split(inner);

        self.draw_tabs(f, layout[0]);
        self.draw_content(f, layout[1]);
        self.draw_action_buttons(f, layout[2]);
    }
    
    fn draw_tabs(&self, f: &mut Frame, area: Rect) {
        let tab_titles: Vec<Line> = MergeModalPage::all()
            .iter()
            .enumerate()
            .map(|(i, page)| {
                let has_conflict = match page {
                    MergeModalPage::Task => self.check_result.task.as_ref().map_or(false, |d| !d.is_identical),
                    MergeModalPage::ServiceOrder => self.check_result.service_order.as_ref().map_or(false, |d| !d.is_identical),
                    MergeModalPage::Customer => self.check_result.customer.as_ref().map_or(false, |d| !d.is_identical),
                    MergeModalPage::Computer => self.check_result.computer.as_ref().map_or(false, |d| !d.is_identical),
                    MergeModalPage::Summary => false,
                };
                
                let indicator = if has_conflict { "⚠" } else { "✅" };
                let label = format!("{} {}", page.as_str(), indicator);
                
                let style = if i == self.tab_index {
                    Style::default().fg(CATPPUCCIN.peach).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(CATPPUCCIN.text)
                };
                
                Line::from(Span::styled(label, style))
            })
            .collect();

        let tabs = Tabs::new(tab_titles)
            .block(Block::default().borders(Borders::BOTTOM).border_style(Style::default().fg(CATPPUCCIN.surface0)))
            .highlight_style(Style::default().fg(CATPPUCCIN.peach).add_modifier(Modifier::BOLD))
            .select(self.tab_index);

        f.render_widget(tabs, area);
    }
    
    fn draw_content(&self, f: &mut Frame, area: Rect) {
        match self.current_page {
            MergeModalPage::Task => self.draw_task_diff(f, area),
            MergeModalPage::ServiceOrder => self.draw_service_diff(f, area),
            MergeModalPage::Customer => self.draw_customer_diff(f, area),
            MergeModalPage::Computer => self.draw_computer_diff(f, area),
            MergeModalPage::Summary => self.draw_summary(f, area),
        }
    }
    
    fn draw_task_diff(&self, f: &mut Frame, area: Rect) {
        if let Some(ref dup) = self.check_result.task {
            if dup.is_identical {
                self.draw_identical_message(f, area, "Task");
                return;
            }
            
            let fields = dup.existing.get_differing_fields(&dup.new);
            self.draw_field_diff(f, area, "Task Differences", &fields, &self.resolution.task_resolution, &self.resolution.task_fields);
        } else {
            self.draw_no_duplicate_message(f, area, "Task");
        }
    }
    
    fn draw_service_diff(&self, f: &mut Frame, area: Rect) {
        if let Some(ref dup) = self.check_result.service_order {
            if dup.is_identical {
                self.draw_identical_message(f, area, "Service Order");
                return;
            }
            
            let fields = dup.existing.get_differing_fields(&dup.new);
            self.draw_field_diff(f, area, "Service Order Differences", &fields, &self.resolution.service_order_resolution, &self.resolution.service_order_fields);
        } else {
            self.draw_no_duplicate_message(f, area, "Service Order");
        }
    }
    
    fn draw_customer_diff(&self, f: &mut Frame, area: Rect) {
        if let Some(ref dup) = self.check_result.customer {
            if dup.is_identical {
                self.draw_identical_message(f, area, "Customer");
                return;
            }
            
            let fields = dup.existing.get_differing_fields(&dup.new);
            self.draw_field_diff(f, area, "Customer Differences", &fields, &self.resolution.customer_resolution, &self.resolution.customer_fields);
        } else {
            self.draw_no_duplicate_message(f, area, "Customer");
        }
    }
    
    fn draw_computer_diff(&self, f: &mut Frame, area: Rect) {
        if let Some(ref dup) = self.check_result.computer {
            if dup.is_identical {
                self.draw_identical_message(f, area, "Computer");
                return;
            }
            
            let fields = dup.existing.get_differing_fields(&dup.new);
            self.draw_field_diff(f, area, "Computer Differences", &fields, &self.resolution.computer_resolution, &self.resolution.computer_fields);
        } else {
            self.draw_no_duplicate_message(f, area, "Computer");
        }
    }
    
    fn draw_identical_message(&self, f: &mut Frame, area: Rect, entity: &str) {
        let message = Paragraph::new(format!("✅ {} records are identical - no action needed", entity))
            .style(Style::default().fg(CATPPUCCIN.green))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });
        f.render_widget(message, area);
    }
    
    fn draw_no_duplicate_message(&self, f: &mut Frame, area: Rect, entity: &str) {
        let message = Paragraph::new(format!("No duplicate {} found", entity))
            .style(Style::default().fg(CATPPUCCIN.subtext0))
            .alignment(Alignment::Center);
        f.render_widget(message, area);
    }
    
    fn draw_field_diff(&self, f: &mut Frame, area: Rect, _title: &str, fields: &[(String, String, String)], resolution: &MergeResolution, selections: &FieldSelections) {
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),  // Resolution selector
                Constraint::Length(2),  // Header
                Constraint::Min(5),     // Field list
            ])
            .split(area);
        
        // Resolution selector
        let res_text = match resolution {
            MergeResolution::KeepExisting => "[K] Keep Existing  [ ] Use New  [ ] Merge",
            MergeResolution::UseNew => "[ ] Keep Existing  [N] Use New  [ ] Merge",
            MergeResolution::Merge => "[ ] Keep Existing  [ ] Use New  [M] Merge",
            MergeResolution::Cancel => "[ ] Keep Existing  [ ] Use New  [ ] Merge",
        };
        let res_para = Paragraph::new(Line::from(vec![
            Span::styled("Resolution: ", Style::default().fg(CATPPUCCIN.text)),
            Span::styled(res_text, Style::default().fg(CATPPUCCIN.yellow)),
        ]))
        .block(Block::default().borders(Borders::BOTTOM).border_style(Style::default().fg(CATPPUCCIN.surface0)));
        f.render_widget(res_para, layout[0]);
        
        // Header
        let header = Paragraph::new(Line::from(vec![
            Span::styled(format!("{:<20}", "Field"), Style::default().fg(CATPPUCCIN.mauve).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{:<30}", "Existing"), Style::default().fg(Color::Rgb(255, 150, 150)).add_modifier(Modifier::BOLD)),
            Span::raw(" → "),
            Span::styled(format!("{:<30}", "New"), Style::default().fg(Color::Rgb(150, 255, 150)).add_modifier(Modifier::BOLD)),
        ]));
        f.render_widget(header, layout[1]);
        
        // Field list
        if fields.is_empty() {
            let no_diff = Paragraph::new("No differences found")
                .style(Style::default().fg(CATPPUCCIN.green))
                .alignment(Alignment::Center);
            f.render_widget(no_diff, layout[2]);
            return;
        }
        
        let show_checkbox = *resolution == MergeResolution::Merge;
        let items: Vec<ListItem> = fields.iter().enumerate().map(|(i, (field_name, existing, new))| {
            let checkbox = if show_checkbox {
                if selections.use_new(field_name) { "[✓] " } else { "[ ] " }
            } else {
                "    "
            };
            
            let existing_display = if existing.is_empty() { "(empty)" } else { existing.as_str() };
            let new_display = if new.is_empty() { "(empty)" } else { new.as_str() };
            
            // Truncate long values
            let existing_truncated = if existing_display.len() > 28 {
                format!("{}...", &existing_display[..25])
            } else {
                existing_display.to_string()
            };
            let new_truncated = if new_display.len() > 28 {
                format!("{}...", &new_display[..25])
            } else {
                new_display.to_string()
            };
            
            let selected = *self.field_index.borrow() == i;
            let style = if selected {
                Style::default().bg(CATPPUCCIN.surface0).fg(CATPPUCCIN.text)
            } else {
                Style::default().fg(CATPPUCCIN.text)
            };
            
            ListItem::new(Line::from(vec![
                Span::styled(checkbox, Style::default().fg(CATPPUCCIN.yellow)),
                Span::styled(format!("{:<16}", field_name), Style::default().fg(CATPPUCCIN.mauve)),
                Span::styled(format!("{:<28}", existing_truncated), Style::default().fg(Color::Rgb(255, 150, 150))),
                Span::raw(" → "),
                Span::styled(format!("{:<28}", new_truncated), Style::default().fg(Color::Rgb(150, 255, 150))),
            ])).style(style)
        }).collect();
        
        let list = List::new(items)
            .block(Block::default())
            .highlight_style(Style::default().bg(CATPPUCCIN.surface0));
        
        let mut state = self.list_state.borrow_mut();
        f.render_stateful_widget(list, layout[2], &mut state);
    }
    
    fn draw_summary(&self, f: &mut Frame, area: Rect) {
        let resolution_text = |res: &MergeResolution| -> &str {
            match res {
                MergeResolution::KeepExisting => "Keep Existing",
                MergeResolution::UseNew => "Use New",
                MergeResolution::Merge => "Merge Fields",
                MergeResolution::Cancel => "Cancel",
            }
        };
        
        let mut lines: Vec<Line> = vec![
            Line::from(Span::styled("Resolution Summary", Style::default().fg(CATPPUCCIN.peach).add_modifier(Modifier::BOLD))),
            Line::from(""),
            Line::from(vec![
                Span::styled(format!("{:<15}", "Entity"), Style::default().fg(CATPPUCCIN.mauve).add_modifier(Modifier::BOLD)),
                Span::styled(format!("{:<15}", "Status"), Style::default().fg(CATPPUCCIN.mauve).add_modifier(Modifier::BOLD)),
                Span::styled("Resolution", Style::default().fg(CATPPUCCIN.mauve).add_modifier(Modifier::BOLD)),
            ]),
            Line::from("─".repeat(50)),
        ];
        
        // Task
        if let Some(ref dup) = self.check_result.task {
            let status = if dup.is_identical { "✅ Identical" } else { "⚠ Conflict" };
            let status_color = if dup.is_identical { CATPPUCCIN.green } else { CATPPUCCIN.yellow };
            lines.push(Line::from(vec![
                Span::styled(format!("{:<15}", "Task"), Style::default().fg(CATPPUCCIN.text)),
                Span::styled(format!("{:<15}", status), Style::default().fg(status_color)),
                Span::styled(resolution_text(&self.resolution.task_resolution), Style::default().fg(CATPPUCCIN.text)),
            ]));
        }
        
        // Service Order
        if let Some(ref dup) = self.check_result.service_order {
            let status = if dup.is_identical { "✅ Identical" } else { "⚠ Conflict" };
            let status_color = if dup.is_identical { CATPPUCCIN.green } else { CATPPUCCIN.yellow };
            lines.push(Line::from(vec![
                Span::styled(format!("{:<15}", "Service Order"), Style::default().fg(CATPPUCCIN.text)),
                Span::styled(format!("{:<15}", status), Style::default().fg(status_color)),
                Span::styled(resolution_text(&self.resolution.service_order_resolution), Style::default().fg(CATPPUCCIN.text)),
            ]));
        }
        
        // Customer
        if let Some(ref dup) = self.check_result.customer {
            let status = if dup.is_identical { "✅ Identical" } else { "⚠ Conflict" };
            let status_color = if dup.is_identical { CATPPUCCIN.green } else { CATPPUCCIN.yellow };
            lines.push(Line::from(vec![
                Span::styled(format!("{:<15}", "Customer"), Style::default().fg(CATPPUCCIN.text)),
                Span::styled(format!("{:<15}", status), Style::default().fg(status_color)),
                Span::styled(resolution_text(&self.resolution.customer_resolution), Style::default().fg(CATPPUCCIN.text)),
            ]));
        }
        
        // Computer
        if let Some(ref dup) = self.check_result.computer {
            let status = if dup.is_identical { "✅ Identical" } else { "⚠ Conflict" };
            let status_color = if dup.is_identical { CATPPUCCIN.green } else { CATPPUCCIN.yellow };
            lines.push(Line::from(vec![
                Span::styled(format!("{:<15}", "Computer"), Style::default().fg(CATPPUCCIN.text)),
                Span::styled(format!("{:<15}", status), Style::default().fg(status_color)),
                Span::styled(resolution_text(&self.resolution.computer_resolution), Style::default().fg(CATPPUCCIN.text)),
            ]));
        }
        
        let paragraph = Paragraph::new(lines)
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: true });
        f.render_widget(paragraph, area);
    }
    
    fn draw_action_buttons(&self, f: &mut Frame, area: Rect) {
        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(20),
                Constraint::Percentage(20),
                Constraint::Percentage(20),
                Constraint::Percentage(20),
                Constraint::Percentage(20),
            ])
            .split(area);
        
        let buttons = [
            ("[Esc] Cancel", CATPPUCCIN.red),
            ("[K] Keep All", CATPPUCCIN.yellow),
            ("[N] Use All New", CATPPUCCIN.blue),
            ("[M] Merge All", CATPPUCCIN.mauve),
            ("[Enter] Confirm", CATPPUCCIN.green),
        ];
        
        for (i, (label, color)) in buttons.iter().enumerate() {
            let para = Paragraph::new(*label)
                .style(Style::default().fg(*color))
                .alignment(Alignment::Center);
            f.render_widget(para, layout[i]);
        }
    }
    
    /// Handle keyboard events
    pub fn handle_key_event(&mut self, key_event: KeyEvent) -> bool {
        match key_event.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.cancelled = true;
                self.close();
                true
            }
            KeyCode::Enter => {
                self.confirmed = true;
                self.close();
                true
            }
            KeyCode::Tab | KeyCode::Right => {
                self.next_tab();
                true
            }
            KeyCode::BackTab | KeyCode::Left => {
                self.prev_tab();
                true
            }
            KeyCode::Up => {
                self.prev_field();
                true
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.next_field();
                true
            }
            KeyCode::Char('K') => {
                // Keep all existing (Shift+K)
                self.resolution.task_resolution = MergeResolution::KeepExisting;
                self.resolution.service_order_resolution = MergeResolution::KeepExisting;
                self.resolution.customer_resolution = MergeResolution::KeepExisting;
                self.resolution.computer_resolution = MergeResolution::KeepExisting;
                true
            }
            KeyCode::Char('N') => {
                // Use all new (Shift+N)
                self.resolution.task_resolution = MergeResolution::UseNew;
                self.resolution.service_order_resolution = MergeResolution::UseNew;
                self.resolution.customer_resolution = MergeResolution::UseNew;
                self.resolution.computer_resolution = MergeResolution::UseNew;
                true
            }
            KeyCode::Char('M') => {
                // Merge all (Shift+M)
                self.resolution.task_resolution = MergeResolution::Merge;
                self.resolution.service_order_resolution = MergeResolution::Merge;
                self.resolution.customer_resolution = MergeResolution::Merge;
                self.resolution.computer_resolution = MergeResolution::Merge;
                true
            }
            KeyCode::Char('k') => {
                // Move up or set current page resolution to keep existing
                self.prev_field();
                true
            }
            KeyCode::Char('n') => {
                // Set current page resolution to use new
                self.set_current_resolution(MergeResolution::UseNew);
                true
            }
            KeyCode::Char('m') => {
                // Set current page resolution to merge
                self.set_current_resolution(MergeResolution::Merge);
                true
            }
            KeyCode::Char(' ') => {
                // Toggle field selection in merge mode
                self.toggle_field_selection();
                true
            }
            _ => false,
        }
    }
    
    /// Handle mouse events
    pub fn handle_mouse_event(&mut self, mouse_event: &MouseEvent) -> bool {
        let modal_area = *self.modal_area.borrow();
        let x = mouse_event.column;
        let y = mouse_event.row;
        
        // Check if click is within modal
        if x < modal_area.x || x >= modal_area.right() || y < modal_area.y || y >= modal_area.bottom() {
            return false;
        }
        
        match mouse_event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                // Check if clicking on tabs area (first 3 lines of modal)
                if y >= modal_area.y + 1 && y < modal_area.y + 4 {
                    let tab_width = modal_area.width / 5;
                    let clicked_tab = ((x - modal_area.x) / tab_width) as usize;
                    if clicked_tab < 5 {
                        self.tab_index = clicked_tab;
                        self.current_page = MergeModalPage::all()[clicked_tab].clone();
                        *self.field_index.borrow_mut() = 0;
                        self.list_state.borrow_mut().select(Some(0));
                    }
                    return true;
                }
                
                // Check if clicking on action buttons area (last 3 lines)
                if y >= modal_area.bottom() - 4 {
                    let button_width = modal_area.width / 5;
                    let clicked_button = ((x - modal_area.x) / button_width) as usize;
                    match clicked_button {
                        0 => { self.cancelled = true; self.close(); } // Cancel
                        1 => { // Keep All
                            self.resolution.task_resolution = MergeResolution::KeepExisting;
                            self.resolution.service_order_resolution = MergeResolution::KeepExisting;
                            self.resolution.customer_resolution = MergeResolution::KeepExisting;
                            self.resolution.computer_resolution = MergeResolution::KeepExisting;
                        }
                        2 => { // Use All New
                            self.resolution.task_resolution = MergeResolution::UseNew;
                            self.resolution.service_order_resolution = MergeResolution::UseNew;
                            self.resolution.customer_resolution = MergeResolution::UseNew;
                            self.resolution.computer_resolution = MergeResolution::UseNew;
                        }
                        3 => { // Merge All
                            self.resolution.task_resolution = MergeResolution::Merge;
                            self.resolution.service_order_resolution = MergeResolution::Merge;
                            self.resolution.customer_resolution = MergeResolution::Merge;
                            self.resolution.computer_resolution = MergeResolution::Merge;
                        }
                        4 => { self.confirmed = true; self.close(); } // Confirm
                        _ => {}
                    }
                    return true;
                }
                
                false
            }
            MouseEventKind::ScrollDown => {
                self.next_field();
                true
            }
            MouseEventKind::ScrollUp => {
                self.prev_field();
                true
            }
            _ => false,
        }
    }
    
    fn next_tab(&mut self) {
        self.tab_index = (self.tab_index + 1) % 5;
        self.current_page = MergeModalPage::all()[self.tab_index].clone();
        *self.field_index.borrow_mut() = 0;
        self.list_state.borrow_mut().select(Some(0));
    }
    
    fn prev_tab(&mut self) {
        self.tab_index = if self.tab_index == 0 { 4 } else { self.tab_index - 1 };
        self.current_page = MergeModalPage::all()[self.tab_index].clone();
        *self.field_index.borrow_mut() = 0;
        self.list_state.borrow_mut().select(Some(0));
    }
    
    fn next_field(&mut self) {
        let field_count = self.get_current_field_count();
        if field_count > 0 {
            let mut idx = self.field_index.borrow_mut();
            *idx = (*idx + 1) % field_count;
            self.list_state.borrow_mut().select(Some(*idx));
        }
    }
    
    fn prev_field(&mut self) {
        let field_count = self.get_current_field_count();
        if field_count > 0 {
            let mut idx = self.field_index.borrow_mut();
            *idx = if *idx == 0 { field_count - 1 } else { *idx - 1 };
            self.list_state.borrow_mut().select(Some(*idx));
        }
    }
    
    fn get_current_field_count(&self) -> usize {
        match self.current_page {
            MergeModalPage::Task => {
                self.check_result.task.as_ref()
                    .map(|dup| dup.existing.get_differing_fields(&dup.new).len())
                    .unwrap_or(0)
            }
            MergeModalPage::ServiceOrder => {
                self.check_result.service_order.as_ref()
                    .map(|dup| dup.existing.get_differing_fields(&dup.new).len())
                    .unwrap_or(0)
            }
            MergeModalPage::Customer => {
                self.check_result.customer.as_ref()
                    .map(|dup| dup.existing.get_differing_fields(&dup.new).len())
                    .unwrap_or(0)
            }
            MergeModalPage::Computer => {
                self.check_result.computer.as_ref()
                    .map(|dup| dup.existing.get_differing_fields(&dup.new).len())
                    .unwrap_or(0)
            }
            MergeModalPage::Summary => 0,
        }
    }
    
    fn set_current_resolution(&mut self, resolution: MergeResolution) {
        match self.current_page {
            MergeModalPage::Task => self.resolution.task_resolution = resolution,
            MergeModalPage::ServiceOrder => self.resolution.service_order_resolution = resolution,
            MergeModalPage::Customer => self.resolution.customer_resolution = resolution,
            MergeModalPage::Computer => self.resolution.computer_resolution = resolution,
            MergeModalPage::Summary => {}
        }
    }
    
    fn toggle_field_selection(&mut self) {
        let idx = *self.field_index.borrow();
        
        let (fields, selections) = match self.current_page {
            MergeModalPage::Task => {
                if let Some(ref dup) = self.check_result.task {
                    (dup.existing.get_differing_fields(&dup.new), &mut self.resolution.task_fields)
                } else {
                    return;
                }
            }
            MergeModalPage::ServiceOrder => {
                if let Some(ref dup) = self.check_result.service_order {
                    (dup.existing.get_differing_fields(&dup.new), &mut self.resolution.service_order_fields)
                } else {
                    return;
                }
            }
            MergeModalPage::Customer => {
                if let Some(ref dup) = self.check_result.customer {
                    (dup.existing.get_differing_fields(&dup.new), &mut self.resolution.customer_fields)
                } else {
                    return;
                }
            }
            MergeModalPage::Computer => {
                if let Some(ref dup) = self.check_result.computer {
                    (dup.existing.get_differing_fields(&dup.new), &mut self.resolution.computer_fields)
                } else {
                    return;
                }
            }
            MergeModalPage::Summary => return,
        };
        
        if let Some((field_name, _, _)) = fields.get(idx) {
            let current = selections.use_new(field_name);
            selections.set_use_new(field_name, !current);
        }
    }
}
