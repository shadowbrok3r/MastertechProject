use ratatui::{crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind}, layout::{Constraint, Layout, Position, Rect}, prelude::Backend, style::{Color, Modifier, Style}, text::{Line, Span, Text}, widgets::{Block, BorderType, Borders, Cell, Clear, Paragraph, Row, Scrollbar, ScrollbarOrientation, ScrollbarState, Table, WidgetRef}, Frame};
use crate::terminal_mode::{fx::unique_border_effect, styling::{CATPPUCCIN, THEME, APP_BACKGROUND}, widgets::{ButtonType, HandleWidget}};
use database::schema::{LiveTaskPayload, RecordIdExt, User};
use unicode_width::UnicodeWidthStr;
use std::cmp::max;
use super::{EditMode, SortColumn, SortDirection, TaskFilter, TasksTab};

// Static default widths for columns (first 5 columns only, description fills rest)
// Due, Status, Task, Assignee, Priority
const DEFAULT_WIDTHS: [u16; 5] = [12, 12, 30, 14, 10];

// Minimum width for description column
const MIN_DESCRIPTION_WIDTH: u16 = 30;

/// Implement the HandleWidget trait for TasksTab.
/// This allows the composite widget to draw itself and handle events.
impl<'a> HandleWidget <'a> for TasksTab<'a> {
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {
        let has_modal = self.open_task_modal.borrow().is_some();

        // Filter bar on top, table below.
        let rows = Layout::vertical([Constraint::Length(3), Constraint::Fill(1)]).split(area);
        self.draw_filter_bar(f, rows[0]);
        self.draw_table_impl(f, rows[1]);

        if has_modal {
            if let Some(ref mut modal) = *self.open_task_modal.borrow_mut() {
                modal.draw::<B>(f, area);
            }
            return;
        }

        // Filter dropdown overlay paints on top of the table.
        let frame = f.area();
        self.filter_dropdown.borrow_mut().render(f, frame);
    }
    
    fn handle_mouse_event(&self, mouse_event: &MouseEvent) {
        // Check modal state and handle events - use separate borrow scopes
        let (has_modal, should_close) = {
            let modal_ref = self.open_task_modal.borrow();
            if let Some(ref modal) = *modal_ref {
                modal.handle_mouse_event(mouse_event);
                (true, modal.should_close())
            } else {
                (false, false)
            }
        };
        
        if should_close {
            self.close_modal();
            return;
        }
        
        if has_modal {
            return;
        }

        // Filter dropdown takes priority (it overlays the table).
        let pos = Position::new(mouse_event.column, mouse_event.row);
        if self.filter_dropdown.borrow().is_open() {
            match mouse_event.kind {
                MouseEventKind::Moved => {
                    self.filter_dropdown.borrow_mut().on_mouse_move(pos);
                    return;
                }
                MouseEventKind::Down(MouseButton::Left) => {
                    let clicked = self.filter_dropdown.borrow_mut().on_click(pos);
                    if let Some(idx) = clicked {
                        if let Some(filter) = TaskFilter::ALL.get(idx) {
                            self.set_filter(*filter);
                        }
                    }
                    self.filter_dropdown.borrow_mut().close();
                    self.filter_trigger.set_menu_open(false);
                    return;
                }
                _ => {}
            }
        }
        if let MouseEventKind::Down(MouseButton::Left) = mouse_event.kind {
            if self.filter_trigger.get_area().map_or(false, |a| a.contains(pos)) {
                let items = self.filter_items();
                let anchor = self.filter_trigger.get_area().unwrap_or_default();
                self.filter_dropdown.borrow_mut().open_at(anchor, items, "Filter");
                self.filter_trigger.set_menu_open(true);
                return;
            }
        }

        let table_area = *self.table_area.borrow();
        let x = mouse_event.column;
        let y = mouse_event.row;

        self.sort_due_btn.handle_mouse_event(mouse_event);
        self.sort_status_btn.handle_mouse_event(mouse_event);
        self.sort_priority_btn.handle_mouse_event(mouse_event);

        match mouse_event.kind {
            MouseEventKind::ScrollDown => self.state.borrow_mut().scroll_down_by(1),
            MouseEventKind::ScrollUp => self.state.borrow_mut().scroll_up_by(1),
            MouseEventKind::ScrollLeft => self.state.borrow_mut().scroll_left_by(1),
            MouseEventKind::ScrollRight => self.state.borrow_mut().scroll_right_by(1),
            MouseEventKind::Moved => {
                // Update hover state
                if x >= table_area.x && x < table_area.right() 
                   && y >= table_area.y && y < table_area.bottom() 
                {
                    let header_height = 4;
                    
                    // Check if hovering over header
                    if y < table_area.y + header_height {
                        // Calculate which column header is being hovered
                        let widths = if self.widths.len() >= 5 { &self.widths } else { &DEFAULT_WIDTHS.to_vec() };
                        let mut col_start = table_area.x + 1;
                        let mut hovered_col = None;
                        
                        for (i, width) in widths.iter().enumerate() {
                            let col_end = col_start + width + 1;
                            if x >= col_start && x < col_end {
                                // Only columns 0 (Due), 1 (Status), 4 (Priority) are sortable
                                if matches!(i, 0 | 1 | 4) {
                                    hovered_col = Some(i);
                                }
                                break;
                            }
                            col_start = col_end;
                        }
                        
                        *self.hovered_header_col.borrow_mut() = hovered_col;
                        *self.hovered_row.borrow_mut() = None;
                    } else {
                        // Resolve the hovered row from recorded row rects.
                        let row = self
                            .row_areas
                            .borrow()
                            .iter()
                            .find(|(_, r)| y >= r.y && y < r.y + r.height)
                            .map(|(idx, _)| *idx);
                        *self.hovered_row.borrow_mut() = row;
                        *self.hovered_header_col.borrow_mut() = None;
                    }
                } else {
                    *self.hovered_row.borrow_mut() = None;
                    *self.hovered_header_col.borrow_mut() = None;
                }
            }
            MouseEventKind::Down(MouseButton::Left) => {
                // Check if click is within table area
                if x >= table_area.x && x < table_area.right() 
                   && y >= table_area.y && y < table_area.bottom() 
                {
                    let header_height = 4;
                    
                    // Check if click is on header (for sorting)
                    if y < table_area.y + header_height {
                        let widths = if self.widths.len() >= 5 { &self.widths } else { &DEFAULT_WIDTHS.to_vec() };
                        let mut col_start = table_area.x + 1;
                        
                        for (i, width) in widths.iter().enumerate() {
                            let col_end = col_start + width + 1;
                            if x >= col_start && x < col_end {
                                // Handle sortable columns
                                match i {
                                    0 => {
                                        // Sort by Due Date - need mutable access
                                        // This will be handled via message passing
                                        log::info!("Sort by Due Date clicked");
                                    }
                                    1 => {
                                        log::info!("Sort by Status clicked");
                                    }
                                    4 => {
                                        log::info!("Sort by Priority clicked");
                                    }
                                    _ => {}
                                }
                                break;
                            }
                            col_start = col_end;
                        }
                        return;
                    }
                    
                    // Resolve the clicked row from recorded row rects.
                    if y >= table_area.y + header_height {
                        let clicked_row = self
                            .row_areas
                            .borrow()
                            .iter()
                            .find(|(_, r)| y >= r.y && y < r.y + r.height)
                            .map(|(idx, _)| *idx);

                        if let Some(row_idx) = clicked_row {
                            // Calculate which column was clicked
                            let widths = if self.widths.len() >= 5 { &self.widths } else { &DEFAULT_WIDTHS.to_vec() };
                            let mut col_start = table_area.x + 1;
                            let mut col_idx = 5; // default to Description (fills remainder)

                            for (i, width) in widths.iter().enumerate() {
                                let col_end = col_start + width + 1;
                                if x >= col_start && x < col_end {
                                    col_idx = i;
                                    break;
                                }
                                col_start = col_end;
                            }

                            {
                                let mut state = self.state.borrow_mut();
                                state.select(Some(row_idx));
                                state.select_column(Some(col_idx));
                                state.select_cell(Some((row_idx, col_idx)));
                            }

                            // Clicking the Task Name column opens the task modal.
                            if col_idx == 2 {
                                self.open_modal(row_idx);
                            }
                        }
                    }
                }
            }
            MouseEventKind::Down(MouseButton::Right) => {
                // Right-click could open context menu in the future
            }
            _ => {}
        }
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) -> bool {
        // Check modal state and handle key events - use separate borrow scopes
        let (has_modal, should_close, handled) = {
            let mut modal_ref = self.open_task_modal.borrow_mut();
            if let Some(ref mut modal) = *modal_ref {
                let handled = modal.handle_key_event(key_event);
                (true, modal.should_close(), handled)
            } else {
                (false, false, false)
            }
        };
        
        if should_close {
            self.close_modal();
            return true;
        }
        
        if has_modal {
            return true; // Consume all keys when modal is open
        }
        
        // If in edit mode, handle edit-specific keys
        if self.is_editing() {
            match key_event.code {
                KeyCode::Up | KeyCode::Char('k') => self.edit_select_prev(),
                KeyCode::Down | KeyCode::Char('j') => self.edit_select_next(),
                KeyCode::Enter => {
                    if let Some((row, field, _value)) = self.confirm_edit() {
                        // Trigger async update for the task
                        self.update_task_field(row, &field);
                    }
                }
                KeyCode::Esc | KeyCode::Char('q') => self.cancel_edit(),
                _ => {}
            }
            return true;
        }
        
        match key_event.code {
            KeyCode::Up | KeyCode::Char('k') => self.previous_row(),
            KeyCode::Down | KeyCode::Char('j') => self.next_row(),
            KeyCode::Left | KeyCode::Char('h') => self.previous_column(),
            KeyCode::Right | KeyCode::Char('l') => self.next_column(),
            KeyCode::Enter => {
                // Open modal for current task if Task Name column selected, otherwise edit
                let state = self.state.borrow();
                if let (Some(row), Some(col)) = (state.selected(), state.selected_column()) {
                    drop(state);
                    if col == 2 {
                        // Task Name column - open modal
                        self.open_modal(row);
                    } else if matches!(col, 1 | 3 | 4) {
                        // Editable columns
                        self.toggle_edit(row, col);
                    }
                }
            }
            KeyCode::Char(' ') => {
                // Space for inline edit on editable columns
                let state = self.state.borrow();
                if let (Some(row), Some(col)) = (state.selected(), state.selected_column()) {
                    drop(state);
                    if matches!(col, 1 | 3 | 4) {
                        self.toggle_edit(row, col);
                    }
                }
            }
            KeyCode::Char('c') => {
                // Toggle completed
                let state = self.state.borrow();
                if let Some(row) = state.selected() {
                    drop(state);
                    if let Some(task) = self.items.get_mut(row) {
                        task.completed = !task.completed;
                        self.update_task_field(row, "completed");
                    }
                }
            }
            KeyCode::Char('d') => {
                // Sort by due date
                self.toggle_sort(SortColumn::DueDate);
            }
            KeyCode::Char('s') => {
                // Sort by status
                self.toggle_sort(SortColumn::Status);
            }
            KeyCode::Char('p') => {
                // Sort by priority
                self.toggle_sort(SortColumn::Priority);
            }
            _ => {}
        }
        true
    }
}

