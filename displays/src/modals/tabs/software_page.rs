use eframe::egui::{Color32, Grid, ScrollArea, Separator, Ui, Vec2, Vec2b, Widget};
use database::schema::ComputerData;

use super::return_colors;

pub fn display_software_page(ui: &mut Ui, computer: &mut ComputerData, avail_size: Vec2) {
    let seb_info = computer.seb_info.as_ref();

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
                if let Some(seb_info) = seb_info {

                    // ui.colored_label(Color32::LIGHT_RED, "Order Details");
                    Grid::new("group3").spacing(Vec2::new(0.0, 6.0))
                    .max_col_width(avail_size.x / 2.14)
                    .min_col_width(avail_size.x / 2.14)
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

            if let Some(seb_info) = seb_info {
                ui.add_space(10.0);
                if let Some(extended_seb) = seb_info.ExtendedSeb.as_ref(){
                    ui.group(|ui| {
                        // ui.colored_label(Color32::LIGHT_RED, "Customer Information");
                        Grid::new("customer_data").max_col_width(avail_size.x / 2.14).min_col_width(avail_size.x / 2.14).with_row_color(|num, style| return_colors(num, style))
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


            ui.group(|ui: &mut Ui| {
                Grid::new("other_software_grid")
                    .spacing(Vec2::new(0.0, 6.0))
                    .spacing(Vec2::new(2., 4.))
                    .max_col_width(avail_size.x / 2.14)
                    .min_col_width(avail_size.x / 2.14)
                    .with_row_color(|num, style| return_colors(num, style))
                    .num_columns(2)
                    .show(ui, |ui| 
                {
                    ui.colored_label(Color32::LIGHT_RED, "Program Name");
                    ui.colored_label(Color32::LIGHT_RED, "Status");
                    ui.end_row();

                    // `current_antivirus` is now `Vec<InstalledSecurityProduct>`
                    // instead of `Vec<String>`. Each row shows the
                    // name + an "Active" / "Disabled" / "—" badge so
                    // operators can tell at a glance whether an
                    // installed AV is actually monitoring.
                    for product in computer.current_antivirus.iter() {
                        let line = match (product.version.as_deref(), product.vendor.as_deref()) {
                            (Some(v), Some(vendor)) => format!("{} {}  ({vendor})", product.name, v),
                            (Some(v), None) => format!("{} {}", product.name, v),
                            (None, Some(vendor)) => format!("{}  ({vendor})", product.name),
                            (None, None) => product.name.clone(),
                        };
                        ui.label(line);
                        let (status_color, status_text) = match product.active {
                            Some(true) => (Color32::from_rgb(100, 200, 100), "Active"),
                            Some(false) => (Color32::from_rgb(255, 150, 80), "Disabled"),
                            None => (Color32::GRAY, "—"),
                        };
                        ui.colored_label(status_color, status_text);
                        ui.end_row();
                    }
                });
            });
            
        });
    });
}