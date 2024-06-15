use eframe::egui::{vec2, Align, Button, Color32, ComboBox, FontId, Grid, Id, Layout, RichText, ScrollArea, Stroke, TextEdit, Ui, Vec2, Widget };
use get_ticket::SendRequest;
use std::path::PathBuf; 
use log::{debug, info};
use tokio::spawn;
use egui_extras::{*, DatePickerButton};
use egui_file::FileDialog;
use crate::{database::GetKeysResponse, app_state::MastertechContext};


pub mod get_ticket;
pub mod submit_tur;
pub mod submit_tur_mtech;
pub mod submit_tur_email;
pub mod email_builder;
pub mod scaffold;

impl MastertechContext {
    pub fn tur_sheet(&mut self, ui: &mut Ui) {
        ui.style_mut().spacing.button_padding = (4.0, 7.0).into();
        ui.shrink_width_to_current();
        ui.shrink_height_to_current();
        ui.style_mut().visuals.selection.stroke.color =  Color32::BLACK;
        ui.style_mut().visuals.selection.bg_fill = Color32::from_rgb(120, 10, 120);
        ui.style_mut().visuals.widgets.inactive.fg_stroke =  Stroke::new(1.0, Color32::WHITE);
        ui.style_mut().visuals.widgets.inactive.weak_bg_fill =  Color32::from_rgb(20, 20, 25);
        ui.style_mut().visuals.widgets.inactive.bg_stroke =  Stroke::new(1.0, Color32::from_rgb(80, 80, 80));
        ui.style_mut().visuals.widgets.open.bg_fill =  Color32::from_black_alpha(50);
        ui.style_mut().visuals.widgets.open.weak_bg_fill =  Color32::from_black_alpha(50);
        ui.style_mut().visuals.widgets.active.weak_bg_fill =  Color32::from_rgb(30,30,30);
        ui.style_mut().visuals.widgets.hovered.weak_bg_fill =  Color32::TRANSPARENT;
        ui.style_mut().visuals.widgets.hovered.bg_fill =  Color32::from_rgb(12, 12, 12);
        ui.style_mut().visuals.widgets.hovered.bg_stroke =  Stroke::new(1.0, Color32::from_rgb(200, 20, 200));

        ui.vertical(|ui|{ui.add_space(8.0);});
        
        ui.with_layout(
            Layout::left_to_right(Align::Center),|ui|
        {     
            ui.horizontal(|ui| {ui.add_space(8.0);});
            StripBuilder::new(ui)
            .cell_layout(Layout::left_to_right(Align::Center))
            .size(Size::exact(174.0)) // allocates top two strips from top -> bottom
            .size(Size::exact(8.0)) // space between top and bottom strips
            .size(Size::exact(235.0)) // allocates bottom two strips from top -> bottom
            .vertical(|mut strip|
            { 
                strip
                .strip(|builder|
                { 
                    builder
                    .size(Size::exact(290.0)) // allocates ticket info from left -> right
                    .size(Size::exact(8.0)) // allocates empty space between HW tests and ticket info
                    .size(Size::exact(290.0)) // allocates HW tests from left -> right
                    .horizontal(|mut strip|
                    { 
                        strip
                        .strip(|builder|
                        {
                            builder
                            .size(Size::exact(30.0)) // 30 top to bottom get_ticket button
                            .size(Size::remainder()) //
                            .vertical(| mut strip|
                            {
                                strip
                                .cell(|ui| // get_ticket button
                                {
                                    let check = !self.so_number.is_empty();

                                    ui.horizontal_top(|ui|{
                                        if ui.add_enabled(
                                            check,
                                            Button::new(
                                                RichText::new("Get Ticket")
                                                    .color(Color32::from_rgb(255, 204, 255))  
                                            )
                                            .stroke(
                                                Stroke::new(1.0, Color32::from_rgb(191, 33, 101))
                                            ).min_size(Vec2::new(145.0, 25.0))
                                        )
                                        .clicked()
                                        { 
                                            self.output_text.clear();
                                            let service_num = self.so_number.clone();
                                            if !service_num.is_empty() && service_num.len() == 8{
                                                self.output_text = "Its Everest, this may take a 'moment'".to_string();
                                                self.spinner = true;
 
                                                SendRequest::get_ticket(service_num, self.scaffold_request.tx.clone(), self.client.clone()); 
                                            }else{
                                                self.output_text = "Didn't enter SO number or SO number < 8 digits".to_string();
                                            }

                                        } 
                                    
                                        ui.add_enabled(
                                            check, 
                                            Button::new( 
                                                RichText::new("Get PrestaShop")
                                                .color(Color32::from_rgb(255, 204, 255)) 
                                            )
                                            .stroke(
                                                Stroke::new(1.0, Color32::from_rgb(191, 33, 101))
                                            ).min_size(
                                                Vec2::new(145.0, 25.0)
                                            )
                                        )
                                    });// horizontal_top
                                }); // strip cell

                                strip
                                .cell(|ui| // ticket_info_grid fields
                                {
                                    //ui.vertical(|ui|{ui.add_space(8.0);});
                                    ui
                                    .group(|ui|
                                    {
                                        ui
                                        .vertical_centered_justified(|ui|
                                        {
                                            ui
                                            .horizontal_top(|ui|
                                            {
                                                Grid::new("ticket_info_grid")
                                                .spacing(vec2(4.0, 7.0))
                                                .min_col_width(self.widget_size+3.0)
                                                .max_col_width(self.widget_size + 8.0)
                                                .num_columns(2)
                                                .show(ui, |ui| 
                                                {
                                                                        /*     ROW 1     */
                                                    ui.add(
                                                        TextEdit::singleline(&mut self.so_number)
                                                        .hint_text("Service #  ")
                                                        .char_limit(8)
                                                        .vertical_align(Align::Center)
                                                        .margin(vec2(4.0, 4.0))
                                                        .min_size(vec2(self.widget_size+2.0,14.0))
                                                    );

                                                    ui.add(
                                                        TextEdit::singleline(&mut self.ticket_info.customer_name)
                                                        .hint_text("Customer Name  ")
                                                        .vertical_align(Align::Center)
                                                        .margin(vec2(4.0, 4.0))
                                                        .min_size(vec2(self.widget_size+2.0,14.0))
                                                    );

                                                    ui.end_row();

                                                                        /*     ROW 2     */
                                                    ui.add(
                                                        TextEdit::singleline(&mut self.ticket_info.customer_phone_1)
                                                        .hint_text("Phone Number 1")
                                                        .vertical_align(Align::Center)
                                                        .margin(vec2(4.0, 4.0))
                                                        .min_size(vec2(self.widget_size+2.0,14.0))
                                                    );
                                                    ui.add(
                                                        TextEdit::singleline(&mut self.ticket_info.customer_phone_2)
                                                        .hint_text("Phone Number 2")
                                                        .vertical_align(Align::Center)
                                                        .margin(vec2(4.0, 4.0))
                                                        .min_size(vec2(self.widget_size+2.0,14.0))
                                                    );     
                                                    
                                                    ui.end_row();

                                                                        /*     ROW 3     */
                                                    ui.add(
                                                        TextEdit::singleline(&mut self.salesman)
                                                        .hint_text("Salesman initials")
                                                        .vertical_align(Align::Center)
                                                        .margin(vec2(4.0, 4.0))
                                                        .min_size(vec2(self.widget_size+2.0,14.0))
                                                    );    
                                                    ui.add(
                                                        TextEdit::singleline(&mut self.technician)
                                                        .hint_text("Technician initials")
                                                        .vertical_align(Align::Center)
                                                        .margin(vec2(4.0, 4.0))
                                                        .min_size(vec2(self.widget_size+2.0,14.0))
                                                    );    
                                                    // ComboBox::from_id_source("salesman_cbox").width(self.widget_size)
                                                    // .selected_text(format!("{:?}", self.salesman_cbox))
                                                    // .show_ui(ui, |ui| {
                                                    //     ui.selectable_value(&mut self.salesman_cbox, scaffold::Salesman::Jake, "Jake");
                                                    //     ui.selectable_value(&mut self.salesman_cbox, scaffold::Salesman::Danny, "Danny");
                                                    // });


                                                    // ComboBox::from_id_source("techs_cbox").width(self.widget_size)
                                                    // .selected_text(format!("{:?}", self.techs_cbox))
                                                    // .show_ui(ui, |ui| {
                                                        
                                                    //     ui.selectable_value(&mut self.techs_cbox, scaffold::Techs::Logan, "Logan");
                                                    //     ui.selectable_value(&mut self.techs_cbox, scaffold::Techs::Bread, "Bread");
                                                    //     ui.selectable_value(&mut self.techs_cbox, scaffold::Techs::Taco, "Taco");
                                                    // });    
                                                    
                                                    ui.end_row();
                                                                        /*     ROW 4     */
                                                    if ui.add_enabled(!self.so_number.is_empty(), Button::new("Get Keys").min_size(vec2(self.widget_size, 3.0)))
                                                    .clicked(){ 
                                                        let service_num = self.so_number.clone();
                                                        self.spinner = true;

                                                        let cps_request = SendRequest::get_cps(service_num, self.client.clone());
                                                        let cps_tx = self.cps_keys_tx.clone();

                                                        spawn(async move{
                                                            
                                                            let unwrapped_request =  cps_request.await.unwrap_or(GetKeysResponse::default());

                                                            match cps_tx.send(unwrapped_request){
                                                                Ok(_) => info!("GetKeysClick -> sent keys successfully"),
                                                                Err(err) => debug!("GetKeysClick -> Error propogating GetKeysResponse to callee -> {err:?}")
                                                            }
                                                        });

                                                        if let Ok(keys) = self.cps_keys_rx.recv(){
                                                            if keys.webroot_key.contains("Error"){
                                                                self.output_text = "Error fetching Keys. Is SW\\/PCLCPS\\/O on ticket?".to_string();
                                                            }
                                                            self.keys = keys;
                                                        }else{
                                                            debug!("GetKeysClick Receive Error");
                                                            self.output_text = format!("GetKeysClick -> Error receiving keys");
                                                        }
                                                    }
                                                    
                                                    if ui.add_enabled(!self.so_number.is_empty(), Button::new("Check SEB").min_size(vec2(self.widget_size, 3.0)))
                                                    .clicked(){ 
                                                        // request_seb_info(self.client, Some(self.ticket_info.customer_email)).await.unwrap();
                                                    }
                        
                                                    ui.end_row();
                                                    
                                                                        /*     ROW 5     */
                                                    if ui.add(Button::new(RichText::new(format!("{}", self.keys.webroot_key))//.size()
                                                    .color(Color32::from_rgb(102, 255, 153)))
                                                    .min_size(vec2(self.widget_size + 2.0, 15.0)))
                                                    .on_hover_text("Click To Copy Webroot Key to Clipboard")
                                                    .clicked(){ 
                                                        let webroot = self.keys.webroot_key.clone();
                                                        ui.output_mut(|o| o.copied_text = webroot);
                                                    }
                                                        
                                                    if ui.add(Button::new(RichText::new(format!("{}", self.keys.superanti_key))//.size()
                                                    .color(Color32::from_rgb(255, 61, 126)))
                                                    .min_size(vec2(self.widget_size + 2.0, 15.0)))
                                                    .on_hover_text("Click To Copy SAS Key to Clipboard")
                                                    .clicked(){ 
                                                        let sas = self.keys.superanti_key.clone();
                                                        ui.output_mut(|o| o.copied_text = sas);
                                                    }

                                                    ui.end_row();
                                                }); // grid
                                            });
                                        }); // v center justified
                                    });
                                }); // strip cell
                            });
                        });
                        
                        strip.empty();

                        strip
                        .cell(|ui|
                        {
                            ui
                            .vertical_centered(|ui|
                            {
                                ui.vertical(|ui| ui.add_space(30.0));
                                ui
                                .group(|ui|
                                {
                                    Grid::new("drive_tests")
                                    .spacing(vec2(4.0, 6.0))
                                    .min_col_width(self.widget_size)
                                    .num_columns(2)
                                    .show(ui, |ui| {
                                                            /*     ROW 1     */
                                        ui.vertical_centered(|ui|  {
                                            ComboBox::from_id_source("ssd_cbox").width(self.widget_size - 5.0)
                                            .selected_text(format!("{}", self.ssd_test_cbox.as_str()))
                                            .show_ui(ui, |ui| {
                                                ui.selectable_value(&mut self.ssd_test_cbox, scaffold::HardwareTest::SsdFail, "SSD Fail");
                                                ui.selectable_value(&mut self.ssd_test_cbox, scaffold::HardwareTest::SsdPass, "SSD Pass");
                                                ui.selectable_value(&mut self.ssd_test_cbox, scaffold::HardwareTest::SsdNotTested, "SSD Not Tested");
                                            }); // Combo Box
                                        });

                                        let date = self.date.get_or_insert_with(||chrono::offset::Utc::now());
                                        ui.vertical_centered(|ui| DatePickerButton::new(&mut date.date_naive()).ui(ui));
                                        ui.end_row();
                                                            /*     ROW 2     */
                                        ui.vertical_centered(|ui|  {
                                            ComboBox::from_id_source("hdd_cbox").width(self.widget_size - 5.0)
                                            .selected_text(format!("{}", self.hdd_test_cbox.as_str()))
                                            .show_ui(ui, |ui| {
                                                ui.selectable_value(&mut self.hdd_test_cbox, scaffold::HardwareTest::HddFail, "HDD Fail");
                                                ui.selectable_value(&mut self.hdd_test_cbox, scaffold::HardwareTest::HddPass, "HDD Pass");
                                                ui.selectable_value(&mut self.hdd_test_cbox, scaffold::HardwareTest::HddNotTested, "HDD Not Tested");
                                            }); // Combo Box
                                        });

                                        ui.checkbox(&mut self.send_specs, "Send System Info");
                                        ui.end_row();

                                        ui.vertical_centered(|ui|  {
                                            ComboBox::from_id_source("ram_cbox").width(self.widget_size - 5.0)
                                            .selected_text(format!("{}", self.ram_test_cbox.as_str()))
                                            .show_ui(ui, |ui| {
                                                ui.selectable_value(&mut self.ram_test_cbox, scaffold::HardwareTest::RamFail, "RAM Fail");
                                                ui.selectable_value(&mut self.ram_test_cbox, scaffold::HardwareTest::RamPass, "RAM Pass");
                                                ui.selectable_value(&mut self.ram_test_cbox, scaffold::HardwareTest::RamNotTested, "RAM Not Tested");
                                            }); // Combo Box
                                        });

                                        let mut attached_file = PathBuf::new();
                                        let mut _hovered_file_txt = "";
                                        // let hovered_files = ui.input_mut(|i| i.raw.take().hovered_files);
                                        // for hovered_file in hovered_files{
                                        //     if let Some(files) = hovered_file.path{
                                        //         hovered_file_txt = files.file_name().unwrap().to_str().unwrap();
                                        //     }
                                        // }
                                        let dropped_files = ui.input_mut(|i| i.raw.take().dropped_files);
                                        for dropped_file in dropped_files{
                                            if let Some(dropped_files) = dropped_file.path{
                                                self.opened_file = Some(dropped_files);
                                            }
                                        }
                                        
                                        if let Some(file) = &self.opened_file{
                                            attached_file = file.to_path_buf();
                                        }
    
                                        // Extract just the file name from the PathBuf
                                        let file_name = attached_file.file_name()
                                        .and_then(|name| name.to_str())
                                        .unwrap_or("");
    
                                        ui.vertical_centered(|ui|  {
                                            let upload_button = ui.add(Button::new(
                                                RichText::new(
                                                        format!("Upload 🗋 {{ {} }}", file_name)
                                                    )
                                                )
                                                .min_size(vec2(self.widget_size, 8.0))
                                            ); //.on_hover_text(format!("{}", &hovered_file_txt));
        
                                            if upload_button
                                            .clicked()
                                            {
                                                let mut dialog = FileDialog::open_file(self.opened_file.clone())
                                                .id(Id::new("File Dialog"));
                                                dialog.open();
                                                self.open_file_dialog = Some(dialog);
                                            };
                                        });

                                        ui.end_row();
                                    }); // Grid  
                                }); // group

                                ui.vertical(|ui| ui.add_space(15.0));

                                ui
                                .horizontal_top(|ui|
                                {
                                    let check = !self.so_number.is_empty() 
                                        && !self.ticket_info.customer_name.is_empty() 
                                        && !self.ticket_info.customer_phone_1.is_empty() 
                                        && !self.salesman.is_empty() 
                                        && !self.technician.is_empty();                                    
                                    if ui
                                    .add_enabled(
                                        check,
                                        Button::new
                                        (
                                            RichText::new("Submit TUR")
                                                .color(Color32::from_rgb(255, 204, 255))
                                        )
                                            .stroke(Stroke::new(1.0, Color32::from_rgb(191, 33, 101)))
                                    )
                                    .clicked()
                                    {  
                                        self.submit_tur();
                                    }


                                    let check = !self.so_number.is_empty() 
                                        && !self.ticket_info.customer_name.is_empty() 
                                        && !self.ticket_info.customer_phone_1.is_empty() 
                                        && !self.salesman.is_empty() 
                                        && !self.technician.is_empty();    
                                    if ui
                                        .add_enabled(check, Button::new( RichText::new("Master-Tech.app")))
                                        .clicked()
                                    {  
                                       self.submit_tur_mastertech(); 
                                    }
                                    
                                    let connect_to_websocket = ui.add(
                                        Button::new(
                                            RichText::new("Connect WS")
                                        )
                                    );
                                    if connect_to_websocket.clicked(){
                                        self.connect_to_ws = true;
                                        self.disconnect_ws = false;
                                    }
                                    if connect_to_websocket.secondary_clicked(){
                                        self.disconnect_ws = true;
                                        self.connect_to_ws = false;
                                    }
                                }); // horizontal_top
                            }); // vertical center
                        }); // cell
                    }); // strip.strip builder
                }); // strip.strip

                strip.empty();

                strip.strip(|builder|
                {
                    builder
                    .size(Size::exact(300.0)) // allocates checkinNotes info from left -> right
                    .size(Size::exact(-5.0)) // allocates empty space between checkin notes and recommendations
                    .size(Size::exact(300.0)) // allocates recommendations from left -> right
                    .horizontal(|mut strip|
                    {
                        strip
                        .cell(|ui|
                        {
                            ScrollArea::new([false, true])
                            .id_source("checkin_notes_scroll")
                            .show(ui, |ui|{
                                ui.add_sized(
                                    vec2(ui.available_width()-4.0, ui.available_height() - 80.0),
                                    TextEdit::multiline(&mut self.ticket_info.checkin_notes)
                                    .hint_text(RichText::new("Checkin Notes").weak())
                                    .font(FontId::proportional(15.0))
                                    .desired_rows(4)
                                );
                            });
                            ui.shrink_height_to_current(); 
                        }); // cell

                        strip.empty();

                        strip.cell(|ui|
                        {
                            ScrollArea::new([false, true])
                            .id_source("recomendations_scroll")
                            .show(ui, |ui|{
                                ui.add_sized(
                                    vec2(ui.available_width()-4.0, ui.available_height() - 80.0), 
                                    TextEdit::multiline(&mut self.recommendations)
                                    .hint_text(RichText::new("Recommendations").weak())
                                    .font(FontId::proportional(15.0))
                                    .desired_rows(4)
                                );
                            });
                            ui.shrink_height_to_current(); 
                        }); // cell
                    }); // strip builder
                }); // strip.strip
            }); //strip builder
        }); // UI layout
    }

}