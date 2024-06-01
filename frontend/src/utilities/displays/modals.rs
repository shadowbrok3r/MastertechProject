use database::{schema::{TaskPayload, TicketPayload}, Database};
use egui::{Grid, Id, RichText, ScrollArea, Sense, Ui};
use log::info;
use serde::Serialize;

use crate::utilities::Displayable;

#[derive(Serialize, Default, Clone)]
pub enum ModalType{
    CreateTaskModal,
    TaskModal(String),
    #[default]
    Null,
}


impl ModalType{
    pub fn create_task_modal(&mut self, _ui: &mut Ui){
        info!("Creating a task!!");
    }
    pub fn task_modal(&mut self, ui: &mut Ui, database: Database, task: &TaskPayload, ticket_payload: &TicketPayload){
        ui.allocate_ui(ui.available_size(), |ui|{

            task_modal(ui, database, task, ticket_payload);
        });

    }
    pub fn other(&mut self, _ui: &mut Ui){
        info!("No modal...");
    }
}

fn task_modal(ui: &mut Ui, database: Database, task: &TaskPayload, ticket_payload: &TicketPayload){


    ScrollArea::both()
        .id_source("ticketScroll")
        .max_height(ui.available_height())
        .show(ui, |ui| 
    {
        ui
            .vertical(|ui| 
        {
            Grid::new(Id::new(format!("Grid")))// self.id.as_ref().unwrap().0.id.clone()
                .num_columns(6)
                .show(ui, |ui| 
            {

                let customer = ticket_payload.customer.as_ref();
                let computer = ticket_payload.computer.as_ref();
                
                ui.label(format!("created_at: {:?}", ticket_payload.created_at));
                ui.label(format!("id: {:?}", ticket_payload.id));
                ui.label(format!("service_task: {:?}", ticket_payload.service_task));
                ui.label(format!("service_number: {:?}", ticket_payload.service_number));
                ui.label(format!("checkin_rep: {:?}", ticket_payload.checkin_rep));
                ui.label(format!("sales_rep: {:?}", ticket_payload.sales_rep));
                ui.end_row();
                ui.label(format!("checkin_notes: {:?}", ticket_payload.checkin_notes));
                ui.label(format!("recommendations: {:?}", ticket_payload.recommendations));
                ui.label(format!("tech: {:?}", ticket_payload.tech));
                ui.label(format!("salesman: {:?}", ticket_payload.salesman));
                ui.label(format!("dep: {:?}", ticket_payload.dep));
                ui.label(format!("terms: {:?}", ticket_payload.terms));
                ui.end_row();
                ui.label(format!("ticket_total: {:?}", ticket_payload.ticket_total));
                ui.label(format!("doc_alias: {:?}", ticket_payload.doc_alias));
                ui.label(format!("current_antivirus: {:?}", ticket_payload.current_antivirus));
                // ui.label(format!("hardware_test_results: {:?}", ticket_payload.hardware_test_results));
                ui.end_row();

                if let Some(customer) = customer{

                    ui.label(format!("part_order_links: {:?}", customer.part_order_links));
                    ui.label(format!("services: {:?}", customer.services));
                    ui.label(format!("cust_code: {:?}", customer.cust_code));
                    ui.label(format!("name: {:?}", customer.name));
                    ui.label(format!("phone_number: {:?}", customer.phone_number));
                    ui.label(format!("phone_number_2: {:?}", customer.phone_number_2));
                    ui.end_row();
                    ui.label(format!("email: {:?}", customer.email));
                    ui.label(format!("li_doc: {:?}", customer.li_doc));
                    ui.label(format!("li_amnt: {:?}", customer.li_amnt));
                    ui.label(format!("num_inv: {:?}", customer.num_inv));
                    ui.end_row();
                }
                ui.end_row();
                if let Some(computer) = computer{
                    let seb_info = computer.seb_info.as_ref();
                    ui.label(format!("hostname: {:?}", computer.hostname));
                    ui.label(format!("operating_system: {:?}", computer.operating_system));
                    ui.label(format!("cpu: {:?}", computer.cpu));
                    ui.label(format!("gpu: {:?}", computer.gpu));
                    ui.label(format!("ram: {:?}", computer.ram));
                    ui.label(format!("drives: {:?}", computer.drives));
                    ui.end_row();

                    if let Some(seb_info) = seb_info{
                        ui.label(format!("InstalledDeviceId: {:?}", seb_info.InstalledDeviceId));
                        ui.label(format!("InstallInstanceId: {:?}", seb_info.InstallInstanceId));
                        ui.label(format!("HasIssues: {:?}", seb_info.HasIssues));
                        ui.label(format!("InstallationStage: {:?}", seb_info.InstallationStage));
                        ui.label(format!("ReasonCode: {:?}", seb_info.ReasonCode));
                        ui.label(format!("ActivationCode: {:?}", seb_info.ActivationCode));
                        ui.end_row();
                        ui.label(format!("InstallVersion: {:?}", seb_info.InstallVersion));
                        ui.label(format!("MachineName: {:?}", seb_info.MachineName));
                        ui.end_row();

                        if let Some(extended_seb) = seb_info.ExtendedSeb.as_ref(){
                            ui.label(format!("email: {:?}", extended_seb.email));
                            ui.label(format!("phone: {:?}", extended_seb.phone));
                            ui.label(format!("userid: {:?}", extended_seb.userid));
                            ui.label(format!("device_name: {:?}", extended_seb.device_name));
                            ui.label(format!("device_id: {:?}", extended_seb.device_id));
                            ui.label(format!("state: {:?}", extended_seb.state));
                            ui.end_row();
                            ui.label(format!("usage_gb: {:?}", extended_seb.usage_gb));
                            ui.label(format!("date_device_created: {:?}", extended_seb.date_device_created));
                            ui.label(format!("activated: {:?}", extended_seb.activated));
                            ui.label(format!("activation_code: {:?}", extended_seb.activation_code));
                            ui.label(format!("last_complete_backup: {:?}", extended_seb.last_complete_backup));
                            ui.label(format!("last_client_status_update: {:?}", extended_seb.last_client_status_update));
                            ui.end_row();
                            ui.label(format!("id_recurly_account: {:?}", extended_seb.id_recurly_account));
                            ui.label(format!("date_last_scan: {:?}", extended_seb.date_last_scan));
                            ui.label(format!("date_email_sent: {:?}", extended_seb.date_email_sent));
                            ui.label(format!("date_canceled_account: {:?}", extended_seb.date_canceled_account));
                            ui.label(format!("date_deleted_account: {:?}", extended_seb.date_deleted_account));
                            ui.end_row();
                            ui.label(format!("current_period_ends_at: {:?}", extended_seb.current_period_ends_at));
                            ui.label(format!("date_modified: {:?}", extended_seb.date_modified));
                            ui.label(format!("date_created: {:?}", extended_seb.date_created));
                            ui.end_row();
                        }
                    }
                }
            }); 
        });
    });

}

