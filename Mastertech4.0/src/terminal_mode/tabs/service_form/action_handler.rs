use crate::{
    tabs::tur_sheet::get_ticket::SendRequest, 
    terminal_mode::{events::action_handler::{get_event_sender, ActionHandler, ApiEvent, WidgetEvent, WidgetId}, modals::DuplicateMergeModal, systems::notification_system::{Notification, NotificationType}}
};

use database::schema::{
    CarboniteResponse, ExtendedSeb, GetKeysResponse, LocalSebData, TaskCreationResult, helper_traits::parse_email_user, prestashop_schema::ServiceOrder, utilities::{PhoneNumberFormatter, get_local_seb_data}
};

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
            // SEB/Carbonite copy buttons
            WidgetId("CopyCarboniteDeviceName".to_string()),
            WidgetId("CopyCarboniteDeviceId".to_string()),
            WidgetId("CopyActivationCode".to_string()),
            WidgetId("CopyRecurlyId".to_string()),
            // Add any other widget IDs handled by this tab
        ]
    }

    fn handle_event(&mut self, event: &WidgetEvent) {
        match event {
            WidgetEvent::ButtonClick { widget_id, button , source: _} => {
                log::info!("Button: {button:?}");
                match widget_id.0.as_str() {
                    "SubmitTur" => if let Ok(ctx) = &mut self.ctx.try_lock() {
                        if let Ok(recommendations) = self.recommendations.input.try_borrow() {
                            // Join all lines with spaces to create a single paragraph
                            // This handles multi-line input that may have been wrapped
                            let all_lines = recommendations.lines();
                            let joined_text = all_lines.iter()
                                .map(|line| line.trim())
                                .filter(|line| !line.is_empty())
                                .collect::<Vec<&str>>()
                                .join(" ");
                            ctx.service_data.task_data.task_description = joined_text;
                        }
                        let description_empty = ctx.service_data.task_data.task_description.is_empty();
                        let assignee_empty = &mut false;
                        if let Ok(mut salesman) =  self.salesman_name.input.try_borrow_mut() {
                            salesman.select_all();
                            salesman.copy();
                            let assignee = salesman.yank_text();
                            if assignee.is_empty() {
                                *assignee_empty = true;
                            } else {
                                ctx.service_data.ticket_data.salesman = parse_email_user(&assignee).to_string();
                                *assignee_empty = false;
                            }
                        }

                        if description_empty || *assignee_empty { // also check salesman here
                            let _ = ctx.data_sender.send(Box::new(Notification::new(
                                NotificationType::Error, 
                                "Missing Recommendations", 
                                "Please write recommendations, then submit tur sheet again", 
                                5
                            )));
                        } else {
                            let _ = ctx.data_sender.send(Box::new(Notification::new(
                                NotificationType::Info, 
                                "Sent TUR sheet", 
                                "", 
                                2
                            )));
                            log::warn!("TICKET COMPUTER DATA: {:#?}", ctx.service_data.ticket_data.computer);
                            log::warn!("\n\n\nCOMPUTER DATA: {:#?}", ctx.service_data.computer_data.seb_info);
                            ctx.service_data.submit_tur_mastertech();
                        }
                    },
                    "GetKeys" => if let Ok(mut ctx) = self.ctx.try_lock() {
                        let (tx, rx) = crossbeam::channel::unbounded::<Vec<GetKeysResponse>>();
                        let svc_data = &mut ctx.service_data;
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
                            let key = keys.get(0).cloned().unwrap_or_default();
                            self.keys = key.clone();
                            self.webroot_key_btn.set_label(key.webroot_key.clone());
                            self.superanti_key_btn.set_label(key.superanti_key.clone());
                        }
                    },
                    "CopyWebroot" => {
                        let wrv = self.webroot_key_btn.get_label();
                        let mut clipboard = arboard::Clipboard::new().unwrap();
                        let set = clipboard.set().text(wrv);
                        if let Ok(ctx) = self.ctx.try_lock() {
                            let _ = ctx.data_sender.send(Box::new(Notification::new(
                                NotificationType::Info, 
                                "Copied Webroot key to clipboard", 
                                wrv, 
                                2
                            )));
                        }
                        log::info!("set text to clip: {set:?}");
                    },
                    "CopySuperAnti" => {
                        let sas = self.superanti_key_btn.get_label();
                        let mut clipboard = arboard::Clipboard::new().unwrap();
                        let set = clipboard.set().text(sas);
                        if let Ok(ctx) = self.ctx.try_lock() {
                            let _ = ctx.data_sender.send(Box::new(Notification::new(
                                NotificationType::Info, 
                                "Copied SAS key to clipboard", 
                                sas, 
                                2
                            )));
                        }
                        log::info!("set text to clip: {set:?}");
                    },
                    "CopyCarboniteDeviceName" => {
                        let val = self.carbonite_device_name_btn.get_label();
                        if val != "Carbonite Device Name" {
                            let mut clipboard = arboard::Clipboard::new().unwrap();
                            let _ = clipboard.set().text(val);
                            if let Ok(ctx) = self.ctx.try_lock() {
                                let _ = ctx.data_sender.send(Box::new(Notification::new(
                                    NotificationType::Info, 
                                    "Copied Carbonite Device Name", 
                                    val, 
                                    2
                                )));
                            }
                        }
                    },
                    "CopyCarboniteDeviceId" => {
                        let val = self.carbonite_device_id_btn.get_label();
                        if val != "Device ID" {
                            let mut clipboard = arboard::Clipboard::new().unwrap();
                            let _ = clipboard.set().text(val);
                            if let Ok(ctx) = self.ctx.try_lock() {
                                let _ = ctx.data_sender.send(Box::new(Notification::new(
                                    NotificationType::Info, 
                                    "Copied Device ID", 
                                    val, 
                                    2
                                )));
                            }
                        }
                    },
                    "CopyActivationCode" => {
                        let val = self.activation_code_btn.get_label();
                        if val != "Activation Code" {
                            let mut clipboard = arboard::Clipboard::new().unwrap();
                            let _ = clipboard.set().text(val);
                            if let Ok(ctx) = self.ctx.try_lock() {
                                let _ = ctx.data_sender.send(Box::new(Notification::new(
                                    NotificationType::Info, 
                                    "Copied Activation Code", 
                                    val, 
                                    2
                                )));
                            }
                        }
                    },
                    "CopyRecurlyId" => {
                        let val = self.recurly_id_btn.get_label();
                        if val != "Recurly ID" {
                            let mut clipboard = arboard::Clipboard::new().unwrap();
                            let _ = clipboard.set().text(val);
                            if let Ok(ctx) = self.ctx.try_lock() {
                                let _ = ctx.data_sender.send(Box::new(Notification::new(
                                    NotificationType::Info, 
                                    "Copied Recurly ID", 
                                    val, 
                                    2
                                )));
                            }
                        }
                    },
                    "GetTicket" => if let Ok(ctx) = &mut self.ctx.try_lock() {
                        let _ = ctx.data_sender.send(Box::new(Notification::new(
                            NotificationType::Info, 
                            "Pulling Ticket Info", 
                            "", 
                            2
                        )));
                        
                        log::info!("ServiceFormTab handled a ButtonClick event.");
                        if let (
                            Ok(service_number), 
                            Ok(phone)
                        ) = (
                            self.order_number.input.try_borrow(), 
                            self.customer_phone.input.try_borrow()
                        ) {
                            let phone_number = phone.lines()[0].to_string();
                            ctx.service_data.ticket_data.service_number = service_number.lines()[0].to_string();
                            log::info!("Current order number: {}\nPhone: {phone_number}", service_number.lines()[0]);
                            
                            if !phone_number.is_empty() {
                                log::info!("Current phone number: {}", phone_number);
                                ctx.service_data.customer_data.phone_number = phone_number;
                            }
                            ctx.service_data.get_ticket();
                        }
                    },
                    _ => {}
                }
            }
            WidgetEvent::Api(api_event) => {
                match api_event {
                    ApiEvent::GetTicketResponse(presta_data) => {
                        log::info!("GetTicketResponse");
                        // Note: order_rows/product rows are no longer displayed
                        
                        if let Ok(ctx) = &mut self.ctx.try_lock() {
                            let _ = ctx.service_data.receive(presta_data.clone());
                            
                            let cust_email = ctx.service_data.customer_data.email.clone();
                            if !cust_email.is_empty() {
                                log::info!("Customer email: {cust_email:?}");
                                let client = self.client.clone();
                                tokio::spawn(async move {
                                    let response_json: Vec<CarboniteResponse> = CarboniteResponse::default()
                                        .from_customer_email(cust_email.clone(), client)
                                        .await?;
                                    log::info!("SEB Response: {:?}", response_json);
                                    let tx = get_event_sender();
                                    tx.try_send(WidgetEvent::Api(ApiEvent::GetSebResponse(response_json)))?;
                                    Ok::<(), anyhow::Error>(())
                                });
                            }

                            log::info!("service_number");
                            // Update service_number
                            {
                                let mut service_number = self.order_number.input.borrow_mut();
                                if service_number.lines()[0].is_empty() {
                                    service_number.select_all();
                                    service_number.cut();
                                    service_number.insert_str(ctx.service_data.ticket_data.service_number.clone());
                                }
                            } // service_number dropped here
                            log::info!("customer_name");
                            // Update customer_name
                            {
                                let mut customer_name = self.customer_name.input.borrow_mut();
                                customer_name.select_all();
                                customer_name.cut();
                                customer_name.insert_str(ctx.service_data.customer_data.name.clone());
                            } // customer_name dropped here
                            log::info!("customer_phone");
                            // Update customer_phone
                            {
                                let mut customer_phone = self.customer_phone.input.borrow_mut();
                                let mut formatter = PhoneNumberFormatter::default();
                                let phone_number = formatter
                                    .format_phone_number(&ctx.service_data.customer_data.phone_number.clone())
                                    .unwrap_or_default();
                                customer_phone.select_all();
                                customer_phone.cut();
                                customer_phone.insert_str(phone_number);
                            } // customer_phone dropped here
                            log::info!("technician_name");
                            // Update technician_name
                            {
                                let mut technician_name = self.technician_name.input.borrow_mut();
                                technician_name.select_all();
                                technician_name.cut();
                                technician_name.insert_str(ctx.service_data.ticket_data.tech.clone());
                            } // technician_name dropped here
                            log::info!("checkin_notes");
                            // Update checkin_notes
                            {
                                let mut checkin_notes = self.checkin_notes.input.borrow_mut();
                                checkin_notes.select_all();
                                checkin_notes.cut();
                                checkin_notes.insert_str(ctx.service_data.ticket_data.checkin_notes.clone());
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
                                        input.insert_str(ctx.service_data.customer_data.email.clone());
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
                            log::info!("SVC DATA: {:?}", ctx.service_data);
                        }
                    },
                    ApiEvent::GetSebResponse(carbonite_response) => {
                        log::info!("GetSebResponse");
                        let carbonite = carbonite_response.get(0).cloned().unwrap_or_default();
                        
                        // Update context with SEB data
                        if let Ok(ctx) = &mut self.ctx.try_lock() {
                            let seb_data = &mut ctx.service_data.computer_data.seb_info;
                            log::warn!("WE HAVE CTX");
                            if let Ok(seb) = get_local_seb_data() {
                                log::warn!("LOCAL SEB: {seb:#?}");
                                *seb_data = Some(seb);
                            } else {
                                *seb_data = Some(LocalSebData {
                                    InstalledDeviceId: carbonite.device_id.clone(),
                                    InstallInstanceId: carbonite.device_id.clone(),
                                    ActivationCode: carbonite.activation_code.clone(),
                                    InstallVersion: carbonite.client_version.clone(),
                                    MachineName: carbonite.device_name.clone(),
                                    ExtendedSeb: Some(ExtendedSeb {
                                        email: carbonite.email.clone(),
                                        phone: carbonite.phone.clone(),
                                        userid: carbonite.userid.clone(),
                                        device_name: carbonite.device_name.clone(),
                                        device_id: carbonite.device_id.clone(),
                                        state: carbonite.state.clone(),
                                        usage_gb: carbonite.usage_gb.clone(),
                                        date_device_created: carbonite.date_device_created.clone(),
                                        activated: carbonite.activated.clone(),
                                        activation_code: carbonite.activation_code.clone(),
                                        last_complete_backup: carbonite.last_complete_backup.clone(),
                                        last_client_status_update: carbonite.last_client_status_update.clone(),
                                        id_recurly_account: carbonite.id_recurly_account.clone(),
                                        date_last_scan: carbonite.date_last_scan.clone(),
                                        date_email_sent: carbonite.date_email_sent.clone(),
                                        date_canceled_account: carbonite.date_canceled_account.clone(),
                                        date_deleted_account: carbonite.date_deleted_account.clone(),
                                        current_period_ends_at: carbonite.current_period_ends_at.clone(),
                                        date_modified: carbonite.date_modified.clone(),
                                        date_created: carbonite.date_created.clone(),
                                    }),
                                    ..Default::default()
                                });

                                log::warn!("svc_data.computer_data.seb_info: {:#?}", seb_data);
                            }
                        }
                        
                        // Update button labels with the SEB values
                        if !carbonite.device_name.is_empty() {
                            self.carbonite_device_name_btn.set_label(carbonite.device_name);
                        }
                        if !carbonite.device_id.is_empty() {
                            self.carbonite_device_id_btn.set_label(carbonite.device_id);
                        }
                        if !carbonite.activation_code.is_empty() {
                            self.activation_code_btn.set_label(carbonite.activation_code);
                        }
                        if !carbonite.id_recurly_account.is_empty() {
                            self.recurly_id_btn.set_label(carbonite.id_recurly_account);
                        }
                    },
                    ApiEvent::DuplicateCheckResponse(check_result) => {
                        log::info!("Received duplicate check response for SO#{}", check_result.service_number);
                        *self.awaiting_duplicate_check.borrow_mut() = false;
                        
                        if check_result.has_conflicts() && !check_result.all_identical() {
                            // Show the duplicate merge modal
                            log::info!("Conflicts found, opening duplicate merge modal");
                            let modal = DuplicateMergeModal::new(check_result.clone());
                            self.duplicate_modal.replace(Some(modal));
                            
                            if let Ok(ctx) = self.ctx.try_lock() {
                                let _ = ctx.data_sender.send(Box::new(Notification::new(
                                    NotificationType::Warning, 
                                    "Duplicate Records Found", 
                                    "Please resolve conflicts in the popup", 
                                    5
                                )));
                            }
                        } else {
                            // No conflicts or all identical - proceed with submission
                            log::info!("No conflicts, proceeding with submission");
                            if let Ok(mut ctx) = self.ctx.try_lock() {
                                ctx.service_data.submit_after_resolution(None);
                            }
                        }
                    },
                    ApiEvent::TaskCreationResponse(result) => {
                        log::info!("Task creation response: {:?}", result);
                        match result {
                            TaskCreationResult::Created { service_number } => {
                                if let Ok(ctx) = self.ctx.try_lock() {
                                    let _ = ctx.data_sender.send(Box::new(Notification::new(
                                        NotificationType::Info, 
                                        "Task Created Successfully", 
                                        &format!("Service Order: {}", service_number), 
                                        5
                                    )));
                                }
                            },
                            TaskCreationResult::Updated { service_number } => {
                                if let Ok(ctx) = self.ctx.try_lock() {
                                    let _ = ctx.data_sender.send(Box::new(Notification::new(
                                        NotificationType::Info, 
                                        "Task Updated", 
                                        &format!("SO#{} was updated", service_number), 
                                        5
                                    )));
                                }
                            },
                            TaskCreationResult::AlreadyExists { service_number } => {
                                if let Ok(ctx) = self.ctx.try_lock() {
                                    let _ = ctx.data_sender.send(Box::new(Notification::new(
                                        NotificationType::Warning, 
                                        "Task Already Exists", 
                                        &format!("SO#{} already has a task", service_number), 
                                        5
                                    )));
                                }
                            },
                            TaskCreationResult::Error { message } => {
                                if let Ok(ctx) = self.ctx.try_lock() {
                                    let _ = ctx.data_sender.send(Box::new(Notification::new(
                                        NotificationType::Error, 
                                        "Task Creation Failed", 
                                        &message, 
                                        5
                                    )));
                                }
                            },
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