use ratatui::{buffer::Buffer, layout::{Constraint, Direction, Layout, Rect}, prelude::Backend, widgets::{Block, Borders, WidgetRef}, Frame};
use crate::terminal_mode::{data::ServiceData, styling::{CATPPUCCIN, CATPPUCCINTHEME}, widgets::SHORTCUT_SET};
use super::{button::{Button, State}, input_field::{InputField, InputFieldId}, ButtonType, ShrinkArea};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent};
use std::cell::RefCell;

// ---------------------------------------------------------------------------
/// ServiceFormWidget: The complete two‑column form.
pub struct ServiceFormWidget<'a> {
    input_idx: RefCell<i32>,
    get_ticket_button: Button<'a>,
    submit_button: Button<'a>,
    order_number: InputField<'a>,

    // Row 1: Customer Info
    pub customer_name: InputField<'a>,
    pub customer_phone: InputField<'a>,

    // Row 2: Sales/Tech Names
    pub salesman_name: InputField<'a>,
    pub technician_name: InputField<'a>,

    // Row 3: Two buttons
    pub get_keys_button: Button<'a>,
    pub check_seb_button: Button<'a>,

    // Row 4: Two buttons
    pub webroot_key_button: Button<'a>,
    pub superanti_key_button: Button<'a>,

    // Row 5: Multiline text fields
    pub checkin_notes: InputField<'a>,
    pub recommendations: InputField<'a>,

    /// Tracks which input field is currently focused.
    pub active_field: RefCell<Option<InputFieldId>>,

    /// The final cursor position after drawing, so the parent can read it
    pub cached_cursor_position: RefCell<Option<(u16, u16)>>,

    /// Service information (this is where all of these fields' values will be stored)
    pub service_data: ServiceData,
}

impl<'a> ServiceFormWidget<'a> {
    pub fn new() -> Self {
        Self {
            input_idx: RefCell::new(0),
            // Single-line inputs for row 1 & 2:
            customer_name: InputField::new("Customer Name"),
            customer_phone: InputField::new("Customer Phone"),
            salesman_name: InputField::new("Salesman Name"),
            technician_name: InputField::new("Technician Name"),
            order_number: InputField::new("Service #"),
            // Buttons:
            get_ticket_button: Button::new("Get Ticket").theme(CATPPUCCINTHEME),
            submit_button: Button::new("Submit").theme(CATPPUCCINTHEME),
            get_keys_button: Button::new("Get Keys").theme(CATPPUCCINTHEME),
            check_seb_button: Button::new("Check SEB").theme(CATPPUCCINTHEME),
            webroot_key_button: Button::new("Webroot Key").theme(CATPPUCCINTHEME),
            superanti_key_button: Button::new("SuperAnti Key").theme(CATPPUCCINTHEME),
            // Multiline inputs:
            checkin_notes: InputField::new("CheckIn Notes"),
            recommendations: InputField::new("Recommendations"),
            active_field: RefCell::new(None),
            cached_cursor_position: RefCell::new(None),
            service_data: ServiceData::default(),
        }
    }

    pub fn _reset_all_states(&self) {
        let _active_field = self.active_field.borrow();
        // Manually reset state for each input field
        
        self.customer_name.set_state(State::Normal);
        self.customer_phone.set_state(State::Normal);
        self.salesman_name.set_state(State::Normal);
        self.technician_name.set_state(State::Normal);
        self.checkin_notes.set_state(State::Normal);
        self.recommendations.set_state(State::Normal);
    }

    fn set_active_field(&self, input_field: InputFieldId) {
        self.active_field.replace(Some(input_field));
        let idx = Self::get_input_idx(input_field);
        self.set_input_idx(idx);
    }

    fn set_input_idx(&self, idx: i32) {
        self.input_idx.replace(idx);
        let idx = *self.input_idx.borrow();
        self.active_field.replace(Some(Self::get_field_id_from_idx(idx)));
    }

    fn set_input_state_from_input_idx(&self, idx: i32, state: State) {
        match Self::get_field_id_from_idx(idx) {
            InputFieldId::ServiceNumber => self.order_number.set_state(state),
            InputFieldId::CustomerName => self.customer_name.set_state(state),
            InputFieldId::CustomerPhone => self.customer_phone.set_state(state),
            InputFieldId::SalesmanName => self.salesman_name.set_state(state),
            InputFieldId::TechnicianName => self.technician_name.set_state(state),
            InputFieldId::CheckInNotes => self.checkin_notes.set_state(state),
            InputFieldId::Recommendations => self.recommendations.set_state(state),
        }
    }

