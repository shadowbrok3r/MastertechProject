use ratatui::{crossterm::event::MouseEvent, layout::{Constraint, Direction, Layout, Rect}, prelude::Backend, Frame};
use crate::terminal_mode::{styling::TURQUOISE, widgets::{button::Button, ButtonType, HandleWidget}};

////////////////////////////////
// SCRIPTS TAB with Buttons
////////////////////////////////
/// Let's say we have a subcomponent called ScriptsTab
pub struct ScriptsTab<'a> {
    tuneup_button: Button<'a>,
    qc_button: Button<'a>,
}

impl<'a> ScriptsTab<'a> {
    pub fn new() -> Self {
        Self {
            tuneup_button: Button::new("Tuneup")
                .theme(TURQUOISE)
                .on_click(|| {
                    log::info!("Tuneup clicked!");
                }),
            qc_button: Button::new("QC")
                .theme(TURQUOISE)
                .on_click(|| {
                    log::info!("QC clicked!");
                }),
        }
    }
}

impl <'a> HandleWidget <'_> for ScriptsTab <'_> {
    /// Draw the entire ScriptsTab, including its buttons
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {

        // Possibly do a Layout to figure out where each button should go
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(50),
                Constraint::Percentage(50),
            ])
            .split(area);

        self.tuneup_button.set_area(chunks[0]);
        self.qc_button.set_area(chunks[1]);
        
        f.render_widget(&self.tuneup_button, chunks[0]);
        f.render_widget(&self.qc_button, chunks[1]);
    }

    /// Handle a mouse event, see if it hits our tuneup_button or qc_button
    fn handle_mouse_event(&mut self, mouse_event: MouseEvent) {
        self.tuneup_button.handle_mouse_event(&mouse_event);
        self.qc_button.handle_mouse_event(&mouse_event);
    }
}
