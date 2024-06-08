use std::{borrow::BorrowMut, cell::RefCell, rc::Rc};

use chrono::{DateTime, Utc};
use database::{schema::TaskPayload, Database};
use egui::{Align, Color32, ComboBox, FontId, Frame, Grid, Layout, Margin, RichText, ScrollArea, Stroke, Style, TextEdit, Ui, Widget};
use egui_extras::{Size, StripBuilder};
use log::info;
use serde::Serialize;

use crate::utilities::{DisplayModal, ModalTypes};

use super::{chats::ChatModal, modals::ModalState};

#[derive(Serialize, Clone, Debug)]
pub struct TaskModal{
    pub title: String,
    #[serde(skip)]
    pub database: Option<Database>, 
    #[serde(skip)]
    pub task: Option<TaskPayload>,
    #[serde(skip)]
    pub chat_view: Option<Rc<RefCell<ChatModal>>>,
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
            database: None,
            task: None,
            min_width: Some(600.0),
            min_height: Some(600.0),
            default_height: Some(800.0),
            full_span_content: false,
            state: ModalState::default(),
            chat_view: None
        }
    }
}

impl TaskModal{
    pub fn new(chats: ChatModal) -> Self {
        Self {
            title: "Task Details".to_string(),
            database: None,
            task: None,
            min_width: Some(600.0),
            min_height: Some(600.0),
            default_height: Some(800.0),
            full_span_content: false,
            state: ModalState::default(),
            chat_view: Some(Rc::new(RefCell::new(chats)))
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
    fn display(&mut self, ui: &mut Ui, current_page_state: ModalAction) -> Option<ModalAction>{
        let mut response: Option<ModalAction> = None;
        ui.style_mut().visuals.selection.stroke.color =  Color32::BLACK;
        ui.style_mut().visuals.selection.bg_fill = Color32::from_rgb(120, 10, 120);
        ui.style_mut().visuals.widgets.inactive.bg_fill =  Color32::GOLD;
        ui.style_mut().visuals.widgets.inactive.fg_stroke =  Stroke::new(1.0, Color32::WHITE);
        ui.style_mut().visuals.widgets.inactive.weak_bg_fill =  Color32::from_rgb(20, 20, 25);
        ui.style_mut().visuals.widgets.inactive.bg_stroke =  Stroke::new(1.0, Color32::from_rgb(80, 80, 80));
        ui.style_mut().visuals.widgets.open.bg_fill =  Color32::from_black_alpha(50);
        ui.style_mut().visuals.widgets.open.weak_bg_fill =  Color32::from_black_alpha(50);
        ui.style_mut().visuals.widgets.active.weak_bg_fill =  Color32::from_rgb(30,30,30);
        ui.style_mut().visuals.widgets.hovered.weak_bg_fill =  Color32::TRANSPARENT;
        ui.style_mut().visuals.widgets.hovered.bg_fill =  Color32::from_rgb(12, 12, 12);
        ui.style_mut().visuals.widgets.hovered.bg_stroke =  Stroke::new(1.0, Color32::from_rgb(200, 20, 200));
        
        StripBuilder::new(ui)
            .cell_layout(Layout::top_down_justified(Align::Center))
            .size(Size::exact(30.0))
            .size(Size::exact(10.0))
            .size(Size::relative(0.8))
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
                            ModalAction::TicketInfoPage => {
                                ui.horizontal_centered(|ui|{
                                    display_task_page(ui, self.task.as_ref())
                                });
                            },
                            ModalAction::ComputerInfoPage => {
                                ui.horizontal_centered(|ui|{
                                    display_computer_page(ui, self.task.as_ref())
                                });
                            },
                            ModalAction::PartOrderPage => {
                                ui.horizontal_centered(|ui|{
                                    display_part_order_page(ui)
                                });
                            },
                            ModalAction::TaskNotePage => {
                                if let Some(chat_view) = &self.chat_view{
                                    info!("Got chat_view");
                                    display_chat_page(ui, &mut chat_view.take());
                                }
                            },
                            _ => display_task_page(ui, self.task.as_ref())
                        };
                        ui.shrink_width_to_current();
                        ui.shrink_height_to_current();
                    });
                });
            });
        });
        ui.shrink_width_to_current();
        ui.shrink_height_to_current();
        response
    }
}


