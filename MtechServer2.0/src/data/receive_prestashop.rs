use crate::{
    app_state::{AppState, MtechServer},
    pages::downloads_page::get_github_releases,
    tabs::stock::{find_attached_serials, get_stock, BoolOrString, MyRowData},
    utilities::ModalType,
};
use database::{
    live_data::{handle_live_delete, listen_data, update_or_insert_anything},
    schema::{
        utilities::{get_connected_clients, get_store_users, get_tasks},
        TaskNotePayload, CONNECTED_CLIENT_TABLE, TASK_NOTE_TABLE, TASK_TABLE, TICKET_TABLE,
    },
    DATABASE,
};
use database::{schema::Store, STORAGE_URL};
use displays::ui_tools::toasts::{Toast, ToastKind, ToastOptions};
use eframe::{
    egui::{Color32, RichText},
    Frame,
};
use egui_dock::DockState;
use log::info;
use log::{debug, error};
use mtechserver::webworker::Input;
use surrealdb::{Action, RecordId};
use wasm_bindgen_futures::spawn_local;

impl MtechServer {
    pub fn receive_prestashop(&mut self) {
        if let Ok(presta_data) = self.context.tur_channel.1.try_recv() {
            self.context.tur.data = presta_data.clone();
            info!("{:?}", self.context.tur.data.clone());
            let customer = &mut self.context.tur.customer_data;
            let ticket = &mut self.context.tur.ticket_data;
            let task = &mut self.context.tur.task_data;
            let task_notes = &mut self.context.tur.task_notes;

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
            info!(
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
            ticket.id = Some(RecordId::from((
                TICKET_TABLE.to_string(),
                ticket.service_number.clone(),
            )));

            if let Some(ticket_id) = &ticket.id {
                services.push(ticket_id.clone());
            }

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

            task.service_ticket = Some(ticket.clone());

            if let ModalType::CreateTaskModal(ref mut create_task_modal) =
                self.context.current_modal
            {
                info!("Updating modal data");
                info!("{:?}", ticket.clone());
                info!("{:?}", customer.clone());
                info!("{:?}", task_notes.clone());
                create_task_modal.tur = self.context.tur.clone();
            }
        }

    }
}