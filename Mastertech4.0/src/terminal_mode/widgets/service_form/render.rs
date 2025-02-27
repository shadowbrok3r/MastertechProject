use ratatui::{buffer::Buffer, layout::{Constraint, Direction, Layout, Rect}, prelude::Backend, widgets::{Block, Borders, WidgetRef}, Frame};
use crate::terminal_mode::{styling::CATPPUCCIN, widgets::{button::State, input_field::InputFieldId, ButtonType, ShrinkArea, SHORTCUT_SET}};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent};

use super::ServiceFormWidget;

/// Implement the HandleWidget trait for ServiceFormWidget.
/// This allows the composite widget to draw itself and handle events.
impl<'a> crate::terminal_mode::widgets::HandleWidget<'a> for ServiceFormWidget<'a> {
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {   
        self.render_ref(area, f.buffer_mut());
    } 

    fn handle_mouse_event(&self, mouse_event: &MouseEvent) {
        // Delegate mouse events to buttons using your HandleMouse trait.
        // Since our Button now uses interior mutability for its area,
        // the call to handle_mouse_event (which internally calls self.get_area())
        // will work as expected.
        self.get_keys_btn.handle_mouse_event(&mouse_event);
        self.check_seb_btn.handle_mouse_event(&mouse_event);
        self.webroot_key_btn.handle_mouse_event(&mouse_event);
        self.superanti_key_btn.handle_mouse_event(&mouse_event);
        self.get_ticket_btn.handle_mouse_event(&mouse_event);
        self.submit_btn.handle_mouse_event(&mouse_event);

        self.customer_name.handle_mouse_event(&mouse_event);
        self.customer_phone.handle_mouse_event(&mouse_event);
        self.salesman_name.handle_mouse_event(&mouse_event);
        self.technician_name.handle_mouse_event(&mouse_event);
        self.checkin_notes.handle_mouse_event(&mouse_event);
        self.recommendations.handle_mouse_event(&mouse_event);
        self.order_number.handle_mouse_event(&mouse_event);
    }

    fn handle_key_event(&mut self, key_event: KeyEvent) -> bool {
        let shift_pressed = key_event.modifiers.contains(KeyModifiers::SHIFT);
        if shift_pressed {
            if let KeyCode::Tab = key_event.code {
                log::info!("SHIFT Tab");
                let Some(active_field) = *self.active_field.borrow() else { return false; };
                let input_idx = Self::get_input_idx(active_field);
                if input_idx > 0 {
                    self.set_input_idx(input_idx - 1);
                }
                true
            } else {
                false
            }
        } else {
            match key_event.code {
                KeyCode::Tab => {
                    let Some(active_field) = *self.active_field.borrow() else { return false; };
                    let input_idx = Self::get_input_idx(active_field);
                    // let current_field = Self::get_field_id_from_idx(input_idx);
                    self.set_input_state_from_input_idx(input_idx, State::Normal);
                    log::info!("active field: {active_field:?} / input_idx: {input_idx:?}");
                    self.set_input_idx(input_idx + 1);
                    self.set_input_state_from_input_idx(input_idx + 1, State::Active);
                    true
                }
                KeyCode::Enter if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                    let text_area_input = self.order_number.input.borrow();
                    let user_input = &text_area_input.lines()[0];
                    log::info!("(Enter) 'Get Ticket' with input: {user_input}");
                    true
                }
                _ => {
                            // Dispatch key events to the active input field.
                    if let Some(active) = *self.active_field.borrow() {
                        match active {
                            InputFieldId::CustomerName => self.customer_name.input.borrow_mut().input(key_event),
                            InputFieldId::CustomerPhone => self.customer_phone.input.borrow_mut().input(key_event),
                            InputFieldId::SalesmanName => self.salesman_name.input.borrow_mut().input(key_event),
                            InputFieldId::TechnicianName => self.technician_name.input.borrow_mut().input(key_event),
                            InputFieldId::CheckInNotes => self.checkin_notes.input.borrow_mut().input(key_event),
                            InputFieldId::Recommendations => self.recommendations.input.borrow_mut().input(key_event),
                            InputFieldId::ServiceNumber => {
                                let order_num_field = &mut self.order_number;
                                let mut text_area_input = order_num_field.input.borrow_mut();
                                let input = text_area_input.input(key_event);
                                if input {
                                    if let Err(err) = text_area_input.lines()[0].parse::<i32>() {
                                        order_num_field.set_block(
                                            Block::default()
                                                .borders(Borders::ALL)
                                                .border_style(CATPPUCCIN.maroon)
                                                .border_set(SHORTCUT_SET)
                                                .title(format!("ERROR: {}", err)),
                                        );
                                        false
                                    } else {
                                        order_num_field.set_block(
                                            Block::default()
                                                .border_style(CATPPUCCIN.green)
                                                .borders(Borders::ALL)
                                                .border_set(SHORTCUT_SET)
                                                .title("Service #"),
                                        );
                                        true
                                    }
                                } else { false }
                            },
                        }
                    } else { false }

                }
            }
        }
    }
}

