use std::default;

use bytes::Bytes;
use chrono::{DateTime, Utc};
use database::{schema::{TaskPayload, TicketPayload, TASK_TABLE, TICKET_TABLE}, Database};
use eframe::egui::{scroll_area::ScrollBarVisibility, Align, Button, Color32, ComboBox, Direction, Grid, Layout, Margin, RichText, ScrollArea, Style, TextEdit, Ui, Vec2, Widget};
use egui_extras::{Size, StripBuilder};
use log::info;
use rfd::AsyncFileDialog;
use serde::Serialize;
use serde_json::Value;
use wasm_bindgen_futures::spawn_local;

use crate::utilities::{displays::chats::ChatView, DisplayModal, ModalTypes, Updatable};

use super::ModalState;


#[derive(Serialize, Clone, Debug)]
pub struct TaskModal{
    pub title: String,
    #[serde(skip)]
    pub database: Option<Database>, 
    #[serde(skip)]
    pub task: Option<TaskPayload>,
    #[serde(skip)]
    pub chat_view: ChatView,
    pub min_width: Option<f32>,
    pub min_height: Option<f32>,
    pub default_height: Option<f32>,
    pub full_span_content: bool,

    pub state: ModalState,
    pub spo: SpecialPartOrder
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
            chat_view: ChatView::default(),
            spo: SpecialPartOrder::default()
        }
    }
}

impl TaskModal{
    pub fn new(chats: ChatView) -> Self {
        Self {
            title: "Task Details".to_string(),
            database: None,
            task: None,
            min_width: Some(600.0),
            min_height: Some(600.0),
            default_height: Some(800.0),
            full_span_content: false,
            state: ModalState::default(),
            chat_view: chats,
            spo: SpecialPartOrder::default()
        }
    }

