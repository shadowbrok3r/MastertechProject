use crate::{app_state::MastertechContext, tabs::tur_sheet::scaffold::HardwareTest::{HddFail, HddNotTested, HddPass, RamFail, RamNotTested, RamPass, SsdFail, SsdNotTested, SsdPass}};
use eframe::egui::{vec2, Align, Button, Color32, ComboBox, FontId, Grid, Key, KeyboardShortcut, Margin, Modifiers, RichText, ScrollArea, Stroke, TextEdit, Ui, Vec2, Widget };
use database::schema::{helper_traits::parse_email_user, CarboniteResponse, CustomerData, GetKeysResponse, LiveTaskPayload, TicketData};
use displays::ui_tools::{autocomplete::AutoCompleteTextEdit, toasts::{Toast, ToastKind, ToastOptions}};
use std::{collections::BTreeSet, f32};
use get_ticket::SendRequest;
use std::path::PathBuf;
use log::{debug, info};
use tokio::spawn;

pub mod get_ticket;
pub mod submit_tur_mtech;
pub mod scaffold;
pub mod presta_api;

impl MastertechContext {
    pub fn tur_sheet(&mut self, ui: &mut Ui) {
        ui.style_mut().spacing.button_padding = (4.0, 7.0).into();
        ScrollArea::new([true, true])
            .auto_shrink(false)
            .max_width(1250.)
            .min_scrolled_width(1100.)
            .show(ui, |ui| 
        {
            ui.vertical_centered(|ui| {
                let enabled = if !self.ticket_data.service_number.is_empty() { true } 
                else if !self.customer_data.phone_number.is_empty() && self.ticket_data.service_number.is_empty() { true } 
                else { false };
        
                ui.add_space(10.);

                ui.horizontal(|ui| {
                    ui.add_space(250.);
                    let style = ui.style().clone();
                    if ui.add_enabled(
                        enabled, 
                        Button::new( 
                            RichText::new("Get PrestaShop Order")
                            .color(style.visuals.warn_fg_color) 
                        )
                        .stroke(style.visuals.window_stroke)
                        .min_size(Vec2::new(160.0, 25.0))
                    ).clicked() {
                        let service_num = self.ticket_data.service_number.clone();
                        self.presta_api();
                        self.ticket_data = TicketData::default();
                        self.task_data = LiveTaskPayload::default();
                        self.customer_data = CustomerData::default();
                        self.task_notes = Vec::new();
                        self.ticket_data.service_number = service_num;
                    }    
                });

                ui.add_space(10.);

                ui.horizontal_top(|ui| {
                    ui.add_space(10.);
                    ui.group(|ui| {
                        self.ticket_info_grid(ui);
                    });

                    ui.add_space(10.);

                    ui.vertical_centered(|ui| {
                        ui.group(|ui| self.device_info_grid(ui) );

                        ui.add_space(10.);

                        ui.horizontal(|ui| {
                            ui.add_space(140.);
                            ui.label(RichText::new("SEB Info").strong().heading());
                        });

                        ui.group(|ui| self.seb_info_grid(ui) );
                    });
                });

                ui.add_space(10.);

                ui.horizontal_top(|ui| {
                    ui.add_space(10.);
                    ui.group(|ui| self.recommendations_grid(ui) );
                });

            });
        });
    }

