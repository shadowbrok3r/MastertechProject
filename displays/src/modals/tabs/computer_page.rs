use eframe::egui::{Grid, ScrollArea, Ui, Vec2, Vec2b};
use database::schema::{random_record_id, ComputerData, RecordIdExt, TicketData, COMPUTER_TABLE};

use crate::{
    ui_tools::{
        controls::{
            self, BarKind, ButtonKind, IconBox, IconLabel, Metrics, TextButton, TextField, Tokens,
        },
        glass_card,
        icons::{self, svg},
        theme,
    },
    PlatformSpawner, Spawner,
};

/// Computer selection data for switching computers in task modal
pub struct ComputerSearchData<'a> {
    /// Stores the currently selected label string so the ComboBox shows the
    /// right item across frames.
    pub search_query: &'a mut String,
    pub customer_computers: &'a Vec<ComputerData>,
    pub selected_computer: &'a mut Option<ComputerData>,
    /// Set to `true` by this widget when the "Import from PrestaShop" button is clicked.
    pub import_presta_clicked: &'a mut bool,
    /// Set to `true` by this widget when the "Import from Everest" button is clicked.
    pub import_everest_clicked: &'a mut bool,
    /// Set by this widget to a customer computer the user wants to view in a
    /// separate modal, without changing the task's current computer.
    pub open_in_second_modal: &'a mut Option<ComputerData>,
}

/// Root-only delete for a computer row, used to clear placeholder records built
/// from an order rather than from client hardware. The delete refuses while the
/// row is still referenced by a client, diagnostic session, or service order.
fn delete_computer_button(ui: &mut Ui, computer: &ComputerData) {
    use crate::{get_toast_sender, ToastMessage};

    let placeholder = database::schema::entity_link::is_placeholder_computer(computer);
    let hint = if placeholder {
        "Double click to delete this placeholder computer record.\n\
         Refused if any client, diagnostic session, or service order still links to it."
    } else {
        "Double click to delete this computer record.\n\
         Refused if any client, diagnostic session, or service order still links to it."
    };
    let response = TextButton::new("Delete")
        .kind(ButtonKind::Danger)
        .icon(svg::TRASH)
        .show(ui)
        .on_hover_text(hint);

    if !response.double_clicked() {
        return;
    }
    let id = computer.id.clone();
    let label = computer.id.key_string();
    PlatformSpawner::spawn(async move {
        let toast = get_toast_sender();
        match database::schema::entity_link::delete_computer_if_unreferenced(&id).await {
            Ok(true) => {
                log::info!("Deleted computer record {label}");
                let _ = toast.try_send(ToastMessage::Success(format!("Deleted computer {label}")));
            }
            Ok(false) => {
                log::info!("Refused to delete computer {label}: still referenced");
                let _ = toast.try_send(ToastMessage::Warning(format!(
                    "Computer {label} is still linked to a client, diagnostic session, or service order"
                )));
            }
            Err(e) => {
                log::error!("Error deleting computer {label}: {e:?}");
                let _ = toast.try_send(ToastMessage::Error(format!("Delete failed: {e}")));
            }
        }
    });
}

/// Build the display label for a computer in the selector.
/// Format: `{Device Mfg}:{Device Model} - {hostname}` (falls back gracefully).
fn computer_label(comp: &ComputerData) -> String {
    let mfg = comp.device_mfg.as_deref().filter(|s| !s.is_empty()).unwrap_or("-");
    let model = comp.device_model.as_deref().filter(|s| !s.is_empty()).unwrap_or("-");
    let host = if comp.hostname.is_empty() { comp.id.key_string() } else { comp.hostname.clone() };
    format!("{mfg}:{model} - {host}")
}

pub fn display_computer_page(
    ui: &mut Ui,
    service_ticket: Option<&mut TicketData>,
    computer: Option<&mut ComputerData>,
    avail_size: Vec2
) {
    display_computer_page_with_search(ui, service_ticket, computer, avail_size, None, None);
}

