use crate::{
    tabs::tur_sheet::get_ticket::SendRequest, 
    terminal_mode::events::action_handler::{get_event_sender, ActionHandler, ApiEvent, WidgetEvent, WidgetId}
};

use database::schema::{
    prestashop_schema::ServiceOrder, utilities::PhoneNumberFormatter, CarboniteResponse, GetKeysResponse
};

use reqwest::header::{ACCEPT, CONTENT_TYPE};

use super::ServiceFormTab;

impl <'a> ActionHandler for ServiceFormTab <'a> {
    fn widget_id(&self) -> WidgetId {
        WidgetId("ServiceFormTab".to_string()) // Unique ID for the tab
    }
    
    fn managed_widget_ids(&self) -> Vec<WidgetId> {
        vec![
            WidgetId("ServiceNumber".to_string()),
            WidgetId("SubmitTur".to_string()),
            WidgetId("GetKeys".to_string()),
            WidgetId("CopyWebroot".to_string()),
            WidgetId("CopySuperAnti".to_string()),
            WidgetId("CheckSeb".to_string()),
            WidgetId("GetTicket".to_string()),
            WidgetId("CustomerName".to_string()),
            WidgetId("CustomerPhone".to_string()),
            WidgetId("SalesmanName".to_string()),
            WidgetId("TechnicianName".to_string()),
            WidgetId("CheckInNotes".to_string()),
            WidgetId("Recommendations".to_string()),
            WidgetId("CustomerEmail".to_string()),
            WidgetId("DeviceName".to_string()),
            WidgetId("DeviceMfg".to_string()),
            WidgetId("DeviceModel".to_string()),
            WidgetId("DeviceSerial".to_string()),
            WidgetId("DevicePassword".to_string()),
            WidgetId("DevicePowerSupply".to_string()),
            WidgetId("CarboniteDeviceName".to_string()),
            WidgetId("CarboniteDeviceId".to_string()),
            WidgetId("ActivationCode".to_string()),
            WidgetId("RecurlyId".to_string()),
            WidgetId("UsageGb".to_string()),
            // Add any other widget IDs handled by this tab
        ]
    }

