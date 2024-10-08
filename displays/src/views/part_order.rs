use eframe::egui::{Align, Button, ComboBox, Direction, Layout, Margin, TextEdit, Ui, Vec2, Widget};
use super::{displays::chats::ChatView, DisplayModal, ModalTypes};
use egui_extras::{Size, StripBuilder};
use database::schema::SpecialPartOrder;
use serde_json::Value;
use log::info;

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

                                    spawn(async move {
                                        // let mut bytes: Bytes = Bytes::new();
                                        // let mut file_name = String::new();

                                        // if let Some(task) = task{
                                        //     let files = task.await.unwrap();
                                        //     for file_handle in files {
                                        //         file_name = file_handle.file_name();
                                        //         bytes = Bytes::copy_from_slice(file_handle.read().await.as_slice());
                                        //     }
                                        // }

                                        let _params: Value = serde_json::json!({
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