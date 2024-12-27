use crate::{app_state::MastertechContext, tabs::tur_sheet::scaffold::HardwareTest::{HddFail, HddNotTested, HddPass, RamFail, RamNotTested, RamPass, SsdFail, SsdNotTested, SsdPass}};
use eframe::egui::{vec2, Align, Button, Color32, ComboBox, FontId, Grid, Key, KeyboardShortcut, Layout, Margin, Modifiers, RichText, ScrollArea, Stroke, TextEdit, Ui, Vec2, Widget };
use database::schema::{CustomerData, GetKeysResponse, LiveTaskPayload, LocalSebData, TicketData};
use displays::ui_tools::{autocomplete::AutoCompleteTextEdit, toasts::{Toast, ToastKind, ToastOptions}};
use get_ticket::{request_seb_info, SendRequest};
use egui_extras::{*, DatePickerButton};
use std::collections::BTreeSet;
use egui_file::FileDialog;
use std::path::PathBuf; 
use log::{debug, error, info};
use tokio::spawn;

pub mod get_ticket;
pub mod submit_tur;
pub mod submit_tur_mtech;
pub mod email_builder;
pub mod scaffold;
pub mod presta_api;


impl MastertechContext {
    pub fn tur_sheet(&mut self, ui: &mut Ui) {
        ui.style_mut().spacing.button_padding = (4.0, 7.0).into();
        ui.shrink_width_to_current();
        ui.shrink_height_to_current();
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
                                    let check = !self.ticket_data.service_number.is_empty();
                                    let style = ui.style().clone();
                                    ui.vertical_centered(|ui|{                                    
                                        if ui.add_enabled(
                                            check, 
                                            Button::new( 
                                                RichText::new("Get PrestaShop Order")
                                                .color(style.visuals.warn_fg_color) 
                                            )
                                            .stroke(style.visuals.window_stroke)
                                            .min_size(Vec2::new(145.0, 25.0))
                                        ).clicked() {
                                            let service_num = self.ticket_data.service_number.clone();
                                            self.presta_api();
                                            self.ticket_data = TicketData::default();
                                            self.task_data = LiveTaskPayload::default();
                                            self.customer_data = CustomerData::default();
                                            self.task_notes = Vec::new();
                                            self.ticket_data.service_number = service_num;
                                        }
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
                                                    let service_num = TextEdit::singleline(&mut self.ticket_data.service_number)
                                                        .hint_text("Service #  ")
                                                        .char_limit(11)
                                                        .vertical_align(Align::Center)
                                                        .margin(vec2(4.0, 4.0))
                                                        .min_size(vec2(self.widget_size+2.0,14.0)).ui(ui);

                                                    service_num.clone().on_hover_text("(hint) Press Enter to pull ticket after typing SO#");

                                                    let accepted_by_keyboard = ui.input_mut(|input| input.key_pressed(Key::Enter));

                                                    if self.ticket_data.service_number.len() == 7 && accepted_by_keyboard && service_num.has_focus() {
                                                        let service_num = self.ticket_data.service_number.clone();
                                                        self.presta_api();
                                                        self.ticket_data = TicketData::default();
                                                        self.task_data = LiveTaskPayload::default();
                                                        self.customer_data = CustomerData::default();
                                                        self.task_notes = Vec::new();
                                                        self.ticket_data.service_number = service_num;
                                                    }

                                                    TextEdit::singleline(&mut self.customer_data.name)
                                                        .hint_text("Customer Name  ")
                                                        .vertical_align(Align::Center)
                                                        .margin(vec2(4.0, 4.0))
                                                        .min_size(vec2(self.widget_size+2.0,14.0))
                                                        .ui(ui);

                                                    ui.end_row();

                                                                        /*     ROW 2     */
                                                    TextEdit::singleline(&mut self.customer_data.phone_number)
                                                        .hint_text("Phone Number 1")
                                                        .vertical_align(Align::Center)
                                                        .margin(vec2(4.0, 4.0))
                                                        .min_size(vec2(self.widget_size+2.0,14.0)).ui(ui);

                                                    TextEdit::singleline(&mut self.customer_data.phone_number_2)
                                                        .hint_text("Phone Number 2")
                                                        .vertical_align(Align::Center)
                                                        .margin(vec2(4.0, 4.0))
                                                        .min_size(vec2(self.widget_size+2.0,14.0))
                                                        .ui(ui);
                                                    
                                                    ui.end_row();

                                                                        /*     ROW 3     */
                                                    let mut inputs = BTreeSet::new();

                                                    for user in self.shared_ctx.store_users.iter(){
                                                        let parsed = user.email.split_once("@").unwrap_or(("","")).0;
                                                        inputs.insert(parsed.to_string());
                                                        // info!("Inputs: {:?}", inputs);
                                                    }
                                                    let size = vec2( self.widget_size + 2.0, 14.0 );
                                                    let _result = AutoCompleteTextEdit::new(&mut self.ticket_data.salesman, inputs.clone())
                                                        .highlight_matches(true)
                                                        .max_suggestions(3)
                                                        .set_text_edit_properties(move |text_edit| 
                                                    {
                                                        text_edit
                                                            .hint_text("Assignee")
                                                            .min_size(size)
                                                            .font(FontId::proportional(12.0))
                                                            .frame(true)
                                                            .return_key(Some(KeyboardShortcut::new(Modifiers::CTRL, Key::Enter)))
                                                            // .horizontal_align(egui::Align::Center)
                                                    })
                                                    .ui(ui);

                                                    // info!("AutoCompleteTextEdit result Assignee: {:?}", result);
                                                    let _result2 = AutoCompleteTextEdit::new(&mut self.ticket_data.tech, inputs.clone())
                                                        .highlight_matches(true)
                                                        .max_suggestions(3)
                                                        .set_text_edit_properties(move |text_edit| 
                                                    {
                                                        text_edit
                                                            .hint_text("Tech")
                                                            .min_size(size)
                                                            .font(FontId::proportional(12.0))
                                                            .frame(true)
                                                            .return_key(Some(KeyboardShortcut::new(Modifiers::CTRL, Key::Enter)))
                                                            // .horizontal_align(egui::Align::Center)
                                                    })
                                                    .ui(ui);
                                                    // info!("AutoCompleteTextEdit result Tech: {:?}", result2);

                                                    
                                                    
                                                    ui.end_row();
                                                                        /*     ROW 4     */
                                                    if ui.add_enabled(!self.ticket_data.service_number.is_empty(), Button::new("Get Keys").min_size(vec2(self.widget_size, 3.0)))
                                                    .clicked(){ 
                                                        let service_num = self.ticket_data.service_number.clone();
                                                        self.spinner = true;

                                                        let cps_request = SendRequest::get_cps(service_num, self.client.clone());
                                                        let cps_tx = self.cps_keys_tx.clone();

                                                        spawn(async move{
                                                            let unwrapped_request =  cps_request.await.unwrap_or(GetKeysResponse::default());

                                                            match cps_tx.try_send(unwrapped_request){
                                                                Ok(_) => info!("GetKeysClick -> sent keys successfully"),
                                                                Err(err) => debug!("GetKeysClick -> Error propogating GetKeysResponse to callee -> {err:?}")
                                                            }
                                                        });


                                                    }
                                                    
                                                    if ui.add_enabled(!self.ticket_data.service_number.is_empty(), Button::new("Check SEB").min_size(vec2(self.widget_size, 3.0)))
                                                    .clicked(){ 
                                                        let client = self.client.clone();
                                                        let email = self.customer_data.email.clone();
                                                        spawn(async move {
                                                            let seb_data: Result<LocalSebData, anyhow::Error> = request_seb_info(client, Some(email)).await;
                                                            match seb_data{
                                                                Ok(seb) => {
                                                                    info!("SEB: {seb:?}");
                                                                },
                                                                Err(e) => error!("Error getting SEB data: {e:?}"),
                                                            }
                                                        });
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
                                            ComboBox::from_id_salt("ssd_cbox").width(self.widget_size - 5.0)
                                            .selected_text(format!("{}", self.ssd_test_cbox.as_str()))
                                            .show_ui(ui, |ui| {
                                                ui.selectable_value(&mut self.ssd_test_cbox, SsdFail, "SSD Fail");
                                                ui.selectable_value(&mut self.ssd_test_cbox, SsdPass, "SSD Pass");
                                                ui.selectable_value(&mut self.ssd_test_cbox, SsdNotTested, "SSD Not Tested");
                                            }); // Combo Box
                                        });

                                        let date = self.date.get_or_insert_with(||chrono::offset::Utc::now());
                                        ui.vertical_centered(|ui| DatePickerButton::new(&mut date.date_naive()).ui(ui));
                                        ui.end_row();
                                                            /*     ROW 2     */
                                        ui.vertical_centered(|ui|  {
                                            ComboBox::from_id_salt("hdd_cbox").width(self.widget_size - 5.0)
                                            .selected_text(format!("{}", self.hdd_test_cbox.as_str()))
                                            .show_ui(ui, |ui| {
                                                ui.selectable_value(&mut self.hdd_test_cbox, HddFail, "HDD Fail");
                                                ui.selectable_value(&mut self.hdd_test_cbox, HddPass, "HDD Pass");
                                                ui.selectable_value(&mut self.hdd_test_cbox, HddNotTested, "HDD Not Tested");
                                            }); // Combo Box
                                        });

                                        ui.checkbox(&mut self.send_specs, "Send System Info");
                                        ui.end_row();

                                        ui.vertical_centered(|ui|  {
                                            ComboBox::from_id_salt("ram_cbox").width(self.widget_size - 5.0)
                                            .selected_text(format!("{}", self.ram_test_cbox.as_str()))
                                            .show_ui(ui, |ui| {
                                                ui.selectable_value(&mut self.ram_test_cbox, RamFail, "RAM Fail");
                                                ui.selectable_value(&mut self.ram_test_cbox, RamPass, "RAM Pass");
                                                ui.selectable_value(&mut self.ram_test_cbox, RamNotTested, "RAM Not Tested");
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
                                                .id("File Dialog");
                                                dialog.open();
                                                self.open_file_dialog = Some(dialog);
                                            };
                                        });

                                        ui.end_row();
                                    }); // Grid  
                                }); // group

                                ui.vertical(|ui| ui.add_space(15.0));

                                ui
                                .vertical_centered(|ui|
                                {
                                    let width = ui.available_width() / 2.0;
                                    let check = !self.ticket_data.service_number.is_empty()
                                        && !self.customer_data.name.is_empty()
                                        && !self.customer_data.phone_number.is_empty()
                                        && !self.ticket_data.salesman.is_empty()
                                        && !self.ticket_data.tech.is_empty();
                                    if ui
                                    .add_enabled(
                                        check,
                                        Button::new(RichText::new("Submit TUR").color(Color32::from_rgb(255, 204, 255)))
                                        .min_size(Vec2::new(width, 20.0))
                                        .stroke(Stroke::new(1.0, Color32::from_rgb(191, 33, 101)))
                                    )
                                    .clicked()
                                    {  
                                        self.submit_tur();
                                    }

                                    let check = !self.ticket_data.service_number.is_empty()
                                        && !self.customer_data.name.is_empty()
                                        && !self.customer_data.phone_number.is_empty()
                                        && !self.ticket_data.salesman.is_empty()
                                        && !self.ticket_data.tech.is_empty();

                                    // let txt = if let Some(usr) = &self.current_user {
                                    //     if usr.email == "tyler.naylor@pclaptops.com".to_string() && self.taco_first_run {
                                    //         "Bitch"
                                    //     } else { "Master-Tech.app" }
                                    // } else { "Master-Tech.app" };

                                    let button = ui.add_enabled(check, 
                                        Button::new( RichText::new("Master-Tech.app"))
                                        .stroke(Stroke::new(1.0, Color32::from_rgb(191, 33, 101)))
                                        .min_size(Vec2::new(width, 20.0))
                                    );

                                    // if self.taco_first_run && txt == "Bitch" {  sleep(Duration::from_secs(1)); self.taco_first_run = false; } 
                                    
                                    if button.clicked() {
                                        self.taco_first_run = true;
                                        info!("Submitting TUR sheet");
                                        self.output_text += "Sent TUR to Master-tech.app";
                                        if self.shared_ctx.current_user.is_some() {
                                            self.submit_tur_mastertech();
                                        } else {
                                            let toast = &mut self.shared_ctx.toasts;
                                            let error_toast = Toast {
                                                kind: ToastKind::Error,
                                                text: "You are not logged in".into(),
                                                options: ToastOptions::default()
                                                    .show_progress(true)
                                                    .duration_in_seconds(6.0)
                                            };
                                            toast.add(error_toast);
                                        }
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
                            .id_salt("checkin_notes_scroll")
                            .show(ui, |ui|{
                                ui.add_sized(
                                    vec2(ui.available_width()-4.0, ui.available_height() - 80.0),
                                    TextEdit::multiline(&mut self.ticket_data.checkin_notes)
                                    .hint_text(RichText::new("Checkin Notes").weak())
                                    .font(FontId::proportional(15.0))
                                    .margin(Margin::symmetric(10., 6.))
                                    .desired_rows(4)
                                );
                            });
                            ui.shrink_height_to_current(); 
                        }); // cell
                        strip.empty();
                        strip.cell(|ui|
                        {
                            // ScrollArea::new([false, true])
                            // .auto_shrink([true, false])
                            // .id_salt("recomendations_scroll")
                            // .show(ui, |ui|{
                                    TextEdit::multiline(&mut self.task_data.task_description)
                                    .min_size(vec2(ui.available_width()-4.0, ui.available_height() - 80.0))
                                    .hint_text(RichText::new("Recommendations").weak())
                                    .font(FontId::proportional(15.0))
                                    .margin(Margin::symmetric(10., 6.))
                                    .desired_rows(4).ui(ui);
                            // });
                            // ui.shrink_height_to_current(); 
                        }); // cell
                    }); // strip builder
                }); // strip.strip
            }); //strip builder
        }); // UI layout
    }

}
