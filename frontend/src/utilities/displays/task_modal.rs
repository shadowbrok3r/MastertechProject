
use chrono::{Date, DateTime, Utc};
use database::{schema::TaskPayload, Database};
use egui::{Align, Color32, Direction, Grid, Id, Layout, Margin, RichText, ScrollArea, Style, TextEdit, Ui, Widget};
use egui_extras::{Size, StripBuilder};
use log::info;
use serde::Serialize;

use crate::utilities::{DisplayModal, ModalTypes};

use super::modals::ModalState;

#[derive(Serialize, Clone, Debug)]
pub struct TaskModal{
    pub title: String,
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

#[derive(Debug, Clone, Serialize, Default)]
pub enum ModalAction{
    TicketInfoPage,
    PartOrderPage,
    ComputerInfoPage,
    TaskNotePage,
    #[default]
    None
}

impl Default for TaskModal{
    fn default() -> Self {
        Self {
            title: "Task Details".to_string(),
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
    fn title(mut self, title: String) -> Self {
        self.modal_state().title = Some(title);
        self
    }
}


impl DisplayModal for TaskModal {
    fn display(&self, ui: &mut Ui, current_page_state: ModalAction) -> Option<ModalAction>{
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
                            
                            let mut ticket_page = false;
                            let mut part_order_page = false;
                            let mut computer_info_page = false;
                            let mut task_note_page = false;
                            match current_page_state{
                                ModalAction::TicketInfoPage => {ticket_page = true},
                                ModalAction::PartOrderPage => {part_order_page = true},
                                ModalAction::ComputerInfoPage => {computer_info_page = true},
                                ModalAction::TaskNotePage => { task_note_page = true },
                                _ => {ticket_page = true},
                            };

                            if ui.selectable_label(ticket_page, RichText::new("🖹").heading()).clicked(){
                                response = Some(ModalAction::TicketInfoPage);
                            };
                            if ui.selectable_label(computer_info_page, RichText::new("🖥").heading()).clicked(){
                                response = Some(ModalAction::ComputerInfoPage);
                            };
                            if ui.selectable_label(part_order_page, RichText::new("🔫").heading()).clicked(){
                                response = Some(ModalAction::PartOrderPage);
                            };
                            if ui.selectable_label(task_note_page, RichText::new("💬").heading()).clicked(){
                                response = Some(ModalAction::TaskNotePage);
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
                    .size(Size::exact(500.0))
                    .horizontal( |mut strip| 
                {
                    strip.cell(|ui|
                    {
                        match current_page_state{
                            ModalAction::TicketInfoPage => display_task_page(ui, self.task.as_ref()),
                            ModalAction::ComputerInfoPage => display_computer_page(ui, self.task.as_ref()),
                            ModalAction::PartOrderPage => display_part_order_page(ui),
                            _ => display_task_page(ui, self.task.as_ref()),
                        };

                        
                    });
                });
            });
        });
        response
    }
}


