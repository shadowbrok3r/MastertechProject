use database::schema::{helper_traits::parse_email_user, CarboniteResponse, TaskNotePayload, TASK_TABLE, TICKET_TABLE};
use crate::{app_state::SharedContext, modals::ModalType, PlatformSpawner, Spawner};
use log::info;

impl SharedContext {
    pub fn receive_prestashop(&mut self) {
        if let Ok(data) = self.tur_channel.1.try_recv() {
            self.tur.data = data.clone();

            info!("SharedContext -> receive_prestashop -> {:?}", self.tur.data.clone());
            let customer = &mut self.tur.customer_data;
            let ticket = &mut self.tur.ticket_data;
            let task = &mut self.tur.task_data;
            let task_notes = &mut self.tur.task_notes;
            let service_details = data.order.associations.order_service.clone();
            let sales_rep = data.sales_rep.clone().unwrap_or_default();
            let split_rep = data.split_rep.clone().unwrap_or_default();
            let email = parse_email_user(&sales_rep.email).to_string();
            let email_split_rep = parse_email_user(&split_rep.email).to_string();
            let customer_email = data.customer.email.clone();
            let client = reqwest::Client::new();
            let carobonite_tx = self.seb_channel.0.clone();
            let mut services: Vec<surrealdb::RecordId> = Vec::new();

            task.id = surrealdb::RecordId::from((TASK_TABLE, surrealdb::RecordIdKey::from_inner(surrealdb::sql::Id::rand().into())));

            PlatformSpawner::spawn(async move {
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

            for msg in data.task_notes.iter() {
                task_notes.push(TaskNotePayload {
                    task_id: Some(task.id.clone()),
                    ..msg.clone()
                });
            }

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
            ticket.id = surrealdb::RecordId::from((TICKET_TABLE.to_string(), ticket.service_number.clone() ));
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

            task.service_ticket = Some(ticket.id.clone());

            for (title, modal) in self.opened_modals.iter_mut() {
                if let ModalType::CreateTaskModal(create_task_modal) = modal {
                    info!("Updating modal data for {title}");
                    create_task_modal.update_tur_info(self.tur.clone());
                }
            }
        }
    }
}