impl<'a> TasksTab<'a> {
    /// Draw the filter trigger ("My Tasks ▾") above the table.
    fn draw_filter_bar(&self, f: &mut Frame, area: Rect) {
        let cols = Layout::horizontal([Constraint::Length(22), Constraint::Fill(1)]).split(area);
        self.filter_trigger.render_ref(cols[0], f.buffer_mut());
    }

    /// Draw the main table
    fn draw_table_impl(&self, f: &mut Frame, area: Rect) {
        // Store the table area for mouse hit-testing
        *self.table_area.borrow_mut() = area;
        
        let mut total_height = 3; // Start with header height
        let widths = if self.widths.len() >= 5 { &self.widths[..5] } else { &DEFAULT_WIDTHS[..] };
        
        // Calculate remaining width for description column
        let fixed_cols_width: u16 = widths.iter().sum::<u16>() + (widths.len() as u16 * 1); // +1 for spacing
        let description_width = area.width.saturating_sub(fixed_cols_width + 4).max(MIN_DESCRIPTION_WIDTH); // -4 for borders
        
        // Get current sort state for header display
        let sort_col = *self.sort_column.borrow();
        let sort_dir = *self.sort_direction.borrow();
        let hovered_header = *self.hovered_header_col.borrow();
        
        // Build header with sort indicators
        let header = Row::new(vec![
            Cell::from(Text::from(Self::header_with_sort("Due", SortColumn::DueDate, sort_col, sort_dir, hovered_header == Some(0), widths[0] as usize))),
            Cell::from(Text::from(Self::header_with_sort("Status", SortColumn::Status, sort_col, sort_dir, hovered_header == Some(1), widths[1] as usize))),
            Cell::from(Text::from(Self::center_text_with_borders("Task".to_string(), widths[2] as usize, 3))),
            Cell::from(Text::from(Self::center_text_with_borders("Assignee".to_string(), widths[3] as usize, 3))),
            Cell::from(Text::from(Self::header_with_sort("Priority", SortColumn::Priority, sort_col, sort_dir, hovered_header == Some(4), widths[4] as usize))),
            Cell::from(Text::from(Self::center_text_with_borders("Description".to_string(), description_width as usize, 3))),
        ])
        .style(Style::default().fg(THEME.accent).bg(APP_BACKGROUND).add_modifier(Modifier::BOLD))
        .height(3)
        .bottom_margin(1);

        let hovered_row = *self.hovered_row.borrow();
        let mut row_heights: Vec<u16> = Vec::with_capacity(self.items.len());

        let rows: Vec<Row> = self.items.iter().enumerate().map(|(i, task)| {
            // Get username from assignee RecordId
            let assignee_name = self.get_username(&task.assignee);

            let wrapped_desc = Self::wrap_text_with_borders(task.task_description.clone(), description_width as usize);
            let height = wrapped_desc.len().max(3) as u16;
            row_heights.push(height);
            total_height += height;
            
            // Color code based on status
            let status_color = match task.status.as_str() {
                "Todo" => CATPPUCCIN.yellow,
                "In Repair" => CATPPUCCIN.blue,
                "Complete" => CATPPUCCIN.green,
                "QC" => CATPPUCCIN.mauve,
                "Sales" => CATPPUCCIN.peach,
                _ => CATPPUCCIN.text,
            };
            
            // Color code based on priority
            let priority_color = match task.priority.as_str() {
                "Express" => CATPPUCCIN.red,
                "RFS" => CATPPUCCIN.peach,
                "Fire" => CATPPUCCIN.maroon,
                "QC" => CATPPUCCIN.mauve,
                _ => CATPPUCCIN.text,
            };
            
            // Determine row background based on hover and alternating
            let is_hovered = hovered_row == Some(i);
            let bg_color = if is_hovered {
                THEME.surface
            } else if i % 2 == 0 {
                CATPPUCCIN.base
            } else {
                APP_BACKGROUND
            };
            
            let fg_color = if is_hovered {
                CATPPUCCIN.text
            } else if i % 2 == 0 {
                CATPPUCCIN.subtext0
            } else {
                CATPPUCCIN.text
            };
            
            // Task name is clickable — always accent-colored, underlined on hover.
            let task_name_style = if is_hovered {
                Style::default().fg(THEME.accent).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
            } else {
                Style::default().fg(THEME.accent)
            };
            
            Row::new(vec![
                Cell::from(Text::from(Self::center_text_with_borders(
                    task.due_date.format("%m/%d/%y").to_string(), 
                    widths[0] as usize, 
                    height
                ))),
                Cell::from(Text::from(Self::center_text_with_borders(
                    task.status.as_str().to_string(), 
                    widths[1] as usize, 
                    height
                ))).style(Style::default().fg(status_color)),
                Cell::from(Text::from(Self::center_text_with_borders(
                    task.task_name.clone(), 
                    widths[2] as usize, 
                    height
                ))).style(task_name_style),
                Cell::from(Text::from(Self::center_text_with_borders(
                    assignee_name, 
                    widths[3] as usize, 
                    height
                ))),
                Cell::from(Text::from(Self::center_text_with_borders(
                    task.priority.as_str().to_string(), 
                    widths[4] as usize, 
                    height
                ))).style(Style::default().fg(priority_color)),
                Cell::from(Text::from(wrapped_desc)),
            ])
            .style(Style::default().fg(fg_color).bg(bg_color))
            .height(height)
        }).collect();

        // Use Fill for description column to take remaining space
        let mut constraints: Vec<_> = widths.iter().map(|&w| Constraint::Length(w)).collect();
        constraints.push(Constraint::Fill(1)); // Description fills remaining
        
        let mut table_state = self.state.borrow_mut();
        if table_state.selected().is_none() && !self.items.is_empty() {
            table_state.select(Some(0));
        }
        if table_state.selected_column().is_none() {
            table_state.select_column(Some(0));
        }
        if table_state.selected_cell().is_none() {
            if let (Some(row), Some(col)) = (table_state.selected(), table_state.selected_column()) {
                table_state.select_cell(Some((row, col)));
            }
        }

        let table = Table::new(rows.to_vec(), constraints)
            .header(header)
            .column_spacing(1)
            .block(
                Block::default()
                .title(" My Tasks ")
                .title_style(THEME.title())
                .border_type(BorderType::Rounded)
                .borders(Borders::ALL)
                .style(Style::default().fg(THEME.tertiary))
                .title_alignment(ratatui::layout::Alignment::Center)
            )
            .column_highlight_style(Style::default().bg(Color::Rgb(20, 20, 28)))
            .cell_highlight_style(Style::default().add_modifier(Modifier::BOLD).bg(Color::Rgb(34, 34, 46)).fg(THEME.accent))
            .row_highlight_style(Style::default().bg(Color::Rgb(20, 20, 28)));

        f.render_stateful_widget(table, area, &mut table_state);

        // Record each visible row's screen rect (with its item index) so mouse
        // hit-testing handles variable row heights and the scroll offset.
        {
            let header_h: u16 = 4;
            let offset = table_state.offset();
            let mut y = area.y + header_h;
            let mut areas: Vec<(usize, Rect)> = Vec::new();
            for (i, &h) in row_heights.iter().enumerate().skip(offset) {
                if y >= area.bottom() {
                    break;
                }
                let vis_h = h.min(area.bottom() - y);
                areas.push((i, Rect { x: area.x, y, width: area.width, height: vis_h }));
                y += h;
            }
            *self.row_areas.borrow_mut() = areas;
        }

        // Apply animated border effect to the table
        {
            let mut effect_stage = self.effect_stage.borrow_mut();
            unique_border_effect(&mut effect_stage, "TasksTableBorder", THEME.accent, area);
            effect_stage.process_effects(tachyonfx::Duration::from_millis(16), f.buffer_mut(), area);
        }

        // Vertical Scrollbar
        if total_height > area.height {
            let mut v_scrollbar_state = ScrollbarState::new(total_height as usize - area.height as usize);
            v_scrollbar_state = v_scrollbar_state.position(table_state.offset());
            f.render_stateful_widget(
                Scrollbar::new(ScrollbarOrientation::VerticalRight)
                    .begin_symbol(Some("▲"))
                    .track_style(Style::new().fg(THEME.surface))
                    .track_symbol(Some("│"))
                    .thumb_symbol("█")
                    .thumb_style(Style::new().fg(THEME.accent))
                    .end_symbol(Some("▼")),
                area,
                &mut v_scrollbar_state,
            );
        }

        // Horizontal Scrollbar
        let total_width: u16 = widths.iter().sum();
        if total_width > area.width {
            let mut h_scrollbar_state = ScrollbarState::new(total_width as usize - area.width as usize);
            h_scrollbar_state = h_scrollbar_state.position(self.scroll_state.borrow().offset().x as usize);
            f.render_stateful_widget(
                Scrollbar::new(ScrollbarOrientation::HorizontalBottom)
                    .begin_symbol(Some("◀"))
                    .track_style(Style::new().fg(THEME.surface))
                    .track_symbol(Some("─"))
                    .thumb_symbol("█")
                    .thumb_style(Style::new().fg(THEME.accent))
                    .end_symbol(Some("▶")),
                area,
                &mut h_scrollbar_state,
            );
        }
        
        // Draw edit popup if in edit mode
        self.draw_edit_popup(f, area);
    }
    
