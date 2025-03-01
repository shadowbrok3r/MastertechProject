
use crate::terminal_mode::{data::ServiceData, events::action_handler::WidgetId, styling::{CATPPUCCINTHEME, DEEPPINK, MEDIUMSLATEBLUE, SPRINGGREEN}};
use super::{button::{Button, ButtonState}, input_field::InputField, ButtonType};
use std::{cell::RefCell, rc::Rc, sync::{Arc, Mutex}};
use database::schema::GetKeysResponse;
use reqwest::Client;

pub mod action_handler;
pub mod render;

// ---------------------------------------------------------------------------
/// ServiceFormWidget: The complete two‑column form.
pub struct ServiceFormWidget<'a> {
    input_idx: RefCell<i32>,
    get_ticket_btn: Button<'a>,
    submit_btn: Button<'a>,
    order_number: Rc<InputField<'a>>,

    // Row 1: Customer Info
    pub customer_name: InputField<'a>,
    pub customer_phone: InputField<'a>,

    // Row 2: Sales/Tech Names
    pub salesman_name: InputField<'a>,
    pub technician_name: InputField<'a>,

    // Row 3: Two buttons
    pub get_keys_btn: Button<'a>,
    pub check_seb_btn: Button<'a>,

    // Row 4: Two buttons
    pub webroot_key_btn: Button<'a>,
    pub superanti_key_btn: Button<'a>,

    // Row 5: Multiline text fields
    pub checkin_notes: InputField<'a>,
    pub recommendations: InputField<'a>,

    /// Tracks which input field is currently focused.
    pub active_field: RefCell<Option<WidgetId>>,

    /// The final cursor position after drawing, so the parent can read it
    pub cached_cursor_position: RefCell<Option<(u16, u16)>>,

    /// Service information (this is where all of these fields' values will be stored)
    pub service_data: Arc<Mutex<ServiceData>>,
    
    client: Client,

    keys: GetKeysResponse
}

impl<'a> ServiceFormWidget<'a> {
    pub fn new() -> Self {
        let service_data =  Arc::new(Mutex::new(ServiceData::new()));
        // Wrap the InputField in an Rc.
        let service_num_field = Rc::new(InputField::new("Service #", WidgetId("ServiceNumber".to_string())));
        // .on_click(move || {tx.try_send(WidgetEvent::ButtonClick { widget_id: WidgetId::GetTicket});});

        Self {
            order_number: service_num_field,
            input_idx: RefCell::new(0),
            customer_name: InputField::new("Customer Name", WidgetId("CustomerName".to_string())),
            customer_phone: InputField::new("Customer Phone", WidgetId("CustomerPhone".to_string())),
            salesman_name: InputField::new("Salesman Name", WidgetId("SalesmanName".to_string())),
            technician_name: InputField::new("Technician Name", WidgetId("TechnicianName".to_string())),
            checkin_notes: InputField::new("CheckIn Notes", WidgetId("CheckInNotes".to_string())),
            recommendations: InputField::new("Recommendations", WidgetId("Recommendations".to_string())),
            get_ticket_btn: Button::new(
                    "Get Ticket",
                    WidgetId("GetTicket".to_owned())
                )
                .theme(MEDIUMSLATEBLUE),
            submit_btn: Button::new(
                    "Submit",
                    WidgetId("SubmitTur".to_owned())
                )
                .theme(DEEPPINK),
            get_keys_btn: Button::new(
                    "Get Keys",
                    WidgetId("GetKeys".to_owned())
                )
                .theme(CATPPUCCINTHEME),
            check_seb_btn: Button::new(
                    "Check SEB",
                    WidgetId("CheckSeb".to_owned())
                )
                .theme(CATPPUCCINTHEME),
            webroot_key_btn: Button::new(
                    "Webroot Key",
                    WidgetId("CopyWebroot".to_owned())
                )
                .theme(SPRINGGREEN),
            superanti_key_btn: Button::new(
                    "SuperAnti Key",
                    WidgetId("CopySuperAnti".to_owned())
                )
                .theme(DEEPPINK),
            service_data,
            client: Client::new(),
            keys: GetKeysResponse::default(),
            active_field: RefCell::new(None),
            cached_cursor_position: RefCell::new(None),
        }
    }

    fn set_active_field(&self, input_field: WidgetId) {
        let idx = Self::get_input_idx(&input_field);
        self.active_field.replace(Some(input_field));
        self.set_input_idx(idx);
    }

    fn set_input_idx(&self, idx: i32) {
        self.input_idx.replace(idx);
        let idx = *self.input_idx.borrow();
        self.active_field.replace(Some(Self::get_field_id_from_idx(idx)));
    }

    fn set_input_state_from_input_idx(&self, idx: i32, state: ButtonState) {
        match Self::get_field_id_from_idx(idx).0.as_str() {
            "ServiceNumber" => self.order_number.set_state(state),
            "CustomerName" => self.customer_name.set_state(state),
            "CustomerPhone" => self.customer_phone.set_state(state),
            "SalesmanName" => self.salesman_name.set_state(state),
            "TechnicianName" => self.technician_name.set_state(state),
            "CheckInNotes" => self.checkin_notes.set_state(state),
            "Recommendations" => self.recommendations.set_state(state),
            _ => {}
        }
    }

    fn get_input_idx(active_field: &WidgetId) -> i32 {
        match active_field.0.as_str() {
            "ServiceNumber" => 0,
            "CustomerName" => 1,
            "CustomerPhone" => 2,
            "SalesmanName" => 3,
            "TechnicianName" => 4,
            "CheckInNotes" => 5,
            "Recommendations" => 6,
            _ => 0
        }
    }

    fn get_field_id_from_idx(input_idx: i32) -> WidgetId {
        match input_idx {
            0 => WidgetId("ServiceNumber".to_string()),
            1 => WidgetId("CustomerName".to_string()),
            2 => WidgetId("CustomerPhone".to_string()),
            3 => WidgetId("SalesmanName".to_string()),
            4 => WidgetId("TechnicianName".to_string()),
            5 => WidgetId("CheckInNotes".to_string()),
            6 => WidgetId("Recommendations".to_string()),
            _ => WidgetId("ServiceNumber".to_string()),
        }
    }

    /// The parent can call this after `render_ref()` to retrieve the local
    /// cursor position, then do `frame.set_cursor_position(...)`.
    pub fn _cursor_position(&self) -> Option<(u16, u16)> {
        *self.cached_cursor_position.borrow()
    }

}



