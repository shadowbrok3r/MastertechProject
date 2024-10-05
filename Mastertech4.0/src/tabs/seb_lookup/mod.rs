use crate::app_state::MastertechContext;
use anyhow::Result;
use database::schema::LocalSebData;
use eframe::egui::{
    Align, Button, CentralPanel, Color32, Direction, Grid, Layout, Separator, Style, TextEdit,
    TopBottomPanel, Ui, Vec2, Widget,
};
use egui_extras::{Size, StripBuilder};
use log::{error, info};
use tokio::spawn;

use super::tur_sheet::get_ticket::request_seb_info;

impl MastertechContext {
    pub fn seb_lookup(&mut self, ui: &mut Ui) {
        TopBottomPanel::top("SebLookupTopPanel")
            .exact_height(30.)
            .show_inside(ui, |ui| {
                ui.horizontal_top(|ui| {
                    TextEdit::singleline(&mut self.data_viewer.filter)
                        .hint_text("Search with Email or Device ID")
                        .ui(ui);

                    ui.add_space(10.);

                    if Button::new("Lookup SEB Info").ui(ui).clicked() {
                        let tx = self.seb_channel.0.clone();
                        let client = self.client.clone();
                        let search_string = self.data_viewer.filter.clone();
                        spawn(async move {
                            let seb_data: Result<LocalSebData, anyhow::Error> =
                                request_seb_info(client, Some(search_string))
                                    .await
                                    .or_else(|err| {
                                        error!("Error Pulling SEB info: {:?}", err.to_string());
                                        Err(err)
                                    })
                                    .and_then(|data| {
                                        info!("Pulled SEB Data successfully: {data:#?}");
                                        Ok(data)
                                    });

                            tx.try_send(seb_data.unwrap()).unwrap();
                        });
                    }
                    ui.add_space(10.);

                    if Button::new("Show Local Device SEB Info").ui(ui).clicked() {
                        let tx = self.seb_channel.0.clone();
                        let client = self.client.clone();
                        spawn(async move {
                            let seb_data: Result<LocalSebData, anyhow::Error> =
                                request_seb_info(client, None)
                                    .await
                                    .or_else(|err| {
                                        error!("Error Pulling SEB info: {:?}", err.to_string());
                                        Err(err)
                                    })
                                    .and_then(|data| {
                                        info!("Pulled SEB Data successfully: {data:#?}");
                                        Ok(data)
                                    });

                            tx.try_send(seb_data.unwrap()).unwrap();
                        });
                    }
                });
            });

        CentralPanel::default().show_inside(ui, |ui| display_seb_page(ui, self.seb_info.clone()));
    }
}

