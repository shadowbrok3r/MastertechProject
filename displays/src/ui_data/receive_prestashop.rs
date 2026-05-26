use database::schema::{
    CarboniteResponse, EntityDraft, PrestaMapMode, PrestaMapOptions, apply_extracted_specs,
    apply_prestashop_payload,
};
use reqwest::Client;
use crate::{app_state::SharedContext, modals::ModalType, PlatformSpawner, Spawner};
use log::info;

impl SharedContext {
    pub fn receive_prestashop(&mut self) {
        if let Ok(data) = self.tur_channel.1.try_recv() {
            self.tur.data = data.clone();
            info!("SharedContext -> receive_prestashop -> {:?}", self.tur.data.clone());

            let customer_email = data.customer.email.clone();
            let client = Client::new();
            let carobonite_tx = self.seb_channel.0.clone();

            let mut draft = EntityDraft {
                customer: self.tur.customer_data.clone(),
                ticket: self.tur.ticket_data.clone(),
                computer: self.tur.computer_data.clone(),
                task: self.tur.task_data.clone(),
                task_notes: self.tur.task_notes.clone(),
            };
            apply_prestashop_payload(
                &data,
                &mut draft,
                &PrestaMapOptions {
                    mode: PrestaMapMode::Web,
                    guard_placeholder_computer: true,
                    ..Default::default()
                },
            );
            self.tur.customer_data = draft.customer;
            self.tur.ticket_data = draft.ticket;
            self.tur.computer_data = draft.computer;
            self.tur.task_data = draft.task;
            self.tur.task_notes = draft.task_notes;

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

            let order_clone = data.order.clone();
            let specs_tx = self.specs_channel.0.clone();
            PlatformSpawner::spawn(async move {
                let specs = order_clone.extract_specs().await;
                let _ = specs_tx.try_send(specs);
            });

            for (title, modal) in self.opened_modals.iter_mut() {
                if let ModalType::CreateTaskModal(create_task_modal) = modal {
                    info!("Updating modal data for {title}");
                    create_task_modal.update_tur_info(self.tur.clone());
                }
            }
        }
    }
    
    /// Receive extracted specs from async extraction
    pub fn receive_extracted_specs(&mut self) {
        if let Ok(specs) = self.specs_channel.1.try_recv() {
            info!(
                "Received extracted specs: cpu='{}', gpu='{}', ram='{}', serial='{}', mfg='{}'",
                specs.cpu, specs.gpu, specs.ram, specs.device_serial, specs.device_mfg
            );
            let mut draft = EntityDraft {
                customer: self.tur.customer_data.clone(),
                ticket: self.tur.ticket_data.clone(),
                computer: self.tur.computer_data.clone(),
                task: self.tur.task_data.clone(),
                task_notes: self.tur.task_notes.clone(),
            };
            apply_extracted_specs(&mut draft, &specs);
            self.tur.computer_data = draft.computer;
            self.tur.ticket_data = draft.ticket;

            for (title, modal) in self.opened_modals.iter_mut() {
                if let ModalType::CreateTaskModal(create_task_modal) = modal {
                    info!("Updating modal with extracted specs for {title}");
                    create_task_modal.update_tur_info(self.tur.clone());
                }
            }
        }
    }
}
