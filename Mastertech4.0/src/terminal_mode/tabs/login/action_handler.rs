use crate::{pages::login_page::Login, terminal_mode::events::action_handler::{ActionHandler, WidgetEvent, WidgetId}};
use super::LoginTab;

impl <'a> ActionHandler for LoginTab <'a> {
    fn handle_event(&mut self, event: &WidgetEvent) {
        match event {
            WidgetEvent::Active { widget_id } => {
                match widget_id.0.as_str() {
                    "Username" => {
                        log::info!("Username field active");
                        self.set_active_field(WidgetId("Username".to_string()));
                    }
                    "Password" => self.set_active_field(WidgetId("Password".to_string())),
                    _ => {},
                }
            }
            WidgetEvent::ButtonClick { widget_id, button: _} => {
                let logout_label = self.login_btn.get_label();
                if logout_label == "Logout" {
                    // let mut file = std::fs::File::
                }
                match widget_id.0.as_str() {
                    "LoginSubmit" => {
                        let mut username_input = self.username_field.input.borrow_mut();
                        let username = username_input.lines()[0].clone();
                        let mut password_input = self.password_field.input.borrow_mut();
                        let password = password_input.lines()[0].clone();
                        
                        if let Ok(context) = self.ctx.lock() {
                            let tx = context.app_state_tx.clone();
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

                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}