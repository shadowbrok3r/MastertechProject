use ratatui::{layout::{Constraint, Rect}, prelude::Backend, style::Style, widgets::{Block, Borders, Row, Scrollbar, ScrollbarOrientation, Table}, Frame};
use crate::terminal_mode::{styling::CATPPUCCIN, widgets::HandleWidget};
use ratatui::crossterm::event::MouseEvent;
use database::schema::TaskPayload;
use std::cmp::max;
use super::TasksTab;

impl TasksTab {
    fn calculate_widths(tasks: &[TaskPayload]) -> Vec<u16> {
        let headers = ["Task", "Check-In Notes", "Description", "Due", "Done", "Status", "Priority", "Assignee"];
        let mut widths = headers.iter().map(|h| h.len() as u16).collect::<Vec<_>>();

        for task in tasks {
            widths[0] = max(widths[0], task.task_name.len() as u16);
            widths[1] = max(widths[1], task.service_ticket.as_ref().map_or(0, |t| t.checkin_notes.len()) as u16);
            widths[2] = max(widths[2], task.task_description.len() as u16);
            widths[3] = max(widths[3], task.due_date.len() as u16);
            widths[4] = max(widths[4], "Yes".len() as u16); // "Yes" or "No"
            widths[5] = max(widths[5], format!("{:?}", task.status).len() as u16);
            widths[6] = max(widths[6], format!("{:?}", task.priority).len() as u16);
            widths[7] = max(widths[7], task.everest_initials.len() as u16);
        }
        widths
    }

    pub fn next_row(&mut self) {
        let i = match self.state.selected() {
            Some(i) => if i >= self.items.len() - 1 { 0 } else { i + 1 },
            None => 0,
        };
        self.state.select(Some(i));
        self.scroll_state = self.scroll_state.position(i * 2);
    }

    pub fn previous_row(&mut self) {
        let i = match self.state.selected() {
            Some(i) => if i == 0 { self.items.len() - 1 } else { i - 1 },
            None => 0,
        };
        self.state.select(Some(i));
        self.scroll_state = self.scroll_state.position(i * 2);
    }
}

/// Implement the HandleWidget trait for ServiceFormTab.
/// This allows the composite widget to draw itself and handle events.
impl<'a> HandleWidget <'a> for TasksTab {
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {
        let header = Row::new(vec![
            "Task", "Check-In Notes", "Description", "Due", "Done", "Status", "Priority", "Assignee"
        ])
        .style(Style::default().fg(CATPPUCCIN.text).bg(CATPPUCCIN.base))
        .height(1);

        let rows = self.items.iter().map(|task| {
            let checkin_notes = task.service_ticket.as_ref().map_or("".to_string(), |t| t.checkin_notes.clone());
            Row::new(vec![
                task.task_name.clone(),
                checkin_notes,
                task.task_description.clone(),
                task.due_date.clone(),
                if task.completed { "Yes" } else { "No" }.to_string(),
                format!("{:?}", task.status),
                format!("{:?}", task.priority),
                task.everest_initials.clone(),
            ])
            .style(Style::default().fg(CATPPUCCIN.text))
            .height(2) // Adjust height as needed
        });

        let table = Table::new(rows, self.widths.iter().map(|&w| Constraint::Length(w)))
            .header(header)
            .block(Block::default().borders(Borders::ALL).title("Tasks"))
            .row_highlight_style(Style::default().bg(CATPPUCCIN.lavender))
            .highlight_symbol("> ");

        f.render_stateful_widget(table, area, &mut self.state);

        // Render scrollbar
        f.render_stateful_widget(
            Scrollbar::default()
                .orientation(ScrollbarOrientation::VerticalRight),
            area,
            &mut self.scroll_state,
        );
    } 

    fn handle_mouse_event(&self, mouse_event: &MouseEvent) {

    }

    // fn handle_key_event(&mut self, key_event: KeyEvent) -> bool {

    // }
}

