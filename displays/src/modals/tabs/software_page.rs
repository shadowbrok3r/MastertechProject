use eframe::egui::{Grid, ScrollArea, Ui, Vec2, Vec2b};
use database::schema::ComputerData;

use crate::ui_tools::{
    controls::{self, Metrics, Tokens},
    glass_card,
    icons::svg,
    theme,
};

/// Two-column grid of dim field labels and their values.
fn field_grid(ui: &mut Ui, id: &str, col_w: f32, rows: &[(&str, &str)]) {
    let t = Tokens::get(ui.ctx());
    let m = Metrics::get(ui.ctx());
    Grid::new(id)
        .spacing(Vec2::new(m.gap.x / 4.0, m.gap.y))
        .max_col_width(col_w)
        .min_col_width(col_w)
        .striped(true)
        .num_columns(2)
        .show(ui, |ui| {
            for (label, value) in rows {
                ui.colored_label(t.text_dim, *label);
                ui.label(*value);
                ui.end_row();
            }
        });
}

pub fn display_software_page(ui: &mut Ui, computer: &mut ComputerData, avail_size: Vec2) {
    let seb_info = computer.seb_info.as_ref();
    let col_w = avail_size.x / 2.14;

    ScrollArea::vertical()
        .max_height(f32::INFINITY)
        .max_width(avail_size.x)
        .auto_shrink(Vec2b::new(false, false))
        .show(ui, |ui|
    {
        let m = Metrics::get(ui.ctx());
        ui.vertical_centered(|ui| {
            if controls::section_header(ui, "task_software_seb", svg::CLOUD_CHECK, "SEB Information") {
                glass_card::group(ui, |ui| {
                    if let Some(seb_info) = seb_info {
                        field_grid(ui, "group3", col_w, &[
                            ("InstalledDeviceId:", &seb_info.InstalledDeviceId),
                            ("InstallInstanceId:", &seb_info.InstallInstanceId),
                            ("HasIssues:", &seb_info.HasIssues),
                            ("InstallationStage:", &seb_info.InstallationStage),
                            ("ReasonCode:", &seb_info.ReasonCode),
                            ("ActivationCode:", &seb_info.ActivationCode),
                            ("InstallVersion:", &seb_info.InstallVersion),
                            ("MachineName:", &seb_info.MachineName),
                        ]);
                    } else {
                        controls::empty_state(
                            ui,
                            svg::CLOUD_SLASH,
                            "No SEB information",
                            "No SEB information was sent with ticket.",
                            &[],
                        );
                    }
                });

                if let Some(seb_info) = seb_info {
                    ui.add_space(m.gap.y);
                    if let Some(extended_seb) = seb_info.ExtendedSeb.as_ref() {
                        glass_card::group(ui, |ui| {
                            field_grid(ui, "customer_data", col_w, &[
                                ("email:", &extended_seb.email),
                                ("phone:", &extended_seb.phone),
                                ("device_name:", &extended_seb.device_name),
                                ("device_id:", &extended_seb.device_id),
                                ("state:", &extended_seb.state),
                                ("usage_gb:", &extended_seb.usage_gb),
                                ("date_device_created:", &extended_seb.date_device_created),
                                ("activated:", &extended_seb.activated),
                                ("activation_code:", &extended_seb.activation_code),
                                ("last_complete_backup:", &extended_seb.last_complete_backup),
                                ("last_client_status_update:", &extended_seb.last_client_status_update),
                                ("id_recurly_account:", &extended_seb.id_recurly_account),
                                ("date_last_scan:", &extended_seb.date_last_scan),
                                ("current_period_ends_at:", &extended_seb.current_period_ends_at),
                                ("date_modified:", &extended_seb.date_modified),
                                ("date_created:", &extended_seb.date_created),
                            ]);
                        });
                    } else {
                        ui.vertical_centered(|ui| {
                            ui.set_max_width(avail_size.x / 2.0);
                            controls::callout(ui, theme::warn(ui), svg::WARNING, |ui| {
                                ui.label(
                                    "SEB information was sent with ticket, but we didnt get the extended SEB info",
                                );
                            });
                        });
                    }
                }
            }

            ui.add_space(m.gap.y);
            if controls::section_header(ui, "task_software_other", svg::APP_WINDOW, "Other software") {
                let t = Tokens::get(ui.ctx());
                glass_card::group(ui, |ui| {
                    Grid::new("other_software_grid")
                        .spacing(Vec2::new(m.gap.x / 4.0, m.pad.y))
                        .max_col_width(col_w)
                        .min_col_width(col_w)
                        .striped(true)
                        .num_columns(2)
                        .show(ui, |ui| {
                            ui.colored_label(t.text_dim, "Program Name");
                            ui.colored_label(t.text_dim, "Status");
                            ui.end_row();

                            // Each product, then whether it is actively monitoring.
                            for product in computer.current_antivirus.iter() {
                                let line = match (product.version.as_deref(), product.vendor.as_deref()) {
                                    (Some(v), Some(vendor)) => format!("{} {}  ({vendor})", product.name, v),
                                    (Some(v), None) => format!("{} {}", product.name, v),
                                    (None, Some(vendor)) => format!("{}  ({vendor})", product.name),
                                    (None, None) => product.name.clone(),
                                };
                                controls::icon_label(ui, svg::SHIELD, &line);
                                match product.active {
                                    Some(true) => {
                                        controls::status_label(ui, svg::SHIELD_CHECK, "Active", theme::success(ui))
                                    }
                                    Some(false) => {
                                        controls::status_label(ui, svg::SHIELD_SLASH, "Disabled", theme::warn(ui))
                                    }
                                    None => ui.colored_label(theme::weak_text(ui), "—"),
                                };
                                ui.end_row();
                            }
                        });
                });
            }
        });
    });
}
