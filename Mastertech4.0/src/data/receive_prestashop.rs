use crate::app_state::MasterTechApp;
use database::schema::{ComputerData, helper_traits::{convert_date_string, parse_email_user, EmployeeHelper}, prestashop_schema::{Employee, ServiceOrder}, utilities::query_user_from_email, CarboniteResponse, HardwareTests, TaskNotePayload, User, TASK_NOTE_TABLE, TASK_TABLE, TICKET_TABLE};

#[cfg(target_os="windows")]
use crate::filesystem::system_info::ComputerInfo;

impl MasterTechApp {
    pub fn receive_prestashop(&mut self) {
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
            let (tx, rx) = crossbeam::channel::bounded::<User>(1);
            let mut services: Vec<surrealdb::RecordId> = Vec::new();
            let user = &mut User::default();

            task.id = surrealdb::RecordId::from((TASK_TABLE, surrealdb::RecordIdKey::from_inner(surrealdb::sql::Id::rand())));

            let employees = data
                .customer_messages
                .iter()
                .map(|msg| msg.id_employee.clone())
                .collect::<Vec<String>>();

            tokio::spawn(async move {
                if !customer_email.is_empty() {
                    let response_json: Vec<CarboniteResponse> = CarboniteResponse::default()
                    .from_customer_email(customer_email.clone(), client)
                    .await?;
                    log::info!("SEB Response: {:?}", response_json);
                    carobonite_tx.try_send(response_json)?;
                }
                for emp in employees.iter() {
                    let employee = Employee::default().get_employee_from_id(emp).await?;
                    let user = query_user_from_email(employee.email).await?;
                    tx.try_send(user)?;
                }
                Ok::<(), anyhow::Error>(())
            });

            if let Ok(usr) = rx.try_recv() { 
                *user = usr;
            }
            
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

            for msg in data.customer_messages.iter() {
                task_notes.push(TaskNotePayload {
                    everest_initials: user.everest_initials.clone(),
                    note: msg.message.clone(),
                    created_at: match convert_date_string(&msg.date_add) {
                        Ok(date) => date,
                        Err(e) => {
                            log::info!("Parse error: {e:?}");
                            msg.date_add.clone()
                        },
                    },
                    id: surrealdb::RecordId::from((TASK_NOTE_TABLE, msg.id.clone())),
                    task_id: Some(task.id.clone()),
                    username: parse_email_user(&user.email).to_string(),
                    user: Some(user.id.clone()),
                    id_customer_thread: Some(msg.id_customer_thread.clone()),
                    id_customer_message: Some(msg.id.clone()),
                    id_employee: Some(msg.id_employee.clone()),
                    service_number: Some(ticket.service_number.clone()),
                })
            }

            customer.id = data.customer.id.clone();
            customer.cust_code = data.customer.cust_code.clone();
            customer.email = data.customer.email.clone();
            customer.name = data.customer.name.clone();
            customer.phone_number = data.customer.phone_number.clone();
            ticket.salesman = email_split_rep;
            ticket.sales_rep = email.clone();
            ticket.tech = email.clone();
            ticket.customer = Some(customer.id.clone());
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
                if service_details.len() == 1 {
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

            #[cfg(target_os = "windows")]
            {
                let cps = &mut self.context.current_antivirus;
                let installed_antivirus = ComputerData::get_antivirus()
                    .map_err(|e| *cps += format!("Error checking antivirus: {e}\n").as_str())
                    .unwrap_or(Vec::new());
                let x: Vec<String> = installed_antivirus
                    .iter()
                    .map(|cps| {
                        if let Some(true) = cps.1 {
                            cps.0.clone()
                        } else {
                            "Not installed".to_string()
                        }
                    })
                    .collect::<Vec<String>>();
                ticket.current_antivirus = Some(x);
            }
            task.service_ticket = Some(ticket.id.clone());

            self.context.output_text +=
                &serde_json::to_string_pretty(&ticket).unwrap_or("".to_string());
            self.context.output_text +=
                &serde_json::to_string_pretty(&customer).unwrap_or("".to_string());
            self.context.output_text +=
                &serde_json::to_string_pretty(&computer).unwrap_or("".to_string());
        }
    }
}


