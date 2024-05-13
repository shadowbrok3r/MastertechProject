use std::sync::Arc;

use chrono::{DateTime, SecondsFormat};
use eframe::egui::{Align, Button, Layout, RichText, Ui};
use egui_extras::{Column, TableBuilder};
use log::{debug, info};
use reqwest_cookie_store::{CookieStoreMutex, CookieStore};
use serde_json::Value;
use tokio::spawn;

use crate::{app_state::MastertechContext, database::{schema::{HardwareTests, TicketResponse}, send_payload, PreTicketData}, handle_api::scaffold::{Salesman, Techs}};

impl MastertechContext{
    pub fn system_information(&mut self, ui: &mut Ui){
        ui.vertical(|ui| {ui.add_space(3.0);}); // leave some margin above the textEdits

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
                .column(Column::initial(100.0).range(50.0..=280.0).clip(true))
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

}