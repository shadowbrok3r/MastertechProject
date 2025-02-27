use ratatui::{crossterm::event::MouseEvent, layout::{Constraint, Direction, Layout, Rect}, prelude::Backend, style::{Color, Style}, text::Span, widgets::{Block, Borders, Paragraph, Wrap}, Frame};
use crate::terminal_mode::{styling::TURQUOISE, widgets::{button::Button, ButtonType, HandleWidget}};

////////////////////////////////
// SCRIPTS TAB with Buttons
////////////////////////////////
/// Let's say we have a subcomponent called ScriptsTab
pub struct ScriptsTab<'a> {
    tuneup_btn: Button<'a>,
    qc_btn: Button<'a>,
    updates_btn: Button<'a>,
    
}

impl<'a> ScriptsTab<'a> {
    pub fn new() -> Self {
        Self {
            tuneup_btn: Button::new("Tuneup").theme(TURQUOISE),
            qc_btn: Button::new("QC").theme(TURQUOISE),
            updates_btn: Button::new("Windows Updates").theme(TURQUOISE),
        }
    }
}

/*
    A Reporting System for each of these things
    like the AHS tuneup 

    Checks:
    - Is SEB installed
    - Is CPS installed
    - Are they active?
    - Storage capacity 
    - 
    Common settings:
    - Sleep / Hibernation
    - Disabling proxy
    - Disable Notifications
    - Superantispyware settings
    - Disabling Startup Apps
    - Unpin copilot
    - Align Taskbar to left 
    - 
    Other:
    - Windows updates
    - 

    Plans for UI
    - 
*/

impl<'a> HandleWidget<'_> for ScriptsTab<'_> {
    /// Draw the ScriptsTab with buttons on the left and a log area on the right
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {
        // Split the total area into two halves: left (buttons) and right (logs)
        let main_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(50), // Left: Buttons
                Constraint::Percentage(50), // Right: Logs
            ])
            .split(area);

        let left_half = main_chunks[0]; // Area for buttons
        let right_half = main_chunks[1]; // Area for log output

        // Further split the left area into a 2-column grid for buttons
        let button_grid = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Ratio(1, 3), // First row (Tuneup & QC)
                Constraint::Ratio(1, 3), // Second row (Updates)
                Constraint::Ratio(1, 3), // Empty for spacing
            ])
            .split(left_half);

        let button_row1 = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(button_grid[0]);

        let button_row2 = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(button_grid[1]);

        // Render buttons in the 2-column grid
        f.render_widget(&self.tuneup_btn, button_row1[0]);
        f.render_widget(&self.qc_btn, button_row1[1]);
        f.render_widget(&self.updates_btn, button_row2[0]); // Second row, left column

        // Render a large text area for logs on the right side
        let log_widget = Paragraph::new("Logs")
            .block(Block::default().borders(Borders::ALL).border_type(ratatui::widgets::BorderType::Rounded).title("Logs"))
            .style(Style::default().fg(Color::White))
            .wrap(Wrap { trim: true });

        f.render_widget(log_widget, right_half);
    }

    /// Handle a mouse event, see if it hits our buttons
    fn handle_mouse_event(&self, mouse_event: &MouseEvent) {
        self.tuneup_btn.handle_mouse_event(&mouse_event);
        self.qc_btn.handle_mouse_event(&mouse_event);
        self.updates_btn.handle_mouse_event(&mouse_event);
    }
}
