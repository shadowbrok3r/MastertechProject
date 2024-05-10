use eframe::egui::{vec2, Align, Align2, Button, CentralPanel, Color32, ComboBox, Grid, Id, Key, Layout, RawInput, RichText, ScrollArea, Spinner, Stroke, TextEdit, Ui, Window };
use crate::{app_state::MastertechContext, database::{database::Database, schema::{HardwareTests, TaskPayload, TicketResponse}, send_payload, GetKeysResponse, PreTicketData}, handle_api::{api_request::request_seb_info, email_builder::{/*asana_html_builder, */ AsanaTask, Info, TaskAssignee}, scaffold::{HardwareTest, Salesman, SendReq, Techs}}, scripting::{Scripts, SCRIPT_ACTIONS}, terminal::setup_terminal, ui_helpers::task_cards::task_card};
use std::{path::PathBuf, sync::{atomic::Ordering, Arc}, collections::HashMap}; 
use chrono::{DateTime, SecondsFormat};
use log::{debug, error, info};
use ratframe::RataguiBackend;
use reqwest_cookie_store::{CookieStore, CookieStoreMutex};
use serde_json::Value;
use tokio::spawn;
use egui_extras::{*, DatePickerButton, Column};
use egui_file::FileDialog;
use puffin_egui;
use lettre::{{Message, SmtpTransport, Transport}, transport::smtp::authentication::Credentials, message::header::ContentType};

use crate::{
    filesystem::file_browser::FileBrowser,
    handle_api::{ api_request::SendRequest, scaffold, Store },
    self_updater::run, handle_api::email_builder::email_builder
};

#[cfg(target_os="windows")]
use crate::scripting::query_antivirus;

impl MastertechContext {
    pub fn simple_demo_menu(&mut self, ui: &mut Ui) {
        ui.label("Secret menu... -.-");
        ui.menu_button("Sub menu", |ui| {
            ui.label("(.)(.)");
        });
        if ui.button("update").clicked(){
            let (tx, rx) = crossbeam::channel::bounded(1);

            tokio::task::spawn_blocking(move || {
                match run(){
                    Ok(response) => {
                        match tx.send((response.0, response.1)){
                            Ok(_) => drop(tx),
                            Err(e) => println!("{e}"),
                        }
                    },
                    Err(e) => println!("err: {e}"),
                }
            });
            if let Ok(res) = rx.recv(){
                self.output_text = format!("Status: \n     {}\nReleases:\n     {}", &res.1.to_string(), &res.0.to_string());
            }
            

        }
    }