    fn ticket_info_grid(&mut self, ui: &mut Ui) {
        Grid::new("ticket_info_grid")
            .spacing(vec2(4., 7.))
            .min_col_width(150.)
            .max_col_width(150.)
            .num_columns(4)
            .show(ui, |ui| 
        {
            let text_edit_size = vec2( 140., 15.0);

            let service_num = TextEdit::singleline(&mut self.ticket_data.service_number)
                .hint_text(" Service #  ")
                .char_limit(11)
                .vertical_align(Align::Center)
                .margin(vec2(4.0, 4.0))
                .min_size(text_edit_size)
                .ui(ui);

            service_num.clone().on_hover_text("(hint) Press Enter to pull ticket after typing SO#");

            let accepted_by_keyboard = ui.input_mut(|input| input.key_pressed(Key::Enter));

            if self.ticket_data.service_number.len() > 6 && accepted_by_keyboard && service_num.lost_focus() {
                let service_num = self.ticket_data.service_number.clone();
                self.presta_api();
                self.ticket_data = TicketData::default();
                self.task_data = LiveTaskPayload::default();
                self.customer_data = CustomerData::default();
                self.task_notes = Vec::new();
                self.ticket_data.service_number = service_num;
            }

            TextEdit::singleline(&mut self.customer_data.name)
                .hint_text(" Customer Name  ")
                .vertical_align(Align::Center)
                .margin(vec2(4.0, 4.0))
                .min_size(text_edit_size)
                .ui(ui);

            TextEdit::singleline(&mut self.customer_data.phone_number)
                .hint_text(" Phone Number 1")
                .vertical_align(Align::Center)
                .margin(vec2(4.0, 4.0))
                .min_size(text_edit_size).ui(ui);

            TextEdit::singleline(&mut self.customer_data.email)
                .hint_text(" Customer email")
                .vertical_align(Align::Center)
                .margin(vec2(4.0, 4.0))
                .min_size(text_edit_size)
                .ui(ui);

            ui.end_row();

            ui.label("");

            let mut inputs = BTreeSet::new();

            for user in self.shared_ctx.store_users.iter(){
                let parsed = parse_email_user(&user.email);
                inputs.insert(parsed.to_string());
            }
            
            let _ = AutoCompleteTextEdit::new(&mut self.ticket_data.salesman, inputs.clone())
                .highlight_matches(true)
                .max_suggestions(3)
                .set_text_edit_properties(move |text_edit| 
            {
                text_edit
                    .hint_text(" Assignee")
                    .min_size(text_edit_size)
                    .desired_rows(1)
                    .font(FontId::proportional(12.0))
                    .frame(true)
                    .return_key(Some(KeyboardShortcut::new(Modifiers::CTRL, Key::Enter)))
            })
            .ui(ui);

            let _ = AutoCompleteTextEdit::new(&mut self.ticket_data.tech, inputs.clone())
                .highlight_matches(true)
                .max_suggestions(3)
                .set_text_edit_properties(move |text_edit| 
            {
                text_edit
                    .hint_text(" Tech")
                    .min_size(text_edit_size)
                    .font(FontId::proportional(12.0))
                    .frame(true)
                    .desired_rows(1)
                    .return_key(Some(KeyboardShortcut::new(Modifiers::CTRL, Key::Enter)))
            })
            .ui(ui);
            
            ui.label("");

            ui.end_row();
            ui.end_row();
            
            if ui.add_enabled(!self.ticket_data.service_number.is_empty(), 
                Button::new("Check SEB").min_size(vec2(140., 3.0))
            )
            .clicked(){ 
                let email = self.customer_data.email.clone();
                let client = self.client.clone();
                let tx = self.seb_channel.0.clone();
                if !email.is_empty() {
                    tokio::spawn(async move {
                        let response_json: Vec<CarboniteResponse> = CarboniteResponse::default()
                            .from_customer_email(email.clone(), client)
                            .await?;
                        log::info!("SEB Response: {:?}", response_json);
                        tx.try_send(response_json)?;
                        Ok::<(), anyhow::Error>(())
                    });
                }
            }
            
            if Button::new(
            RichText::new(format!("{}", self.keys.webroot_key))
                .color(Color32::from_rgb(102, 255, 153))
            )
            .min_size(vec2(140., 15.0))
            .ui(ui)
            .on_hover_text("Click To Copy Webroot Key to Clipboard")
            .clicked() { 
                let webroot = self.keys.webroot_key.clone();
                ui.ctx().copy_text(webroot);
            }
                
            if Button::new(
                RichText::new(format!("{}", self.keys.superanti_key))
                .color(Color32::from_rgb(255, 61, 126))
            )
            .min_size(vec2(140., 15.0))
            .ui(ui)
            .on_hover_text("Click To Copy SAS Key to Clipboard")
            .clicked() { 
                let sas = self.keys.superanti_key.clone();
                ui.ctx().copy_text(sas);
            }

            if ui.add_enabled(!self.ticket_data.service_number.is_empty(), 
            Button::new("Get Keys").min_size(vec2(140., 3.0))
            )
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
            
            ui.end_row();

            ui.vertical_centered(|ui|  {
                ComboBox::from_id_salt("ssd_cbox").width(140. - 5.0)
                .selected_text(format!("{}", self.ssd_test_cbox.as_str()))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.ssd_test_cbox, SsdFail, "SSD Fail");
                    ui.selectable_value(&mut self.ssd_test_cbox, SsdPass, "SSD Pass");
                    ui.selectable_value(&mut self.ssd_test_cbox, SsdNotTested, "SSD Not Tested");
                }); // Combo Box
            });

