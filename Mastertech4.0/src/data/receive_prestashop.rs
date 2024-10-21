use crate::app_state::MasterTechApp;
use database::schema::{HardwareTests, TaskNotePayload, TASK_NOTE_TABLE, TASK_TABLE, TICKET_TABLE};
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
            let customer = &mut self.context.customer_data;
            let ticket = &mut self.context.ticket_data;
            let task = &mut self.context.task_data;
            let task_notes = &mut self.context.task_notes;
            let computer = &mut self.context.computer_data;

            let hdd_test = format!("{:?}", &self.context.hdd_test_cbox);
            let ram_test = format!("{:?}", &self.context.ram_test_cbox);
            let ssd_test = format!("{:?}", &self.context.ssd_test_cbox);

            task.id = RecordId::from_table_key(
                TASK_TABLE, 
                Uuid::new_v4()
                    .to_raw()
                    .split_terminator('-')
                    .collect::<Vec<&str>>()
                    .concat()
            );

            let service_details = data.order.associations.order_service;
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
                    if let Some(users) = self.context.store_users.as_ref() {
                        for user in users {
                            if let Some(presta_id) = user.id_prestashop {
                                if msg.id_employee == presta_id.to_string() {
                                    task_note_payload.everest_initials = user.everest_initials.clone();
                                    task_note_payload.user = Some(user.id.clone());
                                }
                            }
                        }
                    };
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
