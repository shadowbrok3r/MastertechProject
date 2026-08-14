use eframe::egui::{Button, Color32, ComboBox, Grid, RichText, ScrollArea, TextEdit, Ui, Vec2, Vec2b, Widget};
use database::schema::{random_record_id, ComputerData, RecordIdExt, TicketData, COMPUTER_TABLE};

use crate::{ui_tools::icons, PlatformSpawner, Spawner};

use super::return_colors;

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
    let response = Button::new(RichText::new(format!("{} Delete", icons::TRASH)).color(Color32::LIGHT_RED))
        .ui(ui)
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

                    ui.group(|ui| {
                        ui.label(RichText::new("Customer's Computers").strong().color(Color32::LIGHT_BLUE));

                        let selected_text = if search.search_query.is_empty() {
                            "Select a computer…"
                        } else {
                            search.search_query.as_str()
                        };

                        let mut picked: Option<usize> = None;
                        ComboBox::from_id_salt("customer_computers_combo")
                            .selected_text(selected_text)
                            .width(ui.available_width() - 8.0)
                            .show_ui(ui, |ui| {
                                for (i, label) in labels.iter().enumerate() {
                                    let is_selected = *search.search_query == *label;
                                    if ui.selectable_label(is_selected, label).clicked() {
                                        picked = Some(i);
                                    }
                                }
                            });

                        if let Some(idx) = picked {
                            if let Some(comp) = search.customer_computers.get(idx) {
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
                                computer.windows_active = comp.windows_active.clone();
                                ticket.computer = Some(computer.id.clone());
                                log::info!("Selected computer: {} ({})", comp.hostname, comp.id.key_string());
                            }
                        }

                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            if ui.button("Import from PrestaShop").clicked() {
                                *search.import_presta_clicked = true;
                            }
                            if ui.button("Import from Everest").clicked() {
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
                            ui.add_space(6.0);
                            ui.separator();
                            ui.label(RichText::new("Other computers for this customer").weak());
                            for comp in &others {
                                ui.horizontal(|ui| {
                                    ui.label(computer_label(comp));
                                    if ui.button(format!("{} Open", icons::OPEN)).on_hover_text("View this computer in a separate window").clicked() {
                                        *search.open_in_second_modal = Some(comp.clone());
                                    }
                                });
                            }
                        }
                    });
                    ui.add_space(10.0);
                }
            }
            
            ui.horizontal(|ui| {
                if ui.button("Update").clicked() {
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
            ui.group(|ui| {
                Grid::new(format!("{} grid", computer.cpu))
                .max_col_width(avail_size.x / 2.14)
                .min_col_width(avail_size.x / 2.14)
                .with_row_color(|num, style| return_colors(num, style))
                .show(ui, |ui| {
                    ui.colored_label(Color32::LIGHT_RED, "ID");
                    if let Some(sho) = service_history_open.as_deref_mut() {
                        if ui.button(
                            RichText::new(computer.id.key_string())
                                .color(Color32::LIGHT_BLUE)
                        ).on_hover_text("View all services for this computer").clicked() {
                            *sho = true;
                        }
                    } else {
                        ui.label(computer.id.key_string());
                    }
                    ui.end_row();
                    
                    ui.colored_label(Color32::LIGHT_RED, "Linked Customer");
                    ui.label(computer.customer.as_ref().map_or_else(|| "—".to_string(), |id| id.key_string()));
                    ui.end_row();

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