fn display_seb_page(ui: &mut Ui, seb_info: Option<LocalSebData>) {
    fn return_colors(num: usize, _style: &Style) -> Option<Color32> {
        let mut _col = Color32::from_rgb(30, 30, 38);
        if num % 2 == 0 {
            _col = Color32::from_rgb(15, 15, 22);
        } else {
            _col = Color32::from_rgb(30, 30, 38);
        }
        Some(_col)
    }

    ui.horizontal(|ui: &mut Ui| ui.add_space(10.0));

    StripBuilder::new(ui)
        .cell_layout(Layout::from_main_dir_and_cross_align(
            Direction::TopDown,
            Align::Center,
        ))
        .size(Size::remainder())
        .vertical(|mut s| {
            s.strip(|s| {
                s.cell_layout(Layout::centered_and_justified(Direction::TopDown))
                    .size(Size::exact(660.))
                    .horizontal(|mut s| {
                        s.cell(|ui| {
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
                                    if let Some(seb_info) = seb_info.as_ref() {
                                        Grid::new("group3")
                                            .spacing(Vec2::new(0.0, 6.0))
                                            .with_row_color(|num, style| return_colors(num, style))
                                            .show(ui, |ui| {
                                                ui.colored_label(
                                                    Color32::LIGHT_RED,
                                                    "InstalledDeviceId:",
                                                );
                                                ui.label(&seb_info.InstalledDeviceId);
                                                ui.end_row();
                                                ui.colored_label(
                                                    Color32::LIGHT_RED,
                                                    "InstallInstanceId:",
                                                );
                                                ui.label(&seb_info.InstallInstanceId);
                                                ui.end_row();
                                                ui.colored_label(Color32::LIGHT_RED, "HasIssues:");
                                                ui.label(&seb_info.HasIssues);
                                                ui.end_row();
                                                ui.colored_label(
                                                    Color32::LIGHT_RED,
                                                    "InstallationStage:",
                                                );
                                                ui.label(&seb_info.InstallationStage);
                                                ui.end_row();
                                                ui.colored_label(Color32::LIGHT_RED, "ReasonCode:");
                                                ui.label(&seb_info.ReasonCode);
                                                ui.end_row();
                                                ui.colored_label(
                                                    Color32::LIGHT_RED,
                                                    "ActivationCode:",
                                                );
                                                ui.label(&seb_info.ActivationCode);
                                                ui.end_row();
                                                ui.colored_label(
                                                    Color32::LIGHT_RED,
                                                    "InstallVersion:",
                                                );
                                                ui.label(&seb_info.InstallVersion);
                                                ui.end_row();
                                                ui.colored_label(
                                                    Color32::LIGHT_RED,
                                                    "MachineName:",
                                                );
                                                ui.label(&seb_info.MachineName);
                                                ui.end_row();
                                            });
                                    } else {
                                        ui.colored_label(
                                            Color32::LIGHT_RED,
                                            "Type in a customer email or run 'Show Local Device SEB Info' to pull SEB Data",
                                        );
                                    }
                                    if let Some(seb_info) = seb_info {
                                        ui.add_space(10.0);
                                        if let Some(extended_seb) = seb_info.ExtendedSeb.as_ref() {
                                            ui.group(|ui| {
                                                Grid::new("customer_data")
                                                    .with_row_color(|num, style| {
                                                        return_colors(num, style)
                                                    })
                                                    .show(ui, |ui| {
                                                        ui.colored_label(
                                                            Color32::LIGHT_RED,
                                                            "email:",
                                                        );
                                                        ui.label(&extended_seb.email);
                                                        ui.end_row();
                                                        ui.colored_label(
                                                            Color32::LIGHT_RED,
                                                            "phone:",
                                                        );
                                                        ui.label(&extended_seb.phone);
                                                        ui.end_row();
                                                        ui.colored_label(
                                                            Color32::LIGHT_RED,
                                                            "device_name:",
                                                        );
                                                        ui.label(&extended_seb.device_name);
                                                        ui.end_row();
                                                        ui.colored_label(
                                                            Color32::LIGHT_RED,
                                                            "device_id:",
                                                        );
                                                        ui.label(&extended_seb.device_id);
                                                        ui.end_row();
                                                        ui.colored_label(
                                                            Color32::LIGHT_RED,
                                                            "state:",
                                                        );
                                                        ui.label(&extended_seb.state);
                                                        ui.end_row();
                                                        ui.colored_label(
                                                            Color32::LIGHT_RED,
                                                            "usage_gb:",
                                                        );
                                                        ui.label(&extended_seb.usage_gb);
                                                        ui.end_row();
                                                        ui.colored_label(
                                                            Color32::LIGHT_RED,
                                                            "date_device_created:",
                                                        );
                                                        ui.label(&extended_seb.date_device_created);
                                                        ui.end_row();
                                                        ui.colored_label(
                                                            Color32::LIGHT_RED,
                                                            "activated:",
                                                        );
                                                        ui.label(&extended_seb.activated);
                                                        ui.end_row();
                                                        ui.colored_label(
                                                            Color32::LIGHT_RED,
                                                            "activation_code:",
                                                        );
                                                        ui.label(&extended_seb.activation_code);
                                                        ui.end_row();
                                                        ui.colored_label(
                                                            Color32::LIGHT_RED,
                                                            "last_complete_backup:",
                                                        );
                                                        ui.label(
                                                            &extended_seb.last_complete_backup,
                                                        );
                                                        ui.end_row();
                                                        ui.colored_label(
                                                            Color32::LIGHT_RED,
                                                            "last_client_status_update:",
                                                        );
                                                        ui.label(
                                                            &extended_seb.last_client_status_update,
                                                        );
                                                        ui.end_row();
                                                        ui.colored_label(
                                                            Color32::LIGHT_RED,
                                                            "id_recurly_account:",
                                                        );
                                                        ui.label(&extended_seb.id_recurly_account);
                                                        ui.end_row();
                                                        ui.colored_label(
                                                            Color32::LIGHT_RED,
                                                            "date_last_scan:",
                                                        );
                                                        ui.end_row();
                                                        ui.colored_label(
                                                            Color32::LIGHT_RED,
                                                            "current_period_ends_at:",
                                                        );
                                                        ui.label(
                                                            &extended_seb.current_period_ends_at,
                                                        );
                                                        ui.end_row();
                                                        ui.colored_label(
                                                            Color32::LIGHT_RED,
                                                            "date_modified:",
                                                        );
                                                        ui.label(&extended_seb.date_modified);
                                                        ui.end_row();
                                                        ui.colored_label(
                                                            Color32::LIGHT_RED,
                                                            "date_created:",
                                                        );
                                                        ui.label(&extended_seb.date_created);
                                                        ui.end_row();
                                                    });
                                            });
                                        }
                                    }
                                });
                            });
                        });
                    });
            });
        });
}
