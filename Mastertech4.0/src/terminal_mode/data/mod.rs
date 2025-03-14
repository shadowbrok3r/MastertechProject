
use database::schema::{prestashop_schema::PrestashopPayload, utilities::{create_full_task_payload, get_prestashop_payload}, ComputerData, CustomerData, TaskNotePayload, TaskPayload, TicketPayload, TICKET_TABLE};
use crate::filesystem::system_info::ComputerInfo;
use chrono::{DateTime, SecondsFormat, Utc};
use std::sync::{Arc, Condvar, Mutex};
use surrealdb::RecordId;
// use reqwest::Client;

use super::events::action_handler::{get_event_sender, ApiEvent, WidgetEvent};

pub mod first_run;


#[derive(Debug, Clone, Default)]
pub struct ServiceData {
    pub task_data: TaskPayload,
    pub ticket_data: TicketPayload,
    pub customer_data: CustomerData,
    pub computer_data: ComputerData,
    pub task_notes: Vec<TaskNotePayload>,
    send_specs: bool,
    // client: Client,
}

impl ServiceData {
    pub fn new() -> Self {
        let pair = Arc::new(
            (Mutex::new(ComputerData::default()), Condvar::new())
        );
        let pair_clone = Arc::clone(&pair);

        tokio::spawn(async move {
            match ComputerData::default().get_computer_data().await {
                // sysinfo_tx
                Ok(data) => {
                    let (lock, cvar) = &*pair_clone;
                    let mut comp_data = lock.lock().unwrap();
                    *comp_data = data;
                    log::info!("Computer Data: {comp_data:?}");
                    cvar.notify_one();
                }
                Err(e) => log::error!("Error getting specs: {e:?}"),
            }
        });

        // Wait for the spawned task to complete and notify the condition variable
        let (lock, cvar) = &*pair;
        let mut comp_data = lock.lock().unwrap();
        while comp_data.cpu.is_empty() {
            comp_data = cvar.wait(comp_data).unwrap();
        }

        Self {
            task_data: Default::default(),
            ticket_data: Default::default(),
            customer_data: Default::default(),
            computer_data: comp_data.clone(),
            task_notes: Default::default(),
            send_specs: true,
            // client: Client::new(),
        }
    }
    
    pub fn receive(&mut self, presta_data: PrestashopPayload) {
        log::info!("{:?}", serde_json::to_value(&presta_data).unwrap_or_default());
        let customer = &mut self.customer_data;
        let ticket = &mut self.ticket_data;
        let task = &mut self.task_data;
        let task_notes = &mut self.task_notes;

        let service_details = presta_data.order.associations.order_service.clone();
        let mut services: Vec<RecordId> = Vec::new();

        let sales_rep = presta_data.sales_rep.clone().unwrap_or_default();
        let split_rep = presta_data.split_rep.clone().unwrap_or_default();

        let sales_rep_initials = sales_rep.initials.clone();
        let split_initials = split_rep.initials.clone();

        let email = sales_rep
            .email
            .split_once("@")
            .clone()
            .unwrap_or((&sales_rep_initials, ""))
            .0
            .to_string();

        let email_split_rep = split_rep
            .email
            .split_once("@")
            .clone()
            .unwrap_or((&split_initials, ""))
            .0
            .to_string();

        for msg in presta_data.customer_messages.iter() {
            task_notes.push(TaskNotePayload {
                everest_initials: msg.id_employee.clone(),
                note: msg.message.clone(),
                ..Default::default()
            })
        }

        customer.id = presta_data.customer.id.clone();
        customer.cust_code = presta_data.customer.cust_code.clone();
        customer.email = presta_data.customer.email.clone();
        customer.name = presta_data.customer.name.clone();
        customer.phone_number = presta_data.customer.phone_number.clone();
        ticket.salesman = email_split_rep;
        ticket.sales_rep = email.clone();
        ticket.tech = email.clone();
        log::info!(
            "Salesman: {:?}\nTech: {:?}",
            ticket.salesman.clone(),
            ticket.tech.clone()
        );
        ticket.customer = Some(customer.clone());
        ticket.checkin_rep = email;
        ticket.terms = presta_data.order.payment.clone();
        ticket.ticket_total = presta_data.order.total_products_wt.clone();
        ticket.doc_alias = presta_data.order.order_type.clone();
        ticket.service_number = presta_data.order.id.clone();
        ticket.id = RecordId::from((
            TICKET_TABLE.to_string(),
            ticket.service_number.clone(),
        ));

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

        task.service_ticket = Some(ticket.clone());
    }
    
    pub fn get_ticket(&self) {
        let input = self.ticket_data.service_number.clone();
        if !input.is_empty() {
            let tx = get_event_sender();
            tokio::spawn(async move {
                let prestashop_order = get_prestashop_payload(&input).await?;
                tx.try_send(WidgetEvent::Api(ApiEvent::GetTicketResponse(prestashop_order)))?;
                Ok::<(), anyhow::Error>(())
            });
        }
    }

    pub fn submit_tur_mastertech(&mut self) {
        let mut task_data = self.task_data.clone();
        let customer_data = self.customer_data.clone();
        let ticket_data = self.ticket_data.clone();
        let computer_data = self.computer_data.clone();
        let task_notes = self.task_notes.clone();

        task_data.due_date = DateTime::<Utc>::default().to_rfc3339_opts(SecondsFormat::Secs, true);
        let send_specs = self.send_specs.clone();
        tokio::spawn(async move {
            let send_payload_result = create_full_task_payload(
                ticket_data.into(),
                customer_data,
                computer_data,
                task_data.into(),
                task_notes,
                send_specs,
            )
            .await;
            log::info!("send_payload_result: {send_payload_result:?}");
        });
    }

    
}


// fn test_fn<T, R>(&mut self, f: impl FnMut(&mut T) -> R) { f(|t: &mut T| {}); }
// fn another(&mut self) { let x = self.test_fn::<ServiceData, bool>(|x| { true }); }