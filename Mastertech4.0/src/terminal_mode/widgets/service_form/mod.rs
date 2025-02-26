
use crate::terminal_mode::{data::ServiceData, events::action_handler::{ApiEvent, WidgetEvent, WidgetId}, styling::CATPPUCCINTHEME};
use super::{button::{Button, State}, input_field::{InputField, InputFieldId}, ButtonType};
use std::{cell::RefCell, rc::Rc, sync::{Arc, Mutex}};
use crossbeam::channel::Sender;
use database::schema::GetKeysResponse;
use reqwest::Client;

pub mod action_handler;
pub mod render;

// ---------------------------------------------------------------------------
/// ServiceFormWidget: The complete two‑column form.
pub struct ServiceFormWidget<'a> {
    input_idx: RefCell<i32>,
    get_ticket_button: Button<'a>,
    submit_button: Button<'a>,
    order_number: Rc<InputField<'a>>,

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
    pub service_data: Arc<Mutex<ServiceData>>,

    /// A sender for broadcasting widget events.
    event_sender: Sender<WidgetEvent>,
    
    client: Client,

    keys: GetKeysResponse
}

impl<'a> ServiceFormWidget<'a> {
    pub fn new(event_sender: Sender<WidgetEvent>) -> Self {
        let service_data =  Arc::new(Mutex::new(ServiceData::new(event_sender.clone())));

        // Wrap the InputField in an Rc.
        let service_num_field = Rc::new(InputField::new("Service #"));
        let sender_clone = event_sender.clone();
        let sender_clone2 = event_sender.clone();
        let sender_clone3 = event_sender.clone();
        let sender_clone4 = event_sender.clone();
        let get_ticket_button = Button::new("Get Ticket")
            .theme(CATPPUCCINTHEME)
            .on_click(move || {
                // And then send a ButtonClick event to trigger get_ticket().
                let _ = sender_clone.try_send(WidgetEvent::ButtonClick {
                    widget_id: WidgetId::ServiceForm,
                });
            });

        let get_keys_button = Button::new("Get Keys")
            .theme(CATPPUCCINTHEME)
            .on_click(move || {
                // And then send a ButtonClick event to trigger get_ticket().
                let _ = sender_clone2.try_send(WidgetEvent::Api(ApiEvent::GetKeys));
            });

        let check_seb_button = Button::new("Check SEB").theme(CATPPUCCINTHEME);
        let submit_button = Button::new("Submit").theme(CATPPUCCINTHEME);

        let webroot_key_button = Button::new("Webroot Key")
            .theme(CATPPUCCINTHEME)
            .on_click(move || {
                let _ = sender_clone3.try_send(WidgetEvent::CopyWebroot);
            });
        let superanti_key_button = Button::new("SuperAnti Key")
            .theme(CATPPUCCINTHEME)
            .on_click(move || {
                let _ = sender_clone4.try_send(WidgetEvent::CopySuperAnti);
            });

        Self {
            order_number: service_num_field,
            input_idx: RefCell::new(0),
            customer_name: InputField::new("Customer Name"),
            customer_phone: InputField::new("Customer Phone"),
            salesman_name: InputField::new("Salesman Name"),
            technician_name: InputField::new("Technician Name"),
            checkin_notes: InputField::new("CheckIn Notes"),
            recommendations: InputField::new("Recommendations"),
            get_ticket_button,
            submit_button,
            get_keys_button,
            check_seb_button,
            webroot_key_button,
            superanti_key_button,
            service_data,
            event_sender,
            client: Client::new(),
            keys: GetKeysResponse::default(),
            active_field: RefCell::new(None),
            cached_cursor_position: RefCell::new(None),
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



