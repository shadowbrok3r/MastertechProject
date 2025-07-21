use crate::terminal_mode::{context::TerminalContext, events::action_handler::WidgetId, styling::{CATPPUCCINTHEME, DEEPPINK, MEDIUMSLATEBLUE, SPRINGGREEN}, widgets::{autocomplete_input::AutoCompleteInput, button::{Button, ButtonState}, input_field::InputField, ButtonType}};
use std::{rc::Rc, sync::{Arc, Mutex}, cell::RefCell};
use database::schema::{prestashop_schema::OrderRow, GetKeysResponse};
use ratatui::{layout::Rect, style::Style};
use tui_scrollview::ScrollViewState;
use reqwest::Client;

pub mod action_handler;
pub mod render;

////////////////////////////////
// TUR SHEET TAB with SERVICE NUM INPUT
////////////////////////////////
/// ServiceTab Component
pub struct ServiceFormTab<'a> {
    input_idx: RefCell<i32>,
    get_ticket_btn: Button<'a>,
    submit_btn: Button<'a>,
    order_number: Rc<InputField<'a>>,
    pub seb_fields: Vec<InputField<'a>>,
    // Other display only fields
    pub other_fields: Vec<InputField<'a>>,
    pub order_row_fields: Vec<(InputField<'a>, InputField<'a>)>,

    // Row 1: Customer Info
    pub customer_name: InputField<'a>,
    pub customer_phone: InputField<'a>,

    // Row 2: Sales/Tech Names
    pub salesman_name: AutoCompleteInput<'a>,
    pub technician_name: AutoCompleteInput<'a>,

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
    
    client: Client,

    keys: GetKeysResponse,

    scroll_state: RefCell<ScrollViewState>,
    ctx: Arc<Mutex<TerminalContext>>,
    service_form_area: Rc<RefCell<Option<Rect>>>,
    total_offset: Rc<RefCell<u16>>,
}

impl<'a> ServiceFormTab<'a> {
    pub fn new(client: Client, ctx: Arc<Mutex<TerminalContext>>) -> Self {
        // Wrap the InputField in an Rc.
        let service_num_field = Rc::new(InputField::new("Service #", WidgetId("ServiceNumber".to_string())));

        let other_fields = vec![
            InputField::new("Customer Email", WidgetId("CustomerEmail".to_string())),
            InputField::new("Device Name", WidgetId("DeviceName".to_string())),
            InputField::new("Device Mfg", WidgetId("DeviceMfg".to_string())),
            InputField::new("Device Model", WidgetId("DeviceModel".to_string())),
            InputField::new("Device Serial", WidgetId("DeviceSerial".to_string())),
            InputField::new("Device Password", WidgetId("DevicePassword".to_string())),
            InputField::new("Device Power Supply", WidgetId("DevicePowerSupply".to_string()))
        ];

        let seb_fields = vec![
            InputField::new("Carbonite Device Name", WidgetId("CarboniteDeviceName".to_string())),
            InputField::new("Device ID", WidgetId("CarboniteDeviceId".to_string())),
            InputField::new("Activation Code", WidgetId("ActivationCode".to_string())),
            InputField::new("Recurly Id", WidgetId("RecurlyId".to_string())),
            InputField::new("Usage (Gb)", WidgetId("UsageGb".to_string())),
        ];

        Self {
            scroll_state: RefCell::new(ScrollViewState::default()),
            service_form_area: Rc::new(RefCell::new(None)),
            total_offset: Rc::new(RefCell::new(0)),
            keys: GetKeysResponse::default(),
            active_field: RefCell::new(None),
            input_idx: RefCell::new(0),
            customer_name: InputField::new("Customer Name", WidgetId("CustomerName".to_string())),
            customer_phone: InputField::new("Customer Phone", WidgetId("CustomerPhone".to_string())),
            salesman_name: AutoCompleteInput::new("Salesman Name", WidgetId("SalesmanName".to_string())),
            technician_name: AutoCompleteInput::new("Technician Name", WidgetId("TechnicianName".to_string())),
            checkin_notes: InputField::new("CheckIn Notes", WidgetId("CheckInNotes".to_string())),
            recommendations: InputField::new("Recommendations", WidgetId("Recommendations".to_string())),
            get_ticket_btn: Button::new("Get Ticket",WidgetId("GetTicket".to_string())).theme(MEDIUMSLATEBLUE),
            submit_btn: Button::new("Submit",WidgetId("SubmitTur".to_string())).theme(DEEPPINK),
            get_keys_btn: Button::new("Get Keys",WidgetId("GetKeys".to_string())).theme(CATPPUCCINTHEME),
            check_seb_btn: Button::new("Check SEB",WidgetId("CheckSeb".to_string())).theme(CATPPUCCINTHEME),
            webroot_key_btn: Button::new("Webroot Key",WidgetId("CopyWebroot".to_string())).theme(SPRINGGREEN),
            superanti_key_btn: Button::new("SuperAnti Key",WidgetId("CopySuperAnti".to_string())).theme(DEEPPINK),
            order_row_fields: Vec::new(),
            order_number: service_num_field,
            other_fields,
            seb_fields,
            client,
            ctx,
        }
    }

    // Optional: Method to populate order_rows later
    pub fn set_order_rows(&mut self, order_rows: Vec<OrderRow>) {
        log::info!("SetOrderRows");
        self.order_row_fields = order_rows
            .into_iter()
            .enumerate()
            .map(|(i, row)| {
                log::info!("mapping order_row_fields");
    
                log::info!("creating name_field");
                let name_field = InputField::new("Product Name", WidgetId(format!("ProductName{}", i)));
                {
                    let mut name_field_input = name_field.input.borrow_mut();
                    name_field_input.insert_str(row.product_name);
                    name_field_input.set_cursor_style(Style::default());
                } // name_field_input dropped here
    
                log::info!("creating price_field");
                let price_field = InputField::new("Price", WidgetId(format!("ProductPrice{}", i)));
                {
                    let mut price_field_input = price_field.input.borrow_mut();
                    price_field_input.insert_str(row.product_price);
                    price_field_input.set_cursor_style(Style::default());
                } // price_field_input dropped here
                
                log::info!("returning name and price field");
                
                (name_field.clone(), price_field.clone())
            })
            .collect();
    }
    
    fn set_active_field(&self, input_field: WidgetId) {
        let idx = Self::get_input_idx(&input_field);
        self.active_field.replace(Some(input_field));
        self.input_idx.replace(idx);
    }

    fn set_input_idx(&self, idx: i32) {
        let widget_id = Self::get_field_id_from_idx(idx);
        let new_idx = Self::get_input_idx(&widget_id);
        self.input_idx.replace(new_idx);
        let final_widget_id = Self::get_field_id_from_idx(new_idx); // Recalculate if reset
        log::info!("Widget ID: {final_widget_id:?}");
        self.active_field.replace(Some(final_widget_id));
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
            _ => self.order_number.set_state(state)
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
}
