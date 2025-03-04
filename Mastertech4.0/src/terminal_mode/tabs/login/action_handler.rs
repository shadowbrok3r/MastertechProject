use database::{schema::TaskPayload, DATABASE};

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
                    _ => { let _ = self.active_field.replace(None); },
                }
            }
            WidgetEvent::ButtonClick { widget_id } => {
                match widget_id.0.as_str() {
                    "Submit" => {
                        let username_input = self.username_field.input.borrow();
                        let username = &username_input.lines()[0];
                        let password_input = self.username_field.input.borrow();
                        let password = &password_input.lines()[0];

                        log::info!("Logging in");
                        if let Ok(lock) = self.ctx.lock() {
                            let tx = lock.app_state_tx.clone();
                            let login_result = self.login(Login {
                                username: username.to_string(),
                                password: password.to_string(),
                            }, tx);

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
                                    .await?
                                    .take(0)?;

                                log::info!("Query results: {query_results:?}");

                                Ok::<(), anyhow::Error>(())
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