    pub fn update_task(&mut self, task: TaskPayload) {
        self.task = Some(task);
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

#[derive(Debug, Clone, Serialize)]
pub struct SpecialPartOrder {
    customer_name: String,          //  "kathleen Hoffmon",
    customer_phone_number: String,          //  "801-888-8888",
    notes: String,          //  "These are some notes",
    system_order_number: String,            //  "123456",
    id_location: String,            //  "Riverdale",
    request_type: String,           //  "Any",
    shipping_method: String,            //  "2 - 2-3 Day Express",
    part_manufacturer: Manufacturer,          //  "PC Laptops",
    manufacturer_model_number: String,          //  "12345Test",
    manufacturer_serial_number: String,             //  "123456789",
    manufacturer_part_number: String,           //  "324657687",
    part_color: String,             //  "N/A",
    part_description: String,           //  "Test",
    part_lcd_toggle: bool,            //  "0"
    spo_status: SpoStatus,
}

impl DisplayModal for TaskModal {
    fn display(&mut self, ui: &mut Ui, current_page_state: ModalAction) -> Option<ModalAction>{
        let mut response: Option<ModalAction> = None;
        let avail_size = Vec2::new(680.0,600.0);
        
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

                            

                            // if Button::new(RichText::new("Delete Task").color(Color32::LIGHT_RED)).ui(ui).double_clicked() {
                                
                            if Button::new(RichText::new("Delete Task").color(Color32::LIGHT_RED)).ui(ui).double_clicked() {
                                
                                let db = self.database.clone();
                                let mut ids = Vec::new();
                                let task = self.task.as_ref().unwrap();
                                let _task_id = task.id.as_ref().unwrap().0.clone();
                                let _ticket_id = if let Some(ticket) = &task.service_ticket{
                                    Some(ticket.id.clone().unwrap())
                                } else{ None };

                                for message in self.chat_view.messages.iter(){
                                    if let Some(id) = &message.id.clone(){
                                        ids.push(id.0.clone());
                                    }
                                };

                                let mut ids = Vec::new();
                                let task = self.task.as_ref().unwrap();
                                let task_id = task.id.as_ref().unwrap().0.clone();
                                let ticket_id = if let Some(ticket) = &task.service_ticket{
                                    Some(ticket.id.clone().unwrap())
                                } else{ None };

                                for message in self.chat_view.messages.iter(){
                                    if let Some(id) = &message.id.clone(){
                                        ids.push(id.0.clone());
                                    }
                                };

                                spawn_local(async move {
                                    let task_id = task_id.clone();
                                    let ticket_id = ticket_id.clone();
                                    let db = db.unwrap();

                                    if ids.len() > 0 {
                                        let _query = "DELETE ";
                                    } 
                                    if let Some(id) = ticket_id {
                                        info!("deleting task_id: {:?}", id.0.clone());
                                        let _x: Option<TicketPayload> = db.database.delete((TICKET_TABLE, id.0.id)).await.unwrap();
                                    }
                                    info!("deleting task_id: {task_id:?}");
                                    let _y: Option<TaskPayload> = db.database.delete((TASK_TABLE, task_id.id)).await.unwrap();

                                });
                            }

                            ui.add_space(200.0);

                            if ui.selectable_label(ticket_page, RichText::new("🖹").heading()).clicked(){
                                response = Some(ModalAction::TicketInfoPage);
                            };
                            if let Some(task) = &self.task{
                                if let Some(_) = task.service_ticket{
                                    if ui.selectable_label(computer_info_page, RichText::new("🖥").heading()).clicked(){
                                        response = Some(ModalAction::ComputerInfoPage);
                                    };
                                    if ui.selectable_label(part_order_page, RichText::new("🔫").heading()).clicked(){
                                        response = Some(ModalAction::PartOrderPage);
                                    };
                                }
                            }
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
                    .size(Size::exact(avail_size.y))
                    .horizontal( |mut strip| 
                {
                    strip
                        .strip(|s| 
                    {
                        s
                            .size(Size::exact(15.0))
                            .size(Size::exact(avail_size.x))
                            .size(Size::exact(15.0))
                            .cell_layout(Layout::top_down(Align::Center))
                            .cell_layout(Layout::top_down(Align::Center))
                            .cell_layout(Layout::top_down(Align::Center))
                            .vertical(|mut s|
                        {
                            s.empty();
                            s.cell(|ui| {
                                ui.horizontal_centered(|ui|{
                                    match current_page_state{
                                        ModalAction::TicketInfoPage => display_task_page(ui, self.task.as_mut(), avail_size),
                                        ModalAction::ComputerInfoPage => display_computer_page(ui, self.task.as_ref(), avail_size),
                                        ModalAction::PartOrderPage => self.spo.display_part_order_page(ui, avail_size),
                                        ModalAction::TaskNotePage => {
                                            ui.set_width(avail_size.x);
                                            // ui.add_space(15.0);
                                            // eframe::egui
                                            if let Some(new_message) = self.chat_view.ui(ui){
                                                if let (Some(db), Some(task)) = (self.database.clone(), self.task.clone()){
                                                    
                                                    task.update_task_notes(new_message, db);
                                                }
                                            }
                                        },
                                        _ => display_task_page(ui, self.task.as_mut(), avail_size)
                                    };
                                });
                            });
                            s.empty();
                        });
                    });// strip.cell(|ui|{  });
                });
            });
        });
        
        
        response
    }
}


fn display_task_page(ui: &mut Ui, task: Option<&mut TaskPayload>, _avail_size: Vec2){
    fn return_colors(num: usize, _style: &Style) -> Option<Color32>{
        let mut _col = Color32::from_rgb(30, 30, 38);
        if num % 2 == 0{
            _col = Color32::from_rgb(15, 15, 22);
        }else{_col = Color32::from_rgb(30, 30, 38);}
        Some(_col)
    }

    ui.add_space(15.0);

    if let Some(task) = task{
        let ticket = task.service_ticket.as_ref();
        if let Some(ticket) = ticket{
            let customer = ticket.customer.as_ref();
            StripBuilder::new(ui)
                .size(Size::exact(100.0))
                .size(Size::exact(115.0))
                .size(Size::exact(60.0))
                .size(Size::exact(100.0))
                .vertical(|mut strip| 
            {
                strip.strip(|s|{
                    s
                        .size(Size::exact(300.0))
                        .size(Size::exact(12.0))
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
                                    let x = ticket.created_at.as_ref();
                                    if let Some(x) = x{
                                        let date = x.parse::<DateTime<Utc>>();
                                        if let Ok(date) = date{
                                            ui.label("Tur Sent:");
                                            ui.label(date.date_naive().to_string());
                                            ui.end_row();
                                        }
                                    }

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
                        .size(Size::exact(12.0))
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

                                        ui.label("Customer ID:");
                                        ui.label(format!("{}", customer.id.as_ref().unwrap().0.id));
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
                        .size(Size::exact(640.0))
                        .horizontal(|mut s|
                    {
                        s.strip(|s|{
                            s
                                .size(Size::remainder())
                                .size(Size::exact(5.0))
                                .size(Size::remainder())
                                .horizontal(|mut s|
                            {
                                s.cell(|ui|{
                                    ui.vertical_centered_justified(|ui| {
                                        ui.label("Checkin Notes:");
                                        TextEdit::multiline(&mut ticket.checkin_notes.to_string())
                                            .margin(Margin::same(5.0))
                                            .desired_rows(8)
                                            .desired_width(ui.available_width())
                                            .ui(ui);
                                    });
                                });
                                s.empty();
                                s.cell(|ui|{
                                    ui.vertical_centered_justified(|ui| {
                                        ui.label("Recommendations:");
                                        TextEdit::multiline(&mut task.task_description.to_string())
                                            .margin(Margin::same(5.0))
                                            .desired_rows(8)
                                            .desired_width(ui.available_width())
                                            .ui(ui);
                                    });
                                });
                            });
                        });
                    });
                });

            });
        }
    }
}

