use std::sync::{Arc, Mutex};

use database::{schema::TaskPayload, DATABASE};

use crate::{pages::login_page::Login, terminal_mode::{context::TerminalContext, events::action_handler::{ActionHandler, WidgetEvent, WidgetId}}};

use super::LoginTab;

impl <'a> ActionHandler for LoginTab <'a> {
    fn handle_event(&mut self, event: &WidgetEvent, ctx: Arc<Mutex<TerminalContext>>) {
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
            WidgetEvent::ButtonClick { widget_id, button} => {
                log::info!("Button: {button:?}");
                let logout_label = self.login_btn.get_label();
                if logout_label == "Logout" {
                    // let mut file = std::fs::File::
                }
                match widget_id.0.as_str() {
                    "Login" => {
                        let mut username_input = self.username_field.input.borrow_mut();
                        let username = username_input.lines()[0].clone();
                        let mut password_input = self.password_field.input.borrow_mut();
                        let password = password_input.lines()[0].clone();
                        username_input.select_all();
                        username_input.cut();
                        password_input.select_all();
                        password_input.cut();

                        log::info!("Logging in");
                        if let Ok(context) = ctx.lock() {
                            let tx = context.app_state_tx.clone();
                            let render_tx = context.render_sender.clone();
                            let login_result = self.login(Login {
                                username: username.to_string(),
                                password: password.to_string(),
                            }, tx, render_tx);

                            log::info!("Login Result: {login_result:?}");
                            tokio::spawn(async move {
                                let query = r#"
                                    SELECT *, (
                                        SELECT * FROM task_note 
                                            WHERE task_id == $parent.id
                                    ) AS task_note 
                                    FROM task 
                                    WHERE $this.assignee.store == $store AND $this.completed IS false
                                    FETCH 
                                        service_ticket, 
                                        service_ticket.computer, 
                                        service_ticket.customer
                                    PARALLEL
                                "#;
                            
                                let query_results: Vec<TaskPayload> = DATABASE
                                    .query(query)
                                    .bind(("store", "RIV"))
                                    .await.unwrap()
                                    .take(0).unwrap();

                                log::info!("Query results: {query_results:?}");

                                // Ok::<(), anyhow::Error>(())
                            });
                        }

                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}