use ratatui::{layout::{Constraint, Direction, Layout, Rect}, prelude::Backend, widgets::{Block, Borders, WidgetRef}, Frame};
use crate::terminal_mode::{styling::CATPPUCCIN, widgets::{button::ButtonState, ButtonType, ShrinkArea, SHORTCUT_SET}};
use ratatui::crossterm::event::{KeyCode, KeyEvent, MouseEvent};

use super::LoginTab;

/// Implement the HandleWidget trait for ServiceFormWidget.
/// This allows the composite widget to draw itself and handle events.
impl<'a> crate::terminal_mode::widgets::HandleWidget<'a> for LoginTab<'a> {
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {   
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(CATPPUCCIN.maroon)
            .border_set(SHORTCUT_SET)
            .title(format!("Login"));

        block.render_ref(area, f.buffer_mut());

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),  // row 1
                Constraint::Length(3),  // row 2 
            ])
            .split(block.inner(area));

        // Row 1
        let row1 = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
            ])
            .split(rows[0]);

        let row2 = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
            ])
            .split(rows[1]);

        // let area = frame.area();
        // let background = background(area);
        // let popup = Popup::new("Press any key to exit")
        //     .title("tui-popup demo")
        //     .style(Style::new().white().on_blue());

        frame.render_widget(background, area);
        frame.render_widget(&popup, area);

        self.username_field.render_ref(row1[1], f.buffer_mut());
        self.password_field.render_ref(row1[2], f.buffer_mut());
        self.login_btn.render_ref(
            row2[2].shrink(4, 0), 
            f.buffer_mut()
        );

    } 

    fn handle_mouse_event(&self, mouse_event: &MouseEvent) {
        self.login_btn.handle_mouse_event(&mouse_event);
        self.username_field.handle_mouse_event(&mouse_event);
        self.password_field.handle_mouse_event(&mouse_event);
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) -> bool {
        match key_event.code {
            KeyCode::Tab => {
                let Some(ref active_field) = *self.active_field.borrow() else { return false; };
                let input_idx = Self::get_input_idx(&active_field);
                // let current_field = Self::get_field_id_from_idx(input_idx);
                self.set_input_state_from_input_idx(input_idx, ButtonState::Normal);
                log::info!("active field: {active_field:?} / input_idx: {input_idx:?}");
                self.set_input_idx(input_idx + 1);
                self.set_input_state_from_input_idx(input_idx + 1, ButtonState::Active);
                true
            }
            _ => {
                        // Dispatch key events to the active input field.
                if let Some(ref active) = *self.active_field.borrow() {
                    match active.0.as_str() {
                        "CustomerName" => self.username_field.input.borrow_mut().input(key_event),
                        "CustomerPhone" => self.password_field.input.borrow_mut().input(key_event),
                        _ => {false}
                    }
                } else { false }

            }
        }
    }
}