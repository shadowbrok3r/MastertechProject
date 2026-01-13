use database::schema::{CarboniteResponse, DriveData, RecordId, TASK_TABLE, TICKET_TABLE, COMPUTER_TABLE, TaskNotePayload, helper_traits::parse_email_user, prestashop::OrderType};
use database::schema::prestashop_schema::PrestashopPayload;
use database::schema::prestashop::order::ExtractedOrderSpecs;
use database::ReqwestClient;
use crate::{app_state::SharedContext, modals::ModalType, PlatformSpawner, Spawner};
use crossbeam::channel::Sender;
use log::info;
use uuid::Uuid;

impl SharedContext {
    pub fn receive_prestashop(&mut self) {
        if let Ok(data) = self.tur_channel.1.try_recv() {
            self.tur.data = data.clone();

            info!("SharedContext -> receive_prestashop -> {:?}", self.tur.data.clone());
            let customer = &mut self.tur.customer_data;
            let computer = &mut self.tur.computer_data;
            let ticket = &mut self.tur.ticket_data;
            let task = &mut self.tur.task_data;
            let task_notes = &mut self.tur.task_notes;
            let service_details = data.order.associations.order_service.clone();
            let sales_rep = data.sales_rep.clone().unwrap_or_default();
            let split_rep = data.split_rep.clone().unwrap_or_default();
            let email = parse_email_user(&sales_rep.email).to_string();
            let email_split_rep = parse_email_user(&split_rep.email).to_string();
            let customer_email = data.customer.email.clone();
            let client = ReqwestClient::new();
            let carobonite_tx = self.seb_channel.0.clone();
            let mut services: Vec<RecordId> = Vec::new();

            task.id = RecordId::new(TASK_TABLE, Uuid::new_v4().to_string());

            // Spawn async task for SEB check
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

            // Use the Order's extraction methods for computer specs
            // First, set the device model using the extract_model method (sync)
            let model = data.order.extract_model();
            if !model.is_empty() {
                computer.device_model = Some(model);
            }
            
            // Extract drives using Order's method
            for (name, drive_type) in data.order.extract_drives() {
                computer.add_disk(DriveData {
                    drive_letter: name.clone(),
                    drive_type: drive_type.clone(),
                    total_size: name.split('/').last().unwrap_or("").to_string(),
                    space_left: "".to_string(),
                });
            }
            
            // Extract motherboard
            if let Some(mb) = data.order.extract_motherboard() {
                computer.motherboard_name = mb;
            }
            
            // Extract OS
            if let Some(os) = data.order.extract_os() {
                computer.operating_system = os;
            }
            
            // Get device info from service details if available (for non-sales orders like repairs)
            if let Some(service) = service_details.first() {
                if computer.device_mfg.is_none() || computer.device_mfg.as_ref().map(|s| s.is_empty()).unwrap_or(true) {
                    computer.device_mfg = Some(service.device_mfg.clone());
                }
                // Only use service device_model if we didn't find a proper model from order rows
                if computer.device_model.is_none() || computer.device_model.as_ref().map(|s| s.is_empty()).unwrap_or(true) {
                    computer.device_model = Some(service.device_model.clone());
                }
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
            ticket.id = RecordId::new(TICKET_TABLE, ticket.service_number.clone());
            log::info!("Salesman: {:?}\nTech: {:?}",ticket.salesman.clone(),ticket.tech.clone());
            services.push(ticket.id.clone());
            if let Some(service) = service_details.first() {
                ticket.checkin_notes = service.check_in_notes.clone();
            }
            
            // Check if we have computer data and set ticket.computer reference
            if !computer.device_mfg.is_none() || !computer.device_serial.is_none() || !computer.cpu.is_empty() || !computer.gpu.is_empty() || !computer.ram.is_empty() {
                ticket.computer = Some(computer.id.clone());
            }

            task.service_ticket = Some(ticket.id.clone());
            
            // Spawn async task to extract CPU/GPU/RAM/Serial/MFG using the Order's async method
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
            info!("Received extracted specs: cpu='{}', gpu='{}', ram='{}', serial='{}', mfg='{}'", 
                  specs.cpu, specs.gpu, specs.ram, specs.device_serial, specs.device_mfg);
            let computer = &mut self.tur.computer_data;
            
            if !specs.cpu.is_empty() {
                computer.cpu = specs.cpu;
            }
            if !specs.gpu.is_empty() {
                computer.gpu = specs.gpu;
            }
            if !specs.ram.is_empty() {
                computer.ram = specs.ram;
            }
            if !specs.device_serial.is_empty() {
                computer.id = RecordId::new(COMPUTER_TABLE, specs.device_serial.clone());
                computer.device_serial = Some(specs.device_serial);
            }
            if !specs.device_mfg.is_empty() {
                computer.device_mfg = Some(specs.device_mfg);
            }

            self.tur.ticket_data.computer = Some(computer.id.clone());

            // Update modal if open
            for (title, modal) in self.opened_modals.iter_mut() {
                if let ModalType::CreateTaskModal(create_task_modal) = modal {
                    info!("Updating modal with extracted specs for {title}");
                    create_task_modal.update_tur_info(self.tur.clone());
                }
            }
        }
    }
}