    fn get_input_idx(active_field: InputFieldId) -> i32 {
        match active_field {
            InputFieldId::ServiceNumber => 0,
            InputFieldId::CustomerName => 1,
            InputFieldId::CustomerPhone => 2,
            InputFieldId::SalesmanName => 3,
            InputFieldId::TechnicianName => 4,
            InputFieldId::CheckInNotes => 5,
            InputFieldId::Recommendations => 6,
        }
    }

    fn get_field_id_from_idx(input_idx: i32) -> InputFieldId {
        match input_idx {
            0 => InputFieldId::ServiceNumber,
            1 => InputFieldId::CustomerName,
            2 => InputFieldId::CustomerPhone,
            3 => InputFieldId::SalesmanName,
            4 => InputFieldId::TechnicianName,
            5 => InputFieldId::CheckInNotes,
            6 => InputFieldId::Recommendations,
            _ => InputFieldId::ServiceNumber,
        }
    }

    pub fn check_active_field(&self) {
        // Check each field; if it's active, set it as the active field and return.
        if self.order_number.is_active() {
            self.set_active_field(InputFieldId::ServiceNumber);
            return;
        }
        if self.customer_name.is_active() {
            self.set_active_field(InputFieldId::CustomerName);
            return;
        }
        if self.customer_phone.is_active() {
            self.set_active_field(InputFieldId::CustomerPhone);
            return;
        }
        if self.salesman_name.is_active() {
            self.set_active_field(InputFieldId::SalesmanName);
            return;
        }
        if self.technician_name.is_active() {
            self.set_active_field(InputFieldId::TechnicianName);
            return;
        }
        if self.checkin_notes.is_active() {
            self.set_active_field(InputFieldId::CheckInNotes);
            return;
        }
        if self.recommendations.is_active() {
            self.set_active_field(InputFieldId::Recommendations);
            return;
        }
    
        // If none of the fields are active, reset the active field
        self.active_field.replace(None);
    }
    
    /// The parent can call this after `render_ref()` to retrieve the local
    /// cursor position, then do `frame.set_cursor_position(...)`.
    pub fn _cursor_position(&self) -> Option<(u16, u16)> {
        *self.cached_cursor_position.borrow()
    }

}

/// Implement the HandleWidget trait for ServiceFormWidget.
/// This allows the composite widget to draw itself and handle events.
impl<'a> crate::terminal_mode::widgets::HandleWidget<'a> for ServiceFormWidget<'a> {
    fn draw<B: Backend>(&mut self, f: &mut Frame, area: Rect) {    
        // Suppose we want to put the form in `size`
        self.render_ref(area, f.buffer_mut());
        // Then we see if the form has a cursor, and place it
        // if let Some((local_x, local_y)) = self.cursor_position() {
        //     // If you're using a scroll offset or the form is inside a sub-rectangle,
        //     // you'll add (offset_x, offset_y) to these local coords.
        //     f.set_cursor_position((local_x, local_y));
        // }
        
    } 

    fn handle_mouse_event(&self, mouse_event: &MouseEvent) {
        // Delegate mouse events to buttons using your HandleMouse trait.
        // Since our Button now uses interior mutability for its area,
        // the call to handle_mouse_event (which internally calls self.get_area())
        // will work as expected.
        self.get_keys_button.handle_mouse_event(&mouse_event);
        self.check_seb_button.handle_mouse_event(&mouse_event);
        self.webroot_key_button.handle_mouse_event(&mouse_event);
        self.superanti_key_button.handle_mouse_event(&mouse_event);
        self.get_ticket_button.handle_mouse_event(&mouse_event);
        self.submit_button.handle_mouse_event(&mouse_event);

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
        let input = self.order_number.input.borrow();

        self.get_ticket_button.render_ref(get_ticket_btn_area, buf);

        // -- Submit button
        let submit_btn_area = row1[3].shrink(4, 0);
        self.submit_button.render_ref(submit_btn_area, buf);

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

        // Row 4: spacer
        // Row 5: Webroot Key Button | SuperAnti Key Button
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
        self.get_keys_button.render_ref(get_keys_btn_area, buf);

        // Right button
        let check_seb_btn_area = row4[1].shrink(4, 0);
        self.check_seb_button.render_ref(check_seb_btn_area, buf);

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
        self.webroot_key_button.render_ref( webroot_key_btn_area, buf);

        let superanti_key_btn_area = row5[1].shrink(4, 0);
        self.superanti_key_button.render_ref( superanti_key_btn_area, buf);

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