            ui.vertical_centered(|ui|  {
                ComboBox::from_id_salt("hdd_cbox").width(140. - 5.0)
                .selected_text(format!("{}", self.hdd_test_cbox.as_str()))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.hdd_test_cbox, HddFail, "HDD Fail");
                    ui.selectable_value(&mut self.hdd_test_cbox, HddPass, "HDD Pass");
                    ui.selectable_value(&mut self.hdd_test_cbox, HddNotTested, "HDD Not Tested");
                }); // Combo Box
            });

            ui.vertical_centered(|ui|  {
                ComboBox::from_id_salt("ram_cbox").width(140.)
                .selected_text(format!("{}", self.ram_test_cbox.as_str()))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.ram_test_cbox, RamFail, "RAM Fail");
                    ui.selectable_value(&mut self.ram_test_cbox, RamPass, "RAM Pass");
                    ui.selectable_value(&mut self.ram_test_cbox, RamNotTested, "RAM Not Tested");
                }); // Combo Box
            });

            ui.checkbox(&mut self.send_specs, "Send System Info");
            
            ui.end_row();

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
            let _file_name = attached_file.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");

            // ui.vertical_centered(|ui|  {
            //     if Button::new(
            //         RichText::new(format!("Upload 🗋 {{ {} }}", file_name))
            //         )
            //         .min_size(vec2(140., 8.0))
            //         .ui(ui)
            //         .clicked()
            //     {
            //         let mut dialog = FileDialog::open_file(self.opened_file.clone())
            //         .id("File Dialog");
            //         dialog.open();
            //         self.open_file_dialog = Some(dialog);
            //     };
            // });

            // ui.end_row();

            ui.label("");

            let check = !self.ticket_data.service_number.is_empty()
                && !self.customer_data.name.is_empty()
                && !self.customer_data.phone_number.is_empty()
                && !self.ticket_data.salesman.is_empty()
                && !self.ticket_data.tech.is_empty();

            // if ui
            // .add_enabled(
            //     check,
            //     Button::new(RichText::new("Submit TUR").color(Color32::from_rgb(255, 204, 255)))
            //     .min_size(Vec2::new(140., 20.0))
            //     .stroke(Stroke::new(1.0, Color32::from_rgb(191, 33, 101)))
            // )
            // .clicked()
            // {  
            //     self.submit_tur();
            // }

            if ui.add_enabled(
                check, 
                Button::new( 
                    RichText::new("Submit TUR"))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(191, 33, 101)))
                    .min_size(Vec2::new(140., 20.0)
                )
            ).clicked() {
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

            ui.label("");
            ui.end_row();
            if ui.add_enabled(
                false, 
                Button::new( 
                    RichText::new("Complete QC"))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(191, 33, 101)))
                    .min_size(Vec2::new(140., 20.0)
                )
            ).clicked() {
                
            }

            ui.end_row();
        }); // grid
    }

    fn device_info_grid(&mut self, ui: &mut Ui) {
        let text_edit_size = vec2( 140., 15.0);
        Grid::new("Device Details Grid")
            .spacing(vec2(4.0, 7.0))
            .min_col_width(150.)
            .max_col_width(150.)
            .num_columns(2)
            .show(ui, |ui| 
        {
            let service_details = &self.service_details;
            let mut device = service_details.get(0).cloned().unwrap_or_default();
                                /*     ROW 1     */
            TextEdit::singleline(&mut device.device_name)
                .hint_text(" Device Name")
                .vertical_align(Align::Center)
                .margin(vec2(4.0, 4.0))
                .min_size(text_edit_size)
                .ui(ui);

            TextEdit::singleline(&mut device.device_mfg)
                .hint_text(" Device Mfg")
                .vertical_align(Align::Center)
                .margin(vec2(4.0, 4.0))
                .min_size(text_edit_size)
                .ui(ui);

            ui.end_row();
                                /*     ROW 2     */
            TextEdit::singleline(&mut device.device_model)
                .hint_text(" Device Model")
                .vertical_align(Align::Center)
                .margin(vec2(4.0, 4.0))
                .min_size(text_edit_size)
                .ui(ui);

            TextEdit::singleline(&mut device.device_serial)
                .hint_text(" Device Serial")
                .vertical_align(Align::Center)
                .margin(vec2(4.0, 4.0))
                .min_size(text_edit_size)
                .ui(ui);
            
            ui.end_row();

                                /*     ROW 3     */
            TextEdit::singleline(&mut device.device_password)
                .hint_text(" Device Password")
                .vertical_align(Align::Center)
                .margin(vec2(4.0, 4.0))
                .min_size(text_edit_size)
                .ui(ui);

            TextEdit::singleline(&mut device.device_power_supply)
                .hint_text(" Device Power Supply")
                .vertical_align(Align::Center)
                .margin(vec2(4.0, 4.0))
                .min_size(text_edit_size)
                .ui(ui);

            ui.end_row();
        });
    }

    fn seb_info_grid(&mut self, ui: &mut Ui) {
        let text_edit_size = vec2( 140., 15.0);

        Grid::new("SEB Info Grid")
            .spacing(vec2(4.0, 7.0))
            .min_col_width(150.)
            .max_col_width(150.)
            .num_columns(2)
            .show(ui, |ui| 
        {
            let seb_info = &self.seb_info;
            let mut seb_details = seb_info.get(0).cloned().unwrap_or_default();
            TextEdit::singleline(&mut seb_details.device_name)
                .hint_text(" Carbonite Device Name")
                .vertical_align(Align::Center)
                .margin(vec2(4.0, 4.0))
                .min_size(text_edit_size)
                .ui(ui);

            TextEdit::singleline(&mut seb_details.device_id)
                .hint_text(" Device ID")
                .vertical_align(Align::Center)
                .margin(vec2(4.0, 4.0))
                .min_size(text_edit_size)
                .ui(ui);

            ui.end_row();
            
            TextEdit::singleline(&mut seb_details.activated)
                .hint_text(" Activation Code")
                .vertical_align(Align::Center)
                .margin(vec2(4.0, 4.0))
                .min_size(text_edit_size)
                .ui(ui);

            TextEdit::singleline(&mut seb_details.id_recurly_account)
                .hint_text(" Recurly Id")
                .vertical_align(Align::Center)
                .margin(vec2(4.0, 4.0))
                .min_size(text_edit_size)
                .ui(ui);

            ui.end_row();

            TextEdit::singleline(&mut seb_details.usage_gb)
                .hint_text(" Usage (Gb)")
                .vertical_align(Align::Center)
                .margin(vec2(4.0, 4.0))
                .min_size(text_edit_size)
                .ui(ui);

            ui.end_row();

        }); // grid
    }

    fn recommendations_grid(&mut self, ui: &mut Ui) {
        Grid::new("RecommendationsCheckinNotesGrid")
            .spacing(vec2(4., 7.))
            .min_col_width(305.)
            .max_col_width(305.)
            .num_columns(2)
            .show(ui, |ui| 
        {
            TextEdit::multiline(&mut self.ticket_data.checkin_notes)
                .hint_text(RichText::new("Checkin Notes").weak())
                .font(FontId::proportional(15.0))
                .margin(Margin::symmetric(10, 6))
                .desired_width(f32::INFINITY)
                .desired_rows(10)
                .ui(ui);
    
            TextEdit::multiline(&mut self.task_data.task_description)
                .hint_text(RichText::new("Recommendations").weak())
                .font(FontId::proportional(15.0))
                .margin(Margin::symmetric(10, 6))
                .desired_width(f32::INFINITY)
                .desired_rows(10)
                .ui(ui);
        });
    }
}
