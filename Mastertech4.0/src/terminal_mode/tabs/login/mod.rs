use crate::{terminal_mode::{context::TerminalContext, events::action_handler::WidgetId, systems::{communication_system::{DataMessage, Message}, notification_system::{Notification, NotificationType}}, styling::ThemeRole, widgets::{button::{Button, ButtonState}, input_field::InputField, ButtonType}}, utilities::save_encrypted_user_data};
use displays::pages::login_page::{Login, HASH};
use std::{cell::RefCell, sync::{Arc, Mutex}};
use database::{schema::User, Database, DATABASE};
use crossbeam::channel::Sender;
use displays::app_state::{AppState, MainPages};
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
            login_btn: Button::new("Login",WidgetId("LoginSubmit".to_owned())).theme(ThemeRole::Accent),
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
        data_tx: crossbeam::channel::Sender<Box<dyn Message>>
    )
        -> anyhow::Result<(), anyhow::Error>
    {

        tokio::spawn(async move {
            let database = Database::new(
                login.username.clone(), 
                login.password.clone(), 
                None
            ).await;

            match database{
                Ok(db) => {
                    if let Some(ref usr) = db.user{
                        save_encrypted_user_data(&Login {
                            username: login.username.clone(),
                            password: login.password.clone(),
                        }, HASH)?;
                        data_tx.send(Box::new(Notification::new(
                            NotificationType::Info, 
                            "Logged in", 
                            &format!("Welcome, {}", &usr.get_username()), 
                            5
                        ))).unwrap();

                        data_tx.send(Box::new(
                            DataMessage(usr.clone())
                        )).unwrap();
                    }else{ 
                        log::info!("no usr"); 
                        let _ = DATABASE.invalidate().await;
                        appstate_tx.try_send(AppState::NoAuth("No User".to_string()))?;
                    }
                    appstate_tx.try_send(AppState::Authenticated(MainPages::Tasks))?;
                },
                Err(e) => {
                    log::error!("Error with db: {e:?}");
                    let check = e.to_string().contains("Already connected");
                    log::info!("db check: {check}");
                    if check { 
                        let user: Option<User> = DATABASE.query("SELECT * FROM user WHERE id == $auth.id")
                            .await?
                            .take(0)?;
                        log::info!("user: {user:?}");
                        if let Some(usr) = user {
                            save_encrypted_user_data(&Login {
                                username: login.username.clone(),
                                password: login.password.clone(),
                            }, HASH)?;

                            let res = data_tx.send(Box::new(
                                DataMessage(usr.clone())
                            ));

                            log::info!("data_tx: {res:?}");
                            let res1 = data_tx.send(Box::new(Notification::new(
                                NotificationType::Info, 
                                "Logged in", 
                                &format!("Welcome, {}", usr.get_username()), 
                                5
                            )));

                            log::info!("data_tx: {res1:?}");
                        }
                        appstate_tx.try_send(AppState::Authenticated(MainPages::Tasks))?; 

                    }
                    else { appstate_tx.try_send(AppState::NoAuth(e.to_string()))?; }
                },
            }

            Ok::<(), anyhow::Error>(())
        });

        Ok(())
    }
}