    /// Calculate widths for first 5 columns (description fills remaining)
    pub fn calculate_widths(tasks: &[LiveTaskPayload], users: &[User]) -> Vec<u16> {
        let headers = ["Due", "Status", "Task", "Assignee", "Priority"];
        let mut widths = DEFAULT_WIDTHS.to_vec();

        for task in tasks {
            widths[2] = max(widths[2], task.task_name.chars().count() as u16);
            
            // Get username for width calculation
            let assignee_name = users
                .iter()
                .find(|u| u.get_id() == task.assignee)
                .map(|u| u.get_username().to_owned())
                .unwrap_or_else(|| task.assignee.key_string());
            widths[3] = max(widths[3], assignee_name.len() as u16);
        }
        
        // Apply min/max constraints (description is dynamic, not in this list)
        widths[0] = max(widths[0], headers[0].len() as u16).min(14);
        widths[1] = max(widths[1], headers[1].len() as u16).min(14);
        widths[2] = max(widths[2], headers[2].len() as u16).min(35);
        widths[3] = max(widths[3], headers[3].len() as u16).min(16);
        widths[4] = max(widths[4], headers[4].len() as u16).min(12);
        widths
    }
    
    /// Build header cell with sort indicator
    fn header_with_sort<'b>(title: &str, col: SortColumn, current_sort: SortColumn, dir: SortDirection, is_hovered: bool, width: usize) -> Vec<Line<'b>> {
        let indicator = if current_sort == col {
            match dir {
                SortDirection::Ascending => " ▲",
                SortDirection::Descending => " ▼",
            }
        } else if is_hovered {
            " ○" // Show indicator on hover for sortable columns
        } else {
            ""
        };
        
        let display_text = format!("{}{}", title, indicator);
        Self::center_text_with_borders(display_text, width, 3)
    }
    
