use eframe::egui::{Align, Button, Color32, ComboBox, Direction, FontId, Id, Layout, Margin, RichText, TextEdit, TopBottomPanel, Ui, UiBuilder, Vec2, Widget};
use crate::{chats::ChatView, get_current_user_from_auth, get_database_users, DisplayModal, Interaction, PlatformSpawner, Spawner};
use database::schema::{utilities::{delete_task, PhoneNumberFormatter}, ComputerData, CustomerData, LiveTaskPayload, Store, TaskNotePayload, TicketData, User};
use reqwest::{header::{ACCEPT, CONTENT_TYPE}, Client};
use crossbeam::channel::{Receiver, Sender};
use rfd::{AsyncFileDialog, FileHandle};
use egui_extras::{Size, StripBuilder};
use serde_json::Value;
use serde::Serialize;
use std::sync::Arc;
use bytes::Bytes;
use core::f32;
use log::info;

use super::tabs::{display_computer_page, display_job_builder_page, display_software_page, display_ticket_page};

#[cfg(target_arch="wasm32")]
use std::sync::Mutex;

#[cfg(not(target_arch="wasm32"))]
use tokio::sync::Mutex;


// use super::ModalState;

#[derive(Serialize, Clone, Debug)]
pub struct TaskModal {
    pub task: LiveTaskPayload,
    pub service_ticket: Option<TicketData>,
    pub customer: Option<CustomerData>,
    pub computer: Option<ComputerData>,
    pub title: String,
    pub current_page_state: ModalAction,
    #[serde(skip)]
    pub chat_view: ChatView,
    pub min_width: Option<f32>,
    pub min_height: Option<f32>,
    pub default_height: Option<f32>,
    pub spo: SpecialPartOrder,
    store_users: Vec<User>,
    user: User,
    #[serde(skip)]
    pub service_ticket_tx: Sender<TicketData>,
    #[serde(skip)]
    pub service_ticket_rx: Receiver<TicketData>,
    #[serde(skip)]
    pub customer_tx: Sender<CustomerData>,
    #[serde(skip)]
    pub customer_rx: Receiver<CustomerData>,
    #[serde(skip)]
    pub computer_tx: Sender<ComputerData>,
    #[serde(skip)]
    pub computer_rx: Receiver<ComputerData>,
    #[serde(skip)]
    pub initial_notes_tx: Sender<Vec<TaskNotePayload>>,
    #[serde(skip)]
    pub initial_notes_rx: Receiver<Vec<TaskNotePayload>>,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub enum ModalAction {
    #[default]
    TicketInfoPage,
    PartOrderPage,
    SoftwareInfoPage,
    ComputerInfoPage,
    JobBuilderPage,
    TaskNotePage,
    ImportTask,
    Close,
    // TaskPage,
    None,
}

impl TaskModal {
    pub fn new(chat_view: ChatView, task: LiveTaskPayload) -> Self {
        let (service_ticket_tx, service_ticket_rx) = crossbeam::channel::unbounded();
        let (customer_tx, customer_rx) = crossbeam::channel::unbounded();
        let (computer_tx, computer_rx) = crossbeam::channel::unbounded();
        let (initial_notes_tx, initial_notes_rx) = crossbeam::channel::unbounded();
        let comp_tx = computer_tx.clone();
        let cust_tx = customer_tx.clone();
        let svc_tx = service_ticket_tx.clone();
        let notes_tx = initial_notes_tx.clone();
        let id = task.id.clone();
        let service_number = task.service_number.clone();
        PlatformSpawner::spawn(async move {
            match TicketData::get_associated_ticket(id.clone()).await {
                Ok(ticket) => { let _ = svc_tx.try_send(ticket); },
                Err(e) => log::error!("Error getting ticket data: {e:?}"),
            }
            match ComputerData::get_associated_computer(id.clone()).await {
                Ok(computer) => { let _ = comp_tx.try_send(computer); },
                Err(e) => log::error!("Error getting ticket data: {e:?}"),
            }
            match CustomerData::get_associated_customer(id.clone()).await {
                Ok(customer) => { let _ = cust_tx.try_send(customer); },
                Err(e) => log::error!("Error getting ticket data: {e:?}"),
            }
            match TaskNotePayload::get_db_notes_from_task_id(id.clone()).await {
                Ok(notes) => { let _ = notes_tx.try_send(notes); },
                Err(e) => log::error!("Error getting notes from task ID: {e:?}"),
            }

            if let Some(service_number) = service_number {
                match TaskNotePayload::get_prestashop_notes_from_service(&service_number, Some(id.clone())).await {
                    Ok(notes) => { let _ = notes_tx.try_send(notes); },
                    Err(e) => log::error!("Error getting notes from task ID: {e:?}"),
                }
            }
        });

        Self {
            title: task.task_name.clone(),
            current_page_state: ModalAction::TicketInfoPage,
            task,
            service_ticket: None,
            customer: None,
            computer: None,
            service_ticket_tx, service_ticket_rx,
            customer_tx, customer_rx,
            computer_tx, computer_rx,
            initial_notes_tx, initial_notes_rx,
            min_width: Some(600.0),
            min_height: Some(600.0),
            default_height: Some(800.0),
            chat_view,
            spo: SpecialPartOrder::default(),
            store_users: get_database_users(),
            user: if let Some(user) = get_current_user_from_auth() {
                user
            } else {
                User::default()
            }
        }
    }