    fn handle_event(&mut self, event: &WidgetEvent) {
        match event {
            WidgetEvent::ButtonClick { widget_id, button , source: _} => {
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
                            
                            let cps_request = SendRequest::get_cps(
                                service_num.lines()[0].to_string(), 
                                self.client.clone()
                            );

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
                            if let Ok(ctx) = &mut self.ctx.lock() {
                                ctx.service_data = svc_data.clone();
                            }
                            let cust_email = svc_data.customer_data.email.clone();
                            if !cust_email.is_empty() {
                                log::info!("Customer email: {cust_email:?}");
                                let client = self.client.clone();
                                tokio::spawn(async move {
                                    // let mut params: HashMap<&str, &str> = HashMap::new();
                                    // params.insert("user_email", "logan.lees@pclaptops.com");
                                    // params.insert("user_password", "Poolparty1");
                                    // params.insert("application", "carbonite");
                                    // params.insert("action", "search");
                                    // params.insert("search", &cust_email);

                                    let json = serde_json::json!({
                                        "user_email": "logan.lees@pclaptops.com",
                                        "user_password": "Poolparty1",
                                        "application": "carbonite",
                                        "action": "search",
                                        "search": &cust_email
                                    });

                                    let response = client
                                        .post("https://scaffold.pclaptops.com/api/index")
                                        .header(CONTENT_TYPE, "application/json") // application/x-www-form-urlencoded
                                        .header(ACCEPT, "application/json")
                                        .json(&json)
                                        // .form(&params)
                                        .send()
                                        .await?;

                                    log::info!("response: {response:?}");

                                    let response_json: Vec<CarboniteResponse> = response.json().await?;
                                    log::info!("SEB Response: {:?}", response_json);
                                    let tx = get_event_sender();
                                    tx.try_send(WidgetEvent::Api(ApiEvent::GetSebResponse(response_json)))?;
                                    Ok::<(), anyhow::Error>(())
                                });
                            } else {
                                log::info!("Customer email is empty, cannot check SEB.");
                            }
                        }
                    },
                    "GetTicket" => {
                        if let Ok(svc_data) = &mut self.service_data.lock() {
                            log::info!("ServiceFormTab handled a ButtonClick event.");
                            if let (
                                Ok(service_number), 
                                Ok(phone)
                            ) = (
                                self.order_number.input.try_borrow(), 
                                self.customer_phone.input.try_borrow()
                            ) {
                                let phone_number = phone.lines()[0].to_string();
                                svc_data.ticket_data.service_number = service_number.lines()[0].to_string();
                                log::info!("Current order number: {}\nPhone: {phone_number}", service_number.lines()[0]);
                                
                                if !phone_number.is_empty() {
                                    log::info!("Current phone number: {}", phone_number);
                                    svc_data.customer_data.phone_number = phone_number;
                                }
                                svc_data.get_ticket();
                            }
                        }
                    },
                    _ => {}
                }
            }
            WidgetEvent::Api(api_event) => {
                match api_event {
                    ApiEvent::GetTicketResponse(presta_data) => {
                        log::info!("GetTicketResponse");
                        let order_rows = presta_data.order.associations.order_rows.clone();
                        if !order_rows.is_empty() {
                            self.set_order_rows(order_rows);
                        }

                        if let Ok(svc_data) = &mut self.service_data.lock() {

                            let _ = svc_data.receive(presta_data.clone());
                            log::info!("service_number");
                            // Update service_number
                            {
                                let mut service_number = self.order_number.input.borrow_mut();
                                if service_number.lines()[0].is_empty() {
                                    service_number.select_all();
                                    service_number.cut();
                                    service_number.insert_str(svc_data.ticket_data.service_number.clone());
                                }
                            } // service_number dropped here
                            log::info!("customer_name");
                            // Update customer_name
                            {
                                let mut customer_name = self.customer_name.input.borrow_mut();
                                customer_name.select_all();
                                customer_name.cut();
                                customer_name.insert_str(svc_data.customer_data.name.clone());
                            } // customer_name dropped here
                            log::info!("customer_phone");
                            // Update customer_phone
                            {
                                let mut customer_phone = self.customer_phone.input.borrow_mut();
                                let mut formatter = PhoneNumberFormatter::default();
                                let phone_number = formatter
                                    .format_phone_number(&svc_data.customer_data.phone_number.clone())
                                    .unwrap_or_default();
                                customer_phone.select_all();
                                customer_phone.cut();
                                customer_phone.insert_str(phone_number);
                            } // customer_phone dropped here
                            log::info!("salesman_name");
                            // Update salesman_name
                            {
                                let mut salesman_name = self.salesman_name.input.borrow_mut();
                                salesman_name.select_all();
                                salesman_name.cut();
                                salesman_name.insert_str(svc_data.ticket_data.salesman.clone());
                            } // salesman_name dropped here
                            log::info!("technician_name");
                            // Update technician_name
                            {
                                let mut technician_name = self.technician_name.input.borrow_mut();
                                technician_name.select_all();
                                technician_name.cut();
                                technician_name.insert_str(svc_data.ticket_data.tech.clone());
                            } // technician_name dropped here
                            log::info!("checkin_notes");
                            // Update checkin_notes
                            {
                                let mut checkin_notes = self.checkin_notes.input.borrow_mut();
                                checkin_notes.select_all();
                                checkin_notes.cut();
                                checkin_notes.insert_str(svc_data.ticket_data.checkin_notes.clone());
                            } // checkin_notes dropped here
                            log::info!("other_fields");

                            log::info!("Filling other fields");
                            for field in self.other_fields.iter_mut() {
                                let widget_id = field.id();

                                let device_details: Vec<ServiceOrder> = presta_data
                                    .order
                                    .associations
                                    .order_service
                                    .iter()
                                    .map(|o| {
                                        ServiceOrder {
                                            device_name: o.device_name.clone(),
                                            device_mfg: o.device_mfg.clone(),
                                            device_model: o.device_model.clone(),
                                            device_serial: o.device_serial.clone(),
                                            device_password: o.device_password.clone(),
                                            device_power_supply: o.device_power_supply.clone(),
                                            check_in_notes: o.check_in_notes.clone(),
                                            ..Default::default()
                                        }
                                    }).collect();
                    
                                let device = device_details.get(0).cloned().unwrap_or_default();
                                log::info!("Unwrapping device details");
                                match widget_id.0.as_str() {
                                    "CustomerEmail" => {
                                        let mut input = field.input.borrow_mut();
                                        input.select_all();
                                        input.cut();
                                        input.insert_str(svc_data.customer_data.email.clone());
                                    }
                                    "DeviceName" => {
                                        let mut input = field.input.borrow_mut();
                                        input.select_all();
                                        input.cut();
                                        input.insert_str(device.device_name);
                                    }
                                    "DeviceMfg" => {
                                        let mut input = field.input.borrow_mut();
                                        input.select_all();
                                        input.cut();
                                        input.insert_str(device.device_mfg);
                                    }
                                    "DeviceModel" => {
                                        let mut input = field.input.borrow_mut();
                                        input.select_all();
                                        input.cut();
                                        input.insert_str(device.device_model);
                                    }
                                    "DeviceSerial" => {
                                        let mut input = field.input.borrow_mut();
                                        input.select_all();
                                        input.cut();
                                        input.insert_str(device.device_serial);
                                    }
                                    "DevicePassword" => {
                                        let mut input = field.input.borrow_mut();
                                        input.select_all();
                                        input.cut();
                                        input.insert_str(device.device_password);
                                    }
                                    "DevicePowerSupply" => {
                                        let mut input = field.input.borrow_mut();
                                        input.select_all();
                                        input.cut();
                                        input.insert_str(device.device_power_supply);
                                    }
                                    _ => ()
                                }
                            }
                            log::info!("SVC DATA: {svc_data:?}");
                        }
                    },
                    ApiEvent::GetSebResponse(carbonite_response) => {
                        log::info!("GetSebResponse");
                        for field in self.seb_fields.iter_mut() {
                            let id = field.id();
                            let carbonite = carbonite_response.get(0).cloned().unwrap_or_default();
                            match id.0.as_str() {
                                "CarboniteDeviceName" => {
                                    let mut input = field.input.borrow_mut();
                                    input.select_all();
                                    input.cut();
                                    input.insert_str(carbonite.device_name);
                                }
                                "CarboniteDeviceId" => {
                                    let mut input = field.input.borrow_mut();
                                    input.select_all();
                                    input.cut();
                                    input.insert_str(carbonite.device_id);
                                }
                                "ActivationCode" => {
                                    let mut input = field.input.borrow_mut();
                                    input.select_all();
                                    input.cut();
                                    input.insert_str(carbonite.activation_code);
                                }
                                "RecurlyId" => {
                                    let mut input = field.input.borrow_mut();
                                    input.select_all();
                                    input.cut();
                                    input.insert_str(carbonite.id_recurly_account);
                                }
                                "UsageGb" => {
                                    let mut input = field.input.borrow_mut();
                                    input.select_all();
                                    input.cut();
                                    input.insert_str(carbonite.usage_gb);
                                }
                                _ => {}
                            }
                        }
                    },
                }
            },
            WidgetEvent::Active { widget_id } => {
                log::info!("New active field: {widget_id:?}");
                self.set_active_field(widget_id.clone());
            }
        }
    }
}