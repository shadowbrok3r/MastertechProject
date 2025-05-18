use eframe::egui::{Color32, Grid, ScrollArea, Separator, Ui, Vec2, Vec2b, Widget};
use database::schema::{ComputerData, TaskPayload};

use super::return_colors;

pub fn display_software_page(ui: &mut Ui, task: &mut TaskPayload, avail_size: Vec2) {
    let Some(ticket) = task.service_ticket.as_ref() else { return; };
    let computer = if let Some(computer) = ticket.computer.as_ref() { computer } else { &ComputerData::default() };

    let seb_info = computer.seb_info.as_ref();
    ui.horizontal(|ui: &mut Ui| ui.add_space(15.0));

    ScrollArea::vertical()
        .max_height(f32::INFINITY)
        .max_width(avail_size.x)
        .auto_shrink(Vec2b::new(false, false))
        .show(ui, |ui|
    {
        ui.vertical_centered(|ui| {
            ui.scope(|ui| {
                ui.add_space(8.0);
                Separator::default().shrink(150.0).ui(ui);
                ui.add_space(8.0);
                ui.heading("SEB Information");
                ui.add_space(8.0);
                Separator::default().shrink(150.0).ui(ui);
                ui.add_space(8.0);
            });

            ui.group(|ui| {
                if let Some(seb_info) = seb_info{

                    // ui.colored_label(Color32::LIGHT_RED, "Order Details");
                    Grid::new("group3").spacing(Vec2::new(0.0, 6.0))
                    .max_col_width(avail_size.x / 2.15)
                    .min_col_width(avail_size.x / 2.15)
                    .with_row_color(|num, style| return_colors(num, style))
                    .show(ui, |ui| {
                        ui.colored_label(Color32::LIGHT_RED, "InstalledDeviceId:");
                        ui.label(&seb_info.InstalledDeviceId);
                        ui.end_row();
                        ui.colored_label(Color32::LIGHT_RED, "InstallInstanceId:");
                        ui.label(&seb_info.InstallInstanceId);
                        ui.end_row();
                        ui.colored_label(Color32::LIGHT_RED, "HasIssues:");
                        ui.label(&seb_info.HasIssues);
                        ui.end_row();
                        ui.colored_label(Color32::LIGHT_RED, "InstallationStage:");
                        ui.label(&seb_info.InstallationStage);
                        ui.end_row();
                        ui.colored_label(Color32::LIGHT_RED, "ReasonCode:");
                        ui.label(&seb_info.ReasonCode);
                        ui.end_row();
                        ui.colored_label(Color32::LIGHT_RED, "ActivationCode:");
                        ui.label(&seb_info.ActivationCode);
                        ui.end_row();
                        ui.colored_label(Color32::LIGHT_RED, "InstallVersion:");
                        ui.label(&seb_info.InstallVersion);
                        ui.end_row();
                        ui.colored_label(Color32::LIGHT_RED, "MachineName:");
                        ui.label(&seb_info.MachineName);
                        ui.end_row();
                    });
                }else{
                    ui.colored_label(Color32::LIGHT_RED, "No SEB information was sent with ticket.");
                }
            });

            if let Some(seb_info) = seb_info{
                ui.add_space(10.0);
                if let Some(extended_seb) = seb_info.ExtendedSeb.as_ref(){
                    ui.group(|ui| {
                        // ui.colored_label(Color32::LIGHT_RED, "Customer Information");
                        Grid::new("customer_data").max_col_width(avail_size.x / 2.15).min_col_width(avail_size.x / 2.15).with_row_color(|num, style| return_colors(num, style))
                        .show(ui, |ui| {
                            ui.colored_label(Color32::LIGHT_RED, "email:");
                            ui.label(&extended_seb.email);
                            ui.end_row();
                            ui.colored_label(Color32::LIGHT_RED, "phone:");
                            ui.label(&extended_seb.phone);
                            ui.end_row();
                            ui.colored_label(Color32::LIGHT_RED, "device_name:");
                            ui.label(&extended_seb.device_name);
                            ui.end_row();
                            ui.colored_label(Color32::LIGHT_RED, "device_id:");
                            ui.label(&extended_seb.device_id);
                            ui.end_row();
                            ui.colored_label(Color32::LIGHT_RED, "state:");
                            ui.label(&extended_seb.state);
                            ui.end_row();
                            ui.colored_label(Color32::LIGHT_RED, "usage_gb:");
                            ui.label(&extended_seb.usage_gb);
                            ui.end_row();
                            ui.colored_label(Color32::LIGHT_RED, "date_device_created:");
                            ui.label(&extended_seb.date_device_created);
                            ui.end_row();
                            ui.colored_label(Color32::LIGHT_RED, "activated:");
                            ui.label(&extended_seb.activated);
                            ui.end_row();
                            ui.colored_label(Color32::LIGHT_RED, "activation_code:");
                            ui.label(&extended_seb.activation_code);
                            ui.end_row();
                            ui.colored_label(Color32::LIGHT_RED, "last_complete_backup:");
                            ui.label(&extended_seb.last_complete_backup);
                            ui.end_row();
                            ui.colored_label(Color32::LIGHT_RED, "last_client_status_update:");
                            ui.label(&extended_seb.last_client_status_update);
                            ui.end_row();
                            ui.colored_label(Color32::LIGHT_RED, "id_recurly_account:");
                            ui.label(&extended_seb.id_recurly_account);
                            ui.end_row();
                            ui.colored_label(Color32::LIGHT_RED, "date_last_scan:");
                            ui.end_row();
                            ui.colored_label(Color32::LIGHT_RED, "current_period_ends_at:");
                            ui.label(&extended_seb.current_period_ends_at);
                            ui.end_row();
                            ui.colored_label(Color32::LIGHT_RED, "date_modified:");
                            ui.label(&extended_seb.date_modified);
                            ui.end_row();
                            ui.colored_label(Color32::LIGHT_RED, "date_created:");
                            ui.label(&extended_seb.date_created);
                            ui.end_row();
                        });
                    });
                }else{
                    ui.vertical_centered(|ui|{
                        ui.set_max_width(avail_size.x / 2.0);
                        ui.colored_label(Color32::LIGHT_RED, "SEB information was sent with ticket, but we didnt get the extended SEB info");
                    });
                }
            }
        
            ui.scope(|ui| {
                ui.add_space(8.0);
                Separator::default().shrink(150.0).ui(ui);
                ui.add_space(8.0);
                ui.heading("Other software");
                ui.add_space(8.0);
                Separator::default().shrink(150.0).ui(ui);
                ui.add_space(8.0);
            });

            if let Some(antivirus) = ticket.current_antivirus.as_ref() {
                ui.group(|ui: &mut Ui| {
                    Grid::new("other_software_grid")
                        .spacing(Vec2::new(0.0, 6.0))
                        .spacing(Vec2::new(2., 4.))
                        .max_col_width(avail_size.x / 2.15)
                        .min_col_width(avail_size.x / 2.15)
                        .with_row_color(|num, style| return_colors(num, style))
                        .num_columns(2)
                        .show(ui, |ui| 
                    {
                        ui.colored_label(Color32::LIGHT_RED, "Current Antivirus:");
                        ui.label("");
                        ui.end_row();
                        for antivirus in antivirus.iter() {
                            ui.label(antivirus);
                            ui.end_row();
                        }

                        ui.colored_label(Color32::LIGHT_RED, "Installed Programs");
                        ui.label("");
                        ui.end_row();
                    });
                });
            }
        });
    });
}