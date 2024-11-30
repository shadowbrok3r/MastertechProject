use crate::{chats::ChatView, DisplayModal, PlatformSpawner, Spawner};
use bytes::Bytes;
use chrono::{DateTime, Utc};
use core::f32;
use database::schema::{utilities::delete_task, Store, TaskPayload};
use eframe::egui::{
    scroll_area::ScrollBarVisibility, Align, Button, Color32, ComboBox, Direction, FontId, Grid,
    Layout, Margin, RichText, ScrollArea, Separator, Style, TextEdit, Ui, Vec2, Vec2b, Widget
};
use egui_extras::{Size, StripBuilder};
use log::info; // error, 
use reqwest::{
    header::{ACCEPT, CONTENT_TYPE},
    Client,
};
use rfd::{AsyncFileDialog, FileHandle};
use serde::Serialize;
use serde_json::Value;
use std::sync::Arc;
#[cfg(target_arch="wasm32")]
use std::sync::Mutex;

#[cfg(not(target_arch="wasm32"))]
use tokio::sync::Mutex;

use super::ModalState;

#[derive(Serialize, Clone, Debug)]
pub struct TaskModal {
    pub title: String,
    pub task: TaskPayload,
    #[serde(skip)]
    pub chat_view: ChatView,
    pub min_width: Option<f32>,
    pub min_height: Option<f32>,
    pub default_height: Option<f32>,
    pub full_span_content: bool,

    pub state: ModalState,
    pub spo: SpecialPartOrder,
}

#[derive(Debug, Clone, Serialize, Default)]
pub enum ModalAction {
    TicketInfoPage,
    PartOrderPage,
    ComputerInfoPage,
    TaskNotePage,
    ImportTask,
    Close,
    TaskPage,
    #[default]
    None,
}

impl Default for TaskModal {
    fn default() -> Self {
        Self {
            title: "Task Details".to_string(),
            task: TaskPayload::default(),
            min_width: Some(600.0),
            min_height: Some(600.0),
            default_height: Some(800.0),
            full_span_content: false,
            state: ModalState::default(),
            chat_view: ChatView::default(),
            spo: SpecialPartOrder::default(),
        }
    }
}

impl TaskModal {
    pub fn new(chat_view: ChatView, task: TaskPayload) -> Self {
        Self {
            title: "Task Details".to_string(),
            task,
            min_width: Some(600.0),
            min_height: Some(600.0),
            default_height: Some(800.0),
            full_span_content: false,
            state: ModalState::default(),
            chat_view,
            spo: SpecialPartOrder::default(),
        }
    }
}


#[derive(Debug, Clone, Serialize)]
pub struct SpecialPartOrder {
    customer_name: String,              //  "kathleen Hoffmon",
    customer_phone_number: String,      //  "801-888-8888",
    application_comment: String,        //  "These are some notes",
    system_order_number: String,        //  "123456",
    id_location: String,                //  "Riverdale",
    request_type: String,               //  "Any",
    shipping_method: String,            //  "2 - 2-3 Day Express",
    part_manufacturer: Manufacturer,    //  "PC Laptops",
    manufacturer_model_number: String,  //  "12345Test",
    manufacturer_serial_number: String, //  "123456789",
    manufacturer_part_number: String,   //  "324657687",
    part_color: String,                 //  "N/A",
    part_description: String,           //  "Test",
    part_lcd_toggle: bool,              //  "0"
    spo_status: SpoStatus,
    #[serde(skip)]
    files: Arc<Mutex<Option<Vec<FileHandle>>>>,
}