impl<'a> WidgetRef for ServiceFormWidget<'a> {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        // Reset cursor position before drawing
        *self.cached_cursor_position.borrow_mut() = None;
        // let style = Style::default().fg(CATPPUCCIN.teal);
        // For brevity, your real code might define constraints differently
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),  // row 1: Service Number field / Get Ticket / submit button
                Constraint::Length(3),  // row 2: Customer info
                Constraint::Length(3),  // row 3: Sales/Tech
                Constraint::Length(3),  // row 4: first row of buttons
                Constraint::Length(1),  // row 5: spacer
                Constraint::Length(3),  // row 6: second row of buttons
                Constraint::Max(10),    // row 7: multiline text fields
            ])
            .split(area);

        // Row 1: Customer Name | Customer Phone
        let row1 = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
            ])
            .split(rows[0]);

        self.order_number.render_ref(row1[0], buf);

        let get_ticket_btn_area = row1[1].shrink(4, 0);

        self.get_ticket_btn.render_ref(get_ticket_btn_area, buf);

        // -- Submit button
        let submit_btn_area = row1[3].shrink(4, 0);
        self.submit_btn.render_ref(submit_btn_area, buf);

        // Row 2: Salesman Name | Technician Name
        let row2 = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
            ])
            .split(rows[1]);

        // -- Customer Name
        self.customer_name.render_ref(row2[0], buf);

        // -- Customer Phone
        self.customer_phone.render_ref(row2[1], buf);

        // Row 3: Get Keys Button | Check SEB Button
        let row3 = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
            ])
            .split(rows[2]);

        // -- Salesman
        self.salesman_name.render_ref(row3[0], buf);

        // -- Technician
        self.technician_name.render_ref(row3[1], buf);

        let row4 = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
            ])
            .split(rows[3]);

        // Left button
        let get_keys_btn_area = row4[0].shrink(4, 0);
        self.get_keys_btn.render_ref(get_keys_btn_area, buf);

        // Right button
        let check_seb_btn_area = row4[1].shrink(4, 0);
        self.check_seb_btn.render_ref(check_seb_btn_area, buf);

        // Row 6: CheckIn Notes | Recommendations
        let row5 = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
            ])
            .split(rows[5]);

        let webroot_key_btn_area = row5[0].shrink(4, 0);
        self.webroot_key_btn.render_ref( webroot_key_btn_area, buf);

        let superanti_key_btn_area = row5[1].shrink(4, 0);
        self.superanti_key_btn.render_ref( superanti_key_btn_area, buf);

        // Row 7: CheckIn Notes | Recommendations
        let row6 = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
            ])
            .split(rows[6]);

        // -- CheckIn Notes
        self.checkin_notes.render_ref(row6[0], buf);

        // -- Recommendations
        self.recommendations.render_ref(row6[1], buf);
    }
}
