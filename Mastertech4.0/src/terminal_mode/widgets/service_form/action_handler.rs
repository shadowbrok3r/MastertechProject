use std::{collections::HashMap, sync::{Arc, Mutex}};

use crate::{
    tabs::tur_sheet::get_ticket::SendRequest, 
    terminal_mode::{context::TerminalContext, events::action_handler::{ActionHandler, ApiEvent, WidgetEvent}}
};

use database::schema::{
    utilities::PhoneNumberFormatter, GetKeysResponse
};
use reqwest::header::CONTENT_TYPE;

use super::ServiceFormWidget;

impl <'a> ActionHandler for ServiceFormWidget <'a> {
    fn handle_event(&mut self, event: &WidgetEvent, ctx: Arc<Mutex<TerminalContext>>) {
        match event {
            WidgetEvent::ButtonClick { widget_id, button } => {
                log::info!("Button: {button:?}");
                match widget_id.0.as_str() {
                    "SubmitTur" => if let Ok(svc_data) = &mut self.service_data.lock() {
                        svc_data.submit_tur_mastertech();
                    },
                    "GetKeys" => {
                        let (tx, rx) = crossbeam::channel::unbounded::<GetKeysResponse>();
                        if let Ok(svc_data) = &mut self.service_data.lock() {
                            let cps_tx = tx.clone();
                            let service_num = self.order_number.input.borrow().clone();
                            svc_data.ticket_data.service_number = service_num.lines()[0].to_string();
                            let cps_request = SendRequest::get_cps(service_num.lines()[0].to_string(), self.client.clone());

                            tokio::spawn(async move{
                                let req =  cps_request.await.unwrap_or_default();
                                log::info!("Keys response: {req:?}");
                                let _ = cps_tx.send(req);
                            });

                            if let Ok(keys) = rx.recv() {
                                log::info!("Got keys: {keys:?}");
                                self.keys = keys.clone();
                                self.webroot_key_btn.set_label(keys.webroot_key.clone());
                                self.superanti_key_btn.set_label(keys.superanti_key.clone());
                            }
                        }
                    },
                    "CopyWebroot" => {                
                        let sas = self.webroot_key_btn.get_label();
                        let mut clipboard = arboard::Clipboard::new().unwrap();
                        let set = clipboard.set().text(sas);
                        log::info!("set text to clip: {set:?}");
                    },
                    "CopySuperAnti" => {
                        let sas = self.superanti_key_btn.get_label();
                        let mut clipboard = arboard::Clipboard::new().unwrap();
                        let set = clipboard.set().text(sas);
                        log::info!("set text to clip: {set:?}");
                    },
                    "CheckSeb" => {
                        if let Ok(svc_data) = self.service_data.lock() {
                            if let Ok(ctx) = &mut ctx.lock() {
                                ctx.service_data = svc_data.clone();
                            }
                            let cust_email = svc_data.customer_data.email.clone();
                            if !cust_email.is_empty() {
                                let client = self.client.clone();
                                tokio::spawn(async move {
                                    let mut params: HashMap<&str, &str> = HashMap::new();
                                    params.insert("user_email", "logan.lees@pclaptops.com");
                                    params.insert("user_password", "Poolparty1");
                                    params.insert("application", "carbonite");
                                    params.insert("action", "search");
                                    params.insert("search", &cust_email);

                                    let response = client
                                        .post("https://scaffold.pclaptops.com/api/index")
                                        .header(CONTENT_TYPE, "application/json") // application/x-www-form-urlencoded
                                        .form(&params)
                                        .send()
                                        .await?;

                                    let response_json: Vec<serde_json::Value> = response.json().await?;
                                    log::info!("SEB Response: {:?}", response_json);
            
                                    Ok::<(), anyhow::Error>(())
                                });
                            }
                        }
                    },
                    "GetTicket" => {
                        if let Ok(svc_data) = &mut self.service_data.lock() {
                            log::info!("ServiceFormWidget handled a ButtonClick event.");
                            // Here you might access the input field's current value or trigger an API call.
                            let current_text = self.order_number.input.borrow().clone();
                            log::info!("Current order number: {}", current_text.lines()[0]);
                            svc_data.ticket_data.service_number = current_text.lines()[0].to_string();
                            svc_data.get_ticket();
                        }
                    },
                    _ => {}
                }
            }
            WidgetEvent::Api(api_event) => {
                match api_event {
                    ApiEvent::GetTicketResponse(presta_data) => {
                        if let Ok(svc_data) = &mut self.service_data.lock() {

                            let _ = svc_data.receive(presta_data.clone());

                            let mut customer_name = self.customer_name.input.borrow_mut();
                            customer_name.select_all();
                            customer_name.cut();
                            customer_name.insert_str(svc_data.customer_data.name.clone());

                            let mut customer_phone = self.customer_phone.input.borrow_mut();
                            let mut formatter = PhoneNumberFormatter::default();
                            let phone_number = formatter
                                .format_phone_number(
                                    &svc_data.customer_data.phone_number.clone()
                                )
                                .unwrap_or_default();
                            customer_phone.select_all();
                            customer_phone.cut();
                            customer_phone.insert_str(phone_number);

                            let mut salesman_name = self.salesman_name.input.borrow_mut();
                            salesman_name.select_all();
                            salesman_name.cut();
                            salesman_name.insert_str(svc_data.ticket_data.salesman.clone());

                            let mut technician_name = self.technician_name.input.borrow_mut();
                            technician_name.select_all();
                            technician_name.cut();
                            technician_name.insert_str(svc_data.ticket_data.tech.clone());

                            let mut checkin_notes = self.checkin_notes.input.borrow_mut();
                            checkin_notes.select_all();
                            checkin_notes.cut();
                            checkin_notes.insert_str(svc_data.ticket_data.checkin_notes.clone());

                            log::info!("SVC DATA: {svc_data:?}");
                        }
                    },
                }
            },
            WidgetEvent::Active { widget_id } => {
                self.set_active_field(widget_id.clone());
            }
        }
    }
}