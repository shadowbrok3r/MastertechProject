use ratatui::{crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind}, layout::{Constraint, Rect}, prelude::Backend, style::{Color, Modifier, Style}, text::{Line, Span, Text}, widgets::{Block, BorderType, Borders, Cell, Clear, Paragraph, Row, Scrollbar, ScrollbarOrientation, ScrollbarState, Table}, Frame};
use crate::terminal_mode::{fx::unique_border_effect, styling::CATPPUCCIN, widgets::HandleWidget};
use database::schema::{LiveTaskPayload, RecordIdExt, User};
use unicode_width::UnicodeWidthStr;
use std::cmp::max;
use super::{EditMode, TasksTab};

// Static default widths for columns (used when no tasks are available)
// Due, Status, Task, Assignee, Priority, Description
const DEFAULT_WIDTHS: [u16; 6] = [12, 12, 30, 14, 10, 60];

/// Implement the HandleWidget trait for TasksTab.
/// This allows the composite widget to draw itself and handle events.
impl<'a> HandleWidget <'a> for TasksTab {
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {
        // Store the table area for mouse hit-testing
        *self.table_area.borrow_mut() = area;
        
        let mut total_height = 3; // Start with header height
        let widths = if self.widths.len() == 6 { &self.widths } else { &DEFAULT_WIDTHS.to_vec() };
        
        // Use static defaults for header
        let header = Row::new(vec![
            Cell::from(Text::from(Self::center_text_with_borders("Due".to_string(), widths[0] as usize, 3))),
            Cell::from(Text::from(Self::center_text_with_borders("Status".to_string(), widths[1] as usize, 3))),
            Cell::from(Text::from(Self::center_text_with_borders("Task".to_string(), widths[2] as usize, 3))),
            Cell::from(Text::from(Self::center_text_with_borders("Assignee".to_string(), widths[3] as usize, 3))),
            Cell::from(Text::from(Self::center_text_with_borders("Priority".to_string(), widths[4] as usize, 3))),
            Cell::from(Text::from(Self::center_text_with_borders("Description".to_string(), widths[5] as usize, 3))),
        ])
        .style(Style::default().fg(CATPPUCCIN.sapphire).bg(Color::Rgb(12,12,16)).add_modifier(Modifier::BOLD))
        .height(3)
        .bottom_margin(1);

        let rows: Vec<Row> = self.items.iter().enumerate().map(|(i, task)| {
            // Get username from assignee RecordId
            let assignee_name = self.get_username(&task.assignee);
            
            let wrapped_desc = Self::wrap_text_with_borders(task.task_description.clone(), widths[5] as usize);
            let height = wrapped_desc.len().max(3) as u16;
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
                ))),
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
            .style(Style::default()
                .fg( if i % 2 == 0 { CATPPUCCIN.subtext0 } else { CATPPUCCIN.text } )
                .bg( if i % 2 == 0 { CATPPUCCIN.base } else { Color::Rgb(14, 14, 18) } )
            )
            .height(height)
        }).collect();

        let constraints: Vec<_> = widths.iter().map(|&w| Constraint::Length(w)).collect();
        
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
                .border_type(BorderType::Rounded)
                .borders(Borders::ALL)
                .style(Style::default().fg(CATPPUCCIN.lavender))
                .title_alignment(ratatui::layout::Alignment::Center)
            )
            .column_highlight_style(Style::default().bg(Color::Rgb(25, 25, 35)))
            .cell_highlight_style(Style::default().add_modifier(Modifier::BOLD).bg(CATPPUCCIN.surface0).fg(CATPPUCCIN.peach))
            .row_highlight_style(Style::default().bg(Color::Rgb(20, 20, 30)));

        f.render_stateful_widget(table, area, &mut table_state);
        
        // Apply animated border effect to the table
        {
            let mut effect_stage = self.effect_stage.borrow_mut();
            unique_border_effect(&mut effect_stage, "TasksTableBorder", CATPPUCCIN.lavender, area);
            effect_stage.process_effects(tachyonfx::Duration::from_millis(16), f.buffer_mut(), area);
        }

        // Vertical Scrollbar
        if total_height > area.height {
            let mut v_scrollbar_state = ScrollbarState::new(total_height as usize - area.height as usize);
            v_scrollbar_state = v_scrollbar_state.position(table_state.offset());
            f.render_stateful_widget(
                Scrollbar::new(ScrollbarOrientation::VerticalRight)
                    .begin_symbol(Some("▲"))
                    .track_style(Style::new().fg(CATPPUCCIN.base))
                    .track_symbol(Some("│"))
                    .thumb_symbol("█")
                    .thumb_style(Style::new().fg(CATPPUCCIN.sky))
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
                    .track_style(Style::new().fg(CATPPUCCIN.base))
                    .track_symbol(Some("─"))
                    .thumb_symbol("█")
                    .thumb_style(Style::new().fg(CATPPUCCIN.sky))
                    .end_symbol(Some("▶")),
                area,
                &mut h_scrollbar_state,
            );
        }
        
        // Draw edit popup if in edit mode
        self.draw_edit_popup(f, area);
    }
    
    fn handle_mouse_event(&self, mouse_event: &MouseEvent) {
        let table_area = *self.table_area.borrow();
        let x = mouse_event.column;
        let y = mouse_event.row;
        
        match mouse_event.kind {
            MouseEventKind::ScrollDown => self.state.borrow_mut().scroll_down_by(1),
            MouseEventKind::ScrollUp => self.state.borrow_mut().scroll_up_by(1),
            MouseEventKind::ScrollLeft => self.state.borrow_mut().scroll_left_by(1),
            MouseEventKind::ScrollRight => self.state.borrow_mut().scroll_right_by(1),
            MouseEventKind::Down(MouseButton::Left) => {
                // Check if click is within table area
                if x >= table_area.x && x < table_area.right() 
                   && y >= table_area.y && y < table_area.bottom() 
                {
                    // Calculate which row was clicked (accounting for header and borders)
                    let header_height = 4; // Header row + borders
                    if y >= table_area.y + header_height {
                        let relative_y = (y - table_area.y - header_height) as usize;
                        // Approximate row based on default row height of 3
                        let row_idx = relative_y / 3;
                        
                        if row_idx < self.items.len() {
                            let mut state = self.state.borrow_mut();
                            state.select(Some(row_idx));
                            
                            // Calculate which column was clicked
                            let widths = if self.widths.len() == 6 { &self.widths } else { &DEFAULT_WIDTHS.to_vec() };
                            let mut col_start = table_area.x + 1; // Account for border
                            let mut col_idx = 0;
                            for (i, width) in widths.iter().enumerate() {
                                let col_end = col_start + width + 1; // +1 for column spacing
                                if x >= col_start && x < col_end {
                                    col_idx = i;
                                    break;
                                }
                                col_start = col_end;
                            }
                            
                            state.select_column(Some(col_idx));
                            state.select_cell(Some((row_idx, col_idx)));
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
            KeyCode::Enter | KeyCode::Char(' ') => {
                // Enter edit mode for the current cell if editable
                let state = self.state.borrow();
                if let (Some(row), Some(col)) = (state.selected(), state.selected_column()) {
                    drop(state);
                    // Columns 1 (Status), 3 (Assignee), 4 (Priority) are editable
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
            _ => {}
        }
        true
    }
}


impl TasksTab {
    pub fn calculate_widths(tasks: &[LiveTaskPayload], users: &[User]) -> Vec<u16> {
        let headers = ["Due", "Status", "Task", "Assignee", "Priority", "Description"];
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
        
        // Apply min/max constraints
        widths[0] = max(widths[0], headers[0].len() as u16).min(14);
        widths[1] = max(widths[1], headers[1].len() as u16).min(14);
        widths[2] = max(widths[2], headers[2].len() as u16).min(35);
        widths[3] = max(widths[3], headers[3].len() as u16).min(16);
        widths[4] = max(widths[4], headers[4].len() as u16).min(12);
        widths[5] = max(widths[5], headers[5].len() as u16).min(80);
        widths
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
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .style(Style::default().fg(CATPPUCCIN.yellow));
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
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .style(Style::default().fg(CATPPUCCIN.peach).bg(Color::Rgb(20, 20, 25)));
        
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
                Style::default().fg(CATPPUCCIN.base).bg(CATPPUCCIN.peach).add_modifier(Modifier::BOLD)
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
    
    fn _wrap_text<'a>(text: String, width: usize) -> Vec<Line<'a>> {
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

    fn wrap_text_with_borders<'a>(text: String, width: usize) -> Vec<Line<'a>> {
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

    fn center_text_with_borders<'a>(text: String, width: usize, height: u16) -> Vec<Line<'a>> {
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