fn display_task_page(ui: &mut Ui, task: Option<&TaskPayload>){
    fn return_colors(num: usize, _style: &Style) -> Option<Color32>{
        let mut _col = Color32::from_rgb(30, 30, 38);
        if num % 2 == 0{
            _col = Color32::from_rgb(15, 15, 22);
        }else{_col = Color32::from_rgb(30, 30, 38);}
        Some(_col)
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
        ui.shrink_width_to_current();
        ui.shrink_height_to_current();
    }
}

fn display_computer_page(ui: &mut Ui, task: Option<&TaskPayload>){
    fn return_colors(num: usize, _style: &Style) -> Option<Color32>{
        let mut _col = Color32::from_rgb(30, 30, 38);
        if num % 2 == 0{
            _col = Color32::from_rgb(15, 15, 22);
        }else{_col = Color32::from_rgb(30, 30, 38);}
        Some(_col)
    }

    if let Some(task) = task.as_ref(){
        let ticket = task.service_ticket.as_ref().unwrap();
        let computer = ticket.computer.as_ref();
        if let Some(computer) = computer{
            let seb_info = computer.seb_info.as_ref();
            
            ScrollArea::vertical()
                .max_height(600.0)
                .show(ui, |ui| 
            {
                ui.vertical_centered_justified(|ui| {

                
                    StripBuilder::new(ui)
                        .size(Size::exact(180.0))
                        .size(Size::remainder())
                        .vertical(|mut strip| 
                    {
                        strip.cell(|ui|
                        {
                            ui.vertical_centered_justified(|ui|{
                                ui.group(|ui| {
                                    // ui.label("Personnel Information");
                                    Grid::new("group2").min_col_width(ui.available_width() /2.0).with_row_color(|num, style| return_colors(num, style))
                                    .show(ui, |ui| {
                                        ui.label("hostname:");
                                        ui.label(&computer.hostname);
                                        ui.end_row();
                                        ui.label("operating_system:");
                                        ui.label(&computer.operating_system);
                                        ui.end_row();
                                        ui.label("cpu:");
                                        ui.label(&computer.cpu);
                                        ui.end_row();
                                        ui.label("gpu:");
                                        ui.label(&computer.gpu);
                                        ui.end_row();
                                        ui.label("ram:");
                                        ui.label(&computer.ram);
                                        ui.end_row();
                                        
                                        // ui.label("current_antivirus:");
                                        // ui.label(&ticket.current_antivirus);
                                        // ui.end_row();
                                        // ui.label("hardware_test_results:");
                                        // ui.label(&ticket.hardware_test_results);
                                        // ui.end_row();
                                    });
                                });
                                ui.group(|ui| {
                                    // ui.label("Ticket Information");
                                    Grid::new("group1").min_col_width(ui.available_width() / 3.0).with_row_color(|num, style| return_colors(num, style))
                                    .show(ui, |ui| {
                                        ui.label("Letter");
                                        ui.label("Space Left / Total Size");
                                        ui.label("Type");
                                        ui.end_row();
                                        
                                        for drive_data in &computer.drives{
                                            ui.label(&drive_data.drive_letter);
                                            ui.label(format!("{}Gb / {}Gb", &drive_data.space_left, &drive_data.total_size));
                                            ui.label(&drive_data.drive_type);
                                            ui.end_row();
                                        }
                                    });
                                });
                            });
                        });
                        strip.cell(|ui|{
                            ui.vertical_centered_justified(|ui|{
                                ui.separator();
                                ui.heading("SEB Information");
                                ui.group(|ui| {
                                    if let Some(seb_info) = seb_info{
                                        
                                        // ui.label("Order Details");
                                        Grid::new("group3").min_col_width(ui.available_width() /2.0).with_row_color(|num, style| return_colors(num, style))
                                        .show(ui, |ui| {
                                            ui.label("InstalledDeviceId:");
                                            ui.label(RichText::new(&seb_info.InstalledDeviceId).small().font(FontId::proportional(8.0)));
                                            ui.end_row();
                                            ui.label("InstallInstanceId:");
                                            ui.label(RichText::new(&seb_info.InstallInstanceId).small().font(FontId::proportional(8.0)));
                                            ui.end_row();
                                            ui.label("HasIssues:");
                                            ui.label(&seb_info.HasIssues);
                                            ui.end_row();
                                            ui.label("InstallationStage:");
                                            ui.label(&seb_info.InstallationStage);
                                            ui.end_row();
                                            ui.label("ReasonCode:");
                                            ui.label(&seb_info.ReasonCode);
                                            ui.end_row();
                                            ui.label("ActivationCode:");
                                            ui.label(&seb_info.ActivationCode);
                                            ui.end_row();
                                            ui.label("InstallVersion:");
                                            ui.label(&seb_info.InstallVersion);
                                            ui.end_row();
                                            ui.label("MachineName:");
                                            ui.label(&seb_info.MachineName);
                                            ui.end_row();
                                        });
                                    }else{
                                        ui.shrink_height_to_current();
                                        ui.shrink_width_to_current();
                                        ui.label("No SEB information was sent with ticket.");
                                    }
                                });
                                if let Some(seb_info) = seb_info{
                                    if let Some(extended_seb) = seb_info.ExtendedSeb.as_ref(){
                                        ui.group(|ui| {
                                            // ui.label("Customer Information");
                                            Grid::new("customer_data").min_col_width(ui.available_width() /2.0).with_row_color(|num, style| return_colors(num, style))
                                            .show(ui, |ui| {
                                                ui.label("email:");
                                                ui.label(&extended_seb.email);
                                                ui.end_row();
                                                ui.label("phone:");
                                                ui.label(&extended_seb.phone);
                                                ui.end_row();
                                                ui.label("device_name:");
                                                ui.label(RichText::new(&extended_seb.device_name).small().font(FontId::proportional(8.0)));
                                                ui.end_row();
                                                ui.label("device_id:");
                                                ui.label(RichText::new(&extended_seb.device_id).small().font(FontId::proportional(8.0)));
                                                ui.end_row();
                                                ui.label("state:");
                                                ui.label(&extended_seb.state);
                                                ui.end_row();
                                                ui.label("usage_gb:");
                                                ui.label(&extended_seb.usage_gb);
                                                ui.end_row();
                                                ui.label("date_device_created:");
                                                ui.label(&extended_seb.date_device_created);
                                                ui.end_row();
                                                ui.label("activated:");
                                                ui.label(&extended_seb.activated);
                                                ui.end_row();
                                                ui.label("activation_code:");
                                                ui.label(RichText::new(&extended_seb.activation_code).small().font(FontId::proportional(8.0)));
                                                ui.end_row();
                                                ui.label("last_complete_backup:");
                                                ui.label(&extended_seb.last_complete_backup);
                                                ui.end_row();
                                                ui.label("last_client_status_update:");
                                                ui.label(&extended_seb.last_client_status_update);
                                                ui.end_row();
                                                ui.label("id_recurly_account:");
                                                ui.label(&extended_seb.id_recurly_account);
                                                ui.end_row();
                                                ui.label("date_last_scan:");
                                                ui.end_row();
                                                ui.label("current_period_ends_at:");
                                                ui.label(&extended_seb.current_period_ends_at);
                                                ui.end_row();
                                                ui.label("date_modified:");
                                                ui.label(&extended_seb.date_modified);
                                                ui.end_row();
                                                ui.label("date_created:");
                                                ui.label(&extended_seb.date_created);
                                                ui.end_row();
                                            });
                                        });
                                    }else{
                                        ui.label("SEB information was sent with ticket, but we didnt get the extended SEB info");
                                    }
                                }
                            });
                        });
                    });
                });
            });
        } else { ui.label("Computer information was not sent with ticket"); }
    }
    ui.shrink_width_to_current();
    ui.shrink_height_to_current();
}