    fn receive(&mut self) {
        if let Ok(service_ticket) = self.service_ticket_rx.try_recv() {
            log::info!("Received ticket");
            self.service_ticket = Some(service_ticket);
        }
        
        if let Ok(mut customer) = self.customer_rx.try_recv() {
            log::info!("Received customer");
            let mut formatter = PhoneNumberFormatter::default();
            let phone_num = customer.phone_number.to_string();
            customer.phone_number = formatter.format_phone_number(&phone_num).unwrap_or(phone_num);
            self.customer = Some(customer);
        }
        
        if let Ok(computer) = self.computer_rx.try_recv() {
            log::info!("Received computer");
            self.computer = Some(computer);
        }

        if let Ok(notes) = self.initial_notes_rx.try_recv() {
            log::info!("Received notes: {}", notes.len());
            self.chat_view.set_notes(notes);
        } 
    }

}

impl DisplayModal for TaskModal {
    fn display(&mut self, ui: &mut Ui, action_handler: &mut dyn FnMut(ModalAction)) -> Option<ModalAction> {
        self.receive();
        let avail_size = Vec2::new(700.0, 700.0);
        let max_space = Vec2::new(715.0, 700.0);
        ui.set_min_size(max_space);
        ui.set_max_size(max_space);
        ui.style_mut().override_font_id = Some(FontId::proportional(13.0));

        TopBottomPanel::top(format!("Top panel header {}", self.task.id.key().to_string())).exact_height(28.).show_inside(ui, |ui| {

            ui.columns(3, |ui| {
                ui[0].with_layout(Layout::left_to_right(Align::Center), |ui| {
                    let delete_btn = Button::new(
                        RichText::new("Delete Task").color(Color32::LIGHT_RED),
                    )
                    .min_size([150., 22.0].into())
                    .ui(ui)
                    .on_hover_text("Double Click To Delete Task");

                    if delete_btn.double_clicked() {
                        let task_id = self.task.id.clone();
                        PlatformSpawner::spawn(async move {
                            match delete_task(task_id).await {
                                Ok(_) => info!("Deleted task"),
                                Err(e) => log::error!("Error: {e:?}"),
                            }
                        });
                        self.current_page_state = ModalAction::Close;
                    }
                });

                ui[1].vertical_centered(|ui| {
                    ui.horizontal_top(|ui| {
                        ui.add_space(75.);
                        if ui.add_sized([22., 22.], eframe::egui::Button::selectable(
                            self.current_page_state == ModalAction::TicketInfoPage,
                            RichText::new("🖹").heading()
                        ))
                        .clicked() {
                            self.current_page_state = ModalAction::TicketInfoPage;
                        }
                        if ui.add_sized([22., 22.], eframe::egui::Button::selectable(
                            self.current_page_state == ModalAction::ComputerInfoPage,
                            RichText::new("🖥").heading()
                        ))
                        .clicked() {
                            self.current_page_state = ModalAction::ComputerInfoPage;
                        }
                        if ui.add_sized([22., 22.], eframe::egui::Button::selectable(
                            self.current_page_state == ModalAction::SoftwareInfoPage,
                            RichText::new("💾").heading()
                        ))
                        .clicked() {
                            self.current_page_state = ModalAction::SoftwareInfoPage;
                        }
                        if ui.add_sized([22., 22.], eframe::egui::Button::selectable(
                            self.current_page_state == ModalAction::TaskNotePage,
                            RichText::new("💬").heading()
                        ))
                        .clicked() {
                            self.current_page_state = ModalAction::TaskNotePage;
                        }
                    });
                });

                ui[2].with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.push_id(
                        format!("Completed {}", self.task.completed),
                        |ui| self.task.interact_completed(ui),
                    );
                    ui.add_space(6.0);
                    ui.colored_label(Color32::LIGHT_RED, "Completed:");
                });
            });
        });