    /// Draw the edit popup when in edit mode
    fn draw_edit_popup(&self, f: &mut Frame, area: Rect) {
        let edit_mode = self.edit_mode.borrow();
        
        match &*edit_mode {
            EditMode::None => {}
            EditMode::Status { row, options, selected_idx } => {
                let popup_width = 20u16;
                let popup_height = (options.len() + 2).min(10) as u16;
                let popup_area = self.calculate_popup_area(area, *row, 1, popup_width, popup_height);
                
                self.render_selection_popup(f, popup_area, "Status", options.iter().map(|s| s.as_str().to_string()).collect(), *selected_idx);
            }
            EditMode::Assignee { row, options, selected_idx } => {
                let popup_width = 25u16;
                let popup_height = (options.len() + 2).min(12) as u16;
                let popup_area = self.calculate_popup_area(area, *row, 3, popup_width, popup_height);
                
                self.render_selection_popup(f, popup_area, "Assignee", options.iter().map(|(_, name)| name.clone()).collect(), *selected_idx);
            }
            EditMode::Priority { row, options, selected_idx } => {
                let popup_width = 18u16;
                let popup_height = (options.len() + 2).min(8) as u16;
                let popup_area = self.calculate_popup_area(area, *row, 4, popup_width, popup_height);
                
                self.render_selection_popup(f, popup_area, "Priority", options.iter().map(|p| p.as_str().to_string()).collect(), *selected_idx);
            }
            EditMode::DueDate { row: _ } => {
                // Calendar widget would go here
                // For now, just show a message
                let popup_area = Rect {
                    x: area.x + area.width / 2 - 15,
                    y: area.y + area.height / 2 - 3,
                    width: 30,
                    height: 5,
                };
                f.render_widget(Clear, popup_area);
                let block = Block::default()
                    .title(" Due Date ")
                    .title_style(THEME.title())
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .style(Style::default().fg(THEME.accent));
                f.render_widget(block.clone(), popup_area);
                let text = Paragraph::new("Date editing coming soon\nPress ESC to close")
                    .style(Style::default().fg(CATPPUCCIN.text))
                    .alignment(ratatui::layout::Alignment::Center);
                f.render_widget(text, block.inner(popup_area));
            }
        }
    }
    