/*
use crate::app_state::MasterTechApp;
use database::schema::{CarboniteResponse, HardwareTests, TaskNotePayload, TASK_NOTE_TABLE, TASK_TABLE, TICKET_TABLE};
use log::info;
use surrealdb::{sql::Uuid, RecordId};
#[cfg(target_os="windows")]
use {
    database::schema::ComputerData,
    crate::filesystem::system_info::ComputerInfo
};

impl MasterTechApp {
    pub fn receive_prestashop(&mut self) {
        if let Ok(data) = self.context.prestashop_api_rx.try_recv() {
            let service_details = data.order.associations.order_service;
            self.context.service_details = service_details.clone();
            let customer = &mut self.context.customer_data; // self.context.shared_ctx.tur.
            let ticket = &mut self.context.ticket_data;
            let task = &mut self.context.task_data;
            let task_notes = &mut self.context.task_notes;
            let computer = &mut self.context.computer_data;

            let email = data.customer.email.clone();
            let client = self.context.client.clone();
            let carobonite_tx = self.context.seb_channel.0.clone();
            if !email.is_empty() {
                tokio::spawn(async move {
                    let response_json: Vec<CarboniteResponse> = CarboniteResponse::default()
                    .from_customer_email(email.clone(), client)
                    .await?;
                    log::info!("SEB Response: {:?}", response_json);
                    carobonite_tx.try_send(response_json)?;
                    Ok::<(), anyhow::Error>(())
                });
            }

            let hdd_test = format!("{:?}", &self.context.hdd_test_cbox);
            let ram_test = format!("{:?}", &self.context.ram_test_cbox);
            let ssd_test = format!("{:?}", &self.context.ssd_test_cbox);

            if ticket.service_number.is_empty() {
                ticket.service_number = data.order.id;
            }
            
            task.id = RecordId::from_table_key(
                TASK_TABLE, 
                Uuid::new_v4()
                    .to_raw()
                    .split_terminator('-')
                    .collect::<Vec<&str>>()
                    .concat()
            );

            
            let mut owned_computers: Vec<RecordId> = Vec::new();
            let mut services: Vec<RecordId> = Vec::new();

            #[cfg(target_os = "windows")]
            {
                let cps = &mut self.context.current_antivirus;
                let installed_antivirus = ComputerData::get_antivirus()
                    .map_err(|e| *cps += format!("Error checking antivirus: {e}\n").as_str())
                    .unwrap_or(Vec::new());
                let x: Vec<String> = installed_antivirus
                    .iter()
                    .map(|cps| {
                        if let Some(true) = cps.1 {
                            cps.0.clone()
                        } else {
                            "Not installed".to_string()
                        }
                    })
                    .collect::<Vec<String>>();
                ticket.current_antivirus = Some(x);
            }

            let sales_rep = data.sales_rep.unwrap_or_default();
            let split_rep = data.split_rep.unwrap_or_default();
            let email = sales_rep
                .email
                .split_once("@")
                .clone()
                .unwrap_or(("", "pclaptops.com"))
                .0
                .to_string();
            let email_split_rep = split_rep
                .email
                .split_once("@")
                .clone()
                .unwrap_or(("", "pclaptops.com"))
                .0
                .to_string();
            
            for msg in data.customer_messages {
                // let username = msg.id_employee
                if msg.id_employee.clone() == "0" || msg.id_customer_thread == "0" {
                    continue;
                } else {
                    let mut task_note_payload = TaskNotePayload {
                        note: msg.message,
                        created_at: msg.date_add,
                        id_customer_thread: Some(msg.id_customer_thread),
                        id_customer_message: Some(msg.id.clone()),
                        id_employee: Some(msg.id_employee.clone()),
                        id: RecordId::from_table_key(TASK_NOTE_TABLE, msg.id.clone()),
                        task_id: Some(task.id.clone()),
                        ..Default::default()
                    };
                    for user in self.context.shared_ctx.store_users.iter() {
                        if let Some(presta_id) = user.id_prestashop {
                            if msg.id_employee == presta_id.to_string() {
                                task_note_payload.everest_initials = user.everest_initials.clone();
                                task_note_payload.user = Some(user.id.clone());
                            }
                        }
                    }
                    task_notes.push(task_note_payload);
                }
            }

            customer.id = data.customer.id;
            customer.cust_code = data.customer.cust_code;
            customer.email = data.customer.email;
            customer.name = data.customer.name.clone();
            customer.phone_number = data.customer.phone_number;
            computer.customer = Some(customer.id.clone());
            ticket.salesman = email_split_rep;
            ticket.tech = email;
            ticket.customer = Some(customer.id.clone());
            ticket.computer = Some(computer.id.clone());
            ticket.hardware_test_results = HardwareTests {
                hdd_test,
                ssd_test,
                ram_test,
            };
            ticket.doc_alias = data.order.order_type_name.unwrap_or(String::new());

            ticket.id = RecordId::from((
                TICKET_TABLE.to_string(),
                ticket.service_number.clone(),
            ));
            owned_computers.push(computer.id.clone());
            
            // customer.computers = Some(owned_computers);
            services.push(ticket.id.clone());
            

            if !service_details.is_empty() {
                if service_details.len() == 1 {
                    let svc = service_details.get(0);
                    if let Some(service) = svc {
                        ticket.checkin_notes = service.check_in_notes.clone();
                    }
                } else {
                    info!("Theres a couple.... {:?}", service_details);
                }
            }

            self.context.output_text +=
                &serde_json::to_string_pretty(&ticket).unwrap_or("".to_string());
            self.context.output_text +=
                &serde_json::to_string_pretty(&customer).unwrap_or("".to_string());
            self.context.output_text +=
                &serde_json::to_string_pretty(&computer).unwrap_or("".to_string());
        }
    }
}
*/
