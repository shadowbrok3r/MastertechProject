use eframe::egui::{Align, Button, Color32, ComboBox, Direction, FontId, Layout, Margin, RichText, TextEdit, TopBottomPanel, Ui, Vec2, Widget};
use crate::{chats::ChatView, get_current_user_from_auth, get_database_users, DisplayModal, Interaction, PlatformSpawner, Spawner};
use database::{schema::{utilities::{delete_task, PhoneNumberFormatter}, Store, TaskPayload, User}};
use egui_taffy::{taffy::{self, prelude::line}, tui, TuiBuilderLogic};
use taffy::{prelude::{fr, length, percent}, Style as TaffyStyle};
use reqwest::{header::{ACCEPT, CONTENT_TYPE}, Client};
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
    pub title: String,
    pub current_page_state: ModalAction,
    pub task: TaskPayload,
    #[serde(skip)]
    pub chat_view: ChatView,
    pub min_width: Option<f32>,
    pub min_height: Option<f32>,
    pub default_height: Option<f32>,
    pub spo: SpecialPartOrder,
    store_users: Vec<User>,
    user: User
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
    pub fn new(chat_view: ChatView, mut task: TaskPayload) -> Self {

        if let Some(ticket) = task.service_ticket.as_mut() {
            if let Some(customer) = ticket.customer.as_mut() {
                let mut formatter = PhoneNumberFormatter::default();
                let phone_num = customer.phone_number.to_string();
                customer.phone_number = formatter.format_phone_number(&phone_num).unwrap_or(phone_num);
            }
        }

        Self {
            title: task.task_name.clone(),
            current_page_state: ModalAction::TicketInfoPage,
            task,
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
}

impl DisplayModal for TaskModal {
    fn display(&mut self, ui: &mut Ui, action_handler: &mut dyn FnMut(ModalAction)) -> Option<ModalAction> {
        let avail_size = Vec2::new(715.0, 700.0);
        let max_space = Vec2::new(715.0, 700.0);
        ui.set_min_size(avail_size);
        ui.set_max_size(max_space);
        ui.style_mut().override_font_id = Some(FontId::proportional(13.0));

        TopBottomPanel::top(format!("Top panel header {}", self.task.id.key().to_string())).exact_height(28.).show_inside(ui, |ui| {
            tui(ui, format!("Grid  layout 4 {}", self.task.id.key().to_string()))
            .reserve_available_space()
            .style(TaffyStyle {
                display: taffy::Display::Flex,
                flex_direction: taffy::FlexDirection::Row,
                justify_content: Some(taffy::JustifyContent::SpaceBetween),
                size: percent(1.),
                gap: length(0.0),  
                ..Default::default()
            })
            .show(|tui| {
                tui.style(TaffyStyle {
                    flex_grow: 1.0,                  // ← equal slice of row
                    flex_basis: percent(0.0),        // ← ignore intrinsic width
                    align_self: Some(taffy::AlignItems::Stretch), // stretch to full row height
                    flex_direction: taffy::FlexDirection::Column,
                    align_content: Some(taffy::AlignContent::Center),
                    ..Default::default()
                }).add(|tui| {
                    tui.ui(|ui| {
                        let full_w = ui.available_width();
                        let delete_btn = Button::new(
                            RichText::new("Delete Task").color(Color32::LIGHT_RED),
                        )
                        .min_size([full_w/1.7, 22.0].into())
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
                });

                tui.style(TaffyStyle {
                    flex_grow: 1.0,                  // ← equal slice of row
                    flex_basis: percent(0.0),        // ← ignore intrinsic width
                    align_self: Some(taffy::AlignItems::Center), // stretch to full row height
                    flex_direction: taffy::FlexDirection::Column,
                    align_content: Some(taffy::AlignContent::Center),
                    align_items: Some(taffy::AlignItems::Center),
                    ..Default::default()
                }).add_with_border(|tui| {
                    tui.ui(|ui| {
                        ui.with_layout(
                            Layout::left_to_right(Align::Center),
                            |ui| {
                                macro_rules! icon {
                                    ($state:expr, $page:expr, $glyph:literal) => {
                                        if ui
                                            .add_sized(
                                                [22., 22.],
                                                eframe::egui::SelectableLabel::new(
                                                    $state == $page,
                                                    RichText::new($glyph).heading(),
                                                ),
                                            )
                                            .clicked()
                                        {
                                            $state = $page;
                                        }
                                    };
                                }

                                // if self.task.service_ticket.is_some() {
                                    icon!(self.current_page_state, ModalAction::TicketInfoPage,   "🖹");
                                    icon!(self.current_page_state, ModalAction::ComputerInfoPage, "🖥");
                                    icon!(self.current_page_state, ModalAction::SoftwareInfoPage, "💾");
                                // } else {
                                    // icon!(self.current_page_state, ModalAction::TaskPage, "🖹");
                                // }
                                // icon!(self.current_page_state, ModalAction::JobBuilderPage, "📝");
                                icon!(self.current_page_state, ModalAction::TaskNotePage,  "💬");
                            },
                        );
                    });
                });

                tui.style(TaffyStyle {
                    flex_grow: 1.0,                  // ← equal slice of row
                    flex_basis: percent(0.0),        // ← ignore intrinsic width
                    align_self: Some(taffy::AlignItems::Stretch), // stretch to full row height
                    flex_direction: taffy::FlexDirection::Column,
                    align_content: Some(taffy::AlignContent::Center),
                    ..Default::default()
                }).add(|tui| {
                    tui.ui(|ui| {
                        ui.with_layout(
                            Layout::right_to_left(Align::Center),
                            |ui| 
                        {
                            ui.push_id(
                                format!("Completed {}", self.task.completed),
                                |ui| self.task.interact_completed(ui),
                            );
                            ui.add_space(6.0);
                            ui.colored_label(Color32::LIGHT_RED, "Completed:");
                        });
                    });
                });
            });
        });

        ui.add_space(10.);
        tui(ui, format!("Grid layout {}", self.task.id.key().to_string()))
            .reserve_space(max_space)
            .style(TaffyStyle {
                display: taffy::Display::Grid,
                grid_template_columns: vec![
                    length(15.0),   // left gutter
                    fr(1.0),      // stretchy middle track
                    length(10.0),   // right gutter
                ],
                grid_template_rows: vec![fr(1.0)],
                ..Default::default()
            })
            .show(|tui| {
                tui.style(TaffyStyle {
                    grid_column: line(1),
                    ..Default::default()
                })
                .add_empty();
                // put the page body in column 2 (1‑based index)
                tui.style(TaffyStyle {
                    grid_column: line(2),
                    ..Default::default()
                })
                .add(|tui| {
                    // tui.ui(|ui| ui.set_min_width(ui.available_width()) );
                    tui.ui(|ui| {
                        let store_users = self.store_users.clone();
                        // Ensure full width and height
                        ui.set_min_width(ui.available_width());
                        ui.set_min_height(ui.available_height()/1.1);
                        match self.current_page_state {
                            ModalAction::TicketInfoPage   => display_ticket_page(ui, &mut self.task, avail_size, &store_users, self.user.clone()),
                            ModalAction::ComputerInfoPage => display_computer_page(ui, &mut self.task, avail_size),
                            ModalAction::SoftwareInfoPage => display_software_page(ui, &mut self.task, avail_size),
                            ModalAction::JobBuilderPage   => display_job_builder_page(ui),
                            ModalAction::TaskNotePage     => self.chat_view.ui(ui),
                            // ModalAction::TaskPage         => display_task_page(ui, &mut self.task, avail_size),
                            _ => {}
                        }
                    });
                });
                
                tui.style(TaffyStyle {
                    grid_column: line(3),
                    ..Default::default()
                })
                .add_empty();
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