pub fn display_computer_page_with_search(
    ui: &mut Ui,
    service_ticket: Option<&mut TicketData>,
    computer: Option<&mut ComputerData>,
    avail_size: Vec2,
    search_data: Option<ComputerSearchData>,
    mut service_history_open: Option<&mut bool>,
) {
    let ticket = if let Some(ticket) = service_ticket {
        ticket
    } else { &mut TicketData::default() };

    let computer = if let Some(computer) = computer {
        computer
    } else { &mut ComputerData{ id: random_record_id(COMPUTER_TABLE), ..Default::default() } };

    let t = Tokens::get(ui.ctx());
    let m = Metrics::get(ui.ctx());
    let col_w = avail_size.x / 2.14;

    ScrollArea::vertical()
        .max_height(f32::INFINITY)
        .max_width(avail_size.x)
        .auto_shrink(Vec2b::new(false, false))
        .show(ui, |ui|
    {
        ui.vertical_centered(|ui| {
            // Computer search/selection section (if search data is provided)
            if let Some(search) = search_data {
                if !search.customer_computers.is_empty() {
                    // Build a label-per-computer list; the index maps back to the Vec.
                    let labels: Vec<String> = search.customer_computers.iter()
                        .map(|c| computer_label(c))
                        .collect();

                    glass_card::group(ui, |ui| {
                        glass_card::card_header(ui, icons::p::DESKTOP_TOWER, "Customer's Computers", None);

                        let current = labels.iter().position(|l| *l == *search.search_query);
                        let mut options: Vec<(Option<usize>, &str)> = labels
                            .iter()
                            .enumerate()
                            .map(|(i, label)| (Some(i), label.as_str()))
                            .collect();
                        if current.is_none() {
                            options.insert(0, (None, "Select a computer…"));
                        }
                        let mut picked = current;
                        let width = ui.available_width() - m.gap.x;
                        controls::dropdown(ui, "customer_computers_combo", &mut picked, &options, width);

                        if let Some(idx) = picked.filter(|_| picked != current)
                            && let Some(comp) = search.customer_computers.get(idx)
                        {
                            *search.search_query = labels[idx].clone();
                            computer.id = comp.id.clone();
                            computer.hostname = comp.hostname.clone();
                            computer.cpu = comp.cpu.clone();
                            computer.gpu = comp.gpu.clone();
                            computer.ram = comp.ram.clone();
                            computer.operating_system = comp.operating_system.clone();
                            computer.drives = comp.drives.clone();
                            computer.motherboard_name = comp.motherboard_name.clone();
                            computer.device_name = comp.device_name.clone();
                            computer.device_mfg = comp.device_mfg.clone();
                            computer.device_model = comp.device_model.clone();
                            computer.device_serial = comp.device_serial.clone();
                            computer.customer = comp.customer.clone();
                            computer.seb_info = comp.seb_info.clone();
                            computer.windows_active = comp.windows_active;
                            ticket.computer = Some(computer.id.clone());
                            log::info!("Selected computer: {} ({})", comp.hostname, comp.id.key_string());
                        }

                        ui.add_space(m.pad.y);
                        ui.horizontal(|ui| {
                            if controls::icon_text_button(ui, svg::STOREFRONT, "Import from PrestaShop").clicked() {
                                *search.import_presta_clicked = true;
                            }
                            if controls::icon_text_button(ui, svg::DATABASE, "Import from Everest").clicked() {
                                *search.import_everest_clicked = true;
                            }
                        });

                        // Open another of the customer's computers in its own modal,
                        // without overwriting the task's current computer.
                        let others: Vec<ComputerData> = search.customer_computers.iter()
                            .filter(|c| c.id != computer.id)
                            .cloned()
                            .collect();
                        if !others.is_empty() {
                            ui.add_space(m.gap.y);
                            controls::hairline(ui);
                            controls::section_label(ui, "Other computers for this customer");
                            for comp in &others {
                                ui.horizontal(|ui| {
                                    controls::icon_label(ui, svg::LAPTOP, &computer_label(comp));
                                    if controls::icon_button(
                                        ui,
                                        svg::ARROW_SQUARE_OUT,
                                        IconBox::Md,
                                        false,
                                        "View this computer in a separate window",
                                    )
                                    .clicked()
                                    {
                                        *search.open_in_second_modal = Some(comp.clone());
                                    }
                                });
                            }
                        }
                    });
                    ui.add_space(m.gap.y);
                }
            }

            controls::bar(ui, BarKind::Tool, |ui| {
                if controls::icon_text_button(ui, svg::FLOPPY_DISK, "Update").clicked() {
                    let computer_data: ComputerData = computer.clone();
                    let ticket_computer = ticket.computer.clone();
                    PlatformSpawner::spawn(async move {
                        match computer_data.update_computer().await {
                            Ok(pc) => log::info!("Updated computer: {pc:?}"),
                            Err(e) => log::error!("Error creating computer: {e:?}"),
                        }
                        // Also update the ticket if computer was selected
                        if ticket_computer.is_some() {
                            // The ticket update will be handled separately
                        }
                    });
                }
                if crate::tabs::admin_console::current_user_is_root() {
                    delete_computer_button(ui, computer);
                }
            });
            glass_card::group(ui, |ui| {
                glass_card::card_header(ui, icons::p::CPU, "Hardware", None);
                Grid::new(format!("{} grid", computer.cpu))
                .max_col_width(col_w)
                .min_col_width(col_w)
                .striped(true)
                .show(ui, |ui| {
                    ui.colored_label(t.text_dim, "ID");
                    if let Some(sho) = service_history_open.as_deref_mut() {
                        if TextButton::new(&computer.id.key_string())
                            .kind(ButtonKind::Ghost)
                            .icon(svg::CLOCK_COUNTER_CLOCKWISE)
                            .show(ui)
                            .on_hover_text("View all services for this computer")
                            .clicked()
                        {
                            *sho = true;
                        }
                    } else {
                        ui.label(computer.id.key_string());
                    }
                    ui.end_row();

                    ui.colored_label(t.text_dim, "Linked Customer");
                    ui.label(computer.customer.as_ref().map_or_else(|| "—".to_string(), |id| id.key_string()));
                    ui.end_row();

                    ui.colored_label(t.text_dim, "Hostname");
                    TextField::new(&mut computer.hostname).width(col_w).show(ui);
                    ui.end_row();
                    ui.colored_label(t.text_dim, "Operating System");
                    TextField::new(&mut computer.operating_system).width(col_w).icon(svg::WINDOWS_LOGO).show(ui);
                    ui.end_row();
                    if let Some(active) = &computer.windows_active {
                        ui.colored_label(t.text_dim, "Windows Active");
                        ui.label(format!("{active}"));
                        ui.end_row();
                    }
                    ui.colored_label(t.text_dim, "CPU");
                    TextField::new(&mut computer.cpu).width(col_w).icon(svg::CPU).show(ui);
                    ui.end_row();
                    ui.colored_label(t.text_dim, "GPU");
                    TextField::new(&mut computer.gpu).width(col_w).icon(svg::GRAPHICS_CARD).show(ui);
                    ui.end_row();
                    ui.colored_label(t.text_dim, "RAM");
                    TextField::new(&mut computer.ram).width(col_w).icon(svg::MEMORY).show(ui);
                    ui.end_row();
                    if let Some(battery) = computer.battery.clone() {
                        ui.colored_label(t.text_dim, "Battery");
                        let (icon, color) = match battery.health_percent() {
                            Some(pct) if pct < 60 => (svg::BATTERY_WARNING, theme::error(ui)),
                            Some(pct) if pct < 80 => (svg::BATTERY_LOW, theme::warn(ui)),
                            _ => (svg::BATTERY_FULL, theme::text(ui)),
                        };
                        IconLabel::new(icon, &battery.summary())
                            .tone(color)
                            .show(ui)
                            .on_hover_text(battery.name.clone());
                        ui.end_row();
                    }
                    let optional_rows = [
                        ("Device Name", computer.device_name.as_mut()),
                        ("Device Mfg", computer.device_mfg.as_mut()),
                        ("Device Model", computer.device_model.as_mut()),
                        ("Device Serial", computer.device_serial.as_mut()),
                    ];
                    for (label, value) in optional_rows {
                        ui.colored_label(t.text_dim, label);
                        if let Some(value) = value {
                            TextField::new(value).width(col_w).show(ui);
                        } else {
                            ui.label(" - ");
                        }
                        ui.end_row();
                    }
                    ui.end_row();

                    ui.colored_label(t.text_dim, "HDD Test:");
                    ui.label(&ticket.hardware_test_results.hdd_test);
                    ui.end_row();
                    ui.colored_label(t.text_dim, "SSD Test:");
                    ui.label(&ticket.hardware_test_results.ssd_test);
                    ui.end_row();
                    ui.colored_label(t.text_dim, "RAM Test:");
                    ui.label(&ticket.hardware_test_results.ram_test);
                    ui.end_row();

                });
            });

            ui.add_space(2.0 * m.gap.y);

            glass_card::group(ui, |ui| {
                glass_card::card_header(ui, icons::p::HARD_DRIVES, "Drives", None);
                let drive_w = avail_size.x / 3.2 - 2.0;
                Grid::new("group1").max_col_width(drive_w).min_col_width(drive_w).striped(true)
                .show(ui, |ui| {
                    ui.colored_label(t.text_dim, "Letter");
                    ui.colored_label(t.text_dim, "Type");
                    ui.colored_label(t.text_dim, "Space Left / Total Size");
                    ui.end_row();

                    for drive_data in &computer.drives{
                        controls::icon_label(ui, svg::HARD_DRIVE, &drive_data.drive_letter);
                        ui.label(&drive_data.drive_type);
                        ui.label(format!("{} Gb / {} Gb", drive_data.space_left, drive_data.total_size));
                        ui.end_row();
                    }
                });
            });

            ui.add_space(2.0 * m.gap.y);
        });
    });

}
