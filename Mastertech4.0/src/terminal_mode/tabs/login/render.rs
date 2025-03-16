use ratatui::{layout::{Constraint, Direction, Layout, Rect}, prelude::Backend, widgets::{Block, Borders, WidgetRef}, Frame};
use crate::{pages::login_page::Login, terminal_mode::{styling::CATPPUCCIN, widgets::{button::ButtonState, ButtonType, ShrinkArea, SHORTCUT_SET}}};
use ratatui::crossterm::event::{KeyCode, KeyEvent, MouseEvent};
use super::LoginTab;

/// Implement the HandleWidget trait for ServiceFormTab.
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

        self.username_field.render_ref(row1[1], f.buffer_mut());
        self.password_field.render_ref(row1[2], f.buffer_mut());
        self.login_btn.render_ref(row2[2].shrink(4, 0), f.buffer_mut());
    } 

    fn handle_mouse_event(&self, mouse_event: &MouseEvent) {
        self.login_btn.handle_mouse_event(&mouse_event);
        self.username_field.handle_mouse_event(&mouse_event);
        self.password_field.handle_mouse_event(&mouse_event);
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) -> bool {
        match key_event.code {
            KeyCode::Tab => {
                let input_idx = self.input_idx.borrow(); // Get current index
                let current_idx = *input_idx;
                drop(input_idx); // Drop the borrow before mutating

                // Set current field to normal state
                self.set_input_state_from_input_idx(current_idx, ButtonState::Normal);

                // Move to next field
                let next_idx = (current_idx + 1) % 2; // Cycle between 0 and 1
                self.set_input_idx(next_idx);
                self.set_input_state_from_input_idx(next_idx, ButtonState::Active);
                true
            }
            _ => {
                match key_event.code {
                    KeyCode::Enter => {
                        if let Some(ref active) = *self.active_field.borrow() {
                            match active.0.as_str() {
                                "Password" => {
                                    let mut username_input = self.username_field.input.borrow_mut();
                                    let username = username_input.lines()[0].clone();
                                    let mut password_input = self.password_field.input.borrow_mut();
                                    let password = password_input.lines()[0].clone();
                                    
                                    if let Ok(context) = self.ctx.lock() {
                                        let tx = context.app_state_tx.clone();
                                        // let render_tx = context.render_sender.clone();
                                        let data_tx = context.data_sender.clone();
            
                                        let _ = self.login(
                                            Login {
                                                username: username.to_string(),
                                                password: password.to_string(),
                                            }, 
                                            tx, 
                                            data_tx
                                        );
                                        username_input.select_all();
                                        username_input.cut();
                                        password_input.select_all();
                                        password_input.cut();
                                    }
                                },
                                _ => {}
                            }
                            return true;
                        }
                        false
                    }
                    _ => {
                        // Dispatch key events to the active input field.
                        if let Some(ref active) = *self.active_field.borrow() {
                            match active.0.as_str() {
                                "Username" => self.username_field.input.borrow_mut().input_without_shortcuts(key_event),
                                "Password" => self.password_field.input.borrow_mut().input_without_shortcuts(key_event),
                                _ => {false}
                            }
                        } else { false }
                    }
                }
            }
        }
    }
}