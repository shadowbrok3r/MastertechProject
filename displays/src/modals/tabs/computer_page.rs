use eframe::egui::{Color32, Grid, ScrollArea, TextEdit, Ui, Vec2, Vec2b, Widget};
use database::schema::{ComputerData, TaskPayload, COMPUTER_TABLE};
use surrealdb::RecordId;

use super::return_colors;


pub fn display_computer_page(ui: &mut Ui, task: &mut TaskPayload, avail_size: Vec2) {
    let Some(ticket) = task.service_ticket.as_mut() else { return; };
    let computer = if let Some(computer) = ticket.computer.as_mut() { 
        computer 
    } else { 
        &mut ComputerData{ id: RecordId::from((COMPUTER_TABLE, "")), ..Default::default() } 
    };


    ScrollArea::vertical()
        .max_height(f32::INFINITY)
        .max_width(avail_size.x)
        .auto_shrink(Vec2b::new(false, false))
        .show(ui, |ui|
    {
        ui.vertical_centered(|ui| {
            ui.group(|ui| {
                Grid::new(format!("{} grid", computer.cpu))
                .max_col_width(avail_size.x / 2.14)
                .min_col_width(avail_size.x / 2.14)
                .with_row_color(|num, style| return_colors(num, style))
                .show(ui, |ui| {
                    ui.colored_label(Color32::LIGHT_RED, "ID");
                    ui.label(computer.id.key().to_string());
                    ui.end_row();
                    
                    if let Some(cust) = &computer.customer {
                        ui.colored_label(Color32::LIGHT_RED, "Linked Customer");
                        ui.label(cust.key().to_string());
                        ui.end_row();
                    }

                    ui.colored_label(Color32::LIGHT_RED, "Hostname");
                    TextEdit::singleline(&mut computer.hostname).desired_width(avail_size.x / 2.14).ui(ui);
                    ui.end_row();
                    ui.colored_label(Color32::LIGHT_RED, "Operating System");
                    TextEdit::singleline(&mut computer.operating_system).desired_width(avail_size.x / 2.14).ui(ui);
                    ui.end_row();
                    if let Some(active) = &computer.windows_active {
                        ui.colored_label(Color32::LIGHT_RED, "Windows Active");
                        ui.label(format!("{active}"));
                        ui.end_row();
                    }
                    ui.colored_label(Color32::LIGHT_RED, "CPU");
                    TextEdit::singleline(&mut computer.cpu).desired_width(avail_size.x / 2.14).ui(ui);
                    ui.end_row();
                    ui.colored_label(Color32::LIGHT_RED, "GPU");
                    TextEdit::singleline(&mut computer.gpu).desired_width(avail_size.x / 2.14).ui(ui);
                    ui.end_row();
                    ui.colored_label(Color32::LIGHT_RED, "RAM");
                    TextEdit::singleline(&mut computer.ram).desired_width(avail_size.x / 2.14).ui(ui);
                    ui.end_row();
                    ui.colored_label(Color32::LIGHT_RED, "Device Name");
                    if let Some(device_name) = computer.device_name.as_mut() {
                        TextEdit::singleline(device_name).desired_width(avail_size.x / 2.14).ui(ui);
                    } else {
                        ui.label(&format!(" - "));
                    }
                    ui.end_row();

                    ui.colored_label(Color32::LIGHT_RED, "Device Mfg");
                    if let Some(device_mfg) = computer.device_mfg.as_mut() {
                        TextEdit::singleline(device_mfg).desired_width(avail_size.x / 2.14).ui(ui);
                    } else {
                        ui.label(&format!(" - "));
                    }
                    ui.end_row();
                    ui.colored_label(Color32::LIGHT_RED, "Device Model");
                    if let Some(device_model) = computer.device_model.as_mut() {
                        TextEdit::singleline(device_model).desired_width(avail_size.x / 2.14).ui(ui);
                    } else {
                        ui.label(&format!(" - "));
                    }
                    ui.end_row();
                    ui.colored_label(Color32::LIGHT_RED, "Device Serial");
                    if let Some(device_serial) = computer.device_serial.as_mut() {
                        TextEdit::singleline(device_serial).desired_width(avail_size.x / 2.14).ui(ui);
                    } else {
                        ui.label(&format!(" - "));
                    }
                    
                    ui.end_row();
                    ui.end_row();

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

            ui.add_space(15.);

            ui.group(|ui| {
                Grid::new("group1").max_col_width(avail_size.x / 3.2 - 2.).min_col_width(avail_size.x / 3.2-2.).with_row_color(|num, style| return_colors(num, style))
                .show(ui, |ui| {
                    ui.colored_label(Color32::LIGHT_RED, "Letter");
                    ui.colored_label(Color32::LIGHT_RED, "Type");
                    ui.colored_label(Color32::LIGHT_RED, "Space Left / Total Size");
                    ui.end_row();

                    for drive_data in &computer.drives{
                        ui.colored_label(Color32::LIGHT_RED, &drive_data.drive_letter);
                        ui.label(&drive_data.drive_type);
                        ui.label(format!("{} Gb / {} Gb", &drive_data.space_left, &drive_data.total_size));
                        ui.end_row();
                    }
                });
            });

            ui.add_space(15.);
        });
    });
    
}
