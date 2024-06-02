use database::{schema::TaskPayload, Database};
use egui::{Align, Grid, Id, Layout, RichText, ScrollArea, Ui};
use egui_extras::{Size, StripBuilder};
use log::info;
use serde::Serialize;

use crate::utilities::{DisplayModal, ModalTypes};

use super::modals::ModalState;

#[derive(Serialize, Clone, Debug)]
pub struct TaskModal{
    pub ticket_info_page: bool,
    pub computer_info_page: bool,
    pub task_notes_page: bool,
    pub part_order_page: bool,
    #[serde(skip)]
    pub database: Option<Database>, 
    #[serde(skip)]
    pub task: Option<TaskPayload>,

    pub min_width: Option<f32>,
    pub min_height: Option<f32>,
    pub default_height: Option<f32>,
    pub full_span_content: bool,
    pub state: ModalState
}

#[derive(Debug)]
pub enum ModalAction{
    TicketInfoPage,
    PartOrderPage,
    ComputerInfoPage,
    None
}

impl Default for TaskModal{
    fn default() -> Self {
        Self {
            ticket_info_page: true,
            computer_info_page: false,
            task_notes_page: false,
            part_order_page: false,

            database: None,
            task: None,

            min_width: Some(600.0),
            min_height: Some(600.0),
            default_height: Some(800.0),
            full_span_content: false,
            state: ModalState::default(),
        }
    }
}

impl ModalTypes for TaskModal{
    fn modal_state(&mut self) -> &mut ModalState {
        &mut self.state
    }
    fn title(&self) -> String {
        "Task Details".to_string()
    }
}


impl DisplayModal for TaskModal {
    fn display(&self, ui: &mut Ui) -> Option<ModalAction>{
        let mut response: Option<ModalAction> = None;

        StripBuilder::new(ui)
            .cell_layout(Layout::top_down_justified(Align::Center))
            .size(Size::exact(30.0))
            .size(Size::exact(10.0))
            .size(Size::exact(700.0))
            .vertical(|mut strip| 
        {
            strip
                .strip(|strip| 
            {
                strip
                    .size(Size::remainder())
                    .horizontal( |mut strip| 
                {
                    strip.cell(|ui|{
                        ui.horizontal(|ui|{
                            info!("OUTSIDE computer_info_page: {:?}", self.computer_info_page);

                            if ui.selectable_label(self.ticket_info_page, RichText::new("").heading()).clicked(){
                                
                            };
                            if ui.selectable_label(self.part_order_page, RichText::new("⛨").heading()).clicked(){
                                
                            };
                            if ui.selectable_label(self.state.computer_info_page, RichText::new("").heading()).clicked(){
                                response = Some(ModalAction::ComputerInfoPage);
                            };
                        });
                    });
                    
                });
            });
            strip.empty();
            strip
                .strip(|strip| 
            {
                strip
                    .size(Size::remainder())
                    .horizontal( |mut strip| 
                {
                    strip.cell(|ui|
                    {

                        ScrollArea::both()
                            .id_source("ticketScroll")
                            // .max_height(ui.available_height())
                            .show(ui, |ui| 
                        {
                    
                            Grid::new(Id::new(format!("Grid")))// self.id.as_ref().unwrap().0.id.clone()
                                .num_columns(6)
                                .show(ui, |ui| 
                            {
                    
                                if let Some(task) = self.task.as_ref(){
                                    let ticket = task.service_ticket.as_ref().unwrap();
                                    
                                    let customer = ticket.customer.as_ref();
                                    let computer = ticket.computer.as_ref();
                                    
                                    ui.label(format!("created_at: {:?}", ticket.created_at));
                                    ui.label(format!("id: {:?}", ticket.id));
                                    ui.label(format!("service_task: {:?}", ticket.service_task));
                                    ui.label(format!("service_number: {:?}", ticket.service_number));
                                    ui.label(format!("checkin_rep: {:?}", ticket.checkin_rep));
                                    ui.label(format!("sales_rep: {:?}", ticket.sales_rep));
                                    ui.end_row();
                                    ui.label(format!("checkin_notes: {:?}", ticket.checkin_notes));
                                    ui.label(format!("recommendations: {:?}", ticket.recommendations));
                                    ui.label(format!("tech: {:?}", ticket.tech));
                                    ui.label(format!("salesman: {:?}", ticket.salesman));
                                    ui.label(format!("dep: {:?}", ticket.dep));
                                    ui.label(format!("terms: {:?}", ticket.terms));
                                    ui.end_row();
                                    ui.label(format!("ticket_total: {:?}", ticket.ticket_total));
                                    ui.label(format!("doc_alias: {:?}", ticket.doc_alias));
                                    ui.label(format!("current_antivirus: {:?}", ticket.current_antivirus));
                                    // ui.label(format!("hardware_test_results: {:?}", ticket.hardware_test_results));
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
                                }
                            });
                        });
                    });
                });
            });
        });
        response
    }

    fn set_state(mut self, action: ModalAction){
        match action{
            ModalAction::TicketInfoPage => {
                self.ticket_info_page = true;
            },
            ModalAction::PartOrderPage => {
                self.part_order_page = true;
            },
            ModalAction::ComputerInfoPage => {
                self.ticket_info_page = false;
                self.computer_info_page = true;
            },
            ModalAction::None => (),
        }
    }
}
