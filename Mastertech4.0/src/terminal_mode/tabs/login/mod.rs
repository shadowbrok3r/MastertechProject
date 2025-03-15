use crate::{app_state::{AppState, MainPages}, pages::login_page::{Login, HASH}, terminal_mode::{context::TerminalContext, events::action_handler::WidgetId, styling::CATPPUCCINTHEME, systems::{communication_system::Message, notification_system::{Notification, NotificationType}}, widgets::{button::{Button, ButtonState}, input_field::InputField, ButtonType}}, utilities::crypto::pass_hash::save_encrypted_user_data};
use std::{cell::RefCell, sync::{Arc, Mutex}};
use database::{Database, DATABASE};
use crossbeam::channel::Sender;
use reqwest::Client;
pub mod action_handler;
pub mod render;

pub struct LoginTab <'a> {
    login_btn: Button<'a>,
    pub username_field: InputField<'a>,
    pub password_field: InputField<'a>,
    /// Tracks which input field is currently focused.
    pub active_field: RefCell<Option<WidgetId>>,
    pub input_idx: RefCell<i32>,
    pub _client: Client,
    ctx: Arc<Mutex<TerminalContext>>,
}

impl <'a> LoginTab <'a> {
    pub fn new(_client: Client, ctx: Arc<Mutex<TerminalContext>>) -> Self {
        let password_field = InputField::new("Password", WidgetId("Password".to_string()));
        password_field.input.borrow_mut().set_mask_char('*');
        Self {
            login_btn: Button::new(
                "Login",
                WidgetId("Login".to_owned())
            ).theme(CATPPUCCINTHEME),
            username_field: InputField::new("Username", WidgetId("Username".to_string())),
            password_field,
            active_field: RefCell::new(None),
            input_idx: RefCell::new(0),
            _client,
            ctx,
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
        let widget_id = Self::get_field_id_from_idx(idx);
        self.active_field.replace(Some(widget_id));
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
            0 => WidgetId("Username".to_string()),
            1 => WidgetId("Password".to_string()),
            _ => WidgetId("Username".to_string())
        }
    }

    pub fn login(
        &self,
        login: Login,
        appstate_tx: Sender<AppState>,
        render_tx: crossbeam::channel::Sender<Box<dyn Message>>
    )
        -> anyhow::Result<(), anyhow::Error>
    {
        tokio::spawn(async move {
            save_encrypted_user_data(&login, HASH)?;

            let database = Database::new(
                login.username, 
                login.password, 
                None
            ).await;

            match database{
                Ok(db) => {
                    if let Some(ref usr) = db.user{
                        let _usr = serde_json::to_string(&usr).unwrap();
                    }else{ 
                        log::info!("no usr"); 
                        let _ = DATABASE.invalidate().await;
                        appstate_tx.try_send(AppState::Login)?;
                    }
                    appstate_tx.try_send(AppState::Authenticated(MainPages::Tasks))?;
                },
                Err(e) => {
                    log::error!("Error with db: {e:?}");
                    let check = e.to_string().contains("Already connected");
                    if check { 
                        appstate_tx.try_send(AppState::Authenticated(MainPages::Tasks))?; 
                        render_tx.send(Box::new(Notification::new(
                            NotificationType::Info, 
                            "Logged in", 
                            "Switching to TUR Sheet tab", 
                            10
                        )))?;
                    }
                    else { appstate_tx.try_send(AppState::NoAuth(e.to_string()))?; }
                },
            }

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }
}