    /// Calculate the popup position near the selected cell
    fn calculate_popup_area(&self, table_area: Rect, row: usize, col: usize, width: u16, height: u16) -> Rect {
        let widths = if self.widths.len() == 6 { &self.widths } else { &DEFAULT_WIDTHS.to_vec() };
        
        // Calculate x position based on column
        let mut x = table_area.x + 1;
        for i in 0..col {
            x += widths.get(i).unwrap_or(&10) + 1;
        }
        
        // Calculate y position based on row (header is ~4 lines, each row ~3 lines)
        let y = table_area.y + 4 + (row as u16 * 3);
        
        // Ensure popup stays within screen bounds
        let x = x.min(table_area.right().saturating_sub(width + 1));
        let y = if y + height > table_area.bottom() {
            y.saturating_sub(height)
        } else {
            y
        };
        
        Rect { x, y, width, height }
    }
    
    /// Render a selection popup with options
    fn render_selection_popup(&self, f: &mut Frame, area: Rect, title: &str, options: Vec<String>, selected_idx: usize) {
        f.render_widget(Clear, area);
        
        let block = Block::default()
            .title(format!(" {} ", title))
            .title_style(THEME.title())
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .style(Style::default().fg(THEME.accent).bg(THEME.surface));
        
        let inner = block.inner(area);
        f.render_widget(block, area);
        
        // Render options
        let visible_height = inner.height as usize;
        let scroll_offset = if selected_idx >= visible_height {
            selected_idx - visible_height + 1
        } else {
            0
        };
        
        for (i, option) in options.iter().enumerate().skip(scroll_offset).take(visible_height) {
            let y = inner.y + (i - scroll_offset) as u16;
            let style = if i == selected_idx {
                Style::default().fg(APP_BACKGROUND).bg(THEME.accent).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(CATPPUCCIN.text)
            };
            
            let line = if i == selected_idx {
                format!(" ▶ {} ", option)
            } else {
                format!("   {} ", option)
            };
            
            let text = Paragraph::new(line).style(style);
            f.render_widget(text, Rect { x: inner.x, y, width: inner.width, height: 1 });
        }
    }
    
