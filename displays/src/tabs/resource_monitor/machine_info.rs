use eframe::egui::{Align, Layout, RichText, Ui};
use egui_extras::{Column, TableBuilder};
use facet::Facet;

use super::ABSENT;
use crate::ui_tools::{icons, theme};

#[derive(Clone, Debug, Default)]
pub struct MachineDriveRow {
    pub index: usize,
    pub letter: String,
    pub drive_type: String,
    pub space_label: String,
}

#[derive(Clone, Debug, Default, Facet)]
pub struct MachineInfo {
    /// Machine host name reported by the OS.
    #[facet(rename = "System Name")]
    pub hostname: String,
    /// Primary processor model string.
    #[facet(rename = "CPU Name")]
    pub cpu: String,
    /// Installed physical memory.
    #[facet(rename = "Total RAM")]
    pub ram_gb: String,
    /// Primary display-adapter model.
    #[facet(rename = "GPU")]
    pub gpu: String,
    #[facet(opaque)]
    pub drives: Vec<MachineDriveRow>,
}

impl MachineInfo {
    /// Host, CPU, RAM, GPU and volume count on one wrapped line.
    pub fn show_header_line(&self, ui: &mut Ui) {
        ui.horizontal_wrapped(|ui| {
            fact(ui, icons::DESKTOP, &self.hostname, "host");
            fact(ui, icons::p::CPU, &self.cpu, "CPU");
            fact(ui, icons::p::MEMORY, &self.ram_gb, "RAM");
            fact(ui, icons::p::GRAPHICS_CARD, &self.gpu, "GPU");
            let volumes = match self.drives.len() {
                0 => String::new(),
                1 => "1 volume".to_string(),
                n => format!("{n} volumes"),
            };
            fact(ui, icons::HARD_DRIVE, &volumes, "volumes");
        });
    }

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
                for field in database::shape_walk::rows(self) {
                    body.row(20.0, |mut row| {
                        row.col(|ui| {
                            let resp = ui.label(&field.label);
                            if let Some(h) = &field.hover {
                                resp.on_hover_text(h);
                            }
                        });
                        row.col(|ui| {
                            ui.label(&field.value);
                        });
                    });
                }
            });

        ui.add_space(16.0);
        self.show_volumes(ui);
    }

    /// Per-volume letter, filesystem and free/total space.
    pub fn show_volumes(&self, ui: &mut Ui) {
        if self.drives.is_empty() {
            ui.colored_label(theme::weak_text(ui), "No volumes reported.");
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

/// Icon plus one machine fact; an empty or `?` value renders as the absent marker.
fn fact(ui: &mut Ui, glyph: &str, value: &str, hover: &str) {
    let known = !value.trim().is_empty() && value.trim() != "?";
    ui.label(icons::icon_colored(glyph, theme::accent(ui)).small())
        .on_hover_text(hover);
    let text = if known { value } else { ABSENT };
    let color = if known {
        theme::strong_text(ui)
    } else {
        theme::weak_text(ui)
    };
    ui.label(RichText::new(text).small().color(color));
    ui.add_space(8.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_info_rows_use_renames_and_skip_drives() {
        let mi = MachineInfo {
            hostname: "PC-1".into(),
            cpu: "Ryzen 9".into(),
            ram_gb: "32 GB".into(),
            gpu: "RTX 4070".into(),
            drives: vec![MachineDriveRow::default()],
        };
        let rows = database::shape_walk::rows(&mi);
        let pairs: Vec<(&str, &str)> =
            rows.iter().map(|r| (r.label.as_str(), r.value.as_str())).collect();
        assert_eq!(
            pairs,
            [
                ("System Name", "PC-1"),
                ("CPU Name", "Ryzen 9"),
                ("Total RAM", "32 GB"),
                ("GPU", "RTX 4070"),
            ]
        );
        assert!(rows.iter().all(|r| r.label != "Drives"));
        assert_eq!(rows[0].hover.as_deref(), Some("Machine host name reported by the OS."));
    }
}