        ui.add_space(10.);
        ui.scope_builder(
        UiBuilder::new()
        .layout(Layout::from_main_dir_and_cross_align(Direction::LeftToRight, Align::Max)), 
        |ui| {
            let is_sizing_pass = ui.is_sizing_pass();
            let available_width = 715.;
            let total_content_width = match self.current_page_state {
                ModalAction::TicketInfoPage => 670.,
                ModalAction::SoftwareInfoPage => 670.,
                ModalAction::ComputerInfoPage => 670.,
                ModalAction::JobBuilderPage => 670.,
                ModalAction::TaskNotePage => 715.,
                _ => 715.
            };

            // In the rendering pass, add spacing to center the content
            if !is_sizing_pass && available_width > total_content_width {
                let padding = (available_width - total_content_width) / 2.0;
                ui.add_space(padding);
            }

            let store_users = self.store_users.clone();
            match self.current_page_state {
                ModalAction::TicketInfoPage   => display_ticket_page(ui, &mut self.task, self.service_ticket.as_mut(), self.customer.as_mut(), avail_size, &store_users, self.user.clone()),
                ModalAction::ComputerInfoPage => display_computer_page(ui, self.service_ticket.as_mut(), self.computer.as_mut(), avail_size),
                ModalAction::SoftwareInfoPage => display_software_page(ui, self.computer.as_mut().unwrap_or(&mut ComputerData::default()), avail_size),
                ModalAction::JobBuilderPage   => display_job_builder_page(ui),
                ModalAction::TaskNotePage     => self.chat_view.ui(ui),
                // ModalAction::TaskPage         => display_task_page(ui, &mut self.task, avail_size),
                _ => {}
            }

            let id = format!("{:?} Sizing ID", self.task.id);
            // Use egui memory to track if we've already done the sizing pass
            let sizing_pass_done = ui.memory(|mem| mem.data.get_temp::<bool>(Id::new(&id)).unwrap_or(false));

            if is_sizing_pass && !sizing_pass_done {
                ui.ctx().request_discard("Centering ComboBox sizing pass");
                ui.ctx().request_repaint();
                ui.memory_mut(|mem| mem.data.insert_temp(Id::new(&id), true));
            }

            // Reset the flag if the UI is repainted for other reasons (e.g., window resize)
            if !is_sizing_pass && sizing_pass_done {
                ui.memory_mut(|mem| mem.data.insert_temp(Id::new(&id), false));
            }
        });

        if self.current_page_state == ModalAction::Close {
            action_handler(ModalAction::Close);
        }
        Some(self.current_page_state.clone())
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
                                        .margin(Margin::same(5))
                                        .ui(ui);

                                    ui.add_space(15.0);

                                    TextEdit::singleline(&mut self.manufacturer_part_number)
                                        .hint_text("MFG P/N".to_string())
                                        .margin(Margin::same(5))
                                        .frame(true)
                                        .ui(ui);

                                    ui.add_space(15.0);

                                    TextEdit::singleline(&mut self.part_description)
                                        .hint_text("Part Description".to_string())
                                        .margin(Margin::same(5))
                                        .ui(ui);

                                    ui.add_space(15.0);

                                    TextEdit::multiline(&mut self.application_comment)
                                        .hint_text("Notes".to_string())
                                        .margin(Margin::same(5))
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
