use eframe::egui::{Align, Layout, Ui};
use egui_extras::{Column, TableBuilder};
use serde_json::Value;

use crate::app_state::MastertechContext;

impl MastertechContext{
    pub fn system_information(&mut self, ui: &mut Ui){
        self.specs_first_run = false;

        let computer_data = &self.system_info;
        let gpu = computer_data.gpu.clone();
    
        ui.push_id("sysinfo_table",|ui|{
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
        ui.push_id("drives_table",|ui|{
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