fn display_computer_page(ui: &mut Ui, task: Option<&TaskPayload>, avail_size: Vec2){
    fn return_colors(num: usize, _style: &Style) -> Option<Color32>{
        let mut _col = Color32::from_rgb(30, 30, 38);
        if num % 2 == 0{
            _col = Color32::from_rgb(15, 15, 22);
        }else{_col = Color32::from_rgb(30, 30, 38);}
        Some(_col)
    }
    // ui.set_width(avail_size.x - 50.0);
    if let Some(task) = task.as_ref(){
        let ticket = task.service_ticket.as_ref().unwrap();
        let computer = ticket.computer.as_ref();
        if let Some(computer) = computer{
            let seb_info = computer.seb_info.as_ref();
            ui.horizontal(|ui| ui.add_space(50.0));

            StripBuilder::new(ui)
                .cell_layout(Layout::from_main_dir_and_cross_align(Direction::TopDown, Align::Center))
                .size(Size::exact(20.0))
                .size(Size::exact(avail_size.y))
                .size(Size::exact(20.0))
                .vertical(|mut s| 
            {
                s.empty();
                s.strip(|s| 
                {
                    s
                        .cell_layout(Layout::centered_and_justified(Direction::TopDown))
                        .size(Size::exact(avail_size.x))
                        .horizontal(|mut s| 
                    {
                        s.cell(|ui| 
                        {
                            // ui.vertical(|ui| ui.add_space(50.0));
                            ui.vertical_centered(|ui|{
                                ui.group(|ui| {
                                    Grid::new("group2").min_col_width(avail_size.x / 2.5).with_row_color(|num, style| return_colors(num, style))
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
                                    Grid::new("group1").min_col_width(avail_size.x / 3.8).with_row_color(|num, style| return_colors(num, style))
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
                            ui.vertical_centered_justified(|ui|{

                                ui.add_space(8.0);
                                ui.separator();
                                ui.add_space(8.0);
                                ui.heading("SEB Information");
                                ui.add_space(8.0);
                                ui.separator();
                                ui.add_space(8.0);

                                ScrollArea::vertical()
                                    .max_height(avail_size.y)
                                    .max_width(f32::INFINITY)
                                    .scroll_bar_visibility(ScrollBarVisibility::AlwaysVisible)
                                    .show(ui, |ui| 
                                {
                                    ui.group(|ui| {
                                        if let Some(seb_info) = seb_info{
                                            
                                            // ui.label("Order Details");
                                            Grid::new("group3").min_col_width(avail_size.x / 2.5).with_row_color(|num, style| return_colors(num, style))
                                            .show(ui, |ui| {
                                                ui.label("InstalledDeviceId:");
                                                ui.label(&seb_info.InstalledDeviceId);
                                                ui.end_row();
                                                ui.label("InstallInstanceId:");
                                                ui.label(&seb_info.InstallInstanceId);
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
                                            ui.horizontal(|ui|{
                                                ui.label("No SEB information was sent with ticket.");
                                            });
                                        }
                                    });
                                    if let Some(seb_info) = seb_info{
                                        if let Some(extended_seb) = seb_info.ExtendedSeb.as_ref(){
                                            ui.group(|ui| {
                                                // ui.label("Customer Information");
                                                Grid::new("customer_data").min_col_width(avail_size.x / 2.5).with_row_color(|num, style| return_colors(num, style))
                                                .show(ui, |ui| {
                                                    ui.label("email:");
                                                    ui.label(&extended_seb.email);
                                                    ui.end_row();
                                                    ui.label("phone:");
                                                    ui.label(&extended_seb.phone);
                                                    ui.end_row();
                                                    ui.label("device_name:");
                                                    ui.label(&extended_seb.device_name);
                                                    ui.end_row();
                                                    ui.label("device_id:");
                                                    ui.label(&extended_seb.device_id);
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
                                                    ui.label(&extended_seb.activation_code);
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
                                            ui.add_space(15.0);
                                            ui.horizontal(|ui|{
                                                ui.label("SEB information was sent with ticket, but we didnt get the extended SEB info");
                                            });
                                        }
                                    }
                                });
                            });
                        });
                    });
                });
                s.empty();
            });
        } else { ui.label("Computer information was not sent with ticket"); }
    }
    
    
}

impl Default for SpecialPartOrder {
    fn default() -> Self {
        Self {
            customer_name: String::new(),
            customer_phone_number: String::new(),
            notes: String::new(),
            system_order_number: String::new(),
            id_location: "0".to_string(),
            request_type: String::new(),
            shipping_method: "2 - 2-3 Day Express".to_string(),
            part_manufacturer: Manufacturer::Pclaptops,
            manufacturer_model_number: String::new(),
            manufacturer_serial_number: String::new(),
            manufacturer_part_number: String::new(),
            part_color: "N/A".to_string(),
            part_description: String::new(),
            part_lcd_toggle: false,
            spo_status: SpoStatus::AwaitingQuote,
        }
    }
}
#[derive(PartialEq, Default, Debug, Serialize, Clone)]
pub enum SpoStatus {
    #[default]
    AwaitingQuote,
    QuoteFullfilled,
    OrderPendingDM,
}

#[derive(PartialEq, Default, Debug, Serialize, Clone)]
pub enum Manufacturer {
    #[default]
    Pclaptops,
    Other,
}

impl Manufacturer{
    pub fn as_str(&mut self) -> &str{
        match self{
            Manufacturer::Pclaptops => "PC Laptops",
            Manufacturer::Other => "Other",
        }
    }
}

impl SpoStatus{
    pub fn as_str(&mut self) -> &str{
        match self{
            SpoStatus::AwaitingQuote => "Awaiting Quote",
            SpoStatus::OrderPendingDM => "Pending DM",
            SpoStatus::QuoteFullfilled => "Quote Fullfilled"
        }
    }
}

impl SpecialPartOrder {
    fn display_part_order_page(&mut self, ui: &mut Ui, avail_size: Vec2){
        StripBuilder::new(ui)
            .cell_layout(Layout::from_main_dir_and_cross_align(Direction::TopDown, Align::Center))
            .size(Size::exact(50.0))
            .size(Size::remainder())
            .size(Size::remainder())
            .vertical(|mut s| 
        {
            s.empty();
            s.strip(|s| 
            {
                s
                    .cell_layout(Layout::centered_and_justified(Direction::TopDown))
                    .size(Size::exact(avail_size.x / 3.2))
                    .size(Size::exact(200.0))
                    .horizontal(|mut s| 
                {
                    s.empty();
                    s.cell(|ui| 
                    {
                        ui.vertical_centered(|ui| {
                            ui.horizontal(|ui| {
                                ComboBox::new("AwaitingQuoteCombo", "")
                                    .selected_text(self.spo_status.as_str())
                                    .width(50.0)
                                    .show_ui(ui, |ui| 
                                {
                                    ui.selectable_value(&mut self.spo_status, SpoStatus::OrderPendingDM, "Pending DM");
                                    ui.selectable_value(&mut self.spo_status, SpoStatus::QuoteFullfilled, "Quote Fullfilled");
                                    ui.selectable_value(&mut self.spo_status, SpoStatus::AwaitingQuote, "Awaiting Quote");
                                });
                                ComboBox::new("ManufacturerCombo", "")
                                    .selected_text(self.part_manufacturer.as_str())
                                    .width(50.0)
                                    .show_ui(ui, |ui| 
                                {
                                    ui.selectable_value(&mut self.part_manufacturer, Manufacturer::Pclaptops, "PC Laptops");
                                    ui.selectable_value(&mut self.part_manufacturer, Manufacturer::Other, "Other");
                                    
                                });
                            });

                            ui.add_space(15.0);

                            TextEdit::singleline(&mut self.manufacturer_model_number)
                                .hint_text("MFG Model #".to_string())
                                .margin(Margin::same(5.0))
                                .ui(ui);

                            ui.add_space(15.0);

                            TextEdit::singleline(&mut self.manufacturer_part_number)
                                .hint_text("MFG P/N".to_string())
                                .margin(Margin::same(5.0))
                                .frame(true)
                                .ui(ui);
                        
                            ui.add_space(15.0);

                            TextEdit::singleline(&mut self.part_description)
                                .hint_text("Part Description".to_string())
                                .margin(Margin::same(5.0))
                                .ui(ui);
                            
                            ui.add_space(15.0);

                            TextEdit::multiline(&mut self.notes)
                                .hint_text("Notes".to_string())
                                .margin(Margin::same(5.0))
                                .desired_rows(3)
                                .ui(ui);

                            ui.add_space(15.0);

                            // let mut task: Option<AsyncFileDialog> = None;

                            ui.horizontal(|ui| { 
                                let toggle = ui.checkbox(&mut self.part_lcd_toggle, "LCD?");
                                ui.add_space(ui.available_width() / 2.0);
                                let file_upload = ui.selectable_label(false, "Upload Picture");

                                
                                if file_upload.clicked() {
                                    // task = Some(AsyncFileDialog::new().pick_files());
                                }
                                if toggle.clicked() {
                                    info!("self.part_lcd_toggle: {}", self.part_lcd_toggle);
                                }
                            });

                            ui.add_space(15.0);

                            ui.horizontal_top(|ui| { 
                                if Button::new("Submit").min_size(Vec2::new(50.0, 20.0)).ui(ui).clicked() {

                                    let spo = SpecialPartOrder {
                                        customer_name: self.customer_name.clone(),
                                        customer_phone_number: self.customer_phone_number.clone(),
                                        notes: self.notes.clone(),
                                        system_order_number: self.system_order_number.clone(),
                                        id_location: self.id_location.clone(),
                                        request_type: self.request_type.clone(),
                                        shipping_method: self.shipping_method.clone(),
                                        part_manufacturer: self.part_manufacturer.clone(),
                                        manufacturer_model_number: self.manufacturer_model_number.clone(),
                                        manufacturer_serial_number: self.manufacturer_serial_number.clone(),
                                        manufacturer_part_number: self.manufacturer_part_number.clone(),
                                        part_color: self.part_color.clone(),
                                        part_description: self.part_description.clone(),
                                        part_lcd_toggle: self.part_lcd_toggle.clone(),
                                        spo_status: self.spo_status.clone(),
                                    };

                                    spawn_local(async move {
                                        // let mut bytes: Bytes = Bytes::new();
                                        // let mut file_name = String::new();

                                        // if let Some(task) = task{
                                        //     let files = task.await.unwrap();
                                        //     for file_handle in files {
                                        //         file_name = file_handle.file_name();
                                        //         bytes = Bytes::copy_from_slice(file_handle.read().await.as_slice());
                                        //     }
                                        // }

                                        let params: Value = serde_json::json!({
                                            "user_email": "logan.lees@pclaptops.com", 
                                            "user_password": "Poolparty1",
                                            "format_data": "text",
                                            "action": "create",
                                            "application": "customer_request_order", 
                                            "payload": spo,
                                        });

                                        // let client = Client::new();
                                        // client.post("https://scaffold.pclaptops.com/api/index")
                                        //     .header(CONTENT_TYPE, "application/json")
                                        //     .header(ACCEPT, "application/json")
                                        //     .json(&params)
                                        //     .send()
                                        //     .await
                                        //     .unwrap();

                                    });
                                }
                            });
                        });
                    });
                });
            });
            s.empty();
        });
        
        
    }
}