fn display_task_page(ui: &mut Ui, task: Option<&TaskPayload>){
    fn return_colors(num: usize, style: &Style) -> Option<Color32>{
        let mut col = Color32::from_rgb(30, 30, 38);

        if num == 2{
            col = Color32::DARK_GREEN;
        }else if num == 1{
            col = Color32::DARK_BLUE;
        }else if num == 0{
            col = Color32::GOLD;
        }
        Some(col)
    }

    if let Some(task) = task.as_ref(){
        let ticket = task.service_ticket.as_ref().unwrap();
        let customer = ticket.customer.as_ref();
        
        StripBuilder::new(ui)
            .size(Size::exact(100.0))
            .size(Size::exact(115.0))
            .size(Size::exact(20.0))
            .size(Size::exact(100.0))
            .vertical(|mut strip| 
        {

            strip.strip(|s|{
                s
                    .size(Size::exact(300.0))
                    .size(Size::exact(10.0))
                    .size(Size::exact(300.0))
                    .horizontal(|mut s|
                {
                    s.cell(|ui|{
                        ui.group(|ui| {
                            // ui.label("Personnel Information");
                            Grid::new("group2").min_col_width(150.0).with_row_color(|num, style| return_colors(num, style))
                            .show(ui, |ui| {
                                ui.label("Technician:");
                                ui.label(&ticket.tech);
                                ui.end_row();

                                ui.label("Salesman:");
                                ui.label(&ticket.salesman);
                                ui.end_row();

                                ui.label("Split Rep:");
                                ui.label(&ticket.sales_rep);
                                ui.end_row();

                                ui.label("Checkin Rep:");
                                ui.label(&ticket.checkin_rep);
                            });
                        });

                    });
                    s.empty();
                    s.cell(|ui| {
                        ui.group(|ui| {
                            // ui.label("Ticket Information");
                            Grid::new("group1").min_col_width(150.0).with_row_color(|num, style| return_colors(num, style))
                            .show(ui, |ui| {
                                ui.label("SO#:");
                                ui.label(format!("{}", ticket.service_number));
                                ui.end_row();
                                
                                ui.label("Tur Sent:");
                                ui.label(ticket.created_at.as_ref().unwrap().parse::<DateTime<Utc>>().unwrap().date_naive().to_string());
                                ui.end_row();

                                ui.label("Store:");
                                ui.label(&ticket.dep);
                                ui.end_row();
                                ui.label("");
                            });
                        });
                    });
                });
            });
            strip.strip(|s|{
                s
                    .size(Size::exact(300.0))
                    .size(Size::exact(10.0))
                    .size(Size::exact(300.0))
                    .horizontal(|mut s|
                {
                    s.cell(|ui|{
                        ui.group(|ui| {
                            // ui.label("Order Details");
                            Grid::new("group3").min_col_width(150.0).with_row_color(|num, style| return_colors(num, style))
                            .show(ui, |ui| {
                                ui.label("Terms:");
                                ui.label(&ticket.terms);
                                ui.end_row();

                                ui.label("Total on Order:");
                                ui.label(&ticket.ticket_total);
                                ui.end_row();

                                ui.label("Order Type:");
                                ui.label(&ticket.doc_alias);
                                ui.end_row();
                                ui.label("");
                                ui.end_row();
                                ui.label("");
                            });
                        });
                    });
                    s.empty();
                    s.cell(|ui|{
                        if let Some(customer) = &customer {
                            ui.group(|ui| {
                                // ui.label("Customer Information");
                                Grid::new("customer_data").min_col_width(150.0).with_row_color(|num, style| return_colors(num, style))
                                .show(ui, |ui| {
                                    // ui.label("Other Services:");
                                    // ui.with_layout(Layout::centered_and_justified(Direction::LeftToRight), |ui| {
                                    //     ui.label(&customer.services.as_ref().unwrap());
                                    // });
                                    // ui.end_row();

                                    ui.label("Customer Code:");
                                    ui.label(format!("{}", customer.cust_code));
                                    ui.end_row();

                                    ui.label("Customer Name:");
                                    ui.label(&customer.name);
                                    ui.end_row();

                                    ui.label("Phone #:");
                                    ui.label(&customer.phone_number);
                                    ui.end_row();

                                    ui.label("2nd Phone #:");
                                    ui.label(&customer.phone_number_2);
                                    ui.end_row();

                                    ui.label("Customer Email:");
                                    ui.label(&customer.email);
                                    // ui.label("SPO Links:");
                                    // ui.with_layout(Layout::centered_and_justified(Direction::LeftToRight), |ui| {
                                    //     ui.label(&customer.part_order_links);
                                    // });
                                    // ui.end_row();
                                });
                            });
                        }
                    });
                });
            });
            strip.empty();
            strip.strip(|s|{
                s
                    .size(Size::exact(150.0))
                    .vertical(|mut s|
                {
                    s.strip(|s|{
                        s
                            .size(Size::exact(300.0))
                            .size(Size::exact(10.0))
                            .size(Size::exact(300.0))
                            .horizontal(|mut s|
                        {
                            s.cell(|ui|{
                                ui.label("Recommendations:");
                                    TextEdit::multiline(&mut ticket.recommendations.to_string())
                                    .margin(Margin::same(5.0))
                                    .desired_rows(8)
                                    .desired_width(ui.available_width())
                                    .code_editor()
                                    .ui(ui);
                            });
                            s.empty();
                            s.cell(|ui|{
                                ui.label("Checkin Notes:");
                                TextEdit::multiline(&mut ticket.checkin_notes.to_string())
                                .margin(Margin::same(5.0))
                                .desired_rows(8)
                                .desired_width(ui.available_width())
                                .frame(true)
                                .code_editor()
                                .ui(ui);
                                
                            });    
                        });
                    });
                });
            });

        });
    }
}

fn display_computer_page(ui: &mut Ui, task: Option<&TaskPayload>){
    ScrollArea::both()
        .id_source("ticketScroll")
        // .max_height(ui.available_height())
        .show(ui, |ui| 
    {

        Grid::new(Id::new(format!("Grid")))// self.id.as_ref().unwrap().0.id.clone()
            .num_columns(6)
            .show(ui, |ui| 
        {
            if let Some(task) = task.as_ref(){
                let ticket = task.service_ticket.as_ref().unwrap();
                
                let computer = ticket.computer.as_ref();
                if let Some(computer) = computer{
                    let seb_info = computer.seb_info.as_ref();
                    ui.label(format!("hostname: {:?}", computer.hostname));
                    ui.label(format!("operating_system: {:?}", computer.operating_system));
                    ui.label(format!("cpu: {:?}", computer.cpu));
                    ui.label(format!("gpu: {:?}", computer.gpu));
                    ui.label(format!("ram: {:?}", computer.ram));
                    ui.label(format!("drives: {:?}", computer.drives));
                    ui.label(format!("current_antivirus: {:?}", ticket.current_antivirus));
                    ui.label(format!("hardware_test_results: {:?}", ticket.hardware_test_results));
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
}

fn display_part_order_page(ui: &mut Ui){
    ui.label("New page");
}