    /// Update a task field asynchronously
    fn update_task_field(&self, row: usize, field: &str) {
        if let Some(task) = self.items.get(row) {
            let task = task.clone();
            let field = field.to_string();
            
            tokio::spawn(async move {
                let result = match field.as_str() {
                    "status" => task.update_status(task.status.clone()).await,
                    "assignee" => task.update_assignee(task.assignee.clone()).await,
                    "priority" => task.update_priority(Some(task.priority.clone())).await,
                    "completed" => task.update_completed(task.completed).await,
                    _ => Ok(()),
                };
                
                if let Err(e) = result {
                    log::error!("Failed to update task {}: {:?}", field, e);
                } else {
                    log::info!("Updated task {} successfully", field);
                }
            });
        }
    }

    pub fn next_row(&mut self) {
        let mut state = self.state.borrow_mut();
        state.select_next();
        if let (Some(row), Some(col)) = (state.selected(), state.selected_column()) {
            state.select_cell(Some((row, col)));
        }
    }

    pub fn previous_row(&mut self) {
        let mut state = self.state.borrow_mut();
        state.select_previous();
        if let (Some(row), Some(col)) = (state.selected(), state.selected_column()) {
            state.select_cell(Some((row, col)));
        }
    }

    pub fn next_column(&mut self) {
        let mut state = self.state.borrow_mut();
        state.select_next_column();
        if let (Some(row), Some(col)) = (state.selected(), state.selected_column()) {
            state.select_cell(Some((row, col)));
        }
    }