fn display_part_order_page(ui: &mut Ui){
    StripBuilder::new(ui)
        .size(Size::exact(50.0))
        .size(Size::exact(50.0))
        .size(Size::exact(120.0))
        .size(Size::exact(50.0))
        .vertical(|mut strip| 
    {

        strip.strip(|s|{
            s
                .size(Size::exact(170.0))
                .size(Size::exact(10.0))
                .size(Size::exact(170.0))
                .horizontal(|mut s|
            {
                s.cell(|ui|{
                    ComboBox::new("AwaitingQuoteCombo", "")
                        .width(ui.available_width())
                        .selected_text("Awaiting Quote")
                        .show_ui(ui, |ui| 
                    {
                        ui.selectable_value(&mut "Order - Pending DM Approval".to_string(), "Order - Pending DM Approval".to_string(), "Order - Pending DM Approval");
                        ui.selectable_value(&mut "Quote Fullfilled".to_string(), "Quote Fullfilled".to_string(), "Quote Fullfilled");
                        ui.selectable_value(&mut "Awaiting Quote".to_string(), "Awaiting Quote".to_string(), "Awaiting Quote");
                    });
                });
                s.empty();
                s.cell(|ui|{
                    ComboBox::new("ManufacturerCombo", "")
                        .selected_text("PCL")
                        .width(ui.available_width())
                        .show_ui(ui, |ui| 
                    {
                        ui.selectable_value(&mut "PCL".to_string(), "PCL".to_string(), "PCL");
                        ui.selectable_value(&mut "Other".to_string(), "Other".to_string(), "Other");
                    });
                });
            });
        });
        strip.strip(|s|{
            s
                .size(Size::exact(170.0))
                .size(Size::exact(10.0))
                .size(Size::exact(170.0))
                .horizontal(|mut s|
            {
                s.cell(|ui|{
                    TextEdit::singleline(&mut "MFG Model #".to_string())
                        .margin(Margin::same(5.0))
                        .desired_width(ui.available_width())
                        .code_editor()
                        .ui(ui);
                });
                s.empty();
                s.cell(|ui|{
                    TextEdit::singleline(&mut "MFG P/N".to_string())
                        .margin(Margin::same(5.0))
                        .desired_width(ui.available_width())
                        .frame(true)
                        .code_editor()
                        .ui(ui);
                });  
            });
        });
        strip.strip(|s|{
            s
                .size(Size::exact(170.0))
                .vertical(|mut s|
            {
                s.strip(|s|{
                    s
                        .size(Size::exact(170.0))
                        .size(Size::exact(10.0))
                        .size(Size::exact(170.0))
                        .horizontal(|mut s|
                    {
                        s.cell(|ui|{
                            TextEdit::multiline(&mut "Part Description".to_string())
                                .margin(Margin::same(5.0))
                                .desired_rows(6)
                                .desired_width(ui.available_width())
                                .code_editor()
                                .ui(ui);
                        });
                        s.empty();
                        s.cell(|ui|{
                            TextEdit::multiline(&mut "Notes".to_string())
                                .margin(Margin::same(5.0))
                                .desired_rows(6)
                                .desired_width(ui.available_width())
                                .code_editor()
                                .ui(ui);
                            
                        });    
                    });
                });
            });
        });
        strip.strip(|s|{
            s
                .size(Size::exact(170.0))
                .vertical(|mut s|
            {
                s.strip(|s|{
                    s
                        .size(Size::exact(170.0))
                        .size(Size::exact(10.0))
                        .size(Size::exact(170.0))
                        .horizontal(|mut s|
                    {
                        s.cell(|ui|{
                            let _ = ui.radio(false, "LCD?");
                        });
                        s.empty();
                        s.cell(|ui|{
                            ui.label("Upload Picture");
                        });    
                    });
                });
            });
        });
    });
    ui.shrink_width_to_current();
    ui.shrink_height_to_current();
}

fn display_chat_page(ui: &mut Ui, chat_view: &mut ChatModal){
    chat_view.borrow_mut().ui(ui);
}