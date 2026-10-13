use crate::app_state::MastertechContext;
use displays::ui_tools::{
    controls::{self, Metrics},
    icons::svg,
};
use eframe::egui::{Align, Layout, Ui};
use egui_extras::{Column, TableBuilder};
use serde_json::Value;

impl MastertechContext {
    pub fn system_information(&mut self, ui: &mut Ui) {
        let m = Metrics::get(ui.ctx());
        let computer_data = &self.computer_data;
        let gpu = computer_data.gpu.clone();
        let ram = format!("{} Gb", computer_data.ram);
        let specs = [
            (
                svg::DESKTOP_TOWER,
                "System Name",
                computer_data.hostname.as_str(),
            ),
            (svg::CPU, "CPU Name", computer_data.cpu.as_str()),
            (svg::MEMORY, "Total RAM", ram.as_str()),
            (svg::GRAPHICS_CARD, "GPU", gpu.as_str()),
        ];

        ui.push_id("sysinfo_table", |ui| {
            let table = TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .cell_layout(Layout::left_to_right(Align::Center))
                .column(Column::initial(140.0).range(50.0..=280.0).clip(true))
                .column(Column::remainder())
                .min_scrolled_height(0.0);

            table
                .header(m.row_h, |mut header| {
                    header.col(|ui| {
                        controls::section_label(ui, "Hardware Name");
                    });
                    header.col(|ui| {
                        controls::section_label(ui, "Info");
                    });
                })
                .body(|mut body| {
                    for (icon, name, value) in specs {
                        body.row(m.row_h, |mut row| {
                            row.col(|ui| {
                                controls::icon_label(ui, icon, name);
                            });
                            row.col(|ui| {
                                ui.label(value);
                            });
                        });
                    }
                });
        });
        ui.add_space(m.gap.y * 4.0);
        ui.push_id("drives_table", |ui| {
            let disks_table = TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .cell_layout(Layout::left_to_right(Align::Center))
                .column(Column::exact(24.0))
                .column(Column::exact(64.0))
                .column(Column::exact(120.0))
                .column(Column::remainder());

            disks_table
                .header(m.row_h, |mut header| {
                    for title in ["#", "Letter", "Type", "Avail / Total Space"] {
                        header.col(|ui| {
                            controls::section_label(ui, title);
                        });
                    }
                })
                .body(|body| {
                    body.rows(m.row_h, self.disk_num, |mut row| {
                        let disk_index = row.index();
                        if let Some(disk) = self.disks.get(disk_index) {
                            let disk_letter = disk
                                .get("drive_letter")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string();

                            let drive_type =
                                disk.get("drive_type").and_then(Value::as_str).unwrap_or("");

                            row.col(|ui| {
                                ui.label(disk_index.to_string());
                            });
                            row.col(|ui| {
                                controls::icon_label(ui, svg::HARD_DRIVE, &disk_letter);
                            });
                            row.col(|ui| {
                                if !drive_type.starts_with("Unknown") {
                                    ui.label(drive_type);
                                } else {
                                    controls::icon_label(ui, svg::NETWORK, "Network Drive?");
                                }
                            });
                            row.col(|ui| {
                                let disk_space = format!(
                                    "{} Gb / {} Gb",
                                    disk.get("space_left").and_then(Value::as_str).unwrap_or(""),
                                    disk.get("total_size").and_then(Value::as_str).unwrap_or("")
                                );
                                ui.label(disk_space);
                            });
                            self.spinner = false;
                        }
                    });
                });
        });
    }
}
