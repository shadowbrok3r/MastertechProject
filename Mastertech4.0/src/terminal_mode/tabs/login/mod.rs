use crate::terminal_mode::{events::action_handler::WidgetId, styling::CATPPUCCINTHEME, widgets::{button::{Button, ButtonState}, input_field::InputField, ButtonType}};
use std::cell::RefCell;
pub mod action_handler;
pub mod render;


pub struct LoginTab <'a> {
    login_btn: Button<'a>,
    // Row 2: Sales/Tech Names
    pub username_field: InputField<'a>,
    pub password_field: InputField<'a>,

    /// Tracks which input field is currently focused.
    pub active_field: RefCell<Option<WidgetId>>,
    pub input_idx: RefCell<i32>,
}

impl <'a> LoginTab <'a> {
    pub fn new() -> Self {
        
        Self {
            login_btn: Button::new(
                "Login",
                WidgetId("Login".to_owned())
            ).theme(CATPPUCCINTHEME),
            // Row 2: Sales/Tech Names
            username_field: InputField::new("Username", WidgetId("Username".to_string())),
            password_field: InputField::new("Password", WidgetId("Password".to_string())),
            active_field: RefCell::new(None),
            input_idx: RefCell::new(0)
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
            "Username" => self.username_field.set_state(state),
            "Password" => self.password_field.set_state(state),
            _ => {}
        }
    }

    fn get_input_idx(active_field: &WidgetId) -> i32 {
        match active_field.0.as_str() {
            "Username" => 0,
            "Password" => 1,
            _ => 0
        }
    }

    fn get_field_id_from_idx(input_idx: i32) -> WidgetId {
        match input_idx {
            0 => WidgetId("ServiceNumber".to_string()),
            1 => WidgetId("CustomerName".to_string()),
            _ => WidgetId("ServiceNumber".to_string())
        }
    }
}
