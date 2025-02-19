use ratatui::{buffer::Buffer, layout::{Constraint, Direction, Layout, Rect}, prelude::Backend, style::{Color, Style, Stylize}, text::Line, widgets::{Block, BorderType, Borders, Widget, WidgetRef}, Frame};
use crate::terminal_mode::{fx::{effect::{selected_category, UniqueEffectId}, EffectStage}, styling::{CATPPUCCIN, CATPPUCCINTHEME}, widgets::SHORTCUT_SET};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use super::{button::{Button, State, Theme}, ButtonType, ShrinkArea};
use tui_textarea::TextArea;
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
        }
    }

    pub fn reset_all_states(&self) {
        let active_field = self.active_field.borrow();
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

    /// Update the focused input field based on a mouse event with local coordinates.
    pub fn check_active_field(&self, mouse_event: &MouseEvent) {
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
    pub fn cursor_position(&self) -> Option<(u16, u16)> {
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
        if let Some((local_x, local_y)) = self.cursor_position() {
            // If you're using a scroll offset or the form is inside a sub-rectangle,
            // you'll add (offset_x, offset_y) to these local coords.
            f.set_cursor_position((local_x, local_y));
        }
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
                    let current_field = Self::get_field_id_from_idx(input_idx);
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
                                let mut order_num_field = &mut self.order_number;
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
        let style = Style::default().fg(CATPPUCCIN.teal);
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
        self.get_ticket_button.on_click(|| {

        });

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
        self.webroot_key_button.render( webroot_key_btn_area, buf);

        let superanti_key_btn_area = row5[1].shrink(4, 0);
        self.superanti_key_button.render( superanti_key_btn_area, buf);

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

// ---------------------------------------------------------------------------
// InputField: A wrapper around tui_input::Input for our form fields.
#[derive(Clone, Debug)]
pub struct InputField <'a> {
    /// The underlying tui‑input state.
    pub input: RefCell<TextArea<'a>>,
    /// Title shown as the field’s label.
    title: &'static str,
    /// Store the last drawn area.
    area: RefCell<Option<Rect>>,
    /// state of input field
    state: RefCell<State>,
    /// duh
    theme: Theme,
    effect_stage: RefCell<EffectStage<UniqueEffectId>>,
    init: RefCell<bool>,
    block: RefCell<Option<Block<'a>>>,
}

impl <'a> InputField <'a>{
    pub fn new(title: &'static str) -> Self {
        let mut text_area = TextArea::default();
        text_area.set_style(Style::default().fg(CATPPUCCIN.text));

        let input = RefCell::new(text_area);

        Self {
            input,
            title,
            block: RefCell::new(None),
            area: RefCell::new(None),
            state: RefCell::new(State::Normal),
            theme: CATPPUCCINTHEME,
            effect_stage: RefCell::new(EffectStage::default()),
            init: RefCell::new(true),
        }
    }

    fn set_cursor(&self) {
        let mut input = self.input.borrow_mut();
        match *self.state.borrow() {
            State::Active => input.set_cursor_style(Style::default().fg(Color::Cyan)),
            _ => input.set_cursor_style(Style::default().hidden())
        }
    }

    fn add_effect(&self, area: Rect) {
        if *self.init.borrow() {
            *self.init.borrow_mut() = false;
            let effect1 = selected_category(Color::LightRed, area);
            self.effect_stage.borrow_mut().add_effect(effect1);
        }
    }

    fn set_block(&self, block: Block<'a>) {
        self.block.replace(Some(block));
    }
}

impl <'a> WidgetRef for InputField <'a> {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        let (background, text_color, shadow, highlight) = self.colors();
        
        self.add_effect(area);
        
        // ----- Process TachyonFX Effects -----
        // Create a tachyonfx Duration (e.g. 16ms per frame for ~60FPS).
        let fx_duration = tachyonfx::Duration::from_millis(16);
        // Process all effects added to our effect_stage. They will update and render onto f's buffer.
        self.effect_stage.borrow_mut().process_effects(fx_duration, buf, *buf.area());

        // buf.set_style(area, Style::default().bg(background).fg(text_color));
        // Save the area for later use.
        self.set_area(area);
        
        // Draw a bordered block with the field’s title.
        let default_block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .title(Line::raw(self.title).fg(text_color))
            .style(Style::default().fg(highlight));

        // Render the input's value in a Paragraph.
        let mut input = self.input.borrow_mut();
        let block = if let Some(block) = self.block.borrow().clone(){
            block
        } 
        else { 
            default_block 
        };

        input.set_block(block);
        input.render(area, buf);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputFieldId {
    CustomerName,
    CustomerPhone,
    SalesmanName,
    TechnicianName,
    CheckInNotes,
    Recommendations,
    ServiceNumber,
}

impl<'a> ButtonType<'a> for InputField <'a> {
    fn on_click(&self, _f: impl FnMut() + 'a) -> Self {
        // You might not need on_click for an input field.
        self.clone()
    }

    fn click(&self) {
        self.set_state(State::Active);
    }

    fn set_state(&self, state: State) {
        // log::info!("set_state => Setting {:?} to {state:?}", self.title);
        self.state.replace(state);
        self.set_cursor();
    }

    fn get_area(&self) -> Option<Rect> {
        *self.area.borrow()
    }
    
    fn set_area(&self, area: Rect) {
        self.area.replace(Some(area));
    }
    
    fn is_active(&self) -> bool {
        if let State::Active = *self.state.borrow() {
            true
        } else {
            false
        }
    }

    /// Helper method to get the right colors based on the current state.
    fn colors(&self) -> (Color, Color, Color, Color) {
        let t = self.theme;
        match *self.state.borrow() {
            State::Normal => (CATPPUCCIN.lavender, t.text, t.shadow, t.highlight),
            State::Selected => (CATPPUCCIN.blue, CATPPUCCIN.text, CATPPUCCIN.sapphire, CATPPUCCIN.red),
            State::Active => (CATPPUCCIN.green, CATPPUCCIN.teal, CATPPUCCIN.red, CATPPUCCIN.blue),
            State::Hovered => (CATPPUCCIN.lavender, CATPPUCCIN.sapphire, CATPPUCCIN.red, CATPPUCCIN.maroon),
        }
    }

    fn handle_mouse_event(&self, mouse_event: &MouseEvent) {
        // If we haven’t assigned an area yet, do nothing
        let Some(area) = *self.area.borrow() else { return; };

        let c = mouse_event.column;
        let r = mouse_event.row;

        let inside = c >= area.x 
            && c < area.x + area.width 
            && r >= area.y 
            && r < area.y + area.height;

        match mouse_event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if inside {
                    // Toggle: if already active, set to Normal; otherwise, set to Active.
                    if self.is_active() {
                        self.set_state(State::Normal);
                    } else {
                        self.set_state(State::Active);
                    }
                } else {
                    self.set_state(State::Normal);
                }
            }
            MouseEventKind::Moved => {
                // Only change state on hover if we're not already active.
                if !self.is_active(){
                    if inside {
                        self.set_state(State::Hovered);
                    } else {
                        self.set_state(State::Normal);
                    }
                }
            }
            _ => {}
        }
    }

}