impl DisplayModal for TaskModal {
    fn display(&mut self, ui: &mut Ui, current_page_state: ModalAction) -> Option<ModalAction> {
        let mut response: Option<ModalAction> = None;
        let avail_size = Vec2::new(680.0, 620.0);

        StripBuilder::new(ui)
            .cell_layout(Layout::top_down_justified(Align::Center))
            .size(Size::exact(30.0))
            .size(Size::exact(10.0))
            .size(Size::relative(0.8))
            .vertical(|mut strip| {
                strip.strip(|strip| {
                    strip.size(Size::remainder()).horizontal(|mut strip| {
                        strip.cell(|ui| {
                            ui.horizontal(|ui| {
                                let mut ticket_page = false;
                                let mut part_order_page = false;
                                let mut computer_info_page = false;
                                let mut task_note_page = false;
                                let mut task_page = false;
                                match current_page_state {
                                    ModalAction::TicketInfoPage => ticket_page = true,
                                    ModalAction::PartOrderPage => part_order_page = true,
                                    ModalAction::ComputerInfoPage => computer_info_page = true,
                                    ModalAction::TaskNotePage => task_note_page = true,
                                    ModalAction::TaskPage => task_page = true,
                                    _ => {
                                        if self.task.service_ticket.is_some() {
                                            ticket_page = true;
                                        } else {
                                            task_page = true;
                                        }
                                    }
                                };

                                let delete_btn = Button::new(
                                    RichText::new("Delete Task").color(Color32::LIGHT_RED),
                                )
                                .ui(ui)
                                .on_hover_text("Double Click To Delete Task");

                                if delete_btn.double_clicked() {
                                    // let mut ids = Vec::new();
                                    // let _task_id = self.task.id.as_ref().unwrap().clone();
                                    // let _ticket_id = if let Some(ticket) = &self.task.service_ticket{
                                    //     Some(ticket.id.clone())
                                    // } else{ None };

                                    // for message in self.chat_view.messages.iter(){
                                    //     if let Some(id) = &message.id.clone(){
                                    //         ids.push(id.0.clone());
                                    //     }
                                    // };
                                    let task_id = self.task.id.clone();

                                    let id = task_id.clone();
                                    PlatformSpawner::spawn(async move {
                                        match delete_task(id).await {
                                            Ok(_) => info!("Deleted task"),
                                            Err(e) => info!("Error: {e:?}"),
                                        }
                                    });
                                    response = Some(ModalAction::Close);
                                }

                                ui.add_space(200.0);

                                if self.task.service_ticket.is_some() {
                                    if ui
                                        .selectable_label(ticket_page, RichText::new("🖹").heading())
                                        .clicked()
                                    {
                                        response = Some(ModalAction::TicketInfoPage);
                                    };
                                    if ui
                                        .selectable_label(
                                            computer_info_page,
                                            RichText::new("🖥").heading(),
                                        )
                                        .clicked()
                                    {
                                        response = Some(ModalAction::ComputerInfoPage);
                                    };
                                    if ui
                                        .selectable_label(
                                            part_order_page,
                                            RichText::new("🔫").heading(),
                                        )
                                        .clicked()
                                    {
                                        response = Some(ModalAction::PartOrderPage);
                                    };
                                } else {
                                    if ui
                                        .selectable_label(task_page, RichText::new("🖹").heading())
                                        .clicked()
                                    {
                                        response = Some(ModalAction::TaskPage);
                                    };
                                }
                                if ui
                                    .selectable_label(task_note_page, RichText::new("💬").heading())
                                    .clicked()
                                {
                                    response = Some(ModalAction::TaskNotePage);
                                };
                            });
                        });
                    });
                });
                strip.empty();
                strip.strip(|strip| {
                    strip
                        .size(Size::exact(avail_size.y))
                        .horizontal(|mut strip| {
                            strip.strip(|s| {
                                s.size(Size::exact(15.0))
                                    .size(Size::exact(avail_size.x))
                                    .size(Size::exact(15.0))
                                    .cell_layout(Layout::top_down(Align::Center))
                                    .cell_layout(Layout::top_down(Align::Center))
                                    .cell_layout(Layout::top_down(Align::Center))
                                    .vertical(|mut s| {
                                        s.empty();
                                        s.cell(|ui| {
                                            ui.horizontal_centered(|ui| {
                                                ui.style_mut().override_font_id =
                                                    Some(FontId::proportional(13.0));
                                                match current_page_state {
                                                    ModalAction::TicketInfoPage => {
                                                        display_ticket_page(
                                                            ui,
                                                            &mut self.task,
                                                            avail_size,
                                                        )
                                                    }
                                                    ModalAction::ComputerInfoPage => {
                                                        display_computer_page(
                                                            ui,
                                                            &mut self.task,
                                                            avail_size,
                                                        )
                                                    }
                                                    ModalAction::PartOrderPage => {
                                                        self.spo.display_part_order_page(
                                                            ui,
                                                            avail_size,
                                                            self.chat_view
                                                                .current_user
                                                                .clone()
                                                                .unwrap_or_default()
                                                                .store,
                                                        )
                                                    }
                                                    ModalAction::TaskNotePage => {
                                                        ui.set_width(avail_size.x);
                                                        if let Some(_new_message) =
                                                            self.chat_view.ui(ui)
                                                        {
                                                            // self.task.update_task_notes(new_message);
                                                        }
                                                    }
                                                    ModalAction::TaskPage => {
                                                        display_task_page(ui, &mut self.task)
                                                    }
                                                    _ => display_ticket_page(
                                                        ui,
                                                        &mut self.task,
                                                        avail_size,
                                                    ),
                                                };
                                            });
                                        });
                                        s.empty();
                                    });
                            });
                        });
                });
            });

        response
    }
}

