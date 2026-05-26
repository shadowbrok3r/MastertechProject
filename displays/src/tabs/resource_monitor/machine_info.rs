use eframe::egui::{Align, Layout, Ui};
use egui_extras::{Column, TableBuilder};

#[derive(Clone, Debug, Default)]
pub struct MachineDriveRow {
    pub index: usize,
    pub letter: String,
    pub drive_type: String,
    pub space_label: String,
}

#[derive(Clone, Debug, Default)]
pub struct MachineInfo {
    pub hostname: String,
    pub cpu: String,
    pub ram_gb: String,
    pub gpu: String,
    pub drives: Vec<MachineDriveRow>,
}

impl MachineInfo {
    pub fn show(&self, ui: &mut Ui) {
        TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(Layout::left_to_right(Align::Center))
            .column(Column::initial(120.0).range(80.0..=200.0))
            .column(Column::remainder())
            .header(20.0, |mut header| {
                header.col(|ui| {
                    ui.strong("Hardware");
                });
                header.col(|ui| {
                    ui.strong("Info");
                });
            })
            .body(|mut body| {
                for (label, value) in [
                    ("System Name", self.hostname.as_str()),
                    ("CPU Name", self.cpu.as_str()),
                    ("Total RAM", self.ram_gb.as_str()),
                    ("GPU", self.gpu.as_str()),
                ] {
                    body.row(20.0, |mut row| {
                        row.col(|ui| {
                            ui.label(label);
                        });
                        row.col(|ui| {
                            ui.label(value);
                        });
                    });
                }
            });

        ui.add_space(16.0);

        if self.drives.is_empty() {
            return;
        }

        TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(Layout::left_to_right(Align::Center))
            .column(Column::exact(24.0))
            .column(Column::exact(48.0))
            .column(Column::exact(80.0))
            .column(Column::remainder())
            .header(20.0, |mut header| {
                header.col(|ui| {
                    ui.label("#");
                });
                header.col(|ui| {
                    ui.label("Letter");
                });
                header.col(|ui| {
                    ui.label("Type");
                });
                header.col(|ui| {
                    ui.label("Avail / Total");
                });
            })
            .body(|mut body| {
                for drive in &self.drives {
                    body.row(20.0, |mut row| {
                        row.col(|ui| {
                            ui.label(drive.index.to_string());
                        });
                        row.col(|ui| {
                            ui.label(&drive.letter);
                        });
                        row.col(|ui| {
                            ui.label(&drive.drive_type);
                        });
                        row.col(|ui| {
                            ui.label(&drive.space_label);
                        });
                    });
                }
            });
    }
}
