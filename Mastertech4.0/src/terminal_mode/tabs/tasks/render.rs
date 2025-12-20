use ratatui::{crossterm::event::{KeyCode, KeyEvent, MouseEvent}, layout::{Constraint, Rect}, prelude::Backend, style::{Color, Modifier, Style}, text::{Line, Span, Text}, widgets::{Block, BorderType, Borders, Cell, Row, Scrollbar, ScrollbarOrientation, ScrollbarState, Table}, Frame};
use crate::terminal_mode::{styling::CATPPUCCIN, widgets::HandleWidget};
use database::schema::{LiveTaskPayload, RecordIdExt};
use unicode_width::UnicodeWidthStr;
use std::cmp::max;
use super::TasksTab;
// Static default widths for columns (used when no tasks are available)
const DEFAULT_WIDTHS: [u16; 7] = [10, 8, 35, 10, 10, 60, 80]; // Due, Status, Task,

/// Implement the HandleWidget trait for ServiceFormTab.
/// This allows the composite widget to draw itself and handle events.
impl<'a> HandleWidget <'a> for TasksTab {
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {
        let mut total_height = 3; // Start with header height
        // Use static defaults for header, safe even if no tasks
        let header = Row::new(vec![
            Cell::from(Text::from(Self::center_text_with_borders("Due".to_string(), DEFAULT_WIDTHS[0] as usize, 3))),
            Cell::from(Text::from(Self::center_text_with_borders("Status".to_string(), DEFAULT_WIDTHS[1] as usize, 3))),
            Cell::from(Text::from(Self::center_text_with_borders("Task".to_string(), DEFAULT_WIDTHS[2] as usize, 3))),
            Cell::from(Text::from(Self::center_text_with_borders("Assignee".to_string(), DEFAULT_WIDTHS[3] as usize, 3))),
            Cell::from(Text::from(Self::center_text_with_borders("Priority".to_string(), DEFAULT_WIDTHS[4] as usize, 3))),
            // Cell::from(Text::from(Self::center_text_with_borders("Check-In Notes".to_string(), DEFAULT_WIDTHS[5] as usize, 3))),
            Cell::from(Text::from(Self::center_text_with_borders("Description".to_string(), DEFAULT_WIDTHS[6] as usize, 3))),
        ])
        .style(Style::default().fg(CATPPUCCIN.sapphire).bg(Color::Rgb(12,12,16)).add_modifier(Modifier::BOLD))
        .height(3)
        .bottom_margin(1);

        let rows: Vec<Row> = self.items.iter().enumerate().map(|(i, task)| {
            let widths = if self.widths.len() == 7 { &self.widths } else { &DEFAULT_WIDTHS.to_vec() };
            // let checkin_notes = task.service_ticket.as_ref().map_or("".to_string(), |t| t.checkin_notes.clone());
            // let wrapped_notes = Self::wrap_text_with_borders(checkin_notes, self.widths[5] as usize);
            let wrapped_desc = Self::wrap_text_with_borders(task.task_description.clone(), self.widths[6] as usize);
            let height = wrapped_desc.len().max(3) as u16;// max(wrapped_notes.len(), wrapped_desc.len()).max(3) as u16; // Min 3 for top/content/bottom
            total_height += height;
            
            Row::new(vec![
                Cell::from(Text::from(Self::center_text_with_borders(task.due_date.format("%m/%d/%y").to_string(), widths[0] as usize, height))),
                Cell::from(Text::from(Self::center_text_with_borders(task.status.as_str().to_string(), widths[1] as usize, height))),
                Cell::from(Text::from(Self::center_text_with_borders(task.task_name.clone(), widths[2] as usize, height))),
                Cell::from(Text::from(Self::center_text_with_borders(task.assignee.key_string(), widths[3] as usize, height))),
                Cell::from(Text::from(Self::center_text_with_borders(task.priority.as_str().to_string(), widths[4] as usize, height))),
                // Cell::from(Text::from(wrapped_notes)),
                Cell::from(Text::from(wrapped_desc)),
            ])
            .style(Style::default()
                .fg( if i % 2 == 0 { CATPPUCCIN.subtext0 } else { CATPPUCCIN.text } )
                .bg( if i % 2 == 0 { CATPPUCCIN.base } else { Color::Rgb(14, 14, 18) } )
            )
            .height(height)
        }).collect();

        let constraints: Vec<_> = if self.widths.len() == 7 {
            self.widths.iter().map(|&w| Constraint::Length(w)).collect()
        } else {
            DEFAULT_WIDTHS.iter().map(|&w| Constraint::Length(w)).collect()
        };
        
        let mut table_state = self.state.borrow_mut();
        if table_state.selected().is_none() {
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
            .column_spacing(2)
            .block(
                Block::default()
                .title("My Tasks")
                .border_type(BorderType::Rounded)
                .borders(Borders::ALL)
                .style(Style::default().fg(CATPPUCCIN.lavender))
                .title_alignment(ratatui::layout::Alignment::Center)
            )
            .column_highlight_style(Style::default().bg(Color::Rgb(8,8,12)))
            .cell_highlight_style(Style::default().add_modifier(Modifier::BOLD).bg(Color::Rgb(20,20,28)).fg(CATPPUCCIN.peach))
            .row_highlight_style(Style::default().bg(Color::Rgb(8,8,12)));

        f.render_stateful_widget(table, area, &mut table_state);

        // Vertical Scrollbar
        if total_height > area.height { // Show scrollbar if content exceeds visible area
            let mut v_scrollbar_state = ScrollbarState::new(total_height as usize - area.height as usize);
            v_scrollbar_state = v_scrollbar_state.position(table_state.offset());
            f.render_stateful_widget(
                Scrollbar::new(ScrollbarOrientation::VerticalRight)
                    .begin_symbol(Some("🢁"))
                    .track_style(Style::new().fg(CATPPUCCIN.base))
                    .track_symbol(Some("║║"))
                    .thumb_symbol("⦕⦖")
                    .thumb_style(Style::new().fg(CATPPUCCIN.sky))
                    .end_symbol(Some("🢃")),
                area,
                &mut v_scrollbar_state,
            );
        }

        // Horizontal Scrollbar (optional, for visual feedback)
        let total_width = self.widths.iter().sum::<u16>();
        if total_width > area.width {
            let mut h_scrollbar_state = ScrollbarState::new(total_width as usize - area.width as usize);
            h_scrollbar_state = h_scrollbar_state.position(self.scroll_state.borrow().offset().x as usize );
            f.render_stateful_widget(
                Scrollbar::new(ScrollbarOrientation::HorizontalBottom)
                    .begin_symbol(Some("⟸"))
                    .track_style(Style::new().fg(CATPPUCCIN.base))
                    .track_symbol(Some("⥈"))
                    .thumb_symbol("|⟗|")
                    .thumb_style(Style::new().fg(CATPPUCCIN.sky))
                    .end_symbol(Some("⟹")),
                area,
                &mut h_scrollbar_state,
            );
        }
    }
    
    fn handle_mouse_event(&self, mouse_event: &MouseEvent) {
        match mouse_event.kind {
            ratatui::crossterm::event::MouseEventKind::ScrollDown => self.state.borrow_mut().scroll_down_by(1),
            ratatui::crossterm::event::MouseEventKind::ScrollUp => self.state.borrow_mut().scroll_up_by(1),
            ratatui::crossterm::event::MouseEventKind::ScrollLeft => self.state.borrow_mut().scroll_left_by(1),
            ratatui::crossterm::event::MouseEventKind::ScrollRight => self.state.borrow_mut().scroll_right_by(1),
            _ => {}
        }
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) -> bool {
        match key_event.code {
            KeyCode::Up => self.previous_row(),
            KeyCode::Down => self.next_row(),
            KeyCode::Left => self.previous_column(),
            KeyCode::Right => self.next_column(),
            KeyCode::Enter => {
                let mut state = self.state.borrow_mut();
                // Toggle cell selection: if a cell is selected, clear it; otherwise, select current row/col
                if let Some((col, row)) = state.selected_cell() {
                    if state.selected_cell().is_some() {
                        state.select_cell(None);
                    } else {
                        state.select_cell(Some((row, col)));
                    }
                }
            }
            _ => {}
        }
        true
    }
}


impl TasksTab {
    pub fn calculate_widths(tasks: &[LiveTaskPayload]) -> Vec<u16> {
        let headers = ["Due", "Status", "Task", "Assignee", "Priority", "Check-In Notes", "Description"];
        let mut widths = DEFAULT_WIDTHS.to_vec();

        for task in tasks {
            widths[2] = max(widths[2], task.task_name.chars().count() as u16);
            widths[3] = max(widths[3], task.assignee.key_string().len() as u16);
        }
        widths[0] = max(widths[0], headers[0].len() as u16).min(10);
        widths[1] = max(widths[1], headers[1].len() as u16).min(8);
        widths[2] = max(widths[2], headers[2].len() as u16).min(34);
        widths[3] = max(widths[3], headers[3].len() as u16).min(10);
        widths[4] = max(widths[4], headers[4].len() as u16).min(10);
        widths[5] = max(widths[5], headers[5].len() as u16).min(60);
        widths[6] = max(widths[6], headers[6].len() as u16).min(80);
        widths
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