    pub fn previous_column(&mut self) {
        let mut state = self.state.borrow_mut();
        state.select_previous_column();
        if let (Some(row), Some(col)) = (state.selected(), state.selected_column()) {
            state.select_cell(Some((row, col)));
        }
    }

    pub fn _scroll_down(&mut self) {
        self.state.borrow_mut().scroll_down_by(1);
    }

    pub fn _scroll_up(&mut self) {
        self.state.borrow_mut().scroll_up_by(1);
    }

    pub fn _scroll_right(&mut self) {
        self.state.borrow_mut().scroll_right_by(1);
    }

    pub fn _scroll_left(&mut self) {
        self.state.borrow_mut().scroll_left_by(1);
    }
    
    fn _wrap_text<'b>(text: String, width: usize) -> Vec<Line<'b>> {
        let mut lines = Vec::new();
        let mut current = String::new();
        
        for word in text.split_whitespace() {
            let word_width = word.width(); // Use UnicodeWidthStr for accurate width
            if current.width() + word_width + 1 > width {
                if !current.is_empty() {
                    lines.push(Line::from(vec![Span::from(current.clone())]));
                    current.clear();
                }
                if word_width > width {
                    let mut chars = word.chars();
                    while let Some(ch) = chars.next() {
                        if current.width() >= width {
                            lines.push(Line::from(vec![Span::from(current.clone())]));
                            current.clear();
                        }
                        current.push(ch);
                    }
                } else {
                    current = word.to_string();
                }
            } else {
                if !current.is_empty() {
                    current.push(' ');
                }
                current.push_str(word);
            }
        }
        if !current.is_empty() {
            lines.push(Line::from(vec![Span::from(current)]));
        }
        lines
    }

