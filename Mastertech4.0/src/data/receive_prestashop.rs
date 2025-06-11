use database::schema::{ComputerData, helper_traits::parse_email_user, prestashop_schema::ServiceOrder, CarboniteResponse, HardwareTests, TaskNotePayload, TASK_TABLE, TICKET_TABLE};
use eframe::Frame;
use crate::app_state::MasterTechApp;

// #[cfg(target_os="windows")]
// use crate::filesystem::system_info::ComputerInfo;

impl MasterTechApp {
    pub fn receive_prestashop(&mut self, frame: &mut Frame) {
        if let Ok(data) = self.context.prestashop_api_rx.try_recv() {
            let service_details = data.order.associations.order_service.clone();
            self.context.service_details = service_details.clone();
            let customer = &mut self.context.customer_data; // self.context.shared_ctx.tur.
            let ticket = &mut self.context.ticket_data;
            let task = &mut self.context.task_data;
            let task_notes = &mut self.context.task_notes;
            let computer = &mut self.context.computer_data;
            let sales_rep = data.sales_rep.clone().unwrap_or_default();
            let split_rep = data.split_rep.clone().unwrap_or_default();
            let email = parse_email_user(&sales_rep.email).to_string();
            let email_split_rep = parse_email_user(&split_rep.email).to_string();
            let customer_email = data.customer.email.clone();
            let client = self.context.client.clone();
            let carobonite_tx = self.context.seb_channel.0.clone();
            let hdd_test = format!("{:?}", &self.context.hdd_test_cbox);
            let ram_test = format!("{:?}", &self.context.ram_test_cbox);
            let ssd_test = format!("{:?}", &self.context.ssd_test_cbox);
            let mut services: Vec<surrealdb::RecordId> = Vec::new();
            let order_rows: Vec<database::schema::prestashop_schema::OrderRow> = data.order.associations.order_rows.clone();
            self.context.order_rows = order_rows;

            task.id = surrealdb::RecordId::from((TASK_TABLE, surrealdb::RecordIdKey::from_inner(surrealdb::sql::Id::rand().into())));

            tokio::spawn(async move {
                if !customer_email.is_empty() {
                    log::warn!("Spawned thread, checking for CarboniteResponse");
                    let response_json = CarboniteResponse::default()
                        .from_customer_email(customer_email.clone(), client)
                        .await;

                    match response_json {
                        Ok(carbonite_response) => { let _ = carobonite_tx.try_send(carbonite_response); },
                        Err(e) => log::warn!("Error from carbonite response: {e:?}"),
                    }
                }
            });

            let device_details: Vec<ServiceOrder> = data
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
                }
            ).collect();

            let device = device_details.get(0).cloned().unwrap_or_default();

            for msg in data.task_notes.iter() {
                task_notes.push(TaskNotePayload {
                    task_id: Some(task.id.clone()),
                    ..msg.clone()
                });
            }

            log::warn!("receive_prestashop -> NOTES: {task_notes:#?}");

            customer.id = data.customer.id.clone();
            customer.cust_code = data.customer.cust_code.clone();
            customer.email = data.customer.email.clone();
            customer.name = data.customer.name.clone();
            customer.phone_number = data.customer.phone_number.clone();
            ticket.salesman = email_split_rep;
            ticket.sales_rep = email.clone();
            ticket.tech = email.clone();
            ticket.customer = customer.id.clone();
            ticket.checkin_rep = email;
            ticket.terms = data.order.payment.clone();
            ticket.ticket_total = data.order.total_products_wt.clone();
            ticket.doc_alias = data.order.order_type.clone();
            ticket.service_number = data.order.id.clone();
            ticket.hardware_test_results = HardwareTests { hdd_test, ssd_test, ram_test };
            ticket.id = surrealdb::RecordId::from(( TICKET_TABLE.to_string(), ticket.service_number.clone() ));
            log::info!("Salesman: {:?}\nTech: {:?}",ticket.salesman.clone(),ticket.tech.clone());

            services.push(ticket.id.clone());
            
            if !service_details.is_empty() {
                if service_details.len() >= 1 {
                    let svc = service_details.get(0);
                    if let Some(service) = svc {
                        ticket.checkin_notes = service.check_in_notes.clone();
                    }
                } else {
                    log::info!("Theres a couple.... {:?}", service_details);
                }
            }

            *computer = ComputerData {
                device_name: Some(device.device_name),
                device_mfg: Some(device.device_mfg),
                device_model: Some(device.device_model),
                device_serial: Some(device.device_serial),
                customer: Some(customer.id.clone()),
                ..computer.clone()
            };

            ticket.computer = Some(computer.id.clone());
            log::warn!("Ticket.Computer.SEB: {:#?}", computer.seb_info);

            // #[cfg(target_os = "windows")]
            // {
            //     let cps = &mut self.context.current_antivirus;
            //     let installed_antivirus = ComputerData::get_antivirus()
            //         .map_err(|e| *cps += format!("Error checking antivirus: {e}\n").as_str())
            //         .unwrap_or(Vec::new());
            //     let x: Vec<String> = installed_antivirus
            //         .iter()
            //         .map(|cps| {
            //             if let Some(true) = cps.1 {
            //                 cps.0.clone()
            //             } else {
            //                 "Not installed".to_string()
            //             }
            //         })
            //         .collect::<Vec<String>>();
            //     ticket.current_antivirus = Some(x);
            // }
            task.service_ticket = Some(ticket.id.clone());

            if let Some(storage) = frame.storage_mut() {
                storage.set_string("ticket_data", serde_json::to_string(&ticket).unwrap_or_default());
                storage.set_string("task_data", serde_json::to_string(&task).unwrap_or_default());
                storage.set_string("customer_data", serde_json::to_string(&customer).unwrap_or_default());
            }
        }
    }
}