    pub fn file_browser_popup(&mut self, ui: &mut Ui) {
        let current_state = self.show_deferred_viewport.load(Ordering::Relaxed);
        let new_state = !current_state; // Toggle the state: if it's true, make it false, and vice versa

        if current_state{
            if ui.button("Attach File Browser").clicked(){self.show_deferred_viewport.store(new_state, Ordering::Relaxed);}
        }else {
            if ui.button("Detach File Browser").clicked(){self.show_deferred_viewport.store(new_state, Ordering::Relaxed);}
        }
    }

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
            .size(Size::exact(35.0)) // space between top and bottom strips
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
                                    ui.vertical_centered_justified(|ui|{
                                        if ui.add(
                                            Button::new(RichText::new("Get Ticket")
                                                .color(Color32::from_rgb(255, 204, 255))
                                                
                                                
                                            )
                                            .stroke(Stroke::new(2.0, Color32::from_rgb(191, 33, 101)))
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
                                    }); // v center justified
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
                                                    ComboBox::from_id_source("salesman_cbox").width(self.widget_size)
                                                    .selected_text(format!("{:?}", self.salesman_cbox))
                                                    .show_ui(ui, |ui| {
                                                        ui.selectable_value(&mut self.salesman_cbox, scaffold::Salesman::Jake, "Jake");
                                                        ui.selectable_value(&mut self.salesman_cbox, scaffold::Salesman::Danny, "Danny");
                                                    });


                                                    ComboBox::from_id_source("techs_cbox").width(self.widget_size)
                                                    .selected_text(format!("{:?}", self.techs_cbox))
                                                    .show_ui(ui, |ui| {
                                                        
                                                        ui.selectable_value(&mut self.techs_cbox, scaffold::Techs::Logan, "Logan");
                                                        ui.selectable_value(&mut self.techs_cbox, scaffold::Techs::Bread, "Bread");
                                                        ui.selectable_value(&mut self.techs_cbox, scaffold::Techs::Taco, "Taco");
                                                    });    
                                                    
                                                    ui.end_row();
                                                                        /*     ROW 4     */
                                                    if ui.add(Button::new("Get Keys").min_size(vec2(self.widget_size, 3.0)))
                                                    .clicked(){ 
                                                        let service_num = self.so_number.clone();
                                                        self.spinner = true;

                                                        let cps_request = SendRequest::get_cps(service_num, self.client.clone());
                                                        let (tx, rx) = std::sync::mpsc::channel::<GetKeysResponse>();

                                                        spawn(async move{
                                                            let sender = tx.clone();
                                                            let unwrapped_request =  cps_request.await.unwrap_or(GetKeysResponse::default());

                                                            match sender.send(unwrapped_request){
                                                                Ok(_) => info!("GetKeysClick -> sent keys successfully"),
                                                                Err(err) => debug!("GetKeysClick -> Error propogating GetKeysResponse to callee -> {err:?}")
                                                            }
                                                        });

                                                        match rx.recv(){
                                                            Ok(keys) => {
                                                                if keys.webroot_key.contains("Error"){
                                                                    self.output_text = "Error fetching Keys. Is SW\\/PCLCPS\\/O on ticket?".to_string();
                                                                }
                                                                self.keys = keys;
                                                            },
                                                            Err(err) => {
                                                                debug!("GetKeysClick Receive Error -> {err:?}");
                                                                self.output_text = format!("GetKeysClick -> Error receiving keys -> {err:?}");
                                                            }
                                                        }
                                                    }
                                                    
                                                    if ui.add(Button::new("Check SEB").min_size(vec2(self.widget_size, 3.0)))
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
                                ui
                                .group(|ui|
                                {
                                    ui
                                    .horizontal_top(|ui|
                                    {
                                        ui.add_space(self.widget_size/1.8);
            
                                        ComboBox::from_id_source("ram_cbox").width(self.widget_size - 5.0)
                                        .selected_text(format!("{}", self.ram_test_cbox.as_str()))
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(&mut self.ram_test_cbox, scaffold::HardwareTest::RamFail, "RAM Fail");
                                            ui.selectable_value(&mut self.ram_test_cbox, scaffold::HardwareTest::RamPass, "RAM Pass");
                                            ui.selectable_value(&mut self.ram_test_cbox, scaffold::HardwareTest::RamNotTested, "RAM Not Tested");
                                        }); // Combo Box
                                    }); // H top
                

                                    Grid::new("drive_tests")
                                    .spacing(vec2(4.0, 3.0))
                                    .min_col_width(self.widget_size)
                                    .num_columns(2)
                                    .show(ui, |ui| {
                                                            /*     ROW 1     */
                                        ComboBox::from_id_source("ssd_cbox").width(self.widget_size - 5.0)
                                        .selected_text(format!("{}", self.ssd_test_cbox.as_str()))
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(&mut self.ssd_test_cbox, scaffold::HardwareTest::SsdFail, "SSD Fail");
                                            ui.selectable_value(&mut self.ssd_test_cbox, scaffold::HardwareTest::SsdPass, "SSD Pass");
                                            ui.selectable_value(&mut self.ssd_test_cbox, scaffold::HardwareTest::SsdNotTested, "SSD Not Tested");
                                        }); // Combo Box
                
                                                            /*     ROW 2     */
                                        ComboBox::from_id_source("hdd_cbox").width(self.widget_size - 5.0)
                                        .selected_text(format!("{}", self.hdd_test_cbox.as_str()))
                                        .show_ui(ui, |ui| {
                                            ui.selectable_value(&mut self.hdd_test_cbox, scaffold::HardwareTest::HddFail, "HDD Fail");
                                            ui.selectable_value(&mut self.hdd_test_cbox, scaffold::HardwareTest::HddPass, "HDD Pass");
                                            ui.selectable_value(&mut self.hdd_test_cbox, scaffold::HardwareTest::HddNotTested, "HDD Not Tested");
                                        }); // Combo Box
                                        ui.end_row();
                                    }); // Grid   
                


                                    ui.vertical(|ui|{ui.add_space(6.0);});

                                    ui.horizontal_top(|ui|{
                                        Grid::new("other_buttons")
                                        .spacing(vec2(4.0, 3.0))
                                        .min_col_width(self.widget_size)
                                        .num_columns(2)
                                        .show(ui, |ui| {
                                            let date = self.date.get_or_insert_with(|| 
                                                chrono::offset::Utc::now());
                                                // 
                                            ui.add(DatePickerButton::new(&mut date.date_naive()));

                                            ui.checkbox(&mut self.send_specs, "Send System Info");

                                            ui.end_row();
                                        });
                                    });

                                    ui.vertical(|ui|{ui.add_space(6.0);});

                                    let mut attached_file = PathBuf::new();
                                    let mut hovered_file_txt = "";
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
                                }); // group

                                ui.vertical(|ui|{ui.add_space(3.0);});

                                ui
                                .vertical_centered_justified(|ui|
                                {
                                    
                                    if ui
                                    .add(
                                        Button::new
                                        (
                                            RichText::new("Submit TUR Sheet")
                                                .color(Color32::from_rgb(255, 204, 255))
                                        )
                                            .stroke(Stroke::new(1.0, Color32::from_rgb(191, 33, 101)))
                                    )
                                    .clicked()
                                    {  
                                        self.spinner = true;

                                        Window::new("Spinner Window")
                                            .enabled(self.spinner)
                                            .open(&mut self.spinner)
                                            .title_bar(false)
                                            .fixed_size(vec2(10.0,10.0))
                                            // .constrain_to(ctx.available_rect())
                                            .anchor(Align2::CENTER_CENTER, [2.0, 2.0])
                                            .show(&self.ctx, |ui|{
                                                ui.add(
                                                    Spinner::new()
                                                    .color(Color32::LIGHT_RED)
                                                    //.size()
                                                );
                                    });
                                        

                                        let cust = &self.ticket_info.customer_name;
                                        let so_num = &self.so_number;
                
                                        if !cust.is_empty() && !so_num.is_empty()
                                        {

                                            let mut salesman_map = HashMap::new();
                                            let mut tech_map = HashMap::new();

                                            let salesman = &format!("{:?}", &self.salesman_cbox);
                                            let checkin_rep = &self.ticket_info.checkin_rep;
                                            let technician = &format!("{:?}", &self.techs_cbox);

                                            salesman_map.insert("Jake", "1202792432658520");
                                            salesman_map.insert("Danny", "1202791016369879");
                                            tech_map.insert("Logan", "1199992640930465");
                                            tech_map.insert("Bread", "1202792432421640");
                                            tech_map.insert("Taco", "1202792432551073");

                                            // let assigned_salesman = salesman_map.get(salesman.as_str()).unwrap_or(&"1202792432658520").to_string();
                                            // let assigned_tech = tech_map.get(technician.as_str()).unwrap_or(&"1199992640930465").to_string();

                                            let hdd_test = &format!("{:?}", &self.hdd_test_cbox);
                                            let ram_test = &format!("{:?}", &self.ram_test_cbox);
                                            let ssd_test = &format!("{:?}", &self.ssd_test_cbox);

                                            let checkin_notes = &self.ticket_info.checkin_notes;
                                            let recommendations = &self.recommendations;   

                                            let date = self.date.unwrap_or(DateTime::default());
                                            let mut attached_file: Option<PathBuf> = None;
                                            if let Some(file) = &self.opened_file{
                                                attached_file = Some(file.to_path_buf());
                                            }

                                            let mut specs = String::new();
                                            let cps = self.current_antivirus.clone();
                                            let seb_info = self.seb_info.clone().unwrap_or_default();

                                            

                                            let mut final_disk = String::new();
                                            let mut each_disk = String::new();
                                                                                    
                                            let cust_code = &self.ticket_info.cust_code;
                                            let doc_alias = &self.ticket_info.doc_alias;
                                            let department = &self.ticket_info.dep;
                                            //let juris = &self.ticket_info.juris;
                                            let ticket_total = &self.ticket_info.ticket_total;
                                            let cust_email = &self.ticket_info.customer_email;
                                            let last_inv_num = &self.ticket_info.last_invoice_number;
                                            let last_inv_amt = &self.ticket_info.last_invoice_amount;
                                            let total_inv_num = &self.ticket_info.total_invoice_count;
                                            let phone1 = &self.ticket_info.customer_phone_1;
                                            let phone2 = &self.ticket_info.customer_phone_2;
                                            let mut phone_2 = String::new();
                                            if !phone2.is_empty(){
                                                phone_2 = format!("<tr>
                                                <td style=\"padding:1px 1px\">Phone #2</td>
                                                <td colspan=\"2\" data-cell-widths=\"150,150\" style=\"padding:1px 1px\">{phone2}</td>
                                                </tr>");
                                            }

                                            let extra_customer_info = format!
                                            ("
                                            <tr>
                                                <td style=\"padding:1px 1px\">Customer Code</td>
                                                <td colspan=\"2\" data-cell-widths=\"150,150\" style=\"padding:1px 1px\">{cust_code}</td>
                                            </tr>
                                            <tr>
                                                <td style=\"padding:1px 1px\">Phone #</td>
                                                <td colspan=\"2\" data-cell-widths=\"150,150\" style=\"padding:1px 1px\"><strong>{phone1}</strong></td>
                                            </tr>
                                            {phone_2}
                                            <tr>
                                                <td style=\"padding:1px 1px\">Customer Email</td>
                                                <td colspan=\"2\" data-cell-widths=\"150,150\" style=\"padding:1px 1px\">{cust_email}</td>
                                            </tr>
                                            <tr>
                                                <td style=\"padding:1px 1px\">Current Total</td>
                                                <td colspan=\"2\" data-cell-widths=\"150,150\" style=\"padding:1px 1px\">${ticket_total}</td>
                                            </tr>
                                            <tr>
                                                <td style=\"padding:1px 1px\">Last SI#</td>
                                                <td colspan=\"2\" data-cell-widths=\"150,150\" style=\"padding:1px 1px\">{last_inv_num}</td>
                                            </tr>
                                            <tr>
                                                <td style=\"padding:1px 1px\">Last Invoice Total</td>
                                                <td colspan=\"2\" data-cell-widths=\"150,150\" style=\"padding:1px 1px\">{last_inv_amt}</td>
                                            </tr>
                                            <tr>
                                                <td style=\"padding:1px 1px\"># of SI's</td>
                                                <td colspan=\"2\" data-cell-widths=\"150,150\" style=\"padding:1px 1px\">{total_inv_num}</td>
                                            </tr>
                                            ");
                                            if self.send_specs == true{
                                                self.output_text.clear();
                                                self.output_text += "pulling system information. Please wait a moment..\n";
                                                let system_name = &self.system_info.hostname;
                                                let os = &self.system_info.operating_system;
                                                let cpu_name = &self.system_info.cpu;
                                                let total_ram = &self.system_info.ram;
                                                let gpu = &self.system_info.gpu.clone();

                                                for index in 0..self.disk_num
                                                {
                                                    if let Some(disk) = self.disks.get(index)
                                                    {
                                                        let drive_letter = format!("{}", disk.get("drive_letter").and_then(Value::as_str).unwrap_or(""));
                                                        let drive_type = disk.get("drive_type").and_then(Value::as_str).unwrap_or("");
                                                        let space_left = format!("{} Gb", disk.get("space_left").and_then(Value::as_str).unwrap_or(""));
                                                        let total_size = format!("{} Gb", disk.get("total_size").and_then(Value::as_str).unwrap_or(""));

                                                        each_disk += &format!("
                                                        <tr>
                                                        <td style=\"padding:1px 1px\">        {drive_letter}</td>
                                                        <td style=\"padding:1px 1px\">        {drive_type}</td>
                                                        <td style=\"padding:1px 1px\">        {space_left}</td>
                                                        <td style=\"padding:1px 1px\">        {total_size}</td>
                                                        </tr>
                                                        ");

                                                        final_disk = format!
                                                            ("
                                                            <tr>
                                                                <td style=\"padding:1px 4px\">Letter</td>
                                                                <td style=\"padding:1px 4px\">Drive Type</td>
                                                                <td style=\"padding:1px 4px\">Avail Space</td>
                                                                <td style=\"padding:1px 4px\">Total Space</td>
                                                            </tr>
                                                            {each_disk}
                                                        ");

                                                    }
                                                }

                                                specs = format!("
                                                <table>
                                                    <tr>
                                                        <td style=\"text-align:center;\" colspan=\"3\" data-cell-widths=\"130,200,200\" width=\"450\"
                                                        >              <code>       Computer Info        </code></td>
                                                    </tr>
                                                    <tr>
                                                        <td>PC Name</td>
                                                        <td colspan=\"2\" data-cell-widths=\"150,150\">{system_name}</td>
                                                    </tr>
                                                    <tr>
                                                        <td>OS</td>
                                                        <td colspan=\"2\" data-cell-widths=\"150,150\">{os}</td>
                                                    </tr>
                                                    <tr>
                                                        <td>CPU</td>
                                                        <td colspan=\"2\" data-cell-widths=\"150,150\">{cpu_name}</td>
                                                    </tr>
                                                    <tr>
                                                        <td>RAM</td>
                                                        <td colspan=\"2\" data-cell-widths=\"150,150\">{total_ram} Gb</td>
                                                    </tr>
                                                    <tr>
                                                        <td>GPU</td>
                                                        <td colspan=\"2\" data-cell-widths=\"150,150\">{gpu}</td>
                                                    </tr>
                                                    <tr>
                                                        <td>Antivirus</td>
                                                        <td colspan=\"2\" data-cell-widths=\"150,150\">{cps}</td>
                                                    </tr>
                                                    <tr>
                                                        <td>SEB</td>
                                                        <td colspan=\"2\" data-cell-widths=\"150,150\">{seb_info:#?}</td>
                                                    </tr>
                                                    <tr>
                                                    <td colspan=
                                                    \"3\" data-cell-widths=\"100,200,200\" width=\"400\" style=\"text-align:center;\"
                                                    >                <code>        HDD/SSD info        </code></td>
                                                    </tr>
                                                    {final_disk}
                                                    </table>
                                                ").trim().to_string();
                                            }else{
                                                specs = "Computer information was not sent with ticket".to_string();
                                            }
                                            
                                            let html_notes = format!(
                                                "<body>
                                                    <table>
                                                        <tr>
                                                            <td style=\"text-align:center;\" colspan=\"3\" data-cell-widths=\"130,130,130\" width=\"390\"
                                                            >                <code>        {doc_alias} Info        </code>
                                                            </td>
                                                        </tr>
                                                        <tr>
                                                            <td style=\"padding:1px 1px\">Salesman</td>
                                                            <td style=\"padding:1px 1px\">Checkin Rep</td>
                                                            <td style=\"padding:1px 1px\">Technician</td>
                                                        </tr>
                                                        <tr>
                                                            <td style=\"padding:1px 4px\">     {salesman}</td>
                                                            <td style=\"padding:1px 4px\">     {checkin_rep}</td>
                                                            <td style=\"padding:1px 4px\">     {technician}</td>
                                                        </tr>
                                                        <tr>
                                                            <td style=\"text-align:center;\" colspan=\"3\" data-cell-widths=\"130,130,130\" width=\"390\"
                                                            >                <code>           Customer           </code>
                                                            </td>
                                                        </tr>
                                                        {extra_customer_info}
                                                    </table>
                                                    {specs}
                                                    <ul>
                                                        <li><strong>SSD test:</strong>     {ssd_test}</li>
                                                        <li><strong>HDD test:</strong>     {hdd_test}</li>
                                                        <li><strong>RAM test:</strong>     {ram_test}</li>
                                                    </ul>
                                                    <h2><strong><code>           Notes           </code></strong></h2>
                                                    <ul><li><strong>        Checkin Notes:      </strong>     \n{checkin_notes}</li>
                                                        <li><strong>        Recommendations:        </strong>     \n{recommendations}</li></ul></body>",
                                            );

                                            let store: &Store = &self.ticket_info.dep;

                                            if store.as_str() == "RIV"{
                                                let sm = self.salesman_cbox;
                                                let tech = self.techs_cbox;

                                                let task = AsanaTask { 
                                                    task_name: format!("{cust} - {so_num}"), 
                                                    html_notes,
                                                    assignee: TaskAssignee { 
                                                        salesman: sm, 
                                                        tech
                                                    }, 
                                                    file_attachment: self.opened_file.clone() 
                                                };


                                                SendRequest::send_ticket_request(
                                                    self.scaffold_request.tx.clone(), 
                                                    self.client.clone(), 
                                                    task,
                                                    date,
                                                );

                                            }else{
                                                let mtech_username = dotenv::var("MTECH_EMAIL").unwrap_or("not provided".to_string());
                                                let mtech_password = dotenv::var("MTECH_PASS").unwrap_or("not provided".to_string());
                                                let store_email = store.store_email();

                                                let system_name = &self.system_info.hostname;
                                                let cpu_name = &self.system_info.cpu;
                                                let total_ram = &self.system_info.ram;
                                                let gpu = &self.system_info.gpu.clone();
                                                let mut final_disk = String::new();
                                                let mut each_disk = String::new();

                                                for index in 0..self.disk_num
                                                {
                                                    if let Some(disk) = self.disks.get(index)
                                                    {
                                                        let disk_letter = format!("{}", disk.get("letter").and_then(Value::as_str).unwrap_or(""));
                                                        let drive_type = disk.get("drive_type").and_then(Value::as_str).unwrap_or("");
                                                        let disk_available = format!("{} Gb", disk.get("space_left").and_then(Value::as_str).unwrap_or(""));
                                                        let disk_total = format!("{} Gb", disk.get("total_size").and_then(Value::as_str).unwrap_or(""));

                                                        each_disk += &format!("
                                                        <tr>
                                                            <td style=\"text-align: center; padding:1px 1px color: #ffffff\">{disk_letter}</td>
                                                            <td style=\"text-align: center; padding:1px 1px color: #ffffff\">{drive_type}</td>
                                                            <td style=\"text-align: center; padding:1px 1px color: #ffffff\">{disk_available}</td>
                                                            <td style=\"text-align: center; padding:1px 1px color: #ffffff\">{disk_total}</td>
                                                        </tr>
                                                        ");

                                                        final_disk = format!
                                                            ("
                                                            <tr>
                                                                <td style=\"padding:1px 4px; text-align: center; \">Letter</td>
                                                                <td style=\"padding:1px 4px; text-align: center; \">Type</td>
                                                                <td style=\"padding:1px 4px; text-align: center; \">Avail Space</td>
                                                                <td style=\"padding:1px 4px; text-align: center; \">Total Space</td>
                                                            </tr>
                                                            {each_disk}
                                                        ");

                                                    }
                                                }

                                                specs = format!("
                                                <tr>
                                                    <td style=\"color: #ffffff;\"><strong>CPU</strong></td>
                                                    <td style=\"text-align: center; color: #ffffff;\">{cpu_name}</td>
                                                </tr>
                                                <tr>
                                                    <td style=\"color: #ffffff;\"><strong>GPU</strong></td>
                                                    <td style=\"text-align: center; color: #ffffff;\">{gpu}</td>
                                                </tr>
                                                <tr>
                                                    <td style=\"color: #ffffff;\"><strong>RAM</strong></td>
                                                    <td style=\"text-align: center; color: #ffffff;\">{total_ram} Gb</td>
                                                </tr>
                                                <tr>
                                                    <td style=\"color: #ffffff;\"><b>System Name</b></td>
                                                    <td>
                                                        <p style=\"text-align: center; color: #ffffff;\">{system_name}</p>
                                                    </td>
                                                </tr>
                                                <tr>
                                                    <td style=\"color: #ffffff;\"><b>CPS</b></td>
                                                    <td>
                                                        <p style=\"text-align: center; color: #ffffff;\">{cps}</p>
                                                    </td>
                                                </tr>
                                                ");



                                                let info = Info{
                                                    customer_name: cust.to_string(),
                                                    so_num: so_num.to_string(),
                                                    hdd_test: hdd_test.to_string(),
                                                    ram_test: ram_test.to_string(),
                                                    ssd_test: ssd_test.to_string(),
                                                    checkin_notes: checkin_notes.to_string(),
                                                    recommendations: recommendations.to_string(),
                                                    specs,
                                                    cps,
                                                    cust_code: cust_code.to_string(),
                                                    doc_alias: doc_alias.to_string(),
                                                    inv_amt: ticket_total.to_string(),
                                                    cust_email: cust_email.to_string(),
                                                    last_inv_num: last_inv_num.to_string(),
                                                    last_inv_amt: last_inv_amt.to_string(),
                                                    total_inv_num: total_inv_num.to_string(),
                                                    phone1: phone1.to_string(),
                                                    phone2: phone2.to_string(),

                                                    final_disk,

                                                    salesman: salesman.to_string(),
                                                    checkin_rep: checkin_rep.to_string(),
                                                    technician: technician.to_string(),
                                                    extra_customer_info,
                                                };

                                                let html = email_builder(info);
                                                
                                                let email = Message::builder()
                                                    .from("TUR SHEET <pcl.mastertech@gmail.com>".parse().unwrap())
                                                    .to(store_email.parse().unwrap())
                                                    .subject(format!("{cust} - {so_num}"))
                                                    .header(ContentType::TEXT_HTML)
                                                    .body(html)
                                                    .unwrap();

                                                let creds = Credentials::new("pcl.mastertech@gmail.com".to_owned(), "pgumcgekyrcqadah".to_owned());

                                                // Open a remote connection to gmail
                                                let mailer = SmtpTransport::relay("smtp.gmail.com")
                                                    .unwrap()
                                                    .credentials(creds)
                                                    .build();

                                                self.output_text += format!("\n {store_email} {cust_email}").as_str();

                                                // Send the email
                                                match mailer.send(&email) {
                                                    Ok(_) => println!("Email sent successfully!"),
                                                    Err(e) => {
                                                        self.output_text += format!("\n{e:?}").as_str();
                                                        //println!("Could not send email: {e:?}")
                                                    },
                                                }
                                            }

                                            self.spinner = false;
                                            
                                            self.output_text += "\nSent Ticket";
                                        }
                                        else{
                                            self.output_text.clear();
                                            self.output_text = "You need to enter a customer name or Service number".to_string();
                                        }
                                    

                                        self.spinner = false;
                                        self.ctx.request_repaint();
                                    }
                                }); // vertical center justified
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
                                    vec2(ui.available_width()-4.0, ui.available_height()),
                                    TextEdit::multiline(&mut self.ticket_info.checkin_notes)
                                    .hint_text(RichText::new("Checkin Notes").weak())
                                    .desired_rows(15)
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
                                    vec2(ui.available_width()-4.0, ui.available_height()), 
                                    TextEdit::multiline(&mut self.recommendations)
                                    .hint_text(RichText::new("Recommendations")
                                    .weak())
                                    .desired_rows(15)
                                );
                            });
                            ui.shrink_height_to_current(); 
                        }); // cell
                    }); // strip builder
                }); // strip.strip
            }); //strip builder
        }); // UI layout
    }

    pub fn output_console(&mut self, ui: &mut Ui) { 
        setup_terminal(ui, &self.output_text).unwrap();

        // let input = RawInput::default();
        // let _ = self.ctx.run(input, |ctx|{
        //     Window::new("Spinner Window")
        //     .enabled(self.spinner)
        //     .open(&mut self.spinner)
        //     .title_bar(false)
        //     .fixed_size(vec2(20.0,20.0))
        //     .anchor(Align2::CENTER_TOP, [0.0, 0.0])
        //     .show(&ctx, |ui|{
        //         ui.add(
        //             Spinner::new()
        //             .color(Color32::LIGHT_RED)
        //         );
        //     });
        // });
        // ui.add_sized(ui.available_size(), TextEdit::multiline(&mut self.output_text.to_string()).hint_text("Output"));
    }
    
    pub fn system_information(&mut self, ui: &mut Ui){
        ui.vertical(|ui| {ui.add_space(3.0);}); // leave some margin above the textEdits
        Window::new("Spinner Window")
            .enabled(self.spinner)
            .open(&mut self.spinner)
            .title_bar(false)
            .fixed_size(vec2(10.0,10.0))
            // .constrain_to(ctx.available_rect())
            .anchor(Align2::CENTER_CENTER, [2.0, 2.0])
            .show(&self.ctx, |ui|{
                ui.add(
                    Spinner::new()
                    .color(Color32::LIGHT_RED)
                );
            });

        if ui
        .add(Button::new( RichText::new("Send to Master-Tech.app")))
        .clicked()
        {  
            let tech = match self.techs_cbox{
                Techs::Logan => "Logan".to_string(),
                Techs::Bread => "Brett".to_string(),
                Techs::Taco => "Taco".to_string(),
            };

            let salesman = match self.salesman_cbox{
                Salesman::Jake => "Jake".to_string(),
                Salesman::Danny => "Danny".to_string(),
            };
            
            let hdd_test = format!("{:?}", &self.hdd_test_cbox);
            let ram_test = format!("{:?}", &self.ram_test_cbox);
            let ssd_test = format!("{:?}", &self.ssd_test_cbox);

            let mut pre_ticket: PreTicketData = self.ticket_info.clone();

            pre_ticket.due_date = Some(
                self.date.unwrap_or(
                    DateTime::default()
                ).to_rfc3339_opts(SecondsFormat::Secs,  true)
            );
            
            let payload = TicketResponse::serialize_payload(
                &pre_ticket,
                &self.system_info,
                &self.so_number,
                &self.current_antivirus,
                &self.recommendations,
                tech,
                salesman, 
                HardwareTests{
                    hdd_test,
                    ssd_test,
                    ram_test,
                } // example
            );
            // let client = self.client.clone();

            
            let cookies = CookieStore::default();
            let cookie_store = CookieStoreMutex::new(cookies);
            let cookie_store  = Arc::new(cookie_store);

            let client_build = reqwest::Client::builder()
                .cookie_provider(std::sync::Arc::clone(&cookie_store))
                .build();
            
            match client_build{
                Ok(client) => {
                    debug!("Sending reqwest");
                    spawn(async move {
                        
                        let mut output = String::new();
                        let x = send_payload(payload, client, cookie_store).await;
                        match x{
                            Ok(o) => {
                                output = o;
                            },
                            Err(e) => debug!("Error {e:?}"),
                        }
                        info!("output: {output}");
                    });
                }, Err(err) => debug!("Error with client_build => {err:?}"),
            };
            
        }
        
        if ui.add(
            Button::new(
                RichText::new("Connect to WS")
            )
        )
        .clicked(){
            self.connect_to_ws = true;
        }


        self.specs_first_run = false;

        let computer_data = &self.system_info;

        let gpu = computer_data.gpu.clone();
        
        ui.push_id("table 1",|ui|{
            let table = TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .cell_layout(Layout::left_to_right(Align::Center))
                .column(Column::initial(100.0).range(50.0..=300.0).clip(true))
                .column(Column::remainder())
                .min_scrolled_height(0.0);

            table
            .header(20.0, |mut header|{
                header.col(|ui| {
                    ui.strong("Hardware Name");
                });
                header.col(|ui| {
                    ui.strong("Info");
                });
            })
            .body(|mut body| {
                body.row(20.0, |mut row| {
                    row.col(|ui|{
                        ui.label("System Name");
                    });
                    row.col(|ui|{
                        ui.label(&computer_data.hostname);
                    });
                });
                body.row(20.0, |mut row| {
                    row.col(|ui|{
                        ui.label("CPU Name");
                    });
                    row.col(|ui|{
                        ui.label(&computer_data.cpu);
                    });
                });
                body.row(20.0, |mut row| {
                    row.col(|ui|{
                        ui.label("Total RAM");
                    });
                    row.col(|ui|{
                        ui.label(format!("{} Gb", computer_data.ram));
                    });
                });
                body.row(20.0, |mut row| {
                    row.col(|ui|{
                        ui.label("GPU");
                    });
                    row.col(|ui|{
                        ui.label(gpu);
                    });
                });
                
            });

        });
        ui.vertical(|ui|{ui.add_space(20.0)});
        ui.push_id("table 2",|ui|{
            let disks_table = TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .cell_layout(Layout::left_to_right(Align::Center))
                .column(Column::exact(15.0))
                .column(Column::exact(42.0))
                .column(Column::exact(50.0))
                .column(Column::remainder());
            
            disks_table
                .header(20.0, |mut header|
            {
                header.col(|ui|{
                    ui.label("#");
                });
                header.col(|ui|{
                    ui.label("Letter");
                });
                header.col(|ui|{
                    ui.label("Type");
                });
                header.col(|ui|{
                    ui.label("Avail / Total Space");
                });

            })
            .body(|body| {
                body.rows(
                20.0,  // Replace with your desired row height
                self.disk_num,
                |mut row| 
                {                                            
                    let disk_index = row.index();               // this is stupid..
                    if let Some(disk) = self.disks.get(disk_index){
                        let disk_letter = format!("{}", disk
                            .get("drive_letter")
                            .and_then(Value::as_str)
                            .unwrap_or(""));

                        let drive_type = disk
                            .get("drive_type")
                            .and_then(Value::as_str)
                            .unwrap_or("");

                        row.col(|ui| {
                            ui.label(disk_index.to_string());  // Show disk index
                        });
                        row.col(|ui| {
                            ui.label(disk_letter);  // Show disk letter
                        });
                        row.col(|ui| {
                            if !drive_type.starts_with("Unknown"){
                                ui.label(drive_type);  // Show disk type
                            }else{
                                ui.label("Network Drive?");
                            }
                        });
                        row.col(|ui| {
                            let disk_space = format!(
                                "{} Gb / {} Gb",
                                disk.get("space_left").and_then(Value::as_str).unwrap_or(""),
                                disk.get("total_size").and_then(Value::as_str).unwrap_or("")
                            );
                            ui.label(disk_space);  // Show disk space
                        });
                        self.ctx.request_repaint();
                        self.spinner = false;
                    }   

                });
            });
        });
    }

    pub fn file_browse(&mut self, ui: &mut Ui) {
        if !self.show_deferred_viewport.load(Ordering::Relaxed) {
            let (command_tx, command_rx) = crossbeam::channel::unbounded();
            // Lock the Mutex and show the GUI
            let file_browser_clone = Arc::clone(&self.file_browser);
            let mut file_browser = file_browser_clone.lock().unwrap();
            file_browser.show(ui, command_tx, command_rx);
        }
    }
    
    pub fn scripts(&mut self, ui: &mut Ui){
        ui.style_mut().spacing.button_padding = (4.0, 7.0).into();
        ui.shrink_width_to_current();
        ui.shrink_height_to_current();
        ui.vertical(|ui|{ui.add_space(8.0);});
        ui.horizontal(|ui|{ui.add_space(8.0);});

        Grid::new("scripts").min_col_width(self.widget_size).num_columns(3).min_row_height(10.0).show(
        ui, |ui| {

            let mut counter = 0;  // Initialize a counter
            for (name, action) in &*SCRIPT_ACTIONS{
                let color = Color32::from_rgb(191, 33, 101);
                let button = Button::new(RichText::new(*name)).stroke(Stroke::new(1.0, color));


                if ui.add(button).clicked(){
                    info!("Clicked button");

                    let action_clone = action.clone();
                    let so_num = Arc::new(self.so_number.clone());
                    info!("SO number: {}", &so_num);
                    spawn(async move {
                        let y = Arc::clone(&so_num);
                        let scripts = Scripts::new(y.to_string()).await;
                        action_clone.execute(&scripts).await.unwrap();
                    });
                }

                counter += 1;  // Increment the counter

                if counter % 4 == 0 {
                    ui.end_row();  // End the row after every 2 buttons
                }
            }
        }); // Grid


     }

    pub fn puffin_profiler(&mut self, ui: &mut Ui){
        puffin::profile_function!();
        puffin::GlobalProfiler::lock().new_frame(); // call once per frame!
        puffin_egui::profiler_ui(ui);
    }

    pub fn mini_dump(&mut self, ui: &mut Ui){ 
        
        self.minidump_app.poll_processor_state();
        self.minidump_app.update_ui(&self.ctx, ui);
        self.minidump_app.last_status = self.minidump_app.cur_status;
    }

    pub fn quality_check(&mut self, ui: &mut Ui){ }

    pub fn mastertech_website(&mut self, ui: &mut Ui){ 
        ui.style_mut().spacing.button_padding = (4.0, 7.0).into();
        ui.shrink_width_to_current();
        ui.shrink_height_to_current();
        ui.vertical(|ui|{ui.add_space(8.0);});
        ui.horizontal(|ui|{ui.add_space(8.0);});

        let sender = self.db_data_sender.clone();

        if self.query_tasks_first_run{
            self.query_tasks_first_run = false;
            if let Some(db) = &self.database{
                let database = db.clone();
                spawn(async move {
                    let task_data = database.query("SELECT * FROM task").await.unwrap();
                
                    match sender.try_send(task_data){
                        Ok(_) => {
                            debug!("Sent task data");
                        },
                        Err(err) => debug!("Send error: {:?}", err.to_string()),
                    }
                });
            }
        }

        if let Ok(data) = self.db_data_receiver.try_recv(){
            self.ticket_data = Some(data);
        }

        if let Some(tasks) = &self.ticket_data{
            let _ = task_card(tasks.to_vec(), ui);
        }
        

    }
}