    fn wrap_text_with_borders<'b>(text: String, width: usize) -> Vec<Line<'b>> {
        let mut lines = Vec::new();
        let inner_width = width.saturating_sub(2);
        let mut current = String::new();

        for word in text.split_whitespace() {
            let word_width = word.width();
            if current.width() + word_width + 1 > inner_width {
                if !current.is_empty() {
                    lines.push(Line::from(vec![Span::raw(format!("│{:width$}│", current, width = inner_width))]));
                    current.clear();
                }
                if word_width > inner_width {
                    let mut chars = word.chars();
                    while let Some(ch) = chars.next() {
                        if current.width() >= inner_width {
                            lines.push(Line::from(vec![Span::raw(format!("│{:width$}│", current, width = inner_width))]));
                            current.clear();
                        }
                        current.push(ch);
                    }
                } else {
                    current = word.to_string();
                }
            } else {
                if !current.is_empty() {
                    current.push(' ');
                }
                current.push_str(word);
            }
        }
        if !current.is_empty() {
            lines.push(Line::from(vec![Span::raw(format!("│{:width$}│", current, width = inner_width))]));
        }
        if !lines.is_empty() {
            lines.insert(0, Line::from(vec![Span::raw(format!("╭{:─<width$}╮", "", width = inner_width))]));
            lines.push(Line::from(vec![Span::raw(format!("╰{:─<width$}╯", "", width = inner_width))]));
        } else {
            lines.push(Line::from(vec![Span::raw(format!("╭{:─<width$}╮", "", width = inner_width))]));
            lines.push(Line::from(vec![Span::raw(format!("│{:width$}│", "", width = inner_width))]));
            lines.push(Line::from(vec![Span::raw(format!("╰{:─<width$}╯", "", width = inner_width))]));
        }
        lines
    }

    fn center_text_with_borders<'b>(text: String, width: usize, height: u16) -> Vec<Line<'b>> {
        let inner_width = width.saturating_sub(2);
        let content_lines = 1;
        let total_lines = height as usize;
        let padding = (total_lines.saturating_sub(content_lines + 2)) / 2;
        let mut lines = Vec::new();

        lines.push(Line::from(vec![Span::raw(format!("╭{:─<width$}╮", "", width = inner_width))]));
        for _ in 0..padding {
            lines.push(Line::from(vec![Span::raw(format!("│{:width$}│", "", width = inner_width))]));
        }
        lines.push(Line::from(vec![Span::raw(format!("│{:^width$}│", text, width = inner_width))]));
        for _ in 0..(total_lines.saturating_sub(content_lines + 2) - padding) {
            lines.push(Line::from(vec![Span::raw(format!("│{:width$}│", "", width = inner_width))]));
        }
        lines.push(Line::from(vec![Span::raw(format!("╰{:─<width$}╯", "", width = inner_width))]));
        lines
    }
}

pub fn _center_horizontal(area: Rect, width: u16) -> Rect {
    let [area] = ratatui::prelude::Layout::horizontal([
            ratatui::prelude::Constraint::Length(width)
        ])
        .flex(ratatui::layout::Flex::Center)
        .areas(area);
    area
}