pub fn display_task_page(ui: &mut Ui, task: &mut TaskPayload) {
    ui.add_space(ui.available_width() * 0.2 - 15.0);
    ui.vertical_centered_justified(|ui| {
        ui.label(RichText::new("Task Description:").font(FontId::proportional(15.0)));
        TextEdit::multiline(&mut task.task_description.to_string())
            .margin(Margin::same(5.0))
            .desired_rows(8)
            .desired_width(ui.available_width() / 1.4)
            .ui(ui);
    });
}

pub fn display_ticket_page(ui: &mut Ui, task: &mut TaskPayload, _avail_size: Vec2) {
    fn return_colors(num: usize, _style: &Style) -> Option<Color32> {
        let mut _col = Color32::from_rgb(30, 30, 38);
        if num % 2 == 0 {
            _col = Color32::from_rgb(15, 15, 22);
        } else {
            _col = Color32::from_rgb(30, 30, 38);
        }
        Some(_col)
    }

    ui.add_space(15.0);

    let ticket = task.service_ticket.as_ref();
    if let Some(ticket) = ticket {
        let customer = ticket.customer.as_ref();
        StripBuilder::new(ui)
            .size(Size::exact(100.0))
            .size(Size::exact(115.0))
            .size(Size::exact(60.0))
            .size(Size::exact(100.0))
            .vertical(|mut strip| {
                strip.strip(|s| {
                    s.size(Size::exact(300.0))
                        .size(Size::exact(12.0))
                        .size(Size::exact(300.0))
                        .horizontal(|mut s| {
                            s.cell(|ui| {
                                ui.group(|ui| {
                                    Grid::new("group2")
                                        .min_col_width(150.0)
                                        .with_row_color(|num, style| return_colors(num, style))
                                        .show(ui, |ui| {
                                            ui.colored_label(Color32::LIGHT_RED, "Technician:");
                                            ui.label(&ticket.tech);
                                            ui.end_row();

                                            ui.colored_label(Color32::LIGHT_RED, "Salesman:");
                                            ui.label(&ticket.salesman);
                                            ui.end_row();

                                            ui.colored_label(Color32::LIGHT_RED, "Split Rep:");
                                            ui.label(&ticket.sales_rep);
                                            ui.end_row();

                                            ui.colored_label(Color32::LIGHT_RED, "Checkin Rep:");
                                            ui.label(&ticket.checkin_rep);
                                        });
                                });
                            });
                            s.empty();
                            s.cell(|ui| {
                                ui.group(|ui| {
                                    Grid::new("group1")
                                        .min_col_width(150.0)
                                        .with_row_color(|num, style| return_colors(num, style))
                                        .show(ui, |ui| {
                                            ui.colored_label(Color32::LIGHT_RED, "SO#:");
                                            ui.label(format!("{}", ticket.service_number));
                                            ui.end_row();
                                            let x = ticket.created_at.as_ref();
                                            if let Some(x) = x {
                                                let date = x.parse::<DateTime<Utc>>();
                                                if let Ok(date) = date {
                                                    ui.colored_label(
                                                        Color32::LIGHT_RED,
                                                        "Tur Sent:",
                                                    );
                                                    ui.label(date.date_naive().to_string());
                                                    ui.end_row();
                                                }
                                            }
                                            ui.end_row();
                                        });
                                });
                            });
                        });
                });
                strip.strip(|s| {
                    s.size(Size::exact(300.0))
                        .size(Size::exact(12.0))
                        .size(Size::exact(300.0))
                        .horizontal(|mut s| {
                            s.cell(|ui| {
                                ui.group(|ui| {
                                    // ui.colored_label(Color32::LIGHT_RED, "Order Details");
                                    Grid::new("group3")
                                        .min_col_width(150.0)
                                        .with_row_color(|num, style| return_colors(num, style))
                                        .show(ui, |ui| {
                                            ui.colored_label(Color32::LIGHT_RED, "Terms:");
                                            ui.label(&ticket.terms);
                                            ui.end_row();

                                            ui.colored_label(Color32::LIGHT_RED, "Total on Order:");
                                            ui.label(&ticket.ticket_total);
                                            ui.end_row();

                                            ui.colored_label(Color32::LIGHT_RED, "Order Type:");
                                            ui.label(&ticket.doc_alias);
                                            ui.end_row();
                                            ui.colored_label(Color32::LIGHT_RED, "");
                                            ui.end_row();
                                            ui.colored_label(Color32::LIGHT_RED, "");
                                        });
                                });
                            });
                            s.empty();
                            s.cell(|ui| {
                                if let Some(customer) = &customer {
                                    ui.group(|ui| {
                                        // ui.colored_label(Color32::LIGHT_RED, "Customer Information");
                                        Grid::new("customer_data")
                                            .spacing(Vec2::new(0.0, 6.0))
                                            .max_col_width(150.0)
                                            .min_col_width(150.0)
                                            .with_row_color(|num, style| return_colors(num, style))
                                            .show(ui, |ui| {
                                                // ui.colored_label(Color32::LIGHT_RED, "Other Services:");
                                                // ui.with_layout(Layout::centered_and_justified(Direction::LeftToRight), |ui| {
                                                //     ui.label(&customer.services.as_ref().unwrap());
                                                // });
                                                // ui.end_row();

                                                ui.colored_label(Color32::LIGHT_RED, "ID:");
                                                ui.label(format!(
                                                    "{}",
                                                    customer.id.key().to_string()
                                                ));
                                                ui.end_row();

                                                ui.colored_label(Color32::LIGHT_RED, "Name:");
                                                ui.label(&customer.name);
                                                ui.end_row();

                                                ui.colored_label(Color32::LIGHT_RED, "Phone#:");
                                                ui.label(&customer.phone_number);
                                                ui.end_row();

                                                ui.colored_label(Color32::LIGHT_RED, "Phone2:");
                                                ui.label(&customer.phone_number_2);
                                                ui.end_row();

                                                ui.colored_label(Color32::LIGHT_RED, "Email:");
                                                ui.label(&customer.email);
                                                // ui.colored_label(Color32::LIGHT_RED, "SPO Links:");
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
                strip.strip(|s| {
                    s.size(Size::exact(640.0)).horizontal(|mut s| {
                        s.strip(|s| {
                            s.size(Size::remainder())
                                .size(Size::exact(5.0))
                                .size(Size::remainder())
                                .horizontal(|mut s| {
                                    s.cell(|ui| {
                                        ui.vertical_centered_justified(|ui| {
                                            ui.label(
                                                RichText::new("Checkin Notes:")
                                                    .font(FontId::proportional(15.0)),
                                            );
                                            TextEdit::multiline(
                                                &mut ticket.checkin_notes.to_string(),
                                            )
                                            .margin(Margin::same(5.0))
                                            .desired_rows(8)
                                            .desired_width(ui.available_width())
                                            .ui(ui);
                                        });
                                    });
                                    s.empty();
                                    s.cell(|ui| {
                                        ui.vertical_centered_justified(|ui| {
                                            ui.label(
                                                RichText::new("Recommendations:")
                                                    .font(FontId::proportional(15.0)),
                                            );
                                            TextEdit::multiline(
                                                &mut task.task_description.to_string(),
                                            )
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

fn display_computer_page(ui: &mut Ui, task: &mut TaskPayload, avail_size: Vec2) {
    fn return_colors(num: usize, _style: &Style) -> Option<Color32> {
        let mut _col = Color32::from_rgb(30, 30, 38);
        if num % 2 == 0 {
            _col = Color32::from_rgb(15, 15, 22);
        } else {
            _col = Color32::from_rgb(30, 30, 38);
        }
        Some(_col)
    }
    // ui.set_width(612.);
    // ui.set_max_width(612.);
    let ticket = task.service_ticket.as_ref().unwrap();
    let computer = ticket.computer.as_ref();

    if let Some(computer) = computer {
        let seb_info = computer.seb_info.as_ref();
        ui.horizontal(|ui: &mut Ui| ui.add_space(10.0));

        StripBuilder::new(ui)
            .cell_layout(Layout::from_main_dir_and_cross_align(
                Direction::TopDown,
                Align::Center,
            ))
            .size(Size::exact(avail_size.y - 15.0))
            .vertical(|mut s| {
                s.strip(|s| {
                    s.cell_layout(Layout::centered_and_justified(Direction::TopDown))
                        .size(Size::exact(660.))
                        .horizontal(|mut s| {
                                s.cell(|ui|
                                {
                                    ui.vertical_centered(|ui|{
                                        ui.group(|ui| {
                                            Grid::new("group2").max_col_width(avail_size.x / 2.15).min_col_width(avail_size.x / 2.15).with_row_color(|num, style| return_colors(num, style))
                                            .show(ui, |ui| {
                                                ui.colored_label(Color32::LIGHT_RED, "Hostname");
                                                ui.label(&computer.hostname);
                                                ui.end_row();
                                                ui.colored_label(Color32::LIGHT_RED, "Operating System");
                                                ui.label(&computer.operating_system);
                                                ui.end_row();
                                                ui.colored_label(Color32::LIGHT_RED, "CPU");
                                                ui.label(&computer.cpu);
                                                ui.end_row();
                                                ui.colored_label(Color32::LIGHT_RED, "GPU");
                                                ui.label(&computer.gpu);
                                                ui.end_row();
                                                ui.colored_label(Color32::LIGHT_RED, "RAM");
                                                ui.label(format!("{} Gb", &computer.ram));
                                                ui.end_row();
                                                ui.end_row();
                                                ui.colored_label(Color32::LIGHT_RED, "Current Antivirus:");
                                                if let Some(antivirus) = ticket.current_antivirus.as_ref() {
                                                    if antivirus.len() == 0 {
                                                        ui.end_row();
                                                    }

                                                    for antivirus in antivirus.iter() {
                                                        ui.label(antivirus);
                                                        ui.end_row();
                                                    }
                                                } else {
                                                    ui.end_row();
                                                }

                                                ui.colored_label(Color32::LIGHT_RED, "HDD Test:");
                                                ui.label(&ticket.hardware_test_results.hdd_test);
                                                ui.end_row();
                                                ui.colored_label(Color32::LIGHT_RED, "SSD Test:");
                                                ui.label(&ticket.hardware_test_results.ssd_test);
                                                ui.end_row();
                                                ui.colored_label(Color32::LIGHT_RED, "RAM Test:");
                                                ui.label(&ticket.hardware_test_results.ram_test);
                                                ui.end_row();

                                            });
                                        });

                                        ui.group(|ui| {
                                            Grid::new("group1").max_col_width(avail_size.x / 3.2 - 2.).min_col_width(avail_size.x / 3.2-2.).with_row_color(|num, style| return_colors(num, style))
                                            .show(ui, |ui| {
                                                ui.colored_label(Color32::LIGHT_RED, "Letter");
                                                ui.colored_label(Color32::LIGHT_RED, "Space Left / Total Size");
                                                ui.colored_label(Color32::LIGHT_RED, "Type");
                                                ui.end_row();

                                                for drive_data in &computer.drives{
                                                    ui.colored_label(Color32::LIGHT_RED, &drive_data.drive_letter);
                                                    ui.label(format!("{} Gb / {} Gb", &drive_data.space_left, &drive_data.total_size));
                                                    ui.label(&drive_data.drive_type);
                                                    ui.end_row();
                                                }
                                            });
                                        });
                                    });
                                    ui.vertical_centered(|ui|{
                                        ui.scope(|ui| {
                                            ui.add_space(8.0);
                                            Separator::default().shrink(150.0).ui(ui);
                                            ui.add_space(8.0);
                                            ui.heading("SEB Information");
                                            ui.add_space(8.0);
                                            Separator::default().shrink(150.0).ui(ui);
                                            ui.add_space(8.0);
                                        });
                                        ScrollArea::vertical()
                                            .max_height(f32::INFINITY)
                                            .max_width(540.0)
                                            .auto_shrink(Vec2b::new(false, false))
                                            .scroll_bar_visibility(ScrollBarVisibility::AlwaysVisible)
                                            .show(ui, |ui|
                                        {
                                            ui.group(|ui| {
                                                if let Some(seb_info) = seb_info{

                                                    // ui.colored_label(Color32::LIGHT_RED, "Order Details");
                                                    Grid::new("group3").spacing(Vec2::new(0.0, 6.0)).max_col_width(avail_size.x / 2.15).min_col_width(avail_size.x / 2.15).with_row_color(|num, style| return_colors(num, style))
                                                    .show(ui, |ui| {
                                                        ui.colored_label(Color32::LIGHT_RED, "InstalledDeviceId:");
                                                        ui.label(&seb_info.InstalledDeviceId);
                                                        ui.end_row();
                                                        ui.colored_label(Color32::LIGHT_RED, "InstallInstanceId:");
                                                        ui.label(&seb_info.InstallInstanceId);
                                                        ui.end_row();
                                                        ui.colored_label(Color32::LIGHT_RED, "HasIssues:");
                                                        ui.label(&seb_info.HasIssues);
                                                        ui.end_row();
                                                        ui.colored_label(Color32::LIGHT_RED, "InstallationStage:");
                                                        ui.label(&seb_info.InstallationStage);
                                                        ui.end_row();
                                                        ui.colored_label(Color32::LIGHT_RED, "ReasonCode:");
                                                        ui.label(&seb_info.ReasonCode);
                                                        ui.end_row();
                                                        ui.colored_label(Color32::LIGHT_RED, "ActivationCode:");
                                                        ui.label(&seb_info.ActivationCode);
                                                        ui.end_row();
                                                        ui.colored_label(Color32::LIGHT_RED, "InstallVersion:");
                                                        ui.label(&seb_info.InstallVersion);
                                                        ui.end_row();
                                                        ui.colored_label(Color32::LIGHT_RED, "MachineName:");
                                                        ui.label(&seb_info.MachineName);
                                                        ui.end_row();
                                                    });
                                                }else{
                                                    ui.colored_label(Color32::LIGHT_RED, "No SEB information was sent with ticket.");
                                                }
                                            });
                                            if let Some(seb_info) = seb_info{
                                                ui.add_space(10.0);
                                                if let Some(extended_seb) = seb_info.ExtendedSeb.as_ref(){
                                                    ui.group(|ui| {
                                                        // ui.colored_label(Color32::LIGHT_RED, "Customer Information");
                                                        Grid::new("customer_data").max_col_width(avail_size.x / 2.15).min_col_width(avail_size.x / 2.15).with_row_color(|num, style| return_colors(num, style))
                                                        .show(ui, |ui| {
                                                            ui.colored_label(Color32::LIGHT_RED, "email:");
                                                            ui.label(&extended_seb.email);
                                                            ui.end_row();
                                                            ui.colored_label(Color32::LIGHT_RED, "phone:");
                                                            ui.label(&extended_seb.phone);
                                                            ui.end_row();
                                                            ui.colored_label(Color32::LIGHT_RED, "device_name:");
                                                            ui.label(&extended_seb.device_name);
                                                            ui.end_row();
                                                            ui.colored_label(Color32::LIGHT_RED, "device_id:");
                                                            ui.label(&extended_seb.device_id);
                                                            ui.end_row();
                                                            ui.colored_label(Color32::LIGHT_RED, "state:");
                                                            ui.label(&extended_seb.state);
                                                            ui.end_row();
                                                            ui.colored_label(Color32::LIGHT_RED, "usage_gb:");
                                                            ui.label(&extended_seb.usage_gb);
                                                            ui.end_row();
                                                            ui.colored_label(Color32::LIGHT_RED, "date_device_created:");
                                                            ui.label(&extended_seb.date_device_created);
                                                            ui.end_row();
                                                            ui.colored_label(Color32::LIGHT_RED, "activated:");
                                                            ui.label(&extended_seb.activated);
                                                            ui.end_row();
                                                            ui.colored_label(Color32::LIGHT_RED, "activation_code:");
                                                            ui.label(&extended_seb.activation_code);
                                                            ui.end_row();
                                                            ui.colored_label(Color32::LIGHT_RED, "last_complete_backup:");
                                                            ui.label(&extended_seb.last_complete_backup);
                                                            ui.end_row();
                                                            ui.colored_label(Color32::LIGHT_RED, "last_client_status_update:");
                                                            ui.label(&extended_seb.last_client_status_update);
                                                            ui.end_row();
                                                            ui.colored_label(Color32::LIGHT_RED, "id_recurly_account:");
                                                            ui.label(&extended_seb.id_recurly_account);
                                                            ui.end_row();
                                                            ui.colored_label(Color32::LIGHT_RED, "date_last_scan:");
                                                            ui.end_row();
                                                            ui.colored_label(Color32::LIGHT_RED, "current_period_ends_at:");
                                                            ui.label(&extended_seb.current_period_ends_at);
                                                            ui.end_row();
                                                            ui.colored_label(Color32::LIGHT_RED, "date_modified:");
                                                            ui.label(&extended_seb.date_modified);
                                                            ui.end_row();
                                                            ui.colored_label(Color32::LIGHT_RED, "date_created:");
                                                            ui.label(&extended_seb.date_created);
                                                            ui.end_row();
                                                        });
                                                    });
                                                }else{
                                                    ui.vertical_centered(|ui|{
                                                        ui.set_max_width(avail_size.x / 2.0);
                                                        ui.colored_label(Color32::LIGHT_RED, "SEB information was sent with ticket, but we didnt get the extended SEB info");
                                                    });
                                                }
                                            }
                                        });
                                    });
                                });
                        });
                });
            });
    } else {
        ui.vertical_centered(|ui| {
            ui.set_max_width(avail_size.x / 2.0);
            ui.colored_label(
                Color32::LIGHT_RED,
                "Computer information was not sent with ticket",
            );
        });
    }
}

impl Default for SpecialPartOrder {
    fn default() -> Self {
        Self {
            customer_name: String::new(),
            customer_phone_number: String::new(),
            application_comment: String::new(),
            system_order_number: String::new(),
            id_location: "1".to_string(),
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
            files: Arc::new(Mutex::new(None)),
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

impl Manufacturer {
    pub fn as_str(&mut self) -> &str {
        match self {
            Manufacturer::Pclaptops => "PC Laptops",
            Manufacturer::Other => "Other",
        }
    }
}

impl SpoStatus {
    pub fn as_str(&mut self) -> &str {
        match self {
            SpoStatus::AwaitingQuote => "Awaiting Quote",
            SpoStatus::OrderPendingDM => "Pending DM",
            SpoStatus::QuoteFullfilled => "Quote Fullfilled",
        }
    }
}

impl SpecialPartOrder {
    pub fn set_customer(
        &mut self,
        customer_name: String,
        customer_phone_number: String,
        system_order_number: String,
    ) {
        self.customer_name = customer_name;
        self.customer_phone_number = customer_phone_number;
        self.system_order_number = system_order_number;
    }

    pub fn display_part_order_page(&mut self, ui: &mut Ui, avail_size: Vec2, location: Store) {
        StripBuilder::new(ui)
            .cell_layout(Layout::from_main_dir_and_cross_align(
                Direction::TopDown,
                Align::Center,
            ))
            .size(Size::exact(50.0))
            .size(Size::remainder())
            .size(Size::remainder())
            .vertical(|mut s| {
                s.empty();
                s.strip(|s| {
                    s.cell_layout(Layout::centered_and_justified(Direction::TopDown))
                        .size(Size::exact(avail_size.x / 3.2))
                        .size(Size::exact(200.0))
                        .horizontal(|mut s| {
                            s.empty();
                            s.cell(|ui| {
                                ui.vertical_centered(|ui| {
                                    ui.horizontal(|ui| {
                                        ComboBox::new("AwaitingQuoteCombo", "")
                                            .selected_text(self.spo_status.as_str())
                                            .width(50.0)
                                            .show_ui(ui, |ui| {
                                                ui.selectable_value(
                                                    &mut self.spo_status,
                                                    SpoStatus::OrderPendingDM,
                                                    "Pending DM",
                                                );
                                                ui.selectable_value(
                                                    &mut self.spo_status,
                                                    SpoStatus::QuoteFullfilled,
                                                    "Quote Fullfilled",
                                                );
                                                ui.selectable_value(
                                                    &mut self.spo_status,
                                                    SpoStatus::AwaitingQuote,
                                                    "Awaiting Quote",
                                                );
                                            });
                                        ComboBox::new("ManufacturerCombo", "")
                                            .selected_text(self.part_manufacturer.as_str())
                                            .width(50.0)
                                            .show_ui(ui, |ui| {
                                                ui.selectable_value(
                                                    &mut self.part_manufacturer,
                                                    Manufacturer::Pclaptops,
                                                    "PC Laptops",
                                                );
                                                ui.selectable_value(
                                                    &mut self.part_manufacturer,
                                                    Manufacturer::Other,
                                                    "Other",
                                                );
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

                                    TextEdit::multiline(&mut self.application_comment)
                                        .hint_text("Notes".to_string())
                                        .margin(Margin::same(5.0))
                                        .desired_rows(3)
                                        .ui(ui);

                                    ui.add_space(15.0);

                                    // let mut task: Option<AsyncFileDialog> = None;

                                    ui.horizontal(|ui| {
                                        let toggle = ui.checkbox(&mut self.part_lcd_toggle, "LCD?");
                                        ui.add_space(ui.available_width() / 2.0);
                                        let file_upload =
                                            ui.selectable_label(false, "Upload Picture");

                                        if file_upload.clicked() {
                                            let data_clone = Arc::clone(&self.files);
                                            PlatformSpawner::spawn(async move {
                                                #[cfg(not(target_arch="wasm32"))]
                                                {
                                                    let mut data = data_clone.lock().await;
                                                    *data = AsyncFileDialog::new().pick_files().await;
                                                }

                                                #[cfg(target_arch="wasm32")]
                                                {
                                                    let mut data = data_clone.lock().unwrap();
                                                    *data = AsyncFileDialog::new().pick_files().await;
                                                }

                                            });
                                        };
                                        if toggle.clicked() {
                                            info!("self.part_lcd_toggle: {}", self.part_lcd_toggle);
                                        }
                                    });

                                    ui.add_space(15.0);

                                    ui.horizontal_top(|ui| {
                                        if Button::new("Submit")
                                            .min_size(Vec2::new(50.0, 20.0))
                                            .ui(ui)
                                            .clicked()
                                        {
                                            let location = match location {
                                                Store::RIV => "1".to_string(),
                                                Store::LTN => "2".to_string(),
                                                Store::MUR => "4".to_string(),
                                                Store::AF => "7".to_string(),
                                                Store::WJ => "5".to_string(),
                                                Store::ORE => "8".to_string(),
                                                Store::SAN => "6".to_string(),
                                            };

                                            let spo = SpecialPartOrder {
                                                customer_name: self.customer_name.clone(),
                                                customer_phone_number: self
                                                    .customer_phone_number
                                                    .clone(),
                                                application_comment: self
                                                    .application_comment
                                                    .clone(),
                                                system_order_number: self
                                                    .system_order_number
                                                    .clone(),
                                                id_location: location,
                                                request_type: self.request_type.clone(),
                                                shipping_method: self.shipping_method.clone(),
                                                part_manufacturer: self.part_manufacturer.clone(),
                                                manufacturer_model_number: self
                                                    .manufacturer_model_number
                                                    .clone(),
                                                manufacturer_serial_number: self
                                                    .manufacturer_serial_number
                                                    .clone(),
                                                manufacturer_part_number: self
                                                    .manufacturer_part_number
                                                    .clone(),
                                                part_color: self.part_color.clone(),
                                                part_description: self.part_description.clone(),
                                                part_lcd_toggle: self.part_lcd_toggle.clone(),
                                                spo_status: self.spo_status.clone(),
                                                files: self.files.clone(),
                                            };

                                            let data_clone = Arc::clone(&self.files);

                                            PlatformSpawner::spawn(async move {
                                                let mut _bytes: Bytes = Bytes::new();
                                                let mut _file_name = String::new();
                                                #[cfg(not(target_arch="wasm32"))]
                                                {
                                                    let data = data_clone.lock().await;
                                                    if let Some(ref files) = *data {
                                                        for file_handle in files {
                                                            _file_name = file_handle.file_name();
                                                            _bytes = Bytes::copy_from_slice(
                                                                file_handle.read().await.as_slice(),
                                                            );
                                                            info!("file_name: {:?}", _file_name);
                                                        }
                                                    }
                                                }

                                                #[cfg(target_arch="wasm32")]
                                                {
                                                    let data = data_clone.lock().unwrap();
                                                    if let Some(ref files) = *data {
                                                        for file_handle in files {
                                                            _file_name = file_handle.file_name();
                                                            _bytes = Bytes::copy_from_slice(
                                                                file_handle.read().await.as_slice(),
                                                            );
                                                            info!("file_name: {:?}", _file_name);
                                                        }
                                                    }
                                                }

                                                let params: Value = serde_json::json!({
                                                    "user_email": "logan.lees@pclaptops.com",
                                                    "user_password": "Poolparty1",
                                                    "format_data": "text",
                                                    "action": "create",
                                                    "application": "customer_request_order",
                                                    "payload": spo,
                                                });

                                                let client = Client::new();
                                                client
                                                    .post(
                                                        "https://scaffold.pclaptops.com/api/index",
                                                    )
                                                    .header(CONTENT_TYPE, "application/json")
                                                    .header(ACCEPT, "application/json")
                                                    .json(&params)
                                                    .send()
                                                    .await
                                                    .unwrap();
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

/*
 * 1 Riverdale [RIV]
 * 2 Layton [LTN]
 * 3 Salt Lake City [SLC]
 * 4 Murray [MUR]
 * 5 West Jordan [WJ]
 * 6 Sandy [SAN]
 * 7 American Fork [AF]
 * 8 Orem [